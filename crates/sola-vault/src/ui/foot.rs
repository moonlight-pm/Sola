//! The foot: a status line — when the vault last synced (a check, or the
//! warning diamond when the last sync failed) and who is signed in. And
//! the toast that floats above it.

use iced::widget::{Space, button, container, row, text};
use iced::{Alignment, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use super::widgets;
use super::{App, Msg, Toast, style};
use crate::model::when;

const FOOT_H: f32 = 30.0;

impl App {
    pub(super) fn view_foot(&self) -> Element<'_, Msg> {
        let Some(s) = &self.status else {
            return Space::new().height(FOOT_H).into();
        };
        let today = when::today();
        let (mark, line): (Element<'_, Msg>, String) = if s.syncing {
            (widgets::faint("lucide/refresh-cw", 12.0), "Syncing…".into())
        } else if s.sync_error.is_some() {
            let last = s
                .synced_at
                .and_then(when::from_unix)
                .map(|at| format!("Last synced {}", when::last_synced(at, today)))
                .unwrap_or_else(|| "Not synced yet".into());
            (
                text("◆")
                    .size(11)
                    .style(|t: &Theme| text::Style {
                        color: Some(t.extended_palette().warning.base.color),
                    })
                    .into(),
                last,
            )
        } else {
            let line = s
                .synced_at
                .and_then(|at| {
                    when::from_unix(at)
                        .map(|local| when::synced(self.now.saturating_sub(at), local, today))
                })
                .unwrap_or_else(|| "Not synced yet".into());
            (widgets::faint("lucide/check", 12.0), line)
        };
        let sync = button(
            row![mark, text(line).font(fonts::ui()).size(11).style(secondary)]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center),
        )
        .padding(Padding::from([3.0, 6.0]))
        .style(style::tool)
        .on_press_maybe((!s.syncing).then_some(Msg::Sync));
        let who = text(s.email.clone())
            .font(fonts::ui())
            .size(11)
            .style(secondary);
        let lock = widgets::tool("lucide/lock", "Lock", Some(Msg::Lock));
        container(
            row![sync, Space::new().width(Length::Fill), who, lock]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center),
        )
        .height(Length::Fixed(FOOT_H))
        .padding(Padding {
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
            left: 8.0,
        })
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .style(style::raised)
        .into()
    }
}

/// The toast: a line and Dismiss.
pub fn toast(t: &Toast) -> Element<'_, Msg> {
    container(
        row![
            kit_text::body(t.text.as_str()),
            kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissToast),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([6.0, 6.0]).left(14))
    .style(style::toast)
    .into()
}

fn secondary(t: &Theme) -> text::Style {
    text::Style {
        color: Some(style::ink2(t)),
    }
}
