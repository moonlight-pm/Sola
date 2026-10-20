//! Reader toolbar, toast and the erase confirmation strip.

use iced::widget::{Space, container, row};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;

use super::widgets::tool;
use super::{App, CHROME_H, Msg, style};
use crate::protocol::types::folder_label;

pub struct Toast {
    pub text: String,
    /// Offer Undo (a move just happened).
    pub undo: bool,
}

impl App {
    pub(super) fn view_toolbar(&self) -> Element<'_, Msg> {
        let composing = self.compose.is_some();
        let selected = self.selected.as_ref().filter(|_| !composing);
        let letter = selected.and_then(|id| self.reader.body().filter(|b| b.id == *id));
        let row_msg = selected.and_then(|id| self.list.rows.iter().find(|m| m.id == *id));
        let in_drafts = selected.is_some_and(|id| id.folder == "Drafts");
        let on = |enabled: bool, msg: Msg| enabled.then_some(msg);

        let mut bar = row![
            tool("lucide/square-pen", "New message", Some(Msg::Compose)),
            tool("lucide/paperclip", "Attach", on(composing, Msg::Attach)),
        ]
        .spacing(SPACE_XS)
        .align_y(iced::Alignment::Center);

        if in_drafts {
            bar = bar.push(tool(
                "lucide/file-pen",
                "Edit draft",
                on(letter.is_some(), Msg::EditDraft),
            ));
        } else {
            bar = bar
                .push(tool(
                    "lucide/reply",
                    "Reply",
                    on(letter.is_some(), Msg::Reply { all: false }),
                ))
                .push(tool(
                    "lucide/reply-all",
                    "Reply all",
                    on(letter.is_some(), Msg::Reply { all: true }),
                ))
                .push(tool(
                    "lucide/forward",
                    "Forward",
                    on(letter.is_some(), Msg::Forward),
                ));
        }

        bar = bar.push(Space::new().width(SPACE_MD));
        let has = selected.is_some();
        let away_from = |name: &str| has && selected.is_some_and(|id| id.folder != name);
        // Back to Inbox is the useful move out of Junk / Trash / Archive.
        if selected.is_some_and(|id| matches!(id.folder.as_str(), "Junk" | "Trash" | "Archive")) {
            bar = bar.push(tool(
                "lucide/inbox",
                "Move to Inbox",
                Some(Msg::MoveSelected("INBOX".into())),
            ));
        }
        bar = bar
            .push(tool(
                "lucide/archive",
                "Archive",
                on(away_from("Archive"), Msg::MoveSelected("Archive".into())),
            ))
            .push(tool(
                "lucide/trash-2",
                "Move to Trash",
                on(away_from("Trash"), Msg::MoveSelected("Trash".into())),
            ))
            .push(tool(
                "lucide/ban",
                "Move to Junk",
                on(away_from("Junk"), Msg::MoveSelected("Junk".into())),
            ));

        bar = bar.push(Space::new().width(Length::Fill));
        if let Some(m) = row_msg {
            bar = bar
                .push(if m.seen {
                    tool("lucide/mail", "Mark as unread", Some(Msg::ToggleSeen))
                } else {
                    tool("lucide/mail-open", "Mark as read", Some(Msg::ToggleSeen))
                })
                .push(tool(
                    "lucide/flag",
                    if m.flagged { "Unflag" } else { "Flag" },
                    Some(Msg::ToggleFlag),
                ));
        }
        if letter.is_some() {
            bar = bar.push(tool("lucide/copy", "Copy message", Some(Msg::CopyBody)));
        }
        if self.moves.can_undo() {
            bar = bar.push(tool("lucide/undo-2", "Undo move", Some(Msg::Undo)));
        }

        container(bar)
            .width(Length::Fill)
            .height(Length::Fixed(CHROME_H))
            .padding(Padding {
                top: 0.0,
                right: SPACE_LG,
                bottom: 0.0,
                left: SPACE_LG,
            })
            .align_y(iced::Alignment::Center)
            .style(style::canvas)
            .into()
    }

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
