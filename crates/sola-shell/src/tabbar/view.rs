//! The tab bar's view: a head with the showing workspace's name (its menu
//! switches workspaces, [`super::head`]), then a row per window (app icon,
//! title) in the workspace's bar order — one list, not grouped by zone.
//! Board 05's rows: the focused window is the pill; a tiled zone's shown
//! tab is lifted a step while its zone is on screen and bright while it is
//! not (Full while the halves are up, or the halves while Full is); the
//! tabs behind are muted. A tab that is working or waiting on a decision
//! (`Topic::TabActivity`) shows that mark in the icon's place, so the
//! title does not move. Hovering a row shows the titlebar's red stoplight:
//! it closes the window (`Topic::CloseWindow`). While an app starts from
//! the launcher, a ghost row (its name, faint, a turning ring) stands
//! where its window will land. A Projects window's row shows its parts
//! apart (`sola_core::window_label`): the project, the session in it,
//! quieter, and the computer it runs on as a chip in that computer's
//! prompt color.

use iced::widget::{row, text};
use iced::{Alignment, Color, Element, Theme};
use sola_bus::topics::{Activity, Zone};
use sola_core::host_color::{PROMPT_LIGHTNESS, PROMPT_SATURATION, host_hue, hsl_rgb};
use sola_core::window_label::WindowLabel;
use sola_kit::components::{
    LEADING_SLOT, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection, SidebarTag, icon,
    island, status_mark,
};

use super::row_id;
use crate::app::{Msg, Shell};

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let hidden = |app: &str| shell.is_app_hidden(app);
    let on_screen = shell.tabs.on_screen(&hidden);
    // A dialog's focus is its tab's.
    let focused = shell
        .focused_window_id
        .map(|w| shell.tabs.parent_of(w).unwrap_or(w));
    let mut items: Vec<SidebarItem<'_, Msg>> = shell
        .tabs
        .bar()
        .iter()
        .map(|t| {
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
            let activity = shell
                .pairing
                .key_of(t.window_id)
                .and_then(|key| shell.tab_activity.get(key))
                .copied();
            let leading: Element<'_, Msg> = match mark(activity) {
                Some(indicator) => status_mark(indicator),
                None => icon(app_icon(shell, &t.app_id), LEADING_SLOT as u16).into(),
            };
            // Every float is on screen; it is current while it has focus.
            let (current, showing) = if t.zone == Zone::Float {
                (shell.focused_window_id == Some(t.window_id), true)
            } else {
                let zone_shows = shell.tabs.visible_shown(t.zone, &hidden);
                (zone_shows == Some(t.window_id), on_screen.contains(&t.zone))
            };
            let has_focus = focused == Some(t.window_id);
            let mut item = SidebarItem::new(label, Msg::ShowTab(t.window_id))
                .id(row_id(t.window_id))
                .active(current && showing && has_focus)
                .shown(current && showing && !has_focus)
                .bright(current && !showing)
                .on_close(Msg::CloseWindow(t.window_id))
                .stoplight(true)
                .leading(leading);
            if let Some(p) = parts {
                if let Some(session) = p.session {
                    item = item.detail(session);
                }
                item = item.tag(host_tag(&p.host));
            }
            if hidden(&t.app_id) {
                item = item.secondary("hidden");
            }
            item
        })
        .collect();
    if let Some((at, label)) = shell.launch_ghost() {
        items.insert(at.min(items.len()), ghost_row(label));
    }

    let panel = SidebarPanel::new(vec![SidebarSection::unlabeled(items)])
        .controller(&shell.tabbar.gestures, Msg::TabBar)
        .reorderable()
        .fill_width()
        .header(super::head::view(shell))
        .build();
    // An island in the grid's left column (board 06): rounded, rimmed,
    // its corners cleared to the ground.
    island(panel)
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
