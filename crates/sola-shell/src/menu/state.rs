//! Menu cache and synthesized-menu construction.
//!
//! `MenuCache` stores per-app menus and provides shortcut reverse-lookup.
//! `synthesized_menu` builds a default "Quit <App>" menu for external apps
//! that haven't shipped their own.
//!
//! Window display logic (opening, rendering, closing) lands in Task 7.
use std::collections::{HashMap, HashSet};

use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

/// Action id used by the synthesized "Quit <App>" item. Routed by the
/// menu-action handler to `Topic::CloseApp` for any external app that
/// hasn't shipped its own menu.
pub const SYNTHESIZED_CLOSE_ACTION: &str = "_close";

/// Flower / system menu (and the shell's own application menu when focused).
pub fn system_menu() -> MenuDefinition {
    MenuDefinition {
        label: "Shell".into(),
        items: vec![
            action("launch", "Launch Application…", None),
            action("shortcuts", "Keyboard Shortcuts", Some(KeyCode::K.meta())),
            MenuItem::Divider,
            action("restart", "Restart Shell", None),
            MenuItem::Divider,
            action("quit", "Quit Sola", None),
            MenuItem::Divider,
            action(
                crate::power::ACTION_RESTART_COMPUTER,
                "Restart Computer",
                None,
            ),
            action(crate::power::ACTION_SHUT_DOWN, "Shut Down", None),
        ],
    }
}

fn action(id: &str, label: &str, shortcut: Option<KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}

/// Focused-app menus with the kit Window menu injected when missing.
pub fn effective_app_menu(cache: &MenuCache, app_id: &str, label: &str) -> AppMenuPayload {
    let base = cache
        .get_menu(app_id)
        .cloned()
        .unwrap_or_else(|| synthesized_menu(app_id, label));
    sola_kit::menu::ensure_window_menu(base)
}

/// Build a default menu for an external app that hasn't shipped its own.
/// Single menu labeled `<label>` containing one item: "Quit <label>" with
/// the Meta+Q shortcut shown next to it. The chord itself is already
/// dispatched globally in `keys::handle_chord`.
pub fn synthesized_menu(app_id: &str, label: &str) -> AppMenuPayload {
    AppMenuPayload {
        app_id: app_id.to_string(),
        menus: vec![MenuDefinition {
            label: label.to_string(),
            items: vec![MenuItem::Action {
                id: SYNTHESIZED_CLOSE_ACTION.to_string(),
                label: format!("Quit {label}"),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        }],
    }
}

/// Cached app menus and shortcut reverse-lookup.
pub struct MenuCache {
    menus: HashMap<String, AppMenuPayload>,
    /// key chord → (app_id, action_id)
    shortcuts: HashMap<KeyChord, Vec<(String, String)>>,
}

impl MenuCache {
    pub fn new() -> Self {
        Self {
            menus: HashMap::new(),
            shortcuts: HashMap::new(),
        }
    }

    pub fn set_menu(&mut self, payload: AppMenuPayload) {
        tracing::info!(app_id = %payload.app_id, menus = payload.menus.len(), "cached app menu");
        self.menus.insert(payload.app_id.clone(), payload);
        self.rebuild_shortcuts();
    }

    pub fn get_menu(&self, app_id: &str) -> Option<&AppMenuPayload> {
        self.menus.get(app_id)
    }

    /// Look up a menu action for a key chord on the focused app.
    pub fn lookup_shortcut(
        &self,
        chord: &KeyChord,
        focused_app_id: &str,
    ) -> Option<MenuActionPayload> {
        let entries = self.shortcuts.get(chord)?;
        entries
            .iter()
            .find(|(app_id, _)| app_id == focused_app_id)
            .map(|(app_id, action_id)| MenuActionPayload {
                app_id: app_id.clone(),
                action_id: action_id.clone(),
            })
    }

    fn rebuild_shortcuts(&mut self) {
        self.shortcuts.clear();
        for (app_id, payload) in &self.menus {
            for menu in &payload.menus {
                for item in &menu.items {
                    if let MenuItem::Action {
                        id,
                        shortcut: Some(shortcut),
                        ..
                    } = item
                    {
                        self.shortcuts
                            .entry(shortcut.clone())
                            .or_default()
                            .push((app_id.clone(), id.clone()));
                    }
                }
            }
        }
    }

    /// Return a de-duplicated list of key bindings owned by `app_id`.
    /// The shell uses this to register only the focused app's chords with
    /// River — registering globally causes a Sola app's shortcut (e.g.
    /// sola-browser's Meta+W) to be grabbed even when a non-Sola client
    /// like Zed is focused, swallowing the chord.
    pub fn key_bindings_for(&self, app_id: &str) -> Vec<KeyChord> {
        let mut set: HashSet<KeyChord> = HashSet::new();

        for (chord, entries) in &self.shortcuts {
            if entries.iter().any(|(id, _)| id == app_id) {
                set.insert(chord.clone());
            }
        }

        let mut out: Vec<KeyChord> = set.into_iter().collect();
        out.sort_by_key(|b| (b.keycode.raw(), b.meta, b.alt, b.ctrl, b.shift));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::MenuItem;

    fn system_menu_labels() -> Vec<String> {
        system_menu()
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action { label, .. } => label.clone(),
                MenuItem::Divider => "---".into(),
            })
            .collect()
    }

    #[test]
    fn system_menu_has_session_then_machine_power() {
        assert_eq!(
            system_menu_labels(),
            [
                "Launch Application…",
                "Keyboard Shortcuts",
                "---",
                "Restart Shell",
                "---",
                "Quit Sola",
                "---",
                "Restart Computer",
                "Shut Down",
            ]
        );
        let menu = system_menu();
        let ids: Vec<&str> = menu
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            [
                "launch",
                "shortcuts",
                "restart",
                "quit",
                crate::power::ACTION_RESTART_COMPUTER,
                crate::power::ACTION_SHUT_DOWN,
            ]
        );
        // Super+Q is CloseApp, not session or machine power.
        for item in &menu.items {
            if let MenuItem::Action { id, shortcut, .. } = item {
                if id == "quit"
                    || id == crate::power::ACTION_RESTART_COMPUTER
                    || id == crate::power::ACTION_SHUT_DOWN
                {
                    assert!(shortcut.is_none(), "{id} must not steal Super+Q");
                }
            }
        }
    }

    #[test]
    fn synthesized_menu_has_single_quit_item() {
        let payload = synthesized_menu("firefox", "Firefox");
        assert_eq!(payload.app_id, "firefox");
        assert_eq!(payload.menus.len(), 1);
        let menu = &payload.menus[0];
        assert_eq!(menu.label, "Firefox");
        assert_eq!(menu.items.len(), 1);
        match &menu.items[0] {
            MenuItem::Action {
                id,
                label,
                shortcut,
                disabled,
                checked,
            } => {
                assert_eq!(id, SYNTHESIZED_CLOSE_ACTION);
                assert_eq!(label, "Quit Firefox");
                assert!(!disabled);
                assert!(!checked);
                let chord = shortcut.as_ref().expect("quit item must have shortcut");
                assert_eq!(chord.keycode, KeyCode::Q);
                assert!(chord.meta);
                assert!(!chord.alt);
                assert!(!chord.ctrl);
                assert!(!chord.shift);
            }
            other => panic!("expected Action item, got {other:?}"),
        }
    }

    #[test]
    fn synthesized_menu_label_in_quit_string() {
        // "Quit <label>" must use the provided label, not the app_id.
        let payload = synthesized_menu("sola-browser", "Sola Browser");
        let label = match &payload.menus[0].items[0] {
            MenuItem::Action { label, .. } => label.clone(),
            _ => panic!("expected Action"),
        };
        assert_eq!(label, "Quit Sola Browser");
    }

    #[test]
    fn menu_cache_lookup_shortcut_finds_registered_chord() {
        let mut cache = MenuCache::new();
        cache.set_menu(synthesized_menu("firefox", "Firefox"));
        let chord = KeyCode::Q.meta();
        let hit = cache.lookup_shortcut(&chord, "firefox");
        assert!(hit.is_some());
        let action = hit.unwrap();
        assert_eq!(action.app_id, "firefox");
        assert_eq!(action.action_id, SYNTHESIZED_CLOSE_ACTION);
    }

    #[test]
    fn menu_cache_lookup_shortcut_misses_wrong_app() {
        let mut cache = MenuCache::new();
        cache.set_menu(synthesized_menu("firefox", "Firefox"));
        let chord = KeyCode::Q.meta();
        assert!(cache.lookup_shortcut(&chord, "zed").is_none());
    }

    #[test]
    fn effective_app_menu_appends_window() {
        let cache = MenuCache::new();
        let payload = effective_app_menu(&cache, "firefox", "Firefox");
        assert_eq!(payload.menus[0].label, "Firefox");
        assert_eq!(payload.menus[1].label, sola_kit::WINDOW_MENU_LABEL);
    }

    #[test]
    fn menu_cache_key_bindings_for_returns_deduplicated_sorted() {
        let mut cache = MenuCache::new();
        cache.set_menu(synthesized_menu("firefox", "Firefox"));
        let bindings = cache.key_bindings_for("firefox");
        assert!(!bindings.is_empty());
        // All bindings belong to firefox
        for b in &bindings {
            assert!(cache.lookup_shortcut(b, "firefox").is_some());
        }
    }
}
