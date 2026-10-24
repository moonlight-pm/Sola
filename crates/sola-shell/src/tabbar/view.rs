//! The tab bar's view: a head with the showing workspace's name (its menu
//! switches workspaces, [`super::head`]), then a section per zone with a
//! row per window (app icon, title). Board 05's rows: the focused window
//! is the pill; a tiled zone's shown tab is lifted a step while its zone
//! is on screen and bright while it is not (Full while the halves are up,
//! or the halves while Full is); the tabs behind are muted. A tab that is
//! working or waiting on a decision (`Topic::TabActivity`) shows that mark
//! in the icon's place, so the title does not move. Hovering a row shows
//! the titlebar's red stoplight: it closes the window (`Topic::CloseWindow`).

use iced::Element;
use sola_bus::topics::{Activity, Zone};
use sola_kit::components::{
    LEADING_SLOT, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection, icon, island,
    status_mark,
};

use super::{row_id, section_id, zone_label};
use crate::app::{Msg, Shell};
use crate::stacks::ZONES;

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let hidden = |app: &str| shell.is_app_hidden(app);
    let on_screen = shell.tabs.on_screen(&hidden);
    // A dialog's focus is its tab's.
    let focused = shell
        .focused_window_id
        .map(|w| shell.tabs.parent_of(w).unwrap_or(w));
    let mut sections = Vec::new();
    for zone in ZONES {
        let tabs = shell.tabs.tabs(zone);
        let (shown, showing) = if zone == Zone::Float {
            (shell.focused_window_id, true)
        } else {
            (shell.tabs.visible_shown(zone, &hidden), on_screen.contains(&zone))
        };
        let items: Vec<SidebarItem<'_, Msg>> = tabs
            .iter()
            .map(|t| {
                let title = shell
                    .known_windows
                    .iter()
                    .find(|w| w.window_id == t.window_id)
                    .map(|w| w.title.trim())
                    .filter(|s| !s.is_empty());
                let label = title
                    .map(str::to_string)
                    .unwrap_or_else(|| app_label(shell, &t.app_id));
                let activity = shell
                    .pairing
                    .key_of(t.window_id)
                    .and_then(|key| shell.tab_activity.get(key))
                    .copied();
                let leading: Element<'_, Msg> = match mark(activity) {
                    Some(indicator) => status_mark(indicator),
                    None => icon(app_icon(shell, &t.app_id), LEADING_SLOT as u16).into(),
                };
                let current = shown == Some(t.window_id);
                let has_focus = focused == Some(t.window_id);
                let mut item = SidebarItem::new(label, Msg::ShowTab(t.window_id))
                    .id(row_id(t.window_id))
                    .active(current && showing && has_focus)
                    .shown(current && showing && !has_focus)
                    .bright(current && !showing)
                    .on_close(Msg::CloseWindow(t.window_id))
                    .stoplight(true)
                    .leading(leading);
                if hidden(&t.app_id) {
                    item = item.secondary("hidden");
                }
                item
            })
            .collect();
        sections.push(SidebarSection::new(zone_label(zone), items).id(section_id(zone)));
    }

    let panel = SidebarPanel::new(sections)
        .controller(&shell.tabbar.gestures, Msg::TabBar)
        .reorderable()
        .fill_width()
        .header(super::head::view(shell))
        .build();
    // An island in the grid's left column (board 06): rounded, rimmed,
    // its corners cleared to the ground.
    island(panel)
}

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
