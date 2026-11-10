//! sola-kvm-view — the Remote Desktop app (`sola-kvm view <host>`): another machine's
//! screen in a window here, driven from this keyboard and pointer. The other
//! half is the host's agent (on a Mac, `apps/sola-kvm-mac`); the wire is
//! `sola_kvm::view_wire`.

mod app;
mod decode;
mod fit;
mod inhibit;
mod net;
mod nvdec_sys;
mod picture;
mod pictures;

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

fn main() -> iced::Result {
    let target = target();

    startup(APP_ID);

    BusSetup::new(APP_ID)
        .app_kinds(&[])
        .app_menu(
            "Remote Desktop",
            [sola_kit::menu::close_window_item(), sola_kit::menu::quit_item("Quit Remote Desktop")],
        )
        .install();

    let mut settings = window_settings_transparent(APP_ID);
    settings.size = iced::Size::new(1512.0, 1010.0);

    iced::application(move || App::boot(target.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(settings)
        .run()
}
