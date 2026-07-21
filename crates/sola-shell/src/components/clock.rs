//! Clock widget — formats a `chrono::DateTime<chrono::Local>` for display
//! in the menubar right cluster: `HH:MM Weekday YYYY-MM-DD`.
//!
//! Type uses the mono role (data) at menubar chrome size — denser than body
//! UI, matching design-language clock guidance.

use chrono::{DateTime, Local};
use iced::widget::text;
use iced::Element;
use sola_kit::fonts;

/// Menubar clock type size (logical px). Matches menu labels at 13.
const CLOCK_SIZE: f32 = 13.0;

/// Format a local timestamp as `HH:MM Weekday YYYY-MM-DD`.
pub fn format_clock(now: &DateTime<Local>) -> String {
    now.format("%H:%M %a %Y-%m-%d").to_string()
}

/// Render the clock as an iced text widget.
pub fn clock_widget<Msg: 'static>(now: &DateTime<Local>) -> Element<'_, Msg> {
    text(format_clock(now))
        .font(fonts::mono())
        .size(CLOCK_SIZE)
        .into()
}
