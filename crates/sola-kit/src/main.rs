//! `sola-kit` binary — the kit storybook.
//!
//! Iced app that catalogs every component the kit ships, renders each
//! one in a showcase panel, and (eventually) exposes the live theme
//! the kit publishes over `Topic::Theme`. Successor to the now-removed
//! CEF/Remix-v3 storybook; rebuilt against iced so the kit binary runs
//! without CEF and dogfoods the iced surface the rest of Sola is
//! migrating onto.

use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings_transparent};
use sola_kit::fonts::{self, INTER};

mod storybook;

const APP_ID: &str = "sola-kit";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        // CustomTheme is persistent: the bus replays each saved preset
        // on connect. Without subscribing here the storybook never
        // receives them, so themes created in a prior session vanish on
        // reopen even though they're still on disk.
        // Windows + WindowFloating: float CSD tracking.
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::CustomTheme,
            TopicKind::MenuAction,
            TopicKind::Windows,
            TopicKind::WindowFloating,
        ])
        .app_menu("Kit", [("quit", "Quit Kit", KeyCode::Q.meta())])
        .install();

    let app = iced::application(
        storybook::Storybook::boot,
        storybook::Storybook::update,
        storybook::Storybook::view,
    )
    .title(storybook::Storybook::title)
    .subscription(storybook::Storybook::subscription)
    .theme(storybook::Storybook::theme)
    .default_font(INTER)
    .window(window_settings_transparent(APP_ID));
    app.run()
}
