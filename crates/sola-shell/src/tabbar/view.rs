//! The tab bar's view: a head with the showing workspace's name (its menu
//! switches workspaces, [`super::head`]; none when the workspaces are
//! islands in the top row), then a row per window (app icon,
//! title) in the workspace's bar order — one list, not grouped by zone. The
//! windows of each app that opens a window per document, from its second on
//! the workspace, sit in that app's group, under its name in small capitals,
//! one step in; a click on its name folds it to that one line (its count,
//! and its loudest mark). Every loose row comes before the
//! groups (spec § App groups
//! and keys; `design/bar-groups/`, way A). Board 05's rows: the focused window is
//! the pill; a tiled zone's shown tab is lifted a step while its zone is
//! on screen and bright while it is not (Full while the halves are up, or
//! the halves while Full is); the tabs behind are muted. A tab that is
//! working or waiting on a decision (`Topic::TabActivity`) shows that mark
//! in the icon's place, so the title does not move. Hovering a row shows
//! the titlebar's red stoplight: it closes the window
//! (`Topic::CloseWindow`). While an app starts from the launcher, a ghost
//! row (its name, faint, a turning ring) stands where its window will
//! land — in its group when the group is on the bar. A Projects window's
//! row shows its parts apart (`sola_core::window_label`): the project, the
//! session in it, quieter, and the computer it runs on as a chip in that
//! computer's prompt color.

use iced::widget::{row, text};
use iced::{Alignment, Color, Element, Theme};
use sola_bus::topics::{Activity, Zone};
use sola_core::host_color::{PROMPT_LIGHTNESS, PROMPT_SATURATION, host_hue, hsl_rgb};
use sola_core::window_label::WindowLabel;
use sola_kit::components::{
    CaptionFold, LEADING_SLOT, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection, SidebarTag, icon,
    island, status_mark,
};

use super::{group_row_id, row_id};
use crate::app::{Msg, Shell};
use crate::stacks::{Block, Tab, grouped};

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let hidden = |app: &str| shell.is_app_hidden(app);
    let now = Now {
        on_screen: shell.tabs.on_screen(&hidden),
        // A dialog's focus is its tab's.
        focused: shell
            .focused_window_id
            .map(|w| shell.tabs.parent_of(w).unwrap_or(w)),
    };
    let ghost = shell.launch_ghost();
    // The ghost joins its group when the group is on the bar.
    let ghost_group = ghost.filter(|(_, _, app)| {
        grouped(app)
            && shell.tabs.blocks().iter().any(
                |b| matches!(b, Block::Group { app: a, tabs, .. } if a == app && !tabs.is_empty()),
            )
    });
    let mut ghost_loose = ghost.filter(|_| ghost_group.is_none());

    let mut sections: Vec<SidebarSection<'_, Msg>> = Vec::new();
    let mut loose: Vec<SidebarItem<'_, Msg>> = Vec::new();
    // Index in the bar of the next row.
    let mut at = 0;
    for block in shell.tabs.blocks() {
        let rows = block.tabs();
        // An empty group is a remembered place: nothing to draw.
        if rows.is_empty() {
            continue;
        }
        if let Some((_, label, _)) = ghost_loose.filter(|(g, ..)| *g <= at) {
            loose.push(ghost_row(label));
            ghost_loose = None;
        }
        match block {
            Block::Loose(t) => loose.push(tab_row(shell, &now, t, false)),
            Block::Group { app, tabs, .. } => {
                if !loose.is_empty() {
                    sections.push(SidebarSection::unlabeled(std::mem::take(&mut loose)));
                }
                let folded = shell.tabs.is_folded(app);
                let mut items: Vec<SidebarItem<'_, Msg>> = if folded {
                    Vec::new()
                } else {
                    tabs.iter().map(|t| tab_row(shell, &now, t, true)).collect()
                };
                if let Some((ghost_at, label, _)) = ghost_group.filter(|(_, _, g)| g == app) {
                    let i = ghost_at.saturating_sub(at).min(items.len());
                    items.insert(i, ghost_row(label));
                }
                sections.push(group_section(shell, &now, app, tabs, items));
            }
        }
        at += rows.len();
    }
    if let Some((_, label, _)) = ghost_loose {
        loose.push(ghost_row(label));
    }
    if !loose.is_empty() {
        sections.push(SidebarSection::unlabeled(loose));
    }

    let mut panel = SidebarPanel::new(sections)
        .controller(&shell.tabbar.gestures, Msg::TabBar)
        .reorder_within_sections()
        .loose_first()
        .fill_width();
    if !shell.workspace_islands() {
        panel = panel.header(super::head::view(shell));
    }
    let panel = panel.build();
    // An island in the grid's left column (board 06): rounded, rimmed,
    // its corners cleared to the ground.
    island(panel)
}

/// An app's group: its name as a caption, its rows one step in. A click
/// on the name folds it; folded, the caption stands for its rows: how many,
/// the loudest of their marks, and bright while one of them has focus.
/// Super+H says "hidden" here once, not on each row.
fn group_section<'a>(
    shell: &'a Shell,
    now: &Now,
    app: &str,
    tabs: &[Tab],
    items: Vec<SidebarItem<'a, Msg>>,
) -> SidebarSection<'a, Msg> {
    let activity: Vec<Activity> = tabs.iter().filter_map(|t| tab_activity(shell, t)).collect();
    let loudest = [Activity::Waiting, Activity::Working]
        .into_iter()
        .find(|a| activity.contains(a));
    let mut section = SidebarSection::new(app_label(shell, app), items)
        .id(group_row_id(shell.tabs.showing_id(), app))
        .caption()
        .foldable(CaptionFold {
            folded: shell.tabs.is_folded(app),
            count: tabs.len(),
            mark: mark(loudest),
            current: tabs.iter().any(|t| now.focused == Some(t.window_id)),
        });
    if shell.is_app_hidden(app) {
        section = section.note("hidden");
    }
    section
}

/// What every row reads: the zones on screen, the focused tab.
struct Now {
    on_screen: Vec<Zone>,
    focused: Option<u32>,
}

/// A window's row. A grouped one leaves "hidden" to its group's header.
fn tab_row<'a>(shell: &'a Shell, now: &Now, t: &Tab, in_group: bool) -> SidebarItem<'a, Msg> {
    let hidden = |app: &str| shell.is_app_hidden(app);
    let title = shell
        .known_windows
        .iter()
        .find(|w| w.window_id == t.window_id)
        .map(|w| w.title.trim())
        .filter(|s| !s.is_empty());
    let parts = title
        .filter(|_| t.app_id == crate::launcher::projects::APP_ID)
        .and_then(WindowLabel::parse);
    let label = match &parts {
        Some(p) => p.project.clone(),
        None => title
            .map(str::to_string)
            .unwrap_or_else(|| app_label(shell, &t.app_id)),
    };
    let key = shell.pairing.key_of(t.window_id);
    let activity = tab_activity(shell, t);
    // The tab's own icon (a page's favicon), else its app's.
    let face = key
        .and_then(|key| shell.tab_icons.get(key))
        .map_or_else(|| app_icon(shell, &t.app_id), String::as_str);
    let leading: Element<'_, Msg> = match mark(activity) {
        Some(indicator) => status_mark(indicator),
        None => icon(face, LEADING_SLOT as u16).into(),
    };
    // Every float is on screen; it is current while it has focus.
    let (current, showing) = if t.zone == Zone::Float {
        (shell.focused_window_id == Some(t.window_id), true)
    } else {
        let zone_shows = shell.tabs.visible_shown(t.zone, &hidden);
        (zone_shows == Some(t.window_id), now.on_screen.contains(&t.zone))
    };
    let has_focus = now.focused == Some(t.window_id);
    let mut item = SidebarItem::new(label, Msg::ShowTab(t.window_id))
        .id(row_id(shell.tabs.showing_id(), t.window_id))
        .active(current && showing && has_focus)
        .shown(current && showing && !has_focus)
        .bright(current && !showing)
        .stoplight(true)
        .leading(leading);
    item = item.on_close(Msg::CloseWindow(t.window_id));
    if shell.tabs.is_pinned(t.window_id) {
        item = item.pinned();
    }
    if let Some(p) = parts {
        if !p.is_here() {
            item = item.tag(host_tag(&p.host));
        }
        if let Some(session) = p.session {
            item = item.detail(session);
        }
    }
    if hidden(&t.app_id) && !in_group {
        item = item.secondary("hidden");
    }
    item
}

/// What a tab is doing, when it says.
fn tab_activity(shell: &Shell, t: &Tab) -> Option<Activity> {
    let key = shell.pairing.key_of(t.window_id)?;
    shell.tab_activity.get(key).copied()
}

/// Where a starting app's window will land: its name, faint, beside a
/// turning ring. Not a tab: no click, no close, no drag.
fn ghost_row(label: &str) -> SidebarItem<'_, Msg> {
    let body = row![
        status_mark(SidebarIndicator::Working),
        text(label.to_string())
            .size(13)
            .style(|theme: &Theme| text::Style {
                color: Some(Color {
                    a: 0.42,
                    ..theme.extended_palette().background.base.text
                }),
            }),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    SidebarItem::new(label, Msg::Noop).id(GHOST_ID).content(body)
}

/// The chip naming the computer a window runs on: its prompt's hue
/// (Oath's), ink lifted to read on the bar, a faint fill of the prompt
/// color itself.
fn host_tag(host: &str) -> SidebarTag {
    let hue = host_hue(host);
    let color = |l: f32| {
        let [r, g, b] = hsl_rgb(hue, PROMPT_SATURATION, l);
        Color::from_rgb8(r, g, b)
    };
    SidebarTag {
        label: host.to_string(),
        ink: color(0.72),
        fill: Color {
            a: 0.22,
            ..color(PROMPT_LIGHTNESS)
        },
    }
}

/// The ghost row's id (never a window's: those are `w<id>`).
const GHOST_ID: &str = "ghost";

/// The mark a row shows for its tab's activity; none when idle.
fn mark(activity: Option<Activity>) -> Option<SidebarIndicator> {
    match activity? {
        Activity::Working => Some(SidebarIndicator::Working),
        Activity::Waiting => Some(SidebarIndicator::Waiting),
        Activity::Idle => None,
    }
}

fn app_label(shell: &Shell, app_id: &str) -> String {
    shell
        .applications
        .get_for_window(app_id)
        .map(|a| a.label.clone())
        .unwrap_or_else(|| app_id.to_string())
}

fn app_icon<'a>(shell: &'a Shell, app_id: &str) -> &'a str {
    shell
        .applications
        .get_for_window(app_id)
        .map(|a| a.icon.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("lucide/box")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_work_and_decisions_replace_the_icon() {
        assert_eq!(mark(Some(Activity::Waiting)), Some(SidebarIndicator::Waiting));
        assert_eq!(mark(Some(Activity::Working)), Some(SidebarIndicator::Working));
        assert_eq!(mark(Some(Activity::Idle)), None);
        assert_eq!(mark(None), None);
    }
}
