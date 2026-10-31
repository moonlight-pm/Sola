//! The list column: a row per item — its plate, its name with marks for a
//! passkey and a code, the line under it — on the raised fill. A failed
//! sync says so at the top with Retry; items that would not open, at the
//! foot.

use iced::widget::text::Wrapping;
use iced::widget::{Column, Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::scroll;
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_vault_proto::ItemSummary;

use super::widgets::{self, PlateSize};
use super::{App, Msg, Section, style};
use crate::model::plate::plate;
use crate::model::when;

/// The list column's width.
pub const LIST_W: f32 = 300.0;

impl App {
    pub(super) fn view_list(&self) -> Element<'_, Msg> {
        let items = self.shown_items();
        let mut col = Column::new().spacing(2).padding(Padding {
            top: SPACE_SM,
            right: SPACE_SM + scroll::CLEARANCE - 4.0,
            bottom: SPACE_SM,
            left: SPACE_SM,
        });
        if let Some(warn) = self.sync_warning() {
            col = col.push(container(warn).padding(Padding::ZERO.bottom(SPACE_SM)));
        }
        if items.is_empty() {
            col = col.push(self.list_empty());
        }
        for item in &items {
            let selected = self.selected.as_deref() == Some(item.id.as_str());
            col = col.push(list_row(item, selected));
        }
        if self.unreadable > 0 && self.section == Section::All {
            let n = self.unreadable;
            let what = if n == 1 { "1 item" } else { &format!("{n} items") };
            col = col.push(
                container(
                    text(format!(
                        "{what} couldn't be opened — an organization key may not have loaded."
                    ))
                    .font(fonts::ui())
                    .size(12)
                    .style(|t: &Theme| text::Style {
                        color: Some(style::ink2(t)),
                    }),
                )
                .padding(Padding::from([SPACE_MD, 6.0])),
            );
        }
        container(scroll::scrollable(col).height(Length::Fill))
            .width(Length::Fixed(LIST_W))
            .height(Length::Fill)
            .style(style::raised)
            .into()
    }

    fn list_empty(&self) -> Element<'_, Msg> {
        let (title, line) = if !self.loaded {
            ("Opening the vault…", String::new())
        } else if !self.query.trim().is_empty() {
            ("Nothing matches", format!("“{}” in {}", self.query.trim(), self.section_title()))
        } else {
            match &self.section {
                Section::Trash => ("The trash is empty", String::new()),
                Section::Favorites => ("No favorites", "Star an item to keep it here.".into()),
                _ => ("Nothing here yet", "New Item adds one (⌘N).".into()),
            }
        };
        let mut c = column![text(title).font(fonts::ui_medium()).size(13)]
            .spacing(SPACE_XS + 2.0)
            .align_x(Alignment::Center);
        if !line.is_empty() {
            c = c.push(kit_text::caption(line).style(kit_text::muted));
        }
        container(c)
            .width(Length::Fill)
            .padding(Padding::from([28.0, 16.0]))
            .center_x(Length::Fill)
            .into()
    }

    /// "Couldn't reach Bitwarden. These are the items from 10:42." with Retry.
    fn sync_warning(&self) -> Option<Element<'_, Msg>> {
        let s = self.status.as_ref()?;
        let error = s.sync_error.as_deref()?;
        let from = s
            .synced_at
            .and_then(when::from_unix)
            .map(|at| format!(" These are the items from {}.", when::last_synced(at, when::today())))
            .unwrap_or_default();
        let line = format!("{}{from}", sentence(error));
        Some(
            container(
                row![
                    widgets::glyph("lucide/triangle-alert", 14.0, |t: &Theme| t
                        .extended_palette()
                        .warning
                        .base
                        .color),
                    text(line).font(fonts::ui()).size(12).width(Length::Fill),
                    kit_btn::labeled_sm("Retry", kit_btn::ghost)
                        .on_press_maybe((!s.syncing).then_some(Msg::Retry)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([7.0, 10.0]))
            .width(Length::Fill)
            .style(style::warn_box)
            .into(),
        )
    }
}

fn list_row<'a>(item: &'a ItemSummary, selected: bool) -> Element<'a, Msg> {
    let mut name = row![
        text(item.name.as_str())
            .font(fonts::ui_medium())
            .size(13)
            .wrapping(Wrapping::None)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if item.has_passkey {
        name = name.push(widgets::faint("lucide/fingerprint-pattern", 12.0));
    }
    if item.has_totp {
        name = name.push(widgets::faint("lucide/clock", 12.0));
    }
    let mut words = column![container(name).clip(true)].spacing(3).width(Length::Fill);
    if !item.subtitle.is_empty() {
        words = words.push(
            text(item.subtitle.as_str())
                .font(fonts::ui())
                .size(11.5)
                .wrapping(Wrapping::None)
                .style(|t: &Theme| text::Style {
                    color: Some(style::ink2(t)),
                }),
        );
    }
    let mut r = row![widgets::plate(&plate(item.kind, &item.name), PlateSize::Row), words]
        .spacing(10)
        .align_y(Alignment::Center);
    if item.favorite {
        r = r.push(widgets::faint("lucide/star", 12.0));
    } else {
        r = r.push(Space::new().width(0));
    }
    button(container(r).clip(true))
        .padding(Padding {
            top: 8.0,
            right: 8.0,
            bottom: 8.0,
            left: 6.0,
        })
        .width(Length::Fill)
        .style(style::list_row(selected))
        .on_press(Msg::Select(item.id.clone()))
        .into()
}

/// The daemon's message as a sentence (a full stop at its end).
fn sentence(s: &str) -> String {
    let t = s.trim();
    if t.ends_with(['.', '!', '?']) {
        t.to_string()
    } else {
        format!("{t}.")
    }
}

#[cfg(test)]
mod tests {
    use super::sentence;

    #[test]
    fn messages_end_as_sentences() {
        assert_eq!(sentence("Couldn't reach Bitwarden"), "Couldn't reach Bitwarden.");
        assert_eq!(sentence("Timed out."), "Timed out.");
    }
}
