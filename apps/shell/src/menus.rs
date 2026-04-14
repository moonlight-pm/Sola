use std::collections::HashMap;

use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuItem};

/// Cached app menus and shortcut reverse-lookup.
pub struct MenuCache {
    menus: HashMap<String, AppMenuPayload>,
    /// (keycode, shift_required) → (app_id, action_id)
    shortcuts: HashMap<(u32, bool), Vec<(String, String)>>,
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

    /// Look up a menu action for a key event on the focused app.
    pub fn lookup_shortcut(
        &self,
        code: u32,
        shift: bool,
        focused_app_id: &str,
    ) -> Option<MenuActionPayload> {
        let entries = self.shortcuts.get(&(code, shift))?;
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
                        if let Some(key) = parse_shortcut(shortcut) {
                            self.shortcuts
                                .entry(key)
                                .or_default()
                                .push((app_id.clone(), id.clone()));
                        }
                    }
                }
            }
        }
    }
}

/// Parse a shortcut string like "Super+T" or "Super+Shift+N" into (keycode, shift).
fn parse_shortcut(s: &str) -> Option<(u32, bool)> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return None;
    }

    let mut shift = false;
    let mut key_part = None;

    for part in &parts {
        match part.to_lowercase().as_str() {
            "super" => {}
            "shift" => shift = true,
            _ => key_part = Some(*part),
        }
    }

    let key = key_part?;
    let code = key_name_to_code(key)?;
    Some((code, shift))
}

/// Map a key name to XKB keycode (evdev + 8).
fn key_name_to_code(name: &str) -> Option<u32> {
    match name.to_uppercase().as_str() {
        "A" => Some(38),
        "B" => Some(56),
        "C" => Some(54),
        "D" => Some(40),
        "E" => Some(26),
        "F" => Some(41),
        "G" => Some(42),
        "H" => Some(43),
        "I" => Some(31),
        "J" => Some(44),
        "K" => Some(45),
        "L" => Some(46),
        "M" => Some(58),
        "N" => Some(57),
        "O" => Some(32),
        "P" => Some(33),
        "Q" => Some(24),
        "R" => Some(27),
        "S" => Some(39),
        "T" => Some(28),
        "U" => Some(30),
        "V" => Some(55),
        "W" => Some(25),
        "X" => Some(53),
        "Y" => Some(29),
        "Z" => Some(52),
        "1" => Some(10),
        "2" => Some(11),
        "3" => Some(12),
        "4" => Some(13),
        "5" => Some(14),
        "6" => Some(15),
        "7" => Some(16),
        "8" => Some(17),
        "9" => Some(18),
        "0" => Some(19),
        "TAB" => Some(23),
        "BACKSPACE" => Some(22),
        "RETURN" | "ENTER" => Some(36),
        "ESCAPE" | "ESC" => Some(9),
        _ => {
            tracing::debug!(key = name, "unknown key name in shortcut");
            None
        }
    }
}
