//! Toast widget — transient notification message for the menubar.
//!
//! Placed at the **center** of the menubar (see `menubar::view`), not in the
//! right status cluster. The caller is responsible for expiry logic via
//! `Msg::ToastExpire(generation)`.

use iced::widget::text;
use iced::{Element, Length};
use sola_kit::fonts;

/// Chrome size matching menubar labels (`menubar::view::CHROME_SIZE`).
const CHROME_SIZE: f32 = 13.0;

/// Render the toast. Returns an empty (void) text if no toast is active so
/// the menubar stack can skip the overlay layer.
pub fn toast_widget<Msg: 'static>(message: Option<&str>) -> Element<'_, Msg> {
    match message {
        Some(msg) => text(msg)
            .font(fonts::chrome())
            .size(CHROME_SIZE)
            .width(Length::Shrink)
            .into(),
        None => text("").width(Length::Shrink).into(),
    }
}
