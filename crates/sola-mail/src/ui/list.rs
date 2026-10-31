//! Message list: state and rows, drawn as Next's `.mail__list`: a 290 px
//! column on the raised fill, rows a hairline apart, an accent dot on
//! unread mail.

use chrono::{DateTime, Local};

use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, keyed_column, row, stack, text};
use iced::{Alignment, Element, Font, Length, Padding};
use sola_kit::components::style::{SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use super::format::{account_tag, recipients_short, short_name};
use super::widgets::{accent_icon, default_cursor, h_hairline, muted_icon};
use super::{App, Msg, style};
use crate::protocol::FolderRef;
use crate::protocol::MessageSummary;
use crate::protocol::date::list_stamp;
use sola_kit::components::scroll;

/// Next's list column.
pub const LIST_W: f32 = 290.0;
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

impl App {
    pub(super) fn view_list(&self) -> Element<'_, Msg> {
        let list = &self.list;
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

        let scroller: Element<'_, Msg> = scroll::scrollable(default_cursor(body))
            .id(super::list_scroll_id())
            .height(Length::Fill)
            .width(Length::Fill)
            .on_scroll(Msg::ListScrolled)
            .into();
        // The toast floats over the foot of the list: it never resizes it.
        let pane: Element<'_, Msg> = match self.view_notice() {
            Some(card) => stack![
                scroller,
                container(card)
                    .height(Length::Fill)
                    .align_y(Alignment::End)
                    .padding(SPACE_MD),
            ]
            .into(),
            None => scroller,
        };
        container(pane)
            .width(Length::Fixed(LIST_W))
        .height(Length::Fill)
        .style(style::raised)
        .into()
    }
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
}
