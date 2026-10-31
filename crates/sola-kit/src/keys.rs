//! Keybindings (`docs/specs/2026-10-30-keybindings-design.md`): what key
//! every action has now, and who may share a key.
//!
//! Owners declare their actions with defaults (`Topic::KeyActions`); your
//! changes are overrides (`Topic::Keybindings`). [`resolve`] puts them
//! together: your choice wins over a default, the system over an app, and
//! an action whose key is held by one it clashes with has none.
//!
//! Two actions clash when one key could reach both: the same owner, or
//! either is the system's, or one is the kit's shared ("common") and the
//! other an app's. Two different apps never clash — only the focused one
//! gets the key.

use std::collections::HashMap;
use std::sync::RwLock;

use iced::keyboard::Modifiers;
use iced::keyboard::key::{Code, Physical};
use sola_bus::Message;
use sola_bus::topics::{AppMenuPayload, KeyAction, KeyActions, KeyOverride, Keybindings, MenuItem, Topic};
use sola_core::{KeyChord, KeyCode};

/// The shell's actions (zones included).
pub const SYSTEM: &str = "system";
/// The kit's shared actions, in every app.
pub const COMMON: &str = "common";

/// One action and its key now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub owner: String,
    pub action: String,
    pub label: String,
    pub group: String,
    pub default: Option<KeyChord>,
    /// The key it has now: none when it has none, or lost it.
    pub key: Option<KeyChord>,
    /// You changed it (an override).
    pub changed: bool,
    /// The action holding the key this one wanted: (owner, action).
    pub lost_to: Option<(String, String)>,
}

/// Whether one key could reach actions of both owners.
pub fn clash(a: &str, b: &str) -> bool {
    a == b || a == SYSTEM || b == SYSTEM || a == COMMON || b == COMMON
}

fn rank(owner: &str) -> u8 {
    match owner {
        SYSTEM => 0,
        COMMON => 1,
        _ => 2,
    }
}

/// Every declared action with its key now, in declaration order.
pub fn resolve(decls: &[KeyActions], keys: &Keybindings) -> Vec<Binding> {
    let mut all: Vec<Binding> = decls
        .iter()
        .flat_map(|d| {
            d.actions.iter().map(|a| {
                let over = keys
                    .overrides
                    .iter()
                    .find(|o| o.owner == d.owner && o.action == a.id);
                Binding {
                    owner: d.owner.clone(),
                    action: a.id.clone(),
                    label: a.label.clone(),
                    group: a.group.clone(),
                    default: a.default,
                    key: over.map_or(a.default, |o| o.key),
                    changed: over.is_some(),
                    lost_to: None,
                }
            })
        })
        .collect();
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|&i| (!all[i].changed, rank(&all[i].owner)));
    let mut held: Vec<usize> = Vec::new();
    for i in order {
        let Some(key) = all[i].key else { continue };
        let holder = held
            .iter()
            .copied()
            .find(|&j| all[j].key == Some(key) && clash(&all[i].owner, &all[j].owner));
        match holder {
            Some(j) => {
                all[i].lost_to = Some((all[j].owner.clone(), all[j].action.clone()));
                all[i].key = None;
            }
            None => held.push(i),
        }
    }
    all
}

/// The action that holds `key` where `owner`'s action would clash with
/// it, other than `owner`'s `action` itself.
pub fn holder<'a>(
    bindings: &'a [Binding],
    owner: &str,
    action: &str,
    key: KeyChord,
) -> Option<&'a Binding> {
    bindings.iter().find(|b| {
        b.key == Some(key) && clash(owner, &b.owner) && !(b.owner == owner && b.action == action)
    })
}

/// `owner`'s `action`'s key now.
pub fn key_of(bindings: &[Binding], owner: &str, action: &str) -> Option<KeyChord> {
    bindings
        .iter()
        .find(|b| b.owner == owner && b.action == action)
        .and_then(|b| b.key)
}

/// Give `owner`'s `action` `key` (none: no key). Back to its default
/// drops the override.
pub fn set(
    keys: &mut Keybindings,
    owner: &str,
    action: &str,
    default: Option<KeyChord>,
    key: Option<KeyChord>,
) {
    reset(keys, owner, action);
    if key != default {
        keys.overrides.push(KeyOverride {
            owner: owner.to_string(),
            action: action.to_string(),
            key,
        });
    }
}

/// Back to the default.
pub fn reset(keys: &mut Keybindings, owner: &str, action: &str) {
    keys.overrides
        .retain(|o| !(o.owner == owner && o.action == action));
}

/// The kit's shared actions ("Every app" in Settings): one key for the
/// same menu item id in every app. Quit is not among them: it is the
/// system's Close App (Super+Q), whose key every Quit item shows.
pub const COMMON_ACTIONS: &[(&str, &str, &str, KeyChord)] = &[
    ("undo", "Undo", "Edit", KeyCode::Z.meta()),
    ("redo", "Redo", "Edit", KeyCode::Z.meta_shift()),
    ("cut", "Cut", "Edit", KeyCode::X.meta()),
    ("copy", "Copy", "Edit", KeyCode::C.meta()),
    ("paste", "Paste", "Edit", KeyCode::V.meta()),
    ("select_all", "Select All", "Edit", KeyCode::A.meta()),
    ("find", "Find", "Edit", KeyCode::F.meta()),
];

/// The shared actions' declaration (`Topic::KeyActions`, owner `common`).
pub fn common_declaration() -> KeyActions {
    KeyActions {
        owner: COMMON.to_string(),
        label: "Every App".to_string(),
        actions: COMMON_ACTIONS
            .iter()
            .map(|(id, label, group, key)| KeyAction {
                id: (*id).to_string(),
                label: (*label).to_string(),
                group: (*group).to_string(),
                default: Some(*key),
            })
            .collect(),
    }
}

/// The system action a menu item is, if any: the Window menu's items, and
/// Quit (Close App).
fn system_id(item: &str) -> Option<String> {
    use crate::menu::{WindowAction, parse_window_action};
    if item == crate::app::QUIT_ACTION_ID {
        return Some("close-app".into());
    }
    Some(match parse_window_action(item)? {
        WindowAction::Hide => "hide".into(),
        WindowAction::Cycle => "tab.next".into(),
        WindowAction::Zone(sola_bus::topics::Zone::Fullscreen) => "zone.full".into(),
        WindowAction::Zone(sola_bus::topics::Zone::Cinema) => "zone.cinema".into(),
        WindowAction::Zone(sola_bus::topics::Zone::Float) => "zone.float".into(),
        WindowAction::Zone(sola_bus::topics::Zone::Grid(id)) => format!("zone.{id}"),
    })
}

/// The shell's app id: its own menus (the flower) are system actions.
const SHELL_APP_ID: &str = "sola-shell";

/// Who owns a menu item of `app_id`'s: the system, the kit, or the app.
fn owner_of<'a>(app_id: &'a str, item: &str) -> (std::borrow::Cow<'a, str>, String) {
    if app_id == SHELL_APP_ID {
        return (SYSTEM.into(), item.to_string());
    }
    if let Some(id) = system_id(item) {
        return (SYSTEM.into(), id);
    }
    if COMMON_ACTIONS.iter().any(|(id, ..)| *id == item) {
        return (COMMON.into(), item.to_string());
    }
    (app_id.into(), item.to_string())
}

/// An app's own actions, from its menus: every item with a default key
/// that is neither the system's nor a shared one.
pub fn declaration(payload: &AppMenuPayload) -> KeyActions {
    let mut actions: Vec<KeyAction> = Vec::new();
    for menu in &payload.menus {
        for item in &menu.items {
            let MenuItem::Action {
                id,
                label,
                shortcut: Some(key),
                ..
            } = item
            else {
                continue;
            };
            let (owner, _) = owner_of(&payload.app_id, id);
            if owner != payload.app_id.as_str() || actions.iter().any(|a| a.id == *id) {
                continue;
            }
            actions.push(KeyAction {
                id: id.clone(),
                label: label.clone(),
                group: menu.label.clone(),
                default: Some(*key),
            });
        }
    }
    KeyActions {
        owner: payload.app_id.clone(),
        label: payload
            .menus
            .first()
            .map_or_else(|| payload.app_id.clone(), |m| m.label.clone()),
        actions,
    }
}

/// What this process knows of everyone's actions and your overrides,
/// from the bus ([`observe`]).
#[derive(Default)]
struct Known {
    decls: HashMap<String, KeyActions>,
    overrides: Keybindings,
}

static KNOWN: RwLock<Option<Known>> = RwLock::new(None);

/// Learn keys from a bus message (the kit's poller shows it every one).
/// Returns whether any key may have changed.
pub(crate) fn observe(message: &Message) -> bool {
    // By name first: every message passes here.
    if !matches!(message.topic.as_str(), "KeyActions" | "Keybindings") {
        return false;
    }
    let topic = match Topic::parse(message) {
        Some(t @ (Topic::KeyActions(_) | Topic::Keybindings(_))) => t,
        _ => return false,
    };
    let mut known = KNOWN.write().unwrap_or_else(|p| p.into_inner());
    let known = known.get_or_insert_with(Known::default);
    match topic {
        Topic::KeyActions(d) if message.sticky => {
            if known.decls.get(&d.owner) == Some(&d) {
                return false;
            }
            known.decls.insert(d.owner.clone(), d);
        }
        Topic::KeyActions(d) => {
            known.decls.remove(&d.owner);
        }
        Topic::Keybindings(k) => {
            let k = if message.sticky { k } else { Keybindings::default() };
            if known.overrides == k {
                return false;
            }
            known.overrides = k;
        }
        _ => return false,
    }
    true
}

/// `payload`'s menus with every item's key now: your changes, and none
/// where another action holds it (the system wins over an app).
pub fn apply(payload: &AppMenuPayload) -> AppMenuPayload {
    let known = KNOWN.read().unwrap_or_else(|p| p.into_inner());
    let empty = Known::default();
    let known = known.as_ref().unwrap_or(&empty);
    let mut decls: Vec<KeyActions> = known
        .decls
        .values()
        .filter(|d| d.owner == SYSTEM)
        .cloned()
        .collect();
    decls.push(common_declaration());
    decls.push(declaration(payload));
    let bindings = resolve(&decls, &known.overrides);
    let has_system = decls.iter().any(|d| d.owner == SYSTEM);
    let mut out = payload.clone();
    for menu in &mut out.menus {
        for item in &mut menu.items {
            let MenuItem::Action { id, shortcut, .. } = item else {
                continue;
            };
            let (owner, action) = owner_of(&payload.app_id, id);
            // Before the shell's keys are known, a system item keeps its own.
            if owner == SYSTEM && !has_system {
                continue;
            }
            if bindings.iter().any(|b| b.owner == owner && b.action == action) {
                *shortcut = key_of(&bindings, &owner, &action);
            }
        }
    }
    out
}

/// A key pressed in an iced window, as a binding's key. `None` for a
/// modifier alone, or a key outside `sola_core::keys::KEYS`.
pub fn chord_from_iced(physical: &Physical, modifiers: Modifiers) -> Option<KeyChord> {
    let Physical::Code(code) = physical else {
        return None;
    };
    Some(KeyChord {
        keycode: keycode_of(*code)?,
        meta: modifiers.logo(),
        alt: modifiers.alt(),
        ctrl: modifiers.control(),
        shift: modifiers.shift(),
    })
}

fn keycode_of(code: Code) -> Option<KeyCode> {
    Some(match code {
        Code::Escape => KeyCode::ESCAPE,
        Code::Backspace => KeyCode::BACKSPACE,
        Code::Tab => KeyCode::TAB,
        Code::Enter => KeyCode::ENTER,
        Code::Space => KeyCode::SPACE,
        Code::ArrowLeft => KeyCode::LEFT,
        Code::ArrowUp => KeyCode::UP,
        Code::ArrowRight => KeyCode::RIGHT,
        Code::ArrowDown => KeyCode::DOWN,
        Code::Home => KeyCode::HOME,
        Code::End => KeyCode::END,
        Code::PageUp => KeyCode::PAGE_UP,
        Code::PageDown => KeyCode::PAGE_DOWN,
        Code::Insert => KeyCode::INSERT,
        Code::Delete => KeyCode::DELETE,
        Code::PrintScreen => KeyCode::PRINT,
        Code::Digit1 => KeyCode::KEY_1,
        Code::Digit2 => KeyCode::KEY_2,
        Code::Digit3 => KeyCode::KEY_3,
        Code::Digit4 => KeyCode::KEY_4,
        Code::Digit5 => KeyCode::KEY_5,
        Code::Digit6 => KeyCode::KEY_6,
        Code::Digit7 => KeyCode::KEY_7,
        Code::Digit8 => KeyCode::KEY_8,
        Code::Digit9 => KeyCode::KEY_9,
        Code::Digit0 => KeyCode::KEY_0,
        Code::Minus => KeyCode::MINUS,
        Code::Equal => KeyCode::EQUAL,
        Code::Backquote => KeyCode::GRAVE,
        Code::BracketLeft => KeyCode::BRACKET_LEFT,
        Code::BracketRight => KeyCode::BRACKET_RIGHT,
        Code::Semicolon => KeyCode::SEMICOLON,
        Code::Quote => KeyCode::APOSTROPHE,
        Code::Backslash => KeyCode::BACKSLASH,
        Code::Comma => KeyCode::COMMA,
        Code::Period => KeyCode::PERIOD,
        Code::Slash => KeyCode::SLASH,
        Code::KeyA => KeyCode::A,
        Code::KeyB => KeyCode::B,
        Code::KeyC => KeyCode::C,
        Code::KeyD => KeyCode::D,
        Code::KeyE => KeyCode::E,
        Code::KeyF => KeyCode::F,
        Code::KeyG => KeyCode::G,
        Code::KeyH => KeyCode::H,
        Code::KeyI => KeyCode::I,
        Code::KeyJ => KeyCode::J,
        Code::KeyK => KeyCode::K,
        Code::KeyL => KeyCode::L,
        Code::KeyM => KeyCode::M,
        Code::KeyN => KeyCode::N,
        Code::KeyO => KeyCode::O,
        Code::KeyP => KeyCode::P,
        Code::KeyQ => KeyCode::Q,
        Code::KeyR => KeyCode::R,
        Code::KeyS => KeyCode::S,
        Code::KeyT => KeyCode::T,
        Code::KeyU => KeyCode::U,
        Code::KeyV => KeyCode::V,
        Code::KeyW => KeyCode::W,
        Code::KeyX => KeyCode::X,
        Code::KeyY => KeyCode::Y,
        Code::KeyZ => KeyCode::Z,
        Code::F1 => KeyCode::F1,
        Code::F2 => KeyCode::F2,
        Code::F3 => KeyCode::F3,
        Code::F4 => KeyCode::F4,
        Code::F5 => KeyCode::F5,
        Code::F6 => KeyCode::F6,
        Code::F7 => KeyCode::F7,
        Code::F8 => KeyCode::F8,
        Code::F9 => KeyCode::F9,
        Code::F10 => KeyCode::F10,
        Code::F11 => KeyCode::F11,
        Code::F12 => KeyCode::F12,
        Code::Numpad0 => KeyCode::KP_0,
        Code::Numpad1 => KeyCode::KP_1,
        Code::Numpad2 => KeyCode::KP_2,
        Code::Numpad3 => KeyCode::KP_3,
        Code::Numpad4 => KeyCode::KP_4,
        Code::Numpad5 => KeyCode::KP_5,
        Code::Numpad6 => KeyCode::KP_6,
        Code::Numpad7 => KeyCode::KP_7,
        Code::Numpad8 => KeyCode::KP_8,
        Code::Numpad9 => KeyCode::KP_9,
        Code::NumpadDecimal => KeyCode::KP_DECIMAL,
        Code::NumpadEnter => KeyCode::KP_ENTER,
        Code::NumpadMultiply => KeyCode::KP_MULTIPLY,
        Code::NumpadAdd => KeyCode::KP_ADD,
        Code::NumpadSubtract => KeyCode::KP_SUBTRACT,
        Code::NumpadDivide => KeyCode::KP_DIVIDE,
        Code::NumpadEqual => KeyCode::KP_EQUAL,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::KeyAction;

    fn decl(owner: &str, actions: &[(&str, Option<KeyChord>)]) -> KeyActions {
        KeyActions {
            owner: owner.into(),
            label: owner.into(),
            actions: actions
                .iter()
                .map(|(id, key)| KeyAction {
                    id: (*id).into(),
                    label: (*id).into(),
                    group: String::new(),
                    default: *key,
                })
                .collect(),
        }
    }

    #[test]
    fn the_system_keeps_a_key_an_app_also_wants() {
        let decls = [
            decl("sola-browser", &[("focus-url", Some(KeyCode::L.meta()))]),
            decl(SYSTEM, &[("lock", Some(KeyCode::L.meta()))]),
        ];
        let b = resolve(&decls, &Keybindings::default());
        assert_eq!(key_of(&b, SYSTEM, "lock"), Some(KeyCode::L.meta()));
        let url = b.iter().find(|b| b.action == "focus-url").unwrap();
        assert_eq!(url.key, None);
        assert_eq!(url.lost_to, Some((SYSTEM.into(), "lock".into())));
    }

    #[test]
    fn your_choice_wins_over_a_default() {
        let decls = [
            decl(SYSTEM, &[("lock", Some(KeyCode::L.meta()))]),
            decl("sola-browser", &[("focus-url", Some(KeyCode::U.meta()))]),
        ];
        let mut keys = Keybindings::default();
        set(&mut keys, "sola-browser", "focus-url", Some(KeyCode::U.meta()), Some(KeyCode::L.meta()));
        let b = resolve(&decls, &keys);
        assert_eq!(key_of(&b, "sola-browser", "focus-url"), Some(KeyCode::L.meta()));
        assert_eq!(key_of(&b, SYSTEM, "lock"), None);
    }

    #[test]
    fn two_apps_may_share_a_key() {
        let decls = [
            decl("sola-mail", &[("new", Some(KeyCode::N.meta()))]),
            decl("sola-calendar", &[("new", Some(KeyCode::N.meta()))]),
        ];
        let b = resolve(&decls, &Keybindings::default());
        assert!(b.iter().all(|b| b.key == Some(KeyCode::N.meta())));
        assert!(holder(&b, "sola-paint", "open", KeyCode::N.meta()).is_none());
        assert!(holder(&b, COMMON, "new", KeyCode::N.meta()).is_some());
    }

    #[test]
    fn an_app_declares_its_own_keyed_items_only() {
        use sola_bus::topics::MenuDefinition;
        let item = |id: &str, key: Option<KeyChord>| MenuItem::Action {
            id: id.into(),
            label: id.into(),
            shortcut: key,
            disabled: false,
            checked: false,
        };
        let payload = AppMenuPayload {
            app_id: "sola-mail".into(),
            menus: vec![
                MenuDefinition {
                    label: "Mail".into(),
                    items: vec![
                        item("compose", Some(KeyCode::N.meta())),
                        item("about", None),
                        item(crate::app::QUIT_ACTION_ID, Some(KeyCode::Q.meta())),
                    ],
                },
                MenuDefinition {
                    label: "Edit".into(),
                    items: vec![item("copy", Some(KeyCode::C.meta()))],
                },
                crate::menu::window_menu(),
            ],
        };
        let d = declaration(&payload);
        assert_eq!(d.label, "Mail");
        let ids: Vec<&str> = d.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["compose"]);
    }

    #[test]
    fn setting_the_default_drops_the_override() {
        let mut keys = Keybindings::default();
        let d = Some(KeyCode::K.meta());
        set(&mut keys, SYSTEM, "shortcuts", d, None);
        assert_eq!(keys.overrides.len(), 1);
        set(&mut keys, SYSTEM, "shortcuts", d, d);
        assert!(keys.overrides.is_empty());
    }
}
