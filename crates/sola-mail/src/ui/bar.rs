//! The bar across the window, over the list and the letter: the mailbox
//! (a menu of them when they are not beside the list), its unread or what
//! a search found, the check ring; the search field, Erase in Junk and
//! Trash, and a new message at the right.

use std::time::Duration;

use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, stack, text, tooltip};
use iced::{Alignment, Element, Font, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon_svg;
use sola_kit::components::popover::{self, Placement, popover_anchored};
use sola_kit::components::status_mark::{STATUS_MARK_SLOT, SidebarIndicator, status_mark};
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::format::format_count;
use super::list::{Load, SearchState};
use super::widgets::{icon, muted_icon, tool};
use super::{App, Msg, style};
use crate::protocol::FolderRef;
use crate::protocol::boxes::CANONICAL;
use crate::protocol::types::folder_label;

/// The bar's height (Next's `.mail__head`).
pub const BAR_H: f32 = 42.0;
/// The search field's width.
const SEARCH_W: f32 = 240.0;

/// The bar's figure: unread mail, or what a search found.
pub fn head_count(search: Option<&SearchState>, load: Load, rows: usize, unread: u32) -> String {
    match (search, load) {
        (_, Load::Replacing) => String::new(),
        (Some(s), _) if s.capped => format!("{rows}+ found"),
        (Some(_), _) => format!("{} found", format_count(rows as u32)),
        (None, _) if unread > 0 => format!("{} unread", format_count(unread)),
        (None, _) => String::new(),
    }
}

/// The mailbox's lucide icon.
pub fn mailbox_icon(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "inbox" => "lucide/inbox",
        "sent" => "lucide/send",
        "drafts" => "lucide/file-pen",
        "archive" => "lucide/archive",
        "junk" => "lucide/ban",
        "trash" => "lucide/trash",
        _ => "lucide/list-filter",
    }
}

pub fn search_id() -> iced::widget::Id {
    iced::widget::Id::new("mail-search")
}

impl App {
    /// A search is listed, or the field holds text.
    pub(super) fn searching(&self) -> bool {
        !self.search_input.is_empty() || self.list.search.is_some()
    }

    /// The listed mailbox's name, for the bar and the titlebar.
    pub(super) fn list_title(&self) -> String {
        match &self.list.folder {
            FolderRef::Mailbox(name) => folder_label(name),
            FolderRef::Smart(name) => name.clone(),
        }
    }

    /// Unread mail in the listed mailbox.
    pub(super) fn list_unread(&self) -> u32 {
        let (boxes, name) = match &self.list.folder {
            FolderRef::Mailbox(name) => (&self.folders, name),
            FolderRef::Smart(name) => (&self.smart, name),
        };
        boxes
            .iter()
            .find(|f| f.name == *name)
            .map_or(0, |f| f.unread)
    }

    /// `mailboxes`: the mailboxes are beside the list; else the title is
    /// a menu of them.
    pub(super) fn view_bar(&self, mailboxes: bool) -> Element<'_, Msg> {
        let list = &self.list;
        let title = text(self.list_title())
            .font(Font {
                weight: Weight::Semibold,
                ..fonts::ui()
            })
            .size(14)
            .wrapping(Wrapping::None);
        let title: Element<'_, Msg> = if mailboxes {
            container(title).padding(Padding::ZERO.left(6)).into()
        } else {
            let trigger = button(
                row![title, muted_icon("lucide/chevron-down", 13.0)]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center),
            )
            .padding(Padding::from([3.0, 6.0]))
            .style(kit_btn::ghost)
            .on_press(Msg::MailboxMenu(!self.mailbox_menu));
            if self.mailbox_menu {
                popover_anchored(
                    trigger,
                    popover::popover(self.mailbox_menu_list()),
                    Msg::MailboxMenu(false),
                )
                .placement(Placement::Below)
                .into()
            } else {
                trigger.into()
            }
        };
        let count = head_count(
            list.search.as_ref(),
            list.load,
            list.rows.len(),
            self.list_unread(),
        );
        let mut left = row![title, check_mark(list.checking())]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center);
        if !count.is_empty() {
            left = left.push(kit_text::caption(count).style(kit_text::muted));
        }

        let field = text_input("Search mail", &self.search_input)
            .id(search_id())
            .on_input(Msg::SearchChanged)
            .on_submit(Msg::SearchSubmit)
            .size(13)
            .padding(Padding {
                top: 4.0,
                right: 9.0,
                bottom: 4.0,
                left: 28.0,
            })
            .style(style::search_input)
            .width(Length::Fixed(SEARCH_W));
        // The magnifier sits in the field's left padding.
        let glass = container(muted_icon("lucide/search", 13.0))
            .height(Length::Fill)
            .padding(Padding::ZERO.left(9))
            .align_y(Alignment::Center);
        let field = stack![field, glass].height(Length::Shrink);
        let mut right = row![field].spacing(SPACE_XS).align_y(Alignment::Center);
        if self.searching() {
            right = right.push(tool("lucide/x", "Clear search", Some(Msg::SearchClear)));
        }
        right = right.push(Space::new().width(SPACE_SM));
        if let Some(folder) = list.erasable() {
            right = right.push(tool(
                "lucide/trash",
                "Erase mailbox…",
                Some(Msg::AskErase(folder.to_string())),
            ));
        }
        right = right.push(tool("lucide/square-pen", "New message", Some(Msg::Compose)));

        container(
            row![
                container(left).width(Length::Fill).clip(true),
                right
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BAR_H))
        .padding(Padding {
            top: 0.0,
            right: 10.0,
            bottom: 0.0,
            left: 10.0,
        })
        .align_y(Alignment::Center)
        .style(style::canvas)
        .into()
    }

    /// The six mailboxes (and the smart ones) under the bar's title.
    fn mailbox_menu_list(&self) -> Element<'_, Msg> {
        let current = &self.list.folder;
        let boxes = self
            .folders
            .iter()
            .filter(|f| CANONICAL.iter().any(|c| c.eq_ignore_ascii_case(&f.name)))
            .map(|f| (FolderRef::Mailbox(f.name.clone()), folder_label(&f.name), f.unread))
            .chain(
                self.smart
                    .iter()
                    .map(|f| (FolderRef::Smart(f.name.clone()), f.name.clone(), f.unread)),
            );
        let rows = boxes.map(|(target, label, unread)| {
            let glyph = match &target {
                FolderRef::Mailbox(name) => mailbox_icon(name),
                FolderRef::Smart(_) => mailbox_icon(""),
            };
            let on = *current == target;
            let mut line = row![
                icon_svg(icon(glyph), 14),
                text(label)
                    .font(if on { fonts::ui_medium() } else { fonts::ui() })
                    .size(13)
                    .width(Length::Fill),
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center);
            if unread > 0 {
                line = line.push(kit_text::caption(format_count(unread)).style(kit_text::muted));
            }
            button(line)
                .padding(Padding::from([5.0, 8.0]))
                .width(Length::Fill)
                .style(kit_btn::list_item(on))
                .on_press(Msg::SelectFolder(target))
                .into()
        });
        container(column(rows).spacing(SPACE_XS))
            .width(Length::Fixed(196.0))
            .into()
    }
}

/// Working ring while a check is in flight. The slot stays reserved so
/// the count does not jump when the ring appears.
fn check_mark<'a>(busy: bool) -> Element<'a, Msg> {
    let slot: Element<'a, Msg> = if busy {
        let tip = container(kit_text::caption("Checking mail"))
            .padding(Padding {
                top: 5.0,
                right: 8.0,
                bottom: 5.0,
                left: 8.0,
            })
            .style(popover::style);
        tooltip(
            status_mark(SidebarIndicator::Working),
            tip,
            tooltip::Position::Bottom,
        )
        .gap(6)
        .delay(Duration::from_millis(280))
        .into()
    } else {
        Space::new()
            .width(Length::Fixed(STATUS_MARK_SLOT))
            .height(Length::Fixed(STATUS_MARK_SLOT))
            .into()
    };
    container(slot)
        .width(Length::Fixed(STATUS_MARK_SLOT))
        .height(Length::Fixed(STATUS_MARK_SLOT))
        .align_y(Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_unread_or_found() {
        assert_eq!(head_count(None, Load::Idle, 50, 12), "12 unread");
        assert_eq!(head_count(None, Load::Idle, 50, 0), "");
        assert_eq!(head_count(None, Load::Idle, 50, 1234), "1,234 unread");
        let found = SearchState {
            query: "x".into(),
            capped: false,
        };
        assert_eq!(head_count(Some(&found), Load::Idle, 7, 0), "7 found");
        let capped = SearchState {
            query: "x".into(),
            capped: true,
        };
        assert_eq!(head_count(Some(&capped), Load::Idle, 200, 0), "200+ found");
        assert_eq!(head_count(Some(&found), Load::Replacing, 7, 3), "");
    }

    #[test]
    fn mailboxes_have_icons() {
        assert_eq!(mailbox_icon("INBOX"), "lucide/inbox");
        assert_eq!(mailbox_icon("Trash"), "lucide/trash");
        assert_eq!(mailbox_icon("Newsletters"), "lucide/list-filter");
    }
}
