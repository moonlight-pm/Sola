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

fn main() -> iced::Result {
    let Some(target) = std::env::args().nth(1).filter(|a| !a.starts_with('-')) else {
        eprintln!("usage: sola-kvm-view <host>[:<kvm port>]");
        std::process::exit(2);
    };
    let (host, port) = match target.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or(DEFAULT_KVM_PORT)),
        None => (target, DEFAULT_KVM_PORT),
    };

    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(sola_bus::topics::TopicKind::ALL)
        .app_menu(
            "Remote Desktop",
            [sola_kit::menu::close_window_item(), sola_kit::menu::quit_item("Quit Remote Desktop")],
        )
        .install();

    let mut settings = window_settings_transparent(APP_ID);
    settings.size = iced::Size::new(1512.0, 1010.0);

    iced::application(move || App::boot(host.clone(), port), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(settings)
        .run()
}
