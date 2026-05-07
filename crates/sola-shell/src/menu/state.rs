use std::collections::{HashMap, HashSet};

use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem};
use sola_core::{KeyChord, KeyCode};

/// Action id used by the synthesized "Quit <App>" item. Routed by the
/// menu-action handler to `Topic::CloseApp` for any external app that
/// hasn't shipped its own menu.
pub const SYNTHESIZED_CLOSE_ACTION: &str = "_close";

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
