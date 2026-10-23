//! The workspace switcher (Next slice 5, board 04): right of the flower,
//! a pill per workspace — its number and name, the showing one lifted —
//! with what needs you elsewhere: a diamond where a tab waits on a
//! decision, a dot where a Mail window has unread mail. Click shows a
//! workspace; click the showing one (or right-click any) for its menu.

use iced::widget::{container, mouse_area, row, text};
use iced::{Alignment, Color, Element, Length};
use sola_bus::topics::Activity;
use sola_kit::components::{SidebarIndicator, status_mark};
use sola_kit::fonts;

use super::view::{bar_button, menu_pad};
use super::{ICON_SIZE, MENU_PAD_H};
use crate::app::{Msg, Shell};

const SIZE: f32 = 13.0;
const GAP: f32 = 5.0;
const MARK: f32 = 8.0;

/// What a pill shows besides its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Waiting,
    Unread,
}

pub fn view(shell: &Shell) -> Vec<Element<'_, Msg>> {
    let fg = shell.theme.palette().text;
    let muted = Color { a: 0.55, ..fg };
    let accent = shell.theme.extended_palette().primary.base.color;
    let showing = shell.tabs.showing();
    shell
        .tabs
        .workspaces()
        .into_iter()
        .enumerate()
        .map(|(i, ws)| {
            let mut label = row![
                text(format!("{}", i + 1))
                    .font(fonts::chrome())
                    .size(SIZE)
                    .color(muted),
                text(ws.name).font(fonts::chrome()).size(SIZE),
            ]
            .spacing(GAP)
            .align_y(Alignment::Center);
            match mark(shell, i).filter(|_| i != showing) {
                Some(Mark::Waiting) => label = label.push(status_mark(SidebarIndicator::Waiting)),
                Some(Mark::Unread) => label = label.push(dot(accent)),
                None => {}
            }
            let open = if i == showing {
                Msg::WorkspaceMenu(i)
            } else {
                Msg::ShowWorkspace(i)
            };
            mouse_area(bar_button(label, i == showing, open, menu_pad()))
                .on_right_press(Msg::WorkspaceMenu(i))
                .into()
        })
        .collect()
}

/// Left edge of pill `index` in the menubar, for hanging its menu.
pub fn x_of(shell: &Shell, index: usize) -> f32 {
    let showing = shell.tabs.showing();
    let flower = ICON_SIZE as f32 + 2.0 * MENU_PAD_H;
    shell
        .tabs
        .workspaces()
        .iter()
        .take(index)
        .enumerate()
        .map(|(i, ws)| {
            let marked = i != showing && mark(shell, i).is_some();
            2.0 * MENU_PAD_H
                + fonts::line_width(&format!("{}", i + 1), SIZE, fonts::chrome())
                + GAP
                + fonts::line_width(&ws.name, SIZE, fonts::chrome())
                + if marked { GAP + MARK } else { 0.0 }
        })
        .sum::<f32>()
        + flower
}

/// What workspace `index` has waiting: a decision outranks unread mail.
fn mark(shell: &Shell, index: usize) -> Option<Mark> {
    let windows = shell.tabs.windows_on(index);
    let waiting = windows.iter().any(|w| {
        shell
            .pairing
            .key_of(*w)
            .and_then(|key| shell.tab_activity.get(key))
            == Some(&Activity::Waiting)
    });
    if waiting {
        return Some(Mark::Waiting);
    }
    let mail_here = windows.iter().any(|w| {
        shell
            .known_windows
            .iter()
            .any(|k| k.window_id == *w && k.app_id == "sola-mail")
    });
    (mail_here && shell.mail_unread_badge().is_some()).then_some(Mark::Unread)
}

fn dot(color: Color) -> Element<'static, Msg> {
    container(text(""))
        .width(Length::Fixed(6.0))
        .height(Length::Fixed(6.0))
        .style(move |_| container::Style {
            background: Some(color.into()),
            border: iced::Border {
                radius: 3.0.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        })
        .into()
}
