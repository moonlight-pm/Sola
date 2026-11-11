//! sola-kvm-view — the Remote Desktop app (`sola-kvm view <host>`): another machine's
//! screen in a window here, driven from this keyboard and pointer. The other
//! half is the host's agent (on a Mac, `apps/sola-kvm-mac`); the wire is
//! `sola_kvm::view_wire`.

mod app;
mod decode;
mod fit;
mod inhibit;
mod measure;
mod menu;
mod net;
mod nvdec_sys;
mod picture;
mod pictures;
mod pointer;
mod stats;

use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use app::App;

const APP_ID: &str = "sola-kvm-view";
const DEFAULT_KVM_PORT: u16 = 4242;

/// Which machine to show: the one named, or (from the launcher, which names
/// none) the one this machine is paired to view. `Err` says why there is
/// none, in words for the window.
fn target() -> Result<(String, u16), String> {
    if let Some(target) = std::env::args().nth(1).filter(|a| !a.starts_with('-')) {
        return Ok(match target.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.parse().unwrap_or(DEFAULT_KVM_PORT)),
            None => (target, DEFAULT_KVM_PORT),
        });
    }
    let hosts = sola_kvm::link::view_hosts(&sola_kvm::link::dir())
        .map_err(|e| format!("The paired machines cannot be read: {e}"))?;
    match hosts.as_slice() {
        [host] => Ok((host.clone(), DEFAULT_KVM_PORT)),
        [] => Err("No machine is paired for Remote Desktop yet. Run: sola-kvm pair <host> --view".into()),
        many => Err(format!(
            "Several machines are paired: {}. Open one with: sola-kvm view <host>",
            many.join(", ")
        )),
    }
}

/// The word after `flag`, when it is given.
fn arg_after(flag: &str) -> Option<Option<String>> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == flag)?;
    Some(args.get(at + 1).cloned())
}

/// `--measure <pictures>`: how many pictures to measure with no window.
fn measure_arg() -> Option<u32> {
    arg_after("--measure").map(|n| n.and_then(|n| n.parse().ok()).unwrap_or(600))
}

/// `--link <mbps>`, with `--measure`: a slower link than the real one.
fn link_arg() -> Option<f64> {
    arg_after("--link").flatten().and_then(|mbps| mbps.parse().ok())
}

fn main() -> std::process::ExitCode {
    let target = target();
    if let Some(pictures) = measure_arg() {
        return measure::run(target, pictures, link_arg());
    }

    startup(APP_ID);

    BusSetup::new(APP_ID).app_kinds(&[]).install();
    sola_kit::menu::publish(menu::app_menu(false));

    let mut settings = window_settings_transparent(APP_ID);
    settings.size = iced::Size::new(1512.0, 1010.0);

    iced::application(move || App::boot(target.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(settings)
        .run()
        .map_or_else(
            |e| {
                tracing::error!(%e, "remote view: the window failed");
                std::process::ExitCode::FAILURE
            },
            |()| std::process::ExitCode::SUCCESS,
        )
}
