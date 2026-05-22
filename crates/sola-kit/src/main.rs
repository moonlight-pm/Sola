//! `sola-kit` binary — the kit storybook.
//!
//! Iced app that catalogs every component the kit ships, renders each
//! one in a showcase panel, and (eventually) exposes the live theme
//! the kit publishes over `Topic::Theme`. Successor to the CEF/Remix-v3
//! storybook that `sola-kit-legacy` carries; rebuilt against iced so the
//! kit binary runs without CEF and dogfoods the iced surface the rest
//! of Sola is migrating onto.
//!
//! See `crates/sola-kit-legacy/src/app/app.rs` for the legacy storybook
//! for comparison.

use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings};
use sola_kit::fonts::{self, NORMAL as F_NORMAL};

mod storybook;

const APP_ID: &str = "sola-kit";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(&[TopicKind::Theme, TopicKind::MenuAction])
        .app_menu("Kit", [("quit", "Quit Kit", KeyCode::Q.meta())])
        .install();

    let mut app = iced::application(
        storybook::Storybook::default,
        storybook::Storybook::update,
        storybook::Storybook::view,
    )
    .title(storybook::Storybook::title)
    .subscription(storybook::Storybook::subscription)
    .theme(storybook::Storybook::theme)
    .default_font(F_NORMAL)
    .window(window_settings(APP_ID));
    for bytes in fonts::load_all() {
        app = app.font(bytes);
    }
    app.run()
}
