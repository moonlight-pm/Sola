//! Open on a page: the site's logins, the whole vault, search.

use iced::widget::{Space, button, column, container, row, stack};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_button;
use sola_kit::components::scroll;
use sola_kit::components::style::RADIUS_SM;
use sola_kit::components::text_input::text_input;

use super::super::rows::{self, Kind, Mode, Section};
use super::super::update::VaultMsg;
use super::parts::{self, Trailing, caps, icon, ink, ink2, ink3, item_row, label, msg};
use super::{Head, LIST_MAX_H, Sheet, body, id};
use crate::app::{App, Msg};
use crate::engine::Engine;

impl<E: Engine> App<E> {
    pub(super) fn sheet_browse(&self) -> Sheet<'_> {
        let v = &self.vault;
        let host = super::super::page_host(&v.list.url);
        let mode = self.vault_mode();
        let head = match (&host, mode) {
            (Some(h), Mode::Site | Mode::NoneHere | Mode::Search) => {
                Head::new(h.clone()).plate(parts::letter_plate(h, 22.0))
            }
            _ => Head::new("Vault").plate(parts::kind_plate("key-round", 22.0)),
        }
        .button(self.plus_button())
        .button(self.more_button(None));

        let mut col = column![].spacing(8).width(Length::Fill);
        if let Some(s) = v.status.as_ref()
            && let Some(err) = s.sync_error.as_deref()
        {
            col = col.push(parts::notice(
                sync_notice(err, s.synced_at.is_some()),
                Some(("Retry", VaultMsg::SyncNow)),
                true,
            ));
        }
        col = col.push(self.search_field(mode));
        if let Some(err) = v.error.as_ref() {
            col = col.push(parts::error_line(err));
        }
        if mode == Mode::Whole {
            col = col.push(self.kind_tabs());
        }
        col = col.push(self.list_rows(mode));
        if v.list.unreadable > 0 {
            let n = v.list.unreadable;
            col = col.push(parts::notice(
                format!(
                    "{n} item{} couldn't be opened — an organization key may not have loaded.",
                    if n == 1 { "" } else { "s" }
                ),
                None,
                false,
            ));
        }
        Sheet {
            head,
            body: body(col, false),
            foot: Some(self.vault_foot()),
        }
    }

    /// The search field, its glyph in front and its count behind.
    fn search_field(&self, mode: Mode) -> Element<'_, Msg> {
        let v = &self.vault;
        let total = rows::count(&v.list.items);
        let count = if mode == Mode::Search {
            let hits = rows::rows(&v.list.items, mode, v.list.kind, &v.list.query).len();
            format!("{hits} of {total}")
        } else if v.list.loading && v.list.items.is_empty() {
            String::new()
        } else {
            format!("{total} items")
        };
        let input = text_input("Search vault", &v.list.query)
            .id(id("search"))
            .size(13)
            .padding(Padding {
                top: 7.0,
                right: 84.0,
                bottom: 7.0,
                left: 30.0,
            })
            .style(sola_kit::components::text_input::style)
            .width(Length::Fill)
            .on_input(|s| msg(VaultMsg::Search(s)))
            .on_submit(msg(VaultMsg::Enter));
        let over = row![
            icon("search", 14, ink3),
            Space::new().width(Length::Fill),
            label(count, 11.0, ink3),
        ]
        .align_y(Alignment::Center)
        .padding(Padding::from([0, 10]))
        .height(Length::Fill);
        container(stack![input, over])
            .height(Length::Fixed(30.0))
            .into()
    }

    fn kind_tabs(&self) -> Element<'_, Msg> {
        let v = &self.vault;
        let mut r = row![].spacing(2);
        for k in Kind::ALL {
            let on = v.list.kind == k;
            let mut content = row![label(k.label(), 12.0, if on { ink } else { ink2 })]
                .spacing(5)
                .align_y(Alignment::Center);
            if k == Kind::All {
                content = content.push(label(rows::count(&v.list.items).to_string(), 11.0, ink3));
            }
            r = r.push(
                button(content)
                    .padding(Padding::from([3, 8]))
                    .style(move |t: &Theme, status| kind_style(t, status, on))
                    .on_press(msg(VaultMsg::Kind(k))),
            );
        }
        r.into()
    }

    fn list_rows(&self, mode: Mode) -> Element<'_, Msg> {
        let v = &self.vault;
        if v.list.loading && v.list.items.is_empty() {
            return container(label("Looking up the vault…", 12.0, ink2))
                .padding(Padding::from([12, 6]))
                .into();
        }
        if mode == Mode::NoneHere {
            let action = button(
                row![icon("plus", 14, ink), label("New login for this site", 12.0, ink)]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .padding(Padding::from([5, 11]))
            .style(kit_button::secondary)
            .on_press(msg(VaultMsg::NewLogin));
            return parts::empty(
                "key-round",
                "No logins for this site",
                "Save the one you are about to use, or search for one saved under another address.",
                Some(action.into()),
            );
        }
        let listed = rows::rows(&v.list.items, mode, v.list.kind, &v.list.query);
        if listed.is_empty() {
            let (title, line) = match mode {
                Mode::Search => ("Nothing matches", "Try another word, or fewer of them."),
                _ if rows::count(&v.list.items) == 0 => {
                    ("The vault is empty", "Make a login with +, or add items in Vault.")
                }
                _ => ("Nothing of this kind", "Pick another kind above."),
            };
            return parts::empty("search", title, line, None);
        }
        let mut col = column![].spacing(parts::ROW_GAP).width(Length::Fill);
        let mut section: Option<Section> = None;
        for (i, (sec, item)) in listed.iter().enumerate() {
            if section != Some(*sec) && *sec != Section::Results {
                let trailing = (*sec == Section::Site).then(|| {
                    button(
                        row![label("All items", 11.0, ink2), icon("chevron-right", 12, ink3)]
                            .spacing(2)
                            .align_y(Alignment::Center),
                    )
                    .padding(Padding::from([0, 2]))
                    .style(|_t: &Theme, _s| button::Style::default())
                    .on_press(msg(VaultMsg::ShowAll))
                    .into()
                });
                col = col.push(caps(sec.label(), trailing));
            }
            section = Some(*sec);
            let on = i == v.list.chosen;
            let (trailing, press) = match mode {
                Mode::Site => (
                    if on {
                        Trailing::Hint("Fill")
                    } else {
                        Trailing::Open(VaultMsg::Open(item.id.clone()))
                    },
                    VaultMsg::Fill(item.id.clone()),
                ),
                Mode::Search if on => (Trailing::Hint("Open"), VaultMsg::Open(item.id.clone())),
                _ if on => (Trailing::Chevron, VaultMsg::Open(item.id.clone())),
                _ => (Trailing::None, VaultMsg::Open(item.id.clone())),
            };
            col = col.push(item_row(item, on, trailing, Some(press)));
        }
        // Rows run under the scrollbar's lane only when there is one.
        let room = self.list_room(mode);
        let tall = super::super::rows::span(&listed, listed.len().saturating_sub(1)).1 > room;
        container(
            scroll::scrollable(col.padding(Padding {
                right: if tall { scroll::CLEARANCE } else { 0.0 },
                ..Padding::ZERO
            }))
            .id(id("list"))
            .on_scroll(|vp| {
                msg(VaultMsg::ListScrolled {
                    y: vp.absolute_offset().y,
                    h: vp.bounds().height,
                })
            })
            .width(Length::Fill),
        )
        .max_height(room)
        .into()
    }
}

impl<E: Engine> App<E> {
    /// What the sheet has left for the list: 560 less the head, foot,
    /// search, kind tabs and any notices above or below.
    fn list_room(&self, mode: Mode) -> f32 {
        let v = &self.vault;
        let mut room = LIST_MAX_H;
        if mode == Mode::Whole {
            room -= 32.0;
        }
        if v.status.as_ref().is_some_and(|s| s.sync_error.is_some()) {
            room -= 60.0;
        }
        if v.list.unreadable > 0 {
            room -= 44.0;
        }
        if v.error.is_some() {
            room -= 24.0;
        }
        room.max(3.0 * parts::ROW_H)
    }
}

fn kind_style(t: &Theme, status: button::Status, on: bool) -> button::Style {
    let bg = if on {
        Some(sola_kit::theme::selection())
    } else if matches!(status, button::Status::Hovered) {
        Some(sola_kit::components::style::mix_white(parts::face(t), 0.05))
    } else {
        None
    };
    button::Style {
        background: bg.map(Background::Color),
        text_color: ink(t),
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A failed sync, and what the list holds: the last sync's items, or none
/// (the vault never synced on this unlock and none were kept).
fn sync_notice(err: &str, has_items: bool) -> String {
    if has_items {
        format!("Couldn't reach Bitwarden ({err}). These are the items from the last sync.")
    } else {
        format!("Couldn't reach Bitwarden ({err}) — no items until it syncs.")
    }
}

#[cfg(test)]
mod tests {
    use super::sync_notice;

    #[test]
    fn a_failed_sync_never_promises_items_it_lacks() {
        assert!(sync_notice("offline", true).ends_with("These are the items from the last sync."));
        assert_eq!(
            sync_notice("offline", false),
            "Couldn't reach Bitwarden (offline) — no items until it syncs."
        );
    }
}
