//! JSON highlighter — token colours from the live theme.

use iced::widget::column;
use iced::{Element, Theme};

use sola_kit::components::json::{line, pretty};
use sola_kit::components::text::caption;
use sola_kit::components::text::muted;

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel, scene};

const SAMPLE: &str = r#"{
  "owner": "workspaces",
  "method": "workspace.spawn",
  "ok": true,
  "duration_ms": 42,
  "params": { "name": "monitor-polish", "agent": false },
  "data": { "id": "sws-1", "count": 1, "ready": null }
}"#;

const LINE: &str = r#"{"owner":"workspaces","ok":true,"count":1}"#;

pub fn view(theme: &Theme) -> Element<'static, Msg> {
    column![
        lede(
            "JSON",
            "Inspector payloads. Keys stay primary text; strings success; numbers warning; literals accent. Punctuation is muted.",
        ),
        scene("Pretty"),
        panel(pretty(SAMPLE, theme)),
        scene("One line"),
        panel(column![
            caption("Preview clips; wrapping is off.").style(muted),
            line(LINE, theme),
        ]
        .spacing(8)),
    ]
    .spacing(16)
    .into()
}
