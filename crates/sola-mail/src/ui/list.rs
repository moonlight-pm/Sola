//! Message list: state, the head (mailbox, unread, search, new message)
//! and rows, drawn as Next's `.mail__list`: a 290 px column on the raised
//! fill, rows a hairline apart, an accent dot on unread mail.

use chrono::{DateTime, Local};
use std::time::Duration;

use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, column, container, keyed_column, row, scrollable, text, tooltip,
};
use iced::{Alignment, Element, Font, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon_svg;
use sola_kit::components::popover::{self, Placement, popover_anchored};
use sola_kit::components::status_mark::{STATUS_MARK_SLOT, SidebarIndicator, status_mark};
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::format::{account_tag, format_count, recipients_short, short_name};
use super::widgets::{accent_icon, default_cursor, h_hairline, icon as widgets_icon, muted_icon, tool};
use super::{App, Msg, style};
use crate::protocol::FolderRef;
use crate::protocol::MessageSummary;
use crate::protocol::boxes::CANONICAL;
use crate::protocol::date::list_stamp;
use crate::protocol::types::folder_label;

/// Next's list column.
pub const LIST_W: f32 = 290.0;
/// The list's head.
pub const HEAD_H: f32 = 42.0;
/// A row's side padding, and the width its lines have between (a pixel
/// spare for rounding).
const ROW_PAD_X: f32 = 16.0;
const ROW_TEXT_W: f32 = LIST_W - ROW_PAD_X * 2.0 - 1.0;
/// The unread dot, a row's flag / paperclip, and the gap beside each.
const DOT: f32 = 7.0;
const MARK: f32 = 12.0;
const MARK_GAP: f32 = 6.0;
/// The account tag after the subject.
const TAG_SIZE: f32 = 11.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    Idle,
    /// A new list is on its way (folder switch / search): count shows `…`.
    Replacing,
    Refreshing,
    More,
}

pub struct SearchState {
    pub query: String,
    pub capped: bool,
}

pub struct ListState {
    /// Sidebar selection (stays highlighted during a search).
    pub folder: FolderRef,
    pub search: Option<SearchState>,
    pub rows: Vec<MessageSummary>,
    pub total: u32,
    /// Bumped whenever the list changes identity; older pages drop.
    pub epoch: u64,
    pub load: Load,
    /// The last load-more came back empty.
    pub exhausted: bool,
    /// Last scroll Y. Load-more only after the user scrolls down —
    /// shrinking the list (delete) must not look like a fetch of older mail.
    pub scroll_y: f32,
}

impl ListState {
    pub fn new(folder: FolderRef, rows: Vec<MessageSummary>, total: u32) -> Self {
        Self {
            folder,
            search: None,
            rows,
            total,
            epoch: 0,
            load: Load::Idle,
            exhausted: false,
            scroll_y: 0.0,
        }
    }

    pub fn index_of(&self, id: &crate::protocol::MailId) -> Option<usize> {
        self.rows.iter().position(|m| m.id == *id)
    }

    pub fn row_mut(&mut self, id: &crate::protocol::MailId) -> Option<&mut MessageSummary> {
        self.rows.iter_mut().find(|m| m.id == *id)
    }

    /// A first page is on its way (folder open, check, search): the
    /// header ring. Load-more is not a check.
    pub fn checking(&self) -> bool {
        matches!(self.load, Load::Replacing | Load::Refreshing)
    }

    /// Trash / Junk can be erased (not while searching).
    pub fn erasable(&self) -> Option<&str> {
        if self.search.is_some() {
            return None;
        }
        match &self.folder {
            FolderRef::Mailbox(name) if name == "Trash" || name == "Junk" => Some(name),
            _ => None,
        }
    }
}

/// The head's figure: unread mail, or what a search found.
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

/// Per-render context for rows.
struct RowCtx<'a> {
    now: DateTime<Local>,
    /// IMAP account addresses (for the "which inbox" tag).
    peers: &'a [String],
    tag_accounts: bool,
}

fn semibold() -> Font {
    Font {
        weight: Weight::Semibold,
        ..fonts::ui()
    }
}

pub fn search_id() -> iced::widget::Id {
    iced::widget::Id::new("mail-search")
}

impl App {
    /// The search field shows while it is open, holds text, or a search
    /// is listed.
    pub(super) fn search_shown(&self) -> bool {
        self.search_open || !self.search_input.is_empty() || self.list.search.is_some()
    }

    /// The listed mailbox's name, for the head and the window's title.
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

    /// `mailboxes`: the mailboxes are beside the list; else the head's
    /// title picks one.
    pub(super) fn view_list(&self, mailboxes: bool) -> Element<'_, Msg> {
        let list = &self.list;
        let head = if self.search_shown() {
            self.search_head()
        } else {
            self.list_head(mailboxes)
        };

        let body: Element<'_, Msg> = if list.rows.is_empty() && list.load != Load::Replacing {
            let caption = match &list.search {
                Some(s) => format!(
                    "Nothing matches “{}” in Inbox, Sent, Archive or Drafts",
                    s.query
                ),
                None if !self.any_ready() => "Connecting…".into(),
                None => "No messages".into(),
            };
            container(kit_text::caption(caption).style(kit_text::muted))
                .padding(Padding::from([SPACE_XL, 16.0]))
                .width(Length::Fill)
                .into()
        } else {
            let peers = self.account_addresses();
            let ctx = RowCtx {
                now: Local::now(),
                tag_accounts: peers.len() > 1,
                peers: &peers,
            };
            let rows: Vec<(u64, Element<'_, Msg>)> = list
                .rows
                .iter()
                .map(|m| {
                    (
                        row_key(m),
                        message_row(m, self.selected.as_ref() == Some(&m.id), &ctx),
                    )
                })
                .collect();
            keyed_column(rows).spacing(0).width(Length::Fill).into()
        };

        container(
            column![
                head,
                h_hairline(),
                scrollable(default_cursor(body))
                    .id(super::list_scroll_id())
                    .height(Length::Fill)
                    .width(Length::Fill)
                    .on_scroll(Msg::ListScrolled),
            ]
            .height(Length::Fill),
        )
        .width(Length::Fixed(LIST_W))
        .height(Length::Fill)
        .style(style::raised)
        .into()
    }

    /// The mailbox in semibold; the check ring, unread, search and a new
    /// message at the right.
    fn list_head(&self, mailboxes: bool) -> Element<'_, Msg> {
        let title = text(self.list_title())
            .font(semibold())
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
        let count = head_count(None, self.list.load, self.list.rows.len(), self.list_unread());
        let mut bar = row![
            container(title).width(Length::Fill).clip(true),
            check_mark(self.list.checking()),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        if !count.is_empty() {
            bar = bar.push(kit_text::caption(count).style(kit_text::muted));
        }
        if let Some(folder) = self.list.erasable() {
            bar = bar.push(tool(
                "lucide/trash",
                "Erase mailbox…",
                Some(Msg::AskErase(folder.to_string())),
            ));
        }
        bar = bar
            .push(tool("lucide/search", "Search", Some(Msg::SearchOpen)))
            .push(tool("lucide/square-pen", "New message", Some(Msg::Compose)));
        head(bar.into())
    }

    fn search_head(&self) -> Element<'_, Msg> {
        let list = &self.list;
        let field = text_input("Search mail", &self.search_input)
            .id(search_id())
            .on_input(Msg::SearchChanged)
            .on_submit(Msg::SearchSubmit)
            .size(13)
            .padding(Padding::from([4.0, 9.0]))
            .style(style::search_input)
            .width(Length::Fill);
        let count = head_count(list.search.as_ref(), list.load, list.rows.len(), 0);
        let mut bar = row![field, check_mark(list.checking())]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center);
        if !count.is_empty() {
            bar = bar.push(kit_text::caption(count).style(kit_text::muted));
        }
        bar = bar.push(tool("lucide/x", "Close search", Some(Msg::SearchClear)));
        head(bar.into())
    }

    /// The six mailboxes (and the smart ones) under the head's title.
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
            let icon = match &target {
                FolderRef::Mailbox(name) => mailbox_icon(name),
                FolderRef::Smart(_) => mailbox_icon(""),
            };
            let on = *current == target;
            let mut line = row![
                icon_svg(widgets_icon(icon), 14),
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

/// The head's 42 px band.
fn head(content: Element<'_, Msg>) -> Element<'_, Msg> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fixed(HEAD_H))
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 10.0,
        })
        .align_y(Alignment::Center)
        .into()
}

/// Next's `.msg`: who and when, then the subject; the dot on unread mail.
fn message_row<'a>(m: &MessageSummary, selected: bool, ctx: &RowCtx<'_>) -> Element<'a, Msg> {
    // Sent / Drafts list who it went to; everything else who sent it.
    let outgoing = matches!(m.id.folder.as_str(), "Sent" | "Drafts");
    let who = if outgoing {
        recipients_short(&m.to)
    } else {
        short_name(&m.from)
    };
    let subject = if m.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        m.subject.clone()
    };
    let unread = !m.seen;
    let stamp = list_stamp(m.date, ctx.now);
    let tag = match &m.forwarded_for {
        Some(addr) => Some(format!("↳ {}", account_tag(addr, ctx.peers))),
        None if ctx.tag_accounts => Some(account_tag(&m.id.account, ctx.peers)),
        None => None,
    };

    // Both lines end in an ellipsis where they meet what is beside them.
    let name_font = if unread { semibold() } else { fonts::ui_medium() };
    let mut name_room = ROW_TEXT_W - MARK_GAP - fonts::line_width(&stamp, 12.0, fonts::ui());
    if unread {
        name_room -= DOT + MARK_GAP;
    }
    for mark in [m.flagged, m.has_attachment] {
        if mark {
            name_room -= MARK + MARK_GAP;
        }
    }
    let mut subject_room = ROW_TEXT_W;
    if let Some(tag) = &tag {
        subject_room -= fonts::line_width(tag, TAG_SIZE, fonts::ui()) + SPACE_MD;
    }

    let mut top = row![].spacing(MARK_GAP).align_y(Alignment::Center);
    if unread {
        top = top.push(
            container(Space::new().width(DOT).height(DOT))
                .width(DOT)
                .height(DOT)
                .style(style::dot),
        );
    }
    let who = text(fonts::fit_line(&who, 13.0, name_font, name_room))
        .font(name_font)
        .size(13)
        .wrapping(Wrapping::None);
    top = top.push(container(who).width(Length::Fill).clip(true));
    if m.flagged {
        top = top.push(accent_icon("lucide/flag", MARK));
    }
    if m.has_attachment {
        top = top.push(muted_icon("lucide/paperclip", MARK));
    }
    let top = top.push(text(stamp).size(12).style(kit_text::muted));

    let subject = text(fonts::fit_line(&subject, 13.0, fonts::ui(), subject_room))
        .font(fonts::ui())
        .size(13)
        .wrapping(Wrapping::None);
    let mut bottom = row![container(subject).width(Length::Fill).clip(true)]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);
    if let Some(tag) = tag {
        bottom = bottom.push(text(tag).size(TAG_SIZE).style(kit_text::muted));
    }

    let content = column![top, bottom]
        .spacing(3.0)
        .width(Length::Fill)
        .padding(Padding::from([12.0, ROW_PAD_X]));
    column![
        button(content)
            .on_press(Msg::SelectMessage(m.id.clone()))
            .padding(0)
            .width(Length::Fill)
            .style(style::list_row(selected)),
        h_hairline(),
    ]
    .into()
}

fn row_key(m: &MessageSummary) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.id.hash(&mut h);
    h.finish()
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
    fn ring_tracks_first_pages_not_load_more() {
        let mut list = ListState::new(FolderRef::inbox(), Vec::new(), 0);
        for (load, busy) in [
            (Load::Idle, false),
            (Load::Replacing, true),
            (Load::Refreshing, true),
            (Load::More, false),
        ] {
            list.load = load;
            assert_eq!(list.checking(), busy, "{load:?}");
        }
    }

    #[test]
    fn head_counts_unread_or_found() {
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
