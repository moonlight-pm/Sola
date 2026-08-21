//! sola-monitor — bus and call-plane inspector.
//!
//! THESIS: two IPC planes, one inspector. The log stays a one-line
//! scan; selection opens a dedicated payload well — not an accordion
//! inside the table.
//! OWN-WORLD: sola-kit graphite (sidebar, list_item, toolbar, select,
//! hairline splits, token JSON).
//! STORY: pick Bus or Call; filter; pause; read last-known / owners.
//! FIRST VIEWPORT: left plane rail, live log, inspector well, state rail.
//! FORM: Operate surface inside the established Sola kit world.

mod app;

use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts;

use app::App;

const APP_ID: &str = "sola-monitor";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(sola_bus::topics::TopicKind::ALL)
        .app_menu("Monitor", [("quit", "Quit Monitor", KeyCode::Q.meta())])
        .install();
    sola_kit::install_observer(APP_ID);

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID))
        .run()
}
