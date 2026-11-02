//! The workspaces as islands in the top row, by the flower
//! (Settings → Appearance; `Topic::WorkspacePrefs`), in place of the
//! switcher at the head of the bar: each shows its number, its name, what
//! it has waiting (a workspace not showing), and a dot, lit when Super+Tab
//! stops there. Click one to show it, the showing one for its menu; drag one
//! to reorder. A line sets them apart from the islands that follow.

use iced::widget::{Space, button, container, row, text};
use iced::{Alignment, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::{SidebarIndicator, icon_colored, status_mark};
use sola_kit::fonts;

use super::island_row::IslandRow;
use super::view::island;
use super::{CHROME_SIZE, EDGE, ISLAND_GAP, ISLAND_H};
use crate::app::{Msg, Shell};
use crate::tabbar::head::{self, Mark};

/// A name's widest; past it, an ellipsis.
const NAME_MAX: f32 = 140.0;
const GAP: f32 = 6.0;
/// Either side of an island's content.
const PAD_H: f32 = 10.0;
/// The Super+Tab dot, and its button's room either side (its height is the
/// island's): a dot is a small target.
const DOT: f32 = 8.0;
const DOT_PAD_H: f32 = 4.0;
/// The dot unlit.
const DOT_OFF_A: f32 = 0.22;
/// What a workspace has waiting: the diamond, or a mail icon.
const MARK: f32 = 12.0;
/// A workspace not showing, its ink.
const QUIET_A: f32 = 0.62;
/// The line after the islands: its slot's width (a row's gap either side of
/// that), its height and its ink.
const LINE_SLOT: f32 = 9.0;
const LINE_H: f32 = 14.0;
const LINE_A: f32 = 0.38;

fn name_font() -> iced::Font {
    crate::agents::view::name_font()
}

/// The islands, when that is how the workspaces show.
pub fn islands(shell: &Shell) -> Option<Element<'_, Msg>> {
    if !shell.workspace_islands() {
        return None;
    }
    let showing = shell.tabs.showing();
    let fg = shell.theme.palette().text;
    let accent = shell.theme.extended_palette().primary.base.color;
    let islands = shell
        .tabs
        .workspaces()
        .into_iter()
        .enumerate()
        .map(|(i, ws)| {
            let here = i == showing;
            let quiet = Color { a: QUIET_A, ..fg };
            let name = crate::agents::view::fit(&ws.name, NAME_MAX);
            let mut body = row![
                text((i + 1).to_string())
                    .font(fonts::chrome())
                    .size(CHROME_SIZE)
                    .color(if here { accent } else { quiet }),
                text(name)
                    .font(name_font())
                    .size(CHROME_SIZE)
                    .color(if here { fg } else { quiet }),
            ]
            .spacing(GAP)
            .align_y(Alignment::Center);
            if let Some(mark) = waiting(shell, i) {
                body = body.push(mark_view(mark, accent));
            }
            body = body.push(cycle_dot(i, !shell.workspace_skipped(&ws.id), accent));
            let open = here && shell.workspace_menu_open();
            island(
                container(body).height(Length::Fill).align_y(Alignment::Center),
                here || open,
                PAD_H,
            )
        })
        .collect();
    Some(
        IslandRow::new(
            islands,
            ISLAND_GAP,
            |index, x| Msg::WorkspaceIslandClick { index, x },
            |from, to| Msg::WorkspaceIslandMove { from, to },
        )
        .into(),
    )
}

/// What workspace `index` has waiting, when it is not the one showing: the
/// showing one's bar says so itself.
fn waiting(shell: &Shell, index: usize) -> Option<Mark> {
    (index != shell.tabs.showing())
        .then(|| head::mark(shell, index))
        .flatten()
}

fn mark_view(mark: Mark, accent: Color) -> Element<'static, Msg> {
    match mark {
        Mark::Waiting => status_mark(SidebarIndicator::Waiting),
        Mark::Unread => icon_colored("lucide/mail", MARK as u16, accent).into(),
    }
}

/// The dot that says whether Super+Tab stops here; pressing it changes that
/// (and starts no drag).
fn cycle_dot(index: usize, lit: bool, accent: Color) -> Element<'static, Msg> {
    let dot = container(Space::new())
        .width(Length::Fixed(DOT))
        .height(Length::Fixed(DOT))
        .style(move |theme: &Theme| container::Style {
            background: Some(
                if lit {
                    accent
                } else {
                    Color {
                        a: DOT_OFF_A,
                        ..theme.palette().text
                    }
                }
                .into(),
            ),
            border: Border {
                radius: (DOT / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    button(container(dot).height(Length::Fill).align_y(Alignment::Center))
        .padding(Padding::from([0.0, DOT_PAD_H]))
        .height(Length::Fill)
        .style(|theme: &Theme, _| button::Style {
            background: None,
            text_color: theme.palette().text,
            border: Border::default(),
            shadow: Default::default(),
            snap: false,
        })
        .on_press(Msg::WorkspaceCycle(index))
        .into()
}

/// The line between the workspace islands and what follows them, when
/// something does.
pub fn separator(shell: &Shell) -> Option<Element<'_, Msg>> {
    let line = container(Space::new())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(LINE_H))
        .style(|theme: &Theme| container::Style {
            background: Some(
                Color {
                    a: LINE_A,
                    ..theme.palette().text
                }
                .into(),
            ),
            ..container::Style::default()
        });
    separates(shell).then(|| {
        container(line)
            .width(Length::Fixed(LINE_SLOT))
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    })
}

fn separates(shell: &Shell) -> bool {
    shell.workspace_islands() && !shell.agents.islands.is_empty()
}

/// The right edge of the flower, the workspace islands and their line, by
/// the same metrics: where what follows them in the row starts.
pub fn row_end(shell: &Shell) -> f32 {
    let mut x = EDGE + ISLAND_H;
    if shell.workspace_islands() {
        for (i, ws) in shell.tabs.workspaces().iter().enumerate() {
            x += ISLAND_GAP + island_width(i, &ws.name, waiting(shell, i));
        }
    }
    if separates(shell) {
        x += ISLAND_GAP + LINE_SLOT;
    }
    x
}

fn island_width(index: usize, name: &str, mark: Option<Mark>) -> f32 {
    let name = crate::agents::view::fit(name, NAME_MAX);
    let mut w = 2.0 * PAD_H
        + fonts::line_width(&(index + 1).to_string(), CHROME_SIZE, fonts::chrome())
        + GAP
        + fonts::line_width(&name, CHROME_SIZE, name_font())
        + GAP
        + DOT
        + 2.0 * DOT_PAD_H;
    if mark.is_some() {
        w += GAP + MARK;
    }
    w.ceil()
}
