//! The toast and the erase confirmation strip.

use iced::widget::{Space, container, row};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;

use super::{App, Msg, style};
use crate::protocol::types::folder_label;

pub struct Toast {
    pub text: String,
    /// Offer Undo (a move just happened).
    pub undo: bool,
}

impl App {
    pub(super) fn view_toast<'a>(&'a self, toast: &'a Toast) -> Element<'a, Msg> {
        let mut actions = row![].spacing(SPACE_SM);
        if toast.undo && self.moves.can_undo() {
            actions =
                actions.push(kit_btn::labeled_sm("Undo", kit_btn::secondary).on_press(Msg::Undo));
        }
        actions = actions
            .push(kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissToast));
        strip(
            row![
                kit_text::body(toast.text.as_str()),
                Space::new().width(Length::Fill),
                actions,
            ]
            .spacing(SPACE_SM)
            .align_y(iced::Alignment::Center)
            .into(),
        )
    }

    pub(super) fn view_erase_confirm<'a>(&'a self, folder: &'a str) -> Element<'a, Msg> {
        let accounts = if self.connected_accounts() > 1 {
            " on every account"
        } else {
            ""
        };
        strip(
            row![
                kit_text::body(format!(
                    "Erase every message in {}{accounts}? This can’t be undone.",
                    folder_label(folder)
                )),
                Space::new().width(Length::Fill),
                kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::CancelConfirm),
                kit_btn::labeled_sm("Erase", kit_btn::danger).on_press(Msg::ConfirmErase),
            ]
            .spacing(SPACE_SM)
            .align_y(iced::Alignment::Center)
            .into(),
        )
    }
}

fn strip(content: Element<'_, Msg>) -> Element<'_, Msg> {
    container(content)
        .padding(Padding::from([SPACE_MD, SPACE_LG]))
        .width(Length::Fill)
        .style(style::strip)
        .into()
}
