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

/// Action id of the "Close Window" item: the shell closes the focused
/// window, as the tab bar's close does.
pub const SYNTHESIZED_CLOSE_WINDOW_ACTION: &str = sola_kit::menu::ACTION_CLOSE_WINDOW;

/// Its key, in an app's own menu or (for an app without a menu of its own)
/// grabbed while that app has focus.
pub const SYNTHESIZED_CLOSE_WINDOW_KEY: KeyChord = sola_kit::menu::CLOSE_WINDOW_KEY;

/// The flower's item that opens Settings.
pub const ACTION_SETTINGS: &str = "settings";

/// The flower's Lock Vault, while the vault is unlocked.
pub const ACTION_LOCK_VAULT: &str = "lock_vault";

/// The flower menu's rows now: Lock Vault after Lock Screen while the
/// vault is unlocked.
pub fn system_items(mut items: Vec<MenuItem>, vault_unlocked: bool) -> Vec<MenuItem> {
    if !vault_unlocked {
        return items;
    }
    let at = items
        .iter()
        .position(|i| matches!(i, MenuItem::Action { id, .. } if id == crate::lock::ACTION_LOCK))
        .map_or(items.len(), |i| i + 1);
    items.insert(at, action(ACTION_LOCK_VAULT, "Lock Vault", None));
    items
}

/// Flower / system menu (and the shell's own application menu when focused).
pub fn system_menu() -> MenuDefinition {
    MenuDefinition {
        label: "Shell".into(),
        items: vec![
            action("launch", "Launch Application…", None),
            action(ACTION_SETTINGS, "Settings…", None),
            action("shortcuts", "Keyboard Shortcuts", Some(KeyCode::K.meta())),
            MenuItem::Divider,
            action(crate::lock::ACTION_LOCK, "Lock Screen", Some(KeyCode::L.meta())),
            action("quit", "Exit Sola", None),
            MenuItem::Divider,
            action(crate::power::ACTION_SLEEP, "Sleep", None),
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

/// Give a Window menu the layout's grid zones and a Send to item per
/// workspace (`sola_kit::menu`), and every item its key now
/// (`crate::bindings`).
pub fn fill_window_menu(
    menu: &mut MenuDefinition,
    layout: &sola_bus::topics::ZoneLayout,
    workspaces: &sola_bus::topics::Workspaces,
    pinned: bool,
    keys: &crate::bindings::Bindings,
) {
    use crate::bindings::Act;
    use sola_kit::menu::{WindowAction, parse_window_action};
    sola_kit::menu::fill_window_menu(menu, layout);
    sola_kit::menu::fill_workspaces(menu, workspaces, pinned);
    for item in &mut menu.items {
        if let MenuItem::Action { id, shortcut, .. } = item {
            let act = match parse_window_action(id) {
                Some(WindowAction::Hide) => Act::Hide,
                Some(WindowAction::Cycle) => Act::NextTab,
                Some(WindowAction::Zone(z)) => Act::Zone(z),
                Some(WindowAction::SendToWorkspace(i)) => Act::SendToWorkspace(i),
                Some(WindowAction::Pin) | None => continue,
            };
            *shortcut = keys.key(act);
        }
    }
}

/// Build a default menu for an external app that hasn't shipped its own:
/// **Close Window** (Super+W) and **Quit *App*** (Super+Q, a system key),
/// under its name.
pub fn synthesized_menu(app_id: &str, label: &str) -> AppMenuPayload {
    AppMenuPayload {
        app_id: app_id.to_string(),
        menus: vec![MenuDefinition {
            label: label.to_string(),
            items: vec![
                action(
                    SYNTHESIZED_CLOSE_WINDOW_ACTION,
                    "Close Window",
                    Some(SYNTHESIZED_CLOSE_WINDOW_KEY),
                ),
                action(
                    SYNTHESIZED_CLOSE_ACTION,
                    &format!("Quit {label}"),
                    Some(sola_kit::menu::QUIT_KEY),
                ),
            ],
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
            .filter(|(_, action)| sola_kit::menu::parse_window_action(action).is_none())
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
    /// `app_id`'s own menu keys: not its Window menu's, which are system
    /// actions (`crate::bindings`).
    pub fn app_key_bindings(&self, app_id: &str) -> Vec<KeyChord> {
        let mut out: Vec<KeyChord> = self
            .shortcuts
            .iter()
            .filter(|(_, entries)| {
                entries.iter().any(|(id, action)| {
                    id == app_id && sola_kit::menu::parse_window_action(action).is_none()
                })
            })
            .map(|(chord, _)| *chord)
            .collect();
        out.sort_by_key(|b| (b.keycode.raw(), b.meta, b.alt, b.ctrl, b.shift));
        out
    }

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
                "Settings…",
                "Keyboard Shortcuts",
                "---",
                "Lock Screen",
                "Exit Sola",
                "---",
                "Sleep",
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
                ACTION_SETTINGS,
                "shortcuts",
                crate::lock::ACTION_LOCK,
                "quit",
                crate::power::ACTION_SLEEP,
                crate::power::ACTION_RESTART_COMPUTER,
                crate::power::ACTION_SHUT_DOWN,
            ]
        );
        // No key for session or machine power.
        for item in &menu.items {
            if let MenuItem::Action { id, shortcut, .. } = item {
                if id == "quit"
                    || id == crate::power::ACTION_SLEEP
                    || id == crate::power::ACTION_RESTART_COMPUTER
                    || id == crate::power::ACTION_SHUT_DOWN
                {
                    assert!(shortcut.is_none(), "{id} must have no key");
                }
            }
        }
    }

    #[test]
    fn lock_vault_shows_only_while_the_vault_is_unlocked() {
        let labels = |items: Vec<MenuItem>| -> Vec<String> {
            items
                .into_iter()
                .map(|item| match item {
                    MenuItem::Action { label, .. } => label,
                    MenuItem::Divider => "---".into(),
                })
                .collect()
        };
        assert_eq!(labels(system_items(system_menu().items, false)), system_menu_labels());
        let unlocked = labels(system_items(system_menu().items, true));
        let at = unlocked.iter().position(|l| l == "Lock Screen").unwrap();
        assert_eq!(unlocked[at + 1], "Lock Vault");
        assert_eq!(unlocked.len(), system_menu_labels().len() + 1);
    }

    #[test]
    fn synthesized_menu_has_close_window_and_quit() {
        let payload = synthesized_menu("firefox", "Firefox");
        assert_eq!(payload.app_id, "firefox");
        assert_eq!(payload.menus.len(), 1);
        let menu = &payload.menus[0];
        assert_eq!(menu.label, "Firefox");
        assert_eq!(menu.items.len(), 2);
        match &menu.items[0] {
            MenuItem::Action { id, label, shortcut, .. } => {
                assert_eq!(id, SYNTHESIZED_CLOSE_WINDOW_ACTION);
                assert_eq!(label, "Close Window");
                assert_eq!(*shortcut, Some(KeyCode::W.meta()));
            }
            other => panic!("expected Action item, got {other:?}"),
        }
        match &menu.items[1] {
            MenuItem::Action { id, label, shortcut, .. } => {
                assert_eq!(id, SYNTHESIZED_CLOSE_ACTION);
                assert_eq!(label, "Quit Firefox");
                assert_eq!(*shortcut, Some(KeyCode::Q.meta()));
            }
            other => panic!("expected Action item, got {other:?}"),
        }
    }

    #[test]
    fn menu_cache_lookup_shortcut_finds_registered_chord() {
        let mut cache = MenuCache::new();
        cache.set_menu(synthesized_menu("firefox", "Firefox"));
        let chord = KeyCode::W.meta();
        let action = cache.lookup_shortcut(&chord, "firefox").expect("its Close Window");
        assert_eq!(action.app_id, "firefox");
        assert_eq!(action.action_id, SYNTHESIZED_CLOSE_WINDOW_ACTION);
        let quit = cache.lookup_shortcut(&KeyCode::Q.meta(), "firefox").expect("its Quit");
        assert_eq!(quit.action_id, SYNTHESIZED_CLOSE_ACTION);
    }

    #[test]
    fn menu_cache_lookup_shortcut_misses_wrong_app() {
        let mut cache = MenuCache::new();
        cache.set_menu(synthesized_menu("firefox", "Firefox"));
        assert!(cache.lookup_shortcut(&KeyCode::W.meta(), "zed").is_none());
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
