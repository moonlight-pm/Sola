use std::collections::{HashMap, HashSet};

use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuItem};
use sola_core::KeyChord;

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

    /// Return a de-duplicated list of key bindings handled by this cache.
    pub fn key_bindings(&self) -> Vec<KeyChord> {
        let mut set: HashSet<KeyChord> = HashSet::new();

        for chord in self.shortcuts.keys() {
            set.insert(chord.clone());
        }

        let mut out: Vec<KeyChord> = set.into_iter().collect();

        out.sort_by_key(|b| (b.keycode.raw(), b.meta, b.alt, b.ctrl, b.shift));
        out
    }
}
