//! Shortcut catalog + filter state for Super+K.

use sola_bus::topics::{AppMenuPayload, MenuItem, Zone};
use sola_core::{KeyChord, KeyCode};
use sola_kit::menu::{WINDOW_MENU_ENTRIES, WindowAction, WindowMenuEntry, parse_window_action};

#[derive(Clone, Debug)]
pub enum ShortcutAction {
    OpenLauncher,
    OpenSwitcher,
    OpenShortcuts,
    Hide,
    CloseApp,
    Cycle,
    ScreenshotFull,
    ScreenshotRegion,
    ScreenshotWindow,
    Zone(Zone),
    Menu { app_id: String, action_id: String },
}

#[derive(Clone, Debug)]
pub struct ShortcutRow {
    pub group: &'static str,
    pub label: String,
    pub chord: Option<KeyChord>,
    pub action: ShortcutAction,
}

#[derive(Default)]
pub struct ShortcutsState {
    pub active: bool,
    pub prior_focus: Option<u32>,
    pub query: String,
    pub rows: Vec<ShortcutRow>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl ShortcutsState {
    pub fn apply_query(&mut self, query: &str) {
        self.query = query.to_string();
        let q = query.trim().to_lowercase();
        self.filtered = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                if q.is_empty() {
                    return true;
                }
                row.label.to_lowercase().contains(&q)
                    || row.group.to_lowercase().contains(&q)
                    || row
                        .chord
                        .as_ref()
                        .is_some_and(|c| c.display().to_lowercase().contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }

    pub fn selected_row(&self) -> Option<&ShortcutRow> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.rows.get(i))
    }

    pub fn nav(&mut self, up: bool) {
        let len = self.filtered.len();
        if len == 0 {
            return;
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        } else {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }

    /// Rebuild from built-in chords + the focused app's published menus.
    pub fn rebuild(&mut self, focused: Option<&AppMenuPayload>) {
        self.rows = catalog(focused);
        self.apply_query(&self.query.clone());
    }
}

fn catalog(focused: Option<&AppMenuPayload>) -> Vec<ShortcutRow> {
    let mut rows = Vec::new();

    rows.extend(shell_rows());
    rows.extend(window_rows());

    if let Some(payload) = focused {
        for menu in &payload.menus {
            for item in &menu.items {
                let MenuItem::Action {
                    id,
                    label,
                    shortcut,
                    disabled,
                    ..
                } = item
                else {
                    continue;
                };
                if *disabled {
                    continue;
                }
                if parse_window_action(id).is_some() {
                    continue;
                }
                if omit_from_cheatsheet(id) {
                    continue;
                }
                let group = if payload.menus.first().is_some_and(|m| m.label == menu.label) {
                    "App"
                } else {
                    menu_group(&menu.label)
                };
                rows.push(ShortcutRow {
                    group,
                    label: label.clone(),
                    chord: *shortcut,
                    action: ShortcutAction::Menu {
                        app_id: payload.app_id.clone(),
                        action_id: id.clone(),
                    },
                });
            }
        }
    }

    rows
}

/// Session/machine power and synthesized close stay off Super+K so a
/// filter + Enter cannot reboot or quit.
fn omit_from_cheatsheet(id: &str) -> bool {
    id == "quit"
        || id == "_close"
        || id == crate::power::ACTION_RESTART_COMPUTER
        || id == crate::power::ACTION_SHUT_DOWN
}

fn menu_group(label: &str) -> &'static str {
    match label {
        "Edit" => "Edit",
        "File" => "File",
        "View" => "View",
        "Browser" => "Browser",
        "Profiles" => "Profiles",
        "Project" => "Project",
        "Workspaces" => "Workspaces",
        "Mail" => "Mail",
        "Terminal" => "Terminal",
        _ => "App",
    }
}

fn shell_rows() -> Vec<ShortcutRow> {
    vec![
        row(
            "Shell",
            "Launch Application",
            Some(KeyCode::SPACE.meta()),
            ShortcutAction::OpenLauncher,
        ),
        row(
            "Shell",
            "App Switcher",
            Some(KeyCode::TAB.meta()),
            ShortcutAction::OpenSwitcher,
        ),
        row(
            "Shell",
            "Keyboard Shortcuts",
            Some(KeyCode::K.meta()),
            ShortcutAction::OpenShortcuts,
        ),
        row(
            "Shell",
            "Hide App",
            Some(KeyCode::H.meta()),
            ShortcutAction::Hide,
        ),
        row(
            "Shell",
            "Close App",
            Some(KeyCode::Q.meta()),
            ShortcutAction::CloseApp,
        ),
        row(
            "Shell",
            "Cycle Windows",
            Some(KeyCode::GRAVE.meta()),
            ShortcutAction::Cycle,
        ),
        row(
            "Capture",
            "Screenshot Screen",
            Some(KeyCode::KEY_3.meta_shift()),
            ShortcutAction::ScreenshotFull,
        ),
        row(
            "Capture",
            "Screenshot Selection",
            Some(KeyCode::KEY_4.meta_shift()),
            ShortcutAction::ScreenshotRegion,
        ),
        row(
            "Capture",
            "Screenshot Window",
            Some(KeyCode::KEY_5.meta_shift()),
            ShortcutAction::ScreenshotWindow,
        ),
    ]
}

fn window_rows() -> Vec<ShortcutRow> {
    WINDOW_MENU_ENTRIES
        .iter()
        .filter_map(|entry| match entry {
            WindowMenuEntry::Item(i) => {
                let action = match i.action {
                    WindowAction::Hide | WindowAction::Cycle => return None,
                    WindowAction::Zone(z) => ShortcutAction::Zone(z),
                };
                Some(row("Window", i.label, Some(i.shortcut), action))
            }
            WindowMenuEntry::Divider => None,
        })
        .collect()
}

fn row(
    group: &'static str,
    label: &str,
    chord: Option<KeyChord>,
    action: ShortcutAction,
) -> ShortcutRow {
    ShortcutRow {
        group,
        label: label.to_string(),
        chord,
        action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::MenuDefinition;

    #[test]
    fn catalog_includes_super_k_and_zones() {
        let rows = catalog(None);
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::OpenShortcuts))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::Zone(Zone::Float)))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::Zone(Zone::Left)))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::OpenLauncher))
        );
    }

    #[test]
    fn filter_matches_label_and_skips_window_dupes_from_app_menu() {
        let payload = AppMenuPayload {
            app_id: "sola-terminal".into(),
            menus: vec![
                MenuDefinition {
                    label: "Terminal".into(),
                    items: vec![
                        MenuItem::Action {
                            id: "quit".into(),
                            label: "Quit Terminal".into(),
                            shortcut: Some(KeyCode::Q.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Action {
                            id: crate::power::ACTION_RESTART_COMPUTER.into(),
                            label: "Restart Computer".into(),
                            shortcut: None,
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Action {
                            id: crate::power::ACTION_SHUT_DOWN.into(),
                            label: "Shut Down".into(),
                            shortcut: None,
                            disabled: false,
                            checked: false,
                        },
                    ],
                },
                sola_kit::menu::window_menu(),
                MenuDefinition {
                    label: "Edit".into(),
                    items: vec![MenuItem::Action {
                        id: "copy".into(),
                        label: "Copy".into(),
                        shortcut: Some(KeyCode::C.meta()),
                        disabled: false,
                        checked: false,
                    }],
                },
            ],
        };
        let rows = catalog(Some(&payload));
        assert!(rows.iter().any(|r| r.label == "Copy" && r.group == "Edit"));
        assert!(!rows.iter().any(|r| r.label == "Quit Terminal"));
        assert!(!rows.iter().any(|r| r.label == "Restart Computer"));
        assert!(!rows.iter().any(|r| r.label == "Shut Down"));
        let float_hits = rows
            .iter()
            .filter(|r| matches!(r.action, ShortcutAction::Zone(Zone::Float)))
            .count();
        assert_eq!(float_hits, 1);
    }

    #[test]
    fn apply_query_filters_and_resets_selection() {
        let mut s = ShortcutsState::default();
        s.rebuild(None);
        s.selected = 3;
        s.apply_query("float");
        assert_eq!(s.filtered.len(), 1);
        assert_eq!(s.selected, 0);
        assert_eq!(s.selected_row().map(|r| r.label.as_str()), Some("Float"));
    }
}
