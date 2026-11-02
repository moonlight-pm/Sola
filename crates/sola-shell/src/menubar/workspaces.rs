//! The workspaces as islands in the top row, by the flower
//! (Settings → Appearance; `Topic::WorkspacePrefs`), in place of the
//! switcher at the head of the bar: each shows its number, its name, what
//! it has waiting, and a switch for whether Super+Tab stops there. Click one
//! to show it, the showing one for its menu; drag one to reorder.

use iced::widget::{container, row, text, toggler};
use iced::{Alignment, Color, Element, Length};
use sola_kit::components::form::toggle_style;
use sola_kit::fonts;

use super::island_row::IslandRow;
use super::view::island;
use super::{CHROME_SIZE, EDGE, ISLAND_GAP, ISLAND_H};
use crate::app::{Msg, Shell};
use crate::tabbar::head::{self, Mark};

/// A name's widest; past it, an ellipsis.
const NAME_MAX: f32 = 140.0;
const GAP: f32 = 6.0;
/// Either side of an island's content (the switch sits near its round end).
const PAD_H: f32 = 10.0;
/// The switch's height; it is twice as wide.
const SWITCH: f32 = 12.0;
/// A workspace not showing, its ink.
const QUIET_A: f32 = 0.62;

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
            if let Some(mark) = head::mark(shell, i) {
                body = body.push(head::mark_view(shell, mark));
            }
            body = body.push(
                toggler(!shell.workspace_skipped(&ws.id))
                    .size(SWITCH)
                    .on_toggle(move |_| Msg::WorkspaceCycle(i))
                    .style(toggle_style),
            );
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

/// The right edge of the flower and the workspace islands, by the same
/// metrics: where what follows them in the row starts.
pub fn row_end(shell: &Shell) -> f32 {
    let mut x = EDGE + ISLAND_H;
    if shell.workspace_islands() {
        for (i, ws) in shell.tabs.workspaces().iter().enumerate() {
            x += ISLAND_GAP + island_width(i, &ws.name, head::mark(shell, i));
        }
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
        + 2.0 * SWITCH;
    if let Some(mark) = mark {
        w += GAP + head::mark_width(mark);
    }
    w.ceil()
}
