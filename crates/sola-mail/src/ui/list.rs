//! Message list: state, header (search / count / erase) and rows.

use chrono::{DateTime, Local};
use std::time::Duration;

use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, column, container, keyed_column, row, scrollable, text, tooltip,
};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::popover;
use sola_kit::components::status_mark::{STATUS_MARK_SLOT, SidebarIndicator, status_mark};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::format::{account_tag, format_count, recipients_short, short_name};
use super::widgets::{accent_icon, default_cursor, muted_icon, tool};
use super::{App, CHROME_H, Msg, style};
use crate::protocol::date::list_stamp;
use crate::protocol::{FolderRef, MessageSummary};

const LIST_W: f32 = 328.0;

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

impl App {
    pub(super) fn view_list(&self) -> Element<'_, Msg> {
        let list = &self.list;
        let search = text_input("Search mail", &self.search_input)
            .on_input(Msg::SearchChanged)
            .on_submit(Msg::SearchSubmit)
            .size(13)
            .padding(Padding {
                top: 5.0,
                right: 10.0,
                bottom: 5.0,
                left: 10.0,
            })
            .width(Length::Fill);

        let count = match (&list.search, list.load) {
            (_, Load::Replacing) => "…".to_string(),
            (Some(s), _) if s.capped => format!("{}+ found", list.rows.len()),
            (Some(_), _) => format!("{} found", format_count(list.rows.len() as u32)),
            (None, _) => format_count(list.total),
        };
        let mut header = row![
            search,
            check_mark(list.checking()),
            kit_text::caption(count).style(kit_text::muted)
        ]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center);
        if !self.search_input.is_empty() || list.search.is_some() {
            header = header.push(tool("lucide/x", "Clear search", Some(Msg::SearchClear)));
        }
        if let Some(folder) = list.erasable() {
            header = header.push(tool(
                "lucide/trash-2",
                "Erase folder…",
                Some(Msg::AskErase(folder.to_string())),
            ));
        }
        let header = container(header)
            .width(Length::Fill)
            .height(Length::Fixed(CHROME_H))
            .padding(Padding {
                top: 0.0,
                right: SPACE_LG,
                bottom: 0.0,
                left: SPACE_LG,
            })
            .align_y(iced::Alignment::Center)
            .style(style::canvas);

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
                .padding(Padding::from([SPACE_XL, SPACE_MD]))
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
                header,
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
        .style(style::canvas)
        .into()
    }
}

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
    // Mail.app: unread is weight, not a leading dot. Both lines clip to
    // one row so the column stays scannable.
    let weight = if m.seen {
        fonts::ui()
    } else {
        fonts::ui_medium()
    };
    let who = text(who)
        .font(weight)
        .size(13)
        .wrapping(Wrapping::None)
        .width(Length::Fill);
    let mut subject = text(subject)
        .font(weight)
        .size(13)
        .wrapping(Wrapping::None)
        .width(Length::Fill);
    if m.seen {
        subject = subject.style(kit_text::muted);
    }

    let mut top = row![container(who).width(Length::Fill).clip(true)]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);
    if m.flagged {
        top = top.push(accent_icon("lucide/flag", 12.0));
    }
    if m.has_attachment {
        top = top.push(muted_icon("lucide/paperclip", 12.0));
    }
    let top = top.push(kit_text::caption(list_stamp(m.date, ctx.now)).style(kit_text::muted));

    let mut bottom = row![container(subject).width(Length::Fill).clip(true)]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);
    let tag = match &m.forwarded_for {
        Some(addr) => Some(format!("↳ {}", account_tag(addr, ctx.peers))),
        None if ctx.tag_accounts => Some(account_tag(&m.id.account, ctx.peers)),
        None => None,
    };
    if let Some(tag) = tag {
        bottom = bottom.push(kit_text::caption(tag).style(kit_text::muted));
    }

    let content = column![top, bottom]
        .spacing(6.0)
        .width(Length::Fill)
        .padding(Padding {
            top: 14.0,
            right: 16.0,
            bottom: 14.0,
            left: 16.0,
        });
    button(content)
        .on_press(Msg::SelectMessage(m.id.clone()))
        .padding(0)
        .width(Length::Fill)
        .style(kit_btn::list_item(selected))
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
        .align_y(iced::Alignment::Center)
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
}
