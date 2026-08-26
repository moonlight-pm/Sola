//! sola-scope — magnified pixel grid under the pointer.

mod app;
mod grid;
mod sample;

use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use app::App;

const APP_ID: &str = "sola-scope";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(sola_bus::topics::TopicKind::ALL)
        .app_menu("Scope", [("quit", "Quit Scope", KeyCode::Q.meta())])
        .app_menu(
            "View",
            [
                ("zoom_in", "Zoom In", KeyCode::EQUAL.meta()),
                ("zoom_out", "Zoom Out", KeyCode::MINUS.meta()),
            ],
        )
        .app_menu("Edit", [("copy", "Copy Color", KeyCode::C.meta())])
        .install();

    let mut settings = window_settings_transparent(APP_ID);
    settings.size = iced::Size::new(420.0, 520.0);

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(settings)
        .run()
}
