//! The toast and the erase question: a card floating over the foot of
//! the list, so the list and the letter never change size for it (a band
//! under the panes shrank the list as it came and grew it 5 s later).

use iced::widget::{Space, column, container, opaque, row};
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
    /// The card over the foot of the list, if one is up.
    pub(super) fn view_notice(&self) -> Option<Element<'_, Msg>> {
        match (&self.confirm, &self.toast) {
            (Some(folder), _) => Some(self.view_erase_confirm(folder)),
            (None, Some(toast)) => Some(self.view_toast(toast)),
            (None, None) => None,
        }
    }

    fn view_toast<'a>(&'a self, toast: &'a Toast) -> Element<'a, Msg> {
        let mut actions = row![].spacing(SPACE_SM);
        if toast.undo && self.moves.can_undo() {
            actions =
                actions.push(kit_btn::labeled_sm("Undo", kit_btn::secondary).on_press(Msg::Undo));
        }
        actions = actions
            .push(kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissToast));
        card(
            row![
                kit_text::body(toast.text.as_str()).width(Length::Fill),
                actions
            ]
            .spacing(SPACE_MD)
            .align_y(iced::Alignment::Center)
            .into(),
        )
    }

    fn view_erase_confirm<'a>(&'a self, folder: &'a str) -> Element<'a, Msg> {
        let accounts = if self.connected_accounts() > 1 {
            " on every account"
        } else {
            ""
        };
        card(
            column![
                kit_text::body(format!(
                    "Erase every message in {}{accounts}? This can’t be undone.",
                    folder_label(folder)
                )),
                row![
                    Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::CancelConfirm),
                    kit_btn::labeled_sm("Erase", kit_btn::danger).on_press(Msg::ConfirmErase),
                ]
                .spacing(SPACE_SM),
            ]
            .spacing(SPACE_MD)
            .into(),
        )
    }
}

/// Opaque: a click on the card's text must not reach the row under it.
fn card(content: Element<'_, Msg>) -> Element<'_, Msg> {
    opaque(
        container(content)
            .width(Length::Fill)
            .padding(Padding::from([SPACE_MD, SPACE_LG]))
            .style(style::card),
    )
}
