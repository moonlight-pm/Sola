//! sola-kvm-view — the Remote Desktop app (`sola-kvm view <host>`): other
//! machines' screens in windows here, one each, driven from this keyboard
//! and pointer, and a Connections window to open, pair or forget them. The
//! other half is the host's: on a Mac its agent (`apps/sola-kvm-mac`), on
//! Linux `sola-kvm` itself; the wire is `sola_kvm::view_wire`.

mod app;
mod connections;
mod decode;
mod fit;
mod inhibit;
mod instance;
mod measure;
mod menu;
mod net;
mod nvdec_sys;
mod picture;
mod pictures;
mod pointer;
mod session;
mod stats;

use sola_bus::topics::TopicKind;
use sola_kit::app::{BusSetup, startup};
use sola_kit::fonts;

use app::App;

const APP_ID: &str = "sola-kvm-view";

/// The machines named on the command line (`host` or `host:port`).
fn named() -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut named = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            // Flags that take a value, and theirs.
            "--measure" | "--link" => {
                words.next();
            }
            w if w.starts_with('-') => {}
            w => named.push(w.to_string()),
        }
    }
    named
}

/// `--measure`'s machine: the one named, or the only one paired. `Err` says
/// why there is none.
fn measured() -> Result<app::Target, String> {
    if let Some(first) = named().first() {
        return Ok(app::target(first));
    }
    let hosts = sola_kvm::link::view_hosts(&sola_kvm::link::dir())
        .map_err(|e| format!("The paired machines cannot be read: {e}"))?;
    match hosts.as_slice() {
        [host] => Ok((host.clone(), app::DEFAULT_KVM_PORT)),
        [] => Err("No machine is paired for Remote Desktop yet. Run: sola-kvm pair <host> --view".into()),
        many => Err(format!("Several machines are paired: {}. Name one.", many.join(", "))),
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
    if let Some(pictures) = measure_arg() {
        return measure::run(measured(), pictures, link_arg());
    }

    startup(APP_ID);
    // One Remote Desktop: a later launch hands it what it names.
    if !instance::claim() {
        return match instance::handoff(&named()) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                tracing::warn!(error = %e, "handoff to the running Remote Desktop failed");
                std::process::ExitCode::FAILURE
            }
        };
    }

    BusSetup::new(APP_ID)
        .app_kinds(&[TopicKind::WindowPlace, TopicKind::CloseTab])
        .install();

    let targets: Vec<app::Target> = named().iter().map(|t| app::target(t)).collect();
    iced::daemon(move || App::boot(targets.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .run()
        .map_or_else(
            |e| {
                tracing::error!(%e, "remote desktop: the app failed");
                std::process::ExitCode::FAILURE
            },
            |()| std::process::ExitCode::SUCCESS,
        )
}
