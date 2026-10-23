//! The tab bar's view: a head with the tab count, then a section per zone
//! (name, count) with a row per window (app icon, title). A tiled zone's
//! shown tab is the active row; the focused float is too. A tab that is
//! working or waiting on a decision (`Topic::TabActivity`) shows that mark
//! in the icon's place, so the title does not move.

use iced::widget::{container, text};
use iced::{Element, Length, Padding};
use sola_bus::topics::{Activity, Zone};
use sola_kit::components::style::SPACE_MD;
use sola_kit::components::{
    LEADING_SLOT, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection, icon, status_mark,
};
use sola_kit::fonts;

use super::{row_id, section_id, zone_label};
use crate::app::{Msg, Shell};
use crate::stacks::ZONES;

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let hidden = |app: &str| shell.is_app_hidden(app);
    let mut count = 0;
    let mut sections = Vec::new();
    for zone in ZONES {
        let tabs = shell.tabs.tabs(zone);
        count += tabs.len();
        let shown = match zone {
            Zone::Float => shell.focused_window_id,
            _ => shell.tabs.visible_shown(zone, &hidden),
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
                let mut item = SidebarItem::new(label, Msg::ShowTab(t.window_id))
                    .id(row_id(t.window_id))
                    .active(shown == Some(t.window_id))
                    .leading(leading);
                if hidden(&t.app_id) {
                    item = item.secondary("hidden");
                }
                item
            })
            .collect();
        sections.push(
            SidebarSection::new(format!("{} · {}", zone_label(zone), tabs.len()), items)
                .id(section_id(zone)),
        );
    }

    let noun = if count == 1 { "tab" } else { "tabs" };
    let head = container(text(format!("{count} {noun}")).font(fonts::chrome()).size(12))
        .padding(Padding::from([SPACE_MD, SPACE_MD]));
    let panel = SidebarPanel::new(sections)
        .controller(&shell.tabbar.gestures, Msg::TabBar)
        .reorderable()
        .fill_width()
        .header(head.into())
        .build();
    container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &iced::Theme| {
            container::Style::default()
                .background(theme.extended_palette().background.weak.color)
        })
        .into()
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
