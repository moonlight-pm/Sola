//! The tab bar's head: the showing workspace's name, a menu of the
//! workspaces (show one, New, Rename, Move, Delete). It took the place of
//! the top row's workspace pills. Right, what needs you on another
//! workspace: a diamond where a tab waits on a decision, a dot where a Mail
//! window has unread mail.

use iced::widget::{Space, button, container, row, text};
use iced::{Alignment, Border, Color, Element, Length, Padding, Theme};
use sola_bus::topics::Activity;
use sola_kit::components::icon::{icon_handle, icon_svg_colored};
use sola_kit::components::{SidebarIndicator, status_mark};
use sola_kit::fonts;

use crate::app::{Msg, Panel, Shell};

/// The head button's height, and the panel's pad above it (the kit
/// sidebar's header inset): the menu hangs from just under it.
pub const HEAD_H: f32 = 28.0;
const HEAD_TOP: f32 = 10.0;
const HEAD_LEFT: f32 = 10.0;
/// The name's inset in the button (where the rows' icons start).
const PAD_H: f32 = 8.0;
/// Hover and open fills (board 04's `--pill` for an open menu).
const HOVER_A: f32 = 0.07;
const OPEN_A: f32 = 0.14;
const RADIUS: f32 = 7.0;
const CHEVRON: u16 = 12;
const CHEVRON_A: f32 = 0.55;

/// What another workspace shows in the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Waiting,
    Unread,
}

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let fg = shell.theme.palette().text;
    let showing = shell.tabs.showing();
    let open = shell.menu_open
        && matches!(
            shell.open_panel,
            Some(Panel::Workspace(_) | Panel::RenameWorkspace)
        );
    let name = shell
        .tabs
        .workspaces()
        .get(showing)
        .map(|w| w.name.clone())
        .unwrap_or_default();
    let mut label = row![
        text(name)
            .font(iced::Font {
                weight: iced::font::Weight::Semibold,
                ..fonts::chrome()
            })
            .size(13),
        icon_svg_colored(
            icon_handle("lucide/chevron-down"),
            CHEVRON,
            Color { a: CHEVRON_A, ..fg }
        ),
        Space::new().width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    match elsewhere(shell) {
        Some(Mark::Waiting) => label = label.push(status_mark(SidebarIndicator::Waiting)),
        Some(Mark::Unread) => {
            label = label.push(dot(shell.theme.extended_palette().primary.base.color));
        }
        None => {}
    }
    button(container(label).height(Length::Fill).align_y(Alignment::Center))
        .padding(Padding::from([0.0, PAD_H]))
        .width(Length::Fill)
        .height(Length::Fixed(HEAD_H))
        .style(move |theme: &Theme, status| style(theme, status, open))
        .on_press(Msg::WorkspaceMenu(showing))
        .into()
}

/// The head's width: the workspace menu and the rename card take it.
pub const MENU_WIDTH: f32 = crate::zoning::BAR_WIDTH as f32 - 2.0 * HEAD_LEFT;

/// Where the workspace menu hangs, in output coordinates: the head's left
/// edge, just under it.
pub fn menu_anchor() -> (f32, f32) {
    let top = (crate::zoning::MENUBAR_HEIGHT + crate::zoning::GAP) as f32;
    (
        crate::zoning::GAP as f32 + HEAD_LEFT,
        top + HEAD_TOP + HEAD_H + 2.0,
    )
}

fn style(theme: &Theme, status: button::Status, open: bool) -> button::Style {
    let fg = theme.palette().text;
    let fill = if open {
        Some(OPEN_A)
    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Some(HOVER_A)
    } else {
        None
    };
    button::Style {
        background: fill.map(|a| Color { a, ..fg }.into()),
        text_color: fg,
        border: Border {
            radius: RADIUS.into(),
            ..Border::default()
        },
        shadow: Default::default(),
        snap: false,
    }
}

/// The strongest mark on a workspace not showing: a decision outranks
/// unread mail.
fn elsewhere(shell: &Shell) -> Option<Mark> {
    let showing = shell.tabs.showing();
    let marks: Vec<Mark> = (0..shell.tabs.workspace_count())
        .filter(|i| *i != showing)
        .filter_map(|i| mark(shell, i))
        .collect();
    if marks.contains(&Mark::Waiting) {
        Some(Mark::Waiting)
    } else {
        marks.first().copied()
    }
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
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}
