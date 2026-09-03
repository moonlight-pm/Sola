//! Menu subsystem.
//!
//! `state` holds `MenuCache` and `synthesized_menu` — pure data, no window
//! logic. Window management (opening, closing, rendering the dropdown) lands
//! in Task 7.
//! Menu dropdown window — state, lifecycle, and view entry point.
//!
//! The menu window is a **card-sized** transparent surface below the
//! menubar. Kept mapped at 2×2 while dismissed so show is Frame + stack,
//! not a new map. Live size is the dropdown/panel card (not the full
//! output) — a full-output wgpu swapchain pegs software GL.
//! Anchor X comes from `Shell::menu_anchor_x` (menubar label positions).

pub mod state;
pub mod view;

use iced::widget::{container, mouse_area, stack, text};
use iced::window;
use iced::{Element, Length};
use sola_kit::app::window_settings;

use crate::app::Msg;

/// Host a dropdown card in the tight menu overlay. The window is already
/// placed at the card; leftover pixels (unused height / shadow) still
/// dismiss on press.
pub fn host_card<'a>(card: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    let positioned = container(card.into())
        .width(Length::Fill)
        .align_y(iced::alignment::Vertical::Top);
    let backdrop = mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
        .on_press(Msg::CloseMenu);
    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Open the menu parked at 2×2. Show Frames to the card, not the output.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    let p = crate::zoning::OVERLAY_PARK as f32;
    settings.size = iced::Size::new(p, p);
    settings.position = iced::window::Position::Specific(iced::Point::new(
        crate::zoning::OVERLAY_PARK_X as f32,
        crate::zoning::OVERLAY_PARK_Y as f32,
    ));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}
