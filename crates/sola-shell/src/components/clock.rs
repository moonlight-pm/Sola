//! Clock widget — formats a `chrono::DateTime<chrono::Local>` for display
//! in the menubar right cluster: `HH:MM Weekday YYYY-MM-DD`.

use chrono::{DateTime, Local};
use iced::widget::text;
use iced::Element;

/// Format a local timestamp as `HH:MM Weekday YYYY-MM-DD`.
pub fn format_clock(now: &DateTime<Local>) -> String {
    now.format("%H:%M %a %Y-%m-%d").to_string()
}

/// Render the clock as an iced text widget.
pub fn clock_widget<Msg: 'static>(now: &DateTime<Local>) -> Element<'_, Msg> {
    text(format_clock(now))
        .font(sola_kit::fonts::INTER)
        .size(15)
        .into()
}
