//! Toast widget — renders a transient notification message for the menubar
//! right cluster. The caller is responsible for expiry logic via
//! `Msg::ToastExpire(generation)`.

use iced::widget::text;
use iced::{Element, Length};

/// Render the toast. Returns an empty text if no toast is active.
pub fn toast_widget<Msg: 'static>(message: Option<&str>) -> Element<'_, Msg> {
    match message {
        Some(msg) => text(msg).width(Length::Shrink).into(),
        None => text("").width(Length::Shrink).into(),
    }
}
