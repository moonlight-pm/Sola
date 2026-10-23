//! The workspace switcher (Next slice 5, boards 03–04): right of the
//! flower, one island with a pill per workspace — its number and name, the
//! showing one lifted — with what needs you elsewhere: a diamond where a
//! tab waits on a decision, a dot where a Mail window has unread mail.
//! Click shows a workspace; click the showing one (or right-click any) for
//! its menu.

use iced::widget::{button, container, mouse_area, row, text};
use iced::{Alignment, Border, Color, Element, Length, Padding, Theme};
use sola_bus::topics::Activity;
use sola_kit::components::{SidebarIndicator, status_mark};
use sola_kit::fonts;

use super::view::island;
use super::{CHROME_SIZE, EDGE, ISLAND_GAP, ISLAND_H};
use crate::app::{Msg, Shell};

/// The island's pad either side, and between pills.
const ISLAND_PAD: f32 = 3.0;
const PILL_GAP: f32 = 1.0;
/// A pill: 22 px, round ends, 9 px in.
const PILL_H: f32 = 22.0;
const PILL_PAD_H: f32 = 9.0;
/// Between a pill's number, name and mark.
const GAP: f32 = 6.0;
/// The number's size (quieter than the name).
const NUMBER_SIZE: f32 = 11.0;
const MARK: f32 = 8.0;
/// Ink: the name on a pill not showing, the number (board 04's ink-3 /
/// ink-4 on the island).
const NAME_A: f32 = 0.52;
const NUMBER_A: f32 = 0.34;
/// The showing pill's fill (board 04's `--pill`).
const PILL_A: f32 = 0.14;

/// What a pill shows besides its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Waiting,
    Unread,
}

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let fg = shell.theme.palette().text;
    let accent = shell.theme.extended_palette().primary.base.color;
    let showing = shell.tabs.showing();
    let open = matches!(
        shell.open_panel,
        Some(crate::app::Panel::Workspace(_) | crate::app::Panel::RenameWorkspace)
    ) && shell.menu_open;
    let pills: Vec<Element<'_, Msg>> = shell
        .tabs
        .workspaces()
        .into_iter()
        .enumerate()
        .map(|(i, ws)| {
            let on = i == showing;
            let name_ink = if on { fg } else { Color { a: NAME_A, ..fg } };
            let number_ink = Color {
                a: if on { NAME_A } else { NUMBER_A },
                ..fg
            };
            let mut label = row![
                text(format!("{}", i + 1))
                    .font(fonts::chrome())
                    .size(NUMBER_SIZE)
                    .color(number_ink),
                text(ws.name)
                    .font(fonts::chrome())
                    .size(CHROME_SIZE)
                    .color(name_ink),
            ]
            .spacing(GAP)
            .align_y(Alignment::Center);
            match mark(shell, i).filter(|_| !on) {
                Some(Mark::Waiting) => label = label.push(status_mark(SidebarIndicator::Waiting)),
                Some(Mark::Unread) => label = label.push(dot(accent)),
                None => {}
            }
            let press = if on {
                Msg::WorkspaceMenu(i)
            } else {
                Msg::ShowWorkspace(i)
            };
            let pill = button(
                container(label)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
            )
            .padding(Padding {
                top: 0.0,
                right: PILL_PAD_H,
                bottom: 0.0,
                left: PILL_PAD_H,
            })
            .height(Length::Fixed(PILL_H))
            .style(move |theme: &Theme, _status| pill_style(theme, on))
            .on_press(press);
            mouse_area(pill)
                .on_right_press(Msg::WorkspaceMenu(i))
                .into()
        })
        .collect();
    island(
        row(pills).spacing(PILL_GAP).align_y(Alignment::Center),
        open,
        ISLAND_PAD,
    )
}

fn pill_style(theme: &Theme, on: bool) -> button::Style {
    let fg = theme.palette().text;
    button::Style {
        background: on.then(|| Color { a: PILL_A, ..fg }.into()),
        text_color: fg,
        border: Border {
            radius: (PILL_H / 2.0).into(),
            ..Border::default()
        },
        shadow: Default::default(),
        snap: false,
    }
}

/// Left edge of pill `index` in the menubar, for hanging its menu.
pub fn x_of(shell: &Shell, index: usize) -> f32 {
    let showing = shell.tabs.showing();
    let before: f32 = shell
        .tabs
        .workspaces()
        .iter()
        .take(index)
        .enumerate()
        .map(|(i, ws)| {
            let marked = i != showing && mark(shell, i).is_some();
            2.0 * PILL_PAD_H
                + fonts::line_width(&format!("{}", i + 1), NUMBER_SIZE, fonts::chrome())
                + GAP
                + fonts::line_width(&ws.name, CHROME_SIZE, fonts::chrome())
                + if marked { GAP + MARK } else { 0.0 }
                + PILL_GAP
        })
        .sum();
    // The flower island, a gap, then this island's pad.
    EDGE + ISLAND_H + ISLAND_GAP + ISLAND_PAD + before
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
