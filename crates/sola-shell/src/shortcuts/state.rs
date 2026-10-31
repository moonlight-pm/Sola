//! Shortcut catalog + filter state for Super+K.

use sola_bus::topics::{AppMenuPayload, MenuItem};
use sola_core::KeyChord;
use sola_kit::menu::parse_window_action;

use crate::bindings::{Act, Bindings};

#[derive(Clone, Debug)]
pub enum ShortcutAction {
    /// A system action (`crate::bindings`).
    System(Act),
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

    /// Rebuild from the system's keys + the focused app's published menus.
    pub fn rebuild(&mut self, focused: Option<&AppMenuPayload>, keys: &Bindings) {
        self.rows = catalog(focused, keys);
        self.apply_query(&self.query.clone());
    }
}

fn catalog(focused: Option<&AppMenuPayload>, keys: &Bindings) -> Vec<ShortcutRow> {
    let mut rows: Vec<ShortcutRow> = keys
        .rows()
        .into_iter()
        .map(|(group, label, chord, act)| ShortcutRow {
            group,
            label,
            chord,
            action: ShortcutAction::System(act),
        })
        .collect();

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
        "Project" => "Project",
        "Projects" => "Projects",
        "Mail" => "Mail",
        "Terminal" => "Terminal",
        _ => "App",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::{MenuDefinition, Zone};
    use sola_core::KeyCode;

    #[test]
    fn catalog_includes_super_k_and_zones() {
        let rows = catalog(None, &Bindings::default());
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::System(Act::Shortcuts)))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::System(Act::Zone(Zone::Float))))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::System(Act::Zone(Zone::Left))))
        );
        assert!(
            rows.iter()
                .any(|r| matches!(r.action, ShortcutAction::System(Act::Launcher)))
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
        let rows = catalog(Some(&payload), &Bindings::default());
        assert!(rows.iter().any(|r| r.label == "Copy" && r.group == "Edit"));
        assert!(!rows.iter().any(|r| r.label == "Quit Terminal"));
        assert!(!rows.iter().any(|r| r.label == "Restart Computer"));
        assert!(!rows.iter().any(|r| r.label == "Shut Down"));
        let float_hits = rows
            .iter()
            .filter(|r| matches!(r.action, ShortcutAction::System(Act::Zone(Zone::Float))))
            .count();
        assert_eq!(float_hits, 1);
    }

    #[test]
    fn apply_query_filters_and_resets_selection() {
        let mut s = ShortcutsState::default();
        s.rebuild(None, &Bindings::default());
        s.selected = 3;
        s.apply_query("float");
        assert_eq!(s.filtered.len(), 1);
        assert_eq!(s.selected, 0);
        assert_eq!(s.selected_row().map(|r| r.label.as_str()), Some("Float"));
    }
}
