//! Clock widget — formats a `chrono::DateTime<chrono::Local>` for display
//! in the menubar right cluster: `HH:MM Weekday YYYY-MM-DD`.
//!
//! Uses the same chrome face/size as menu titles (macOS menu bar clock is
//! system UI type, not mono).

use chrono::{DateTime, Local};
use iced::Element;
use iced::widget::text;
use sola_kit::fonts;

/// Matches menubar menu-label size.
const CLOCK_SIZE: f32 = 13.0;

/// Format a local timestamp as `HH:MM Weekday YYYY-MM-DD`.
pub fn format_clock(now: &DateTime<Local>) -> String {
    now.format("%H:%M %a %Y-%m-%d").to_string()
}

/// Render the clock as an iced text widget.
pub fn clock_widget<Msg: 'static>(now: &DateTime<Local>) -> Element<'_, Msg> {
    text(format_clock(now))
        .font(fonts::chrome())
        .size(CLOCK_SIZE)
        .into()
}
