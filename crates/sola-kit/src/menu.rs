//! An app's menus: the ones its window titlebars show, and the shared
//! **Window** menu.
//!
//! An app publishes its menus once with [`BusSetup`](crate::app::BusSetup)
//! or, when they change at runtime, with [`publish`]. The kit keeps the
//! latest payload for the titlebars
//! ([`crate::components::window_frame`]) and the shell caches the same
//! payload for keyboard shortcuts. A titlebar choice comes back through
//! [`choose`]: a Window item goes to the shell, anything else to the app's
//! own bus stream as a `MenuAction`, so an app handles a click and a
//! shortcut the same way.
//!
//! The **Window** menu is the mouse path for compositor actions that used
//! to live only on Super+numpad / Super+H / Super+`. Apps publish it via
//! [`window_menu`] / [`BusSetup::window_menu`](crate::app::BusSetup::window_menu);
//! titlebars add it last when an app omits it ([`ensure_window_menu`]).

use std::sync::RwLock;

use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, Zone, ZoneLayout};
use sola_core::{KeyChord, KeyCode};

/// This process's latest published menus.
static APP_MENU: RwLock<Option<AppMenuPayload>> = RwLock::new(None);

/// Menus published for other app ids (a web app's), as declared, so they
/// follow your key changes too.
static HOSTED: RwLock<Vec<AppMenuPayload>> = RwLock::new(Vec::new());

/// The zone layout (`Topic::ZoneLayout`), for the Window menu's grid
/// zones. `None`: the default.
static LAYOUT: RwLock<Option<ZoneLayout>> = RwLock::new(None);

/// [`current`]'s answer until something it reads changes.
static CURRENT: RwLock<Option<AppMenuPayload>> = RwLock::new(None);

/// Learn the zone layout from a bus message (the kit's poller shows it
/// every one).
pub(crate) fn observe(message: &sola_bus::Message) {
    if message.topic != "ZoneLayout" {
        return;
    }
    if let Some(Topic::ZoneLayout(l)) = Topic::parse(message) {
        *LAYOUT.write().unwrap_or_else(|p| p.into_inner()) = message.sticky.then_some(l);
        forget_current();
    }
}

/// Give a Window menu the layout's grid zones, after Float, named by
/// their place ("Column 1"); their keys come with the rest
/// (`crate::keys::apply`, or the shell's own table).
pub fn fill_window_menu(menu: &mut MenuDefinition, layout: &ZoneLayout) {
    menu.items.retain(|item| {
        !matches!(item, MenuItem::Action { id, .. } if id.starts_with(ACTION_GRID_PREFIX))
    });
    let at = menu
        .items
        .iter()
        .position(|item| matches!(item, MenuItem::Action { id, .. } if id == ACTION_FLOAT))
        .map_or(menu.items.len(), |i| i + 1);
    let mut block = Vec::new();
    for z in &layout.zones {
        block.push(MenuItem::Action {
            id: grid_action_id(z.id),
            label: layout.label(z.id),
            shortcut: None,
            disabled: false,
            checked: false,
        });
    }
    if !block.is_empty() {
        block.insert(0, MenuItem::Divider);
        menu.items.splice(at..at, block);
    }
}

/// Publish this app's menus: kept for its titlebars and sent to the shell.
/// Call again whenever they change.
pub fn publish(payload: AppMenuPayload) {
    remember(payload.clone());
    send(&payload);
}

/// Send `payload` with every item's key now (`crate::keys::apply`), and
/// declare its app's keyed actions and the shared ones to Settings
/// (`Topic::KeyActions`).
fn send(payload: &AppMenuPayload) {
    let Some(bus) = crate::app::try_bus() else {
        return;
    };
    if let Ok(mut bus) = bus.lock() {
        if let Err(e) = bus.emit(Topic::SetAppMenu(crate::keys::apply(payload))) {
            tracing::warn!("publish app menu failed: {e}");
        }
        let _ = bus.emit(Topic::KeyActions(crate::keys::declaration(payload)));
        let _ = bus.emit(Topic::KeyActions(crate::keys::common_declaration()));
    }
}

/// Send every menu this process published again, with its keys now (your
/// key changes, or the shell's, arrived).
pub(crate) fn republish() {
    forget_current();
    let own = APP_MENU.read().unwrap_or_else(|p| p.into_inner()).clone();
    let hosted = HOSTED.read().unwrap_or_else(|p| p.into_inner()).clone();
    let Some(bus) = crate::app::try_bus() else {
        return;
    };
    if let Ok(mut bus) = bus.lock() {
        for payload in own.iter().chain(hosted.iter()) {
            let _ = bus.emit(Topic::SetAppMenu(crate::keys::apply(payload)));
        }
    }
}

/// Send the menus of an app this process hosts (a web app's windows in the
/// browser) to the shell, for its shortcuts. The process's own menus stay;
/// those windows pass theirs to their titlebars
/// ([`WindowFrame::menus`](crate::components::window_frame::WindowFrame::menus)).
pub fn publish_for(payload: AppMenuPayload) {
    {
        let mut hosted = HOSTED.write().unwrap_or_else(|p| p.into_inner());
        hosted.retain(|p| p.app_id != payload.app_id);
        hosted.push(payload.clone());
    }
    send(&payload);
}

/// Keep `payload` for the titlebars without sending it.
pub(crate) fn remember(payload: AppMenuPayload) {
    *APP_MENU.write().unwrap_or_else(|p| p.into_inner()) = Some(payload);
    forget_current();
}

/// The menus this app's titlebars show, the Window menu last. `None`
/// until the app publishes.
pub fn current() -> Option<AppMenuPayload> {
    if let Some(p) = CURRENT.read().unwrap_or_else(|p| p.into_inner()).as_ref() {
        return Some(p.clone());
    }
    let declared = APP_MENU.read().unwrap_or_else(|p| p.into_inner()).clone()?;
    let prepared = prepare(declared);
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = Some(prepared.clone());
    Some(prepared)
}

/// What a titlebar shows of `payload`: the Window menu (added when it has
/// none) with the layout's grid zones, and every item's key now.
pub fn prepare(payload: AppMenuPayload) -> AppMenuPayload {
    let mut payload = ensure_window_menu(payload);
    let layout = LAYOUT
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default();
    if let Some(menu) = payload.menus.iter_mut().find(|m| m.label == WINDOW_MENU_LABEL) {
        fill_window_menu(menu, &layout);
    }
    crate::keys::apply(&payload)
}

/// The item of this app's menus whose key now is `key` (not a Window
/// item): for keys an app reads itself (a letter, no Super), so they
/// follow your changes in Settings → Keybindings.
pub fn action_for_key(key: KeyChord) -> Option<String> {
    current()?
        .menus
        .iter()
        .flat_map(|m| m.items.iter())
        .find_map(|item| match item {
            MenuItem::Action {
                id,
                shortcut: Some(k),
                disabled: false,
                ..
            } if *k == key && parse_window_action(id).is_none() => Some(id.clone()),
            _ => None,
        })
}

/// [`current`] made again on its next call (the menus, the keys or the
/// layout changed).
fn forget_current() {
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Act on a titlebar menu choice. Window items are the shell's (it acts on
/// the focused window); the rest go to this app as its own `MenuAction`.
pub fn choose(app_id: &str, action_id: &str) {
    let topic = Topic::MenuAction(MenuActionPayload {
        app_id: app_id.to_string(),
        action_id: action_id.to_string(),
    });
    if parse_window_action(action_id).is_some() {
        if let Some(Ok(mut bus)) = crate::app::try_bus().map(|b| b.lock()) {
            if let Err(e) = bus.emit(topic) {
                tracing::warn!(action_id, "window menu action failed: {e}");
            }
        }
    } else {
        crate::app::deliver_locally(topic);
    }
}

/// Menubar label. The shell uses this to decide whether to inject a
/// default Window menu or honor the app's own.
pub const WINDOW_MENU_LABEL: &str = "Window";

pub const ACTION_HIDE: &str = "window.hide";
pub const ACTION_CYCLE: &str = "window.cycle";
pub const ACTION_FLOAT: &str = "window.float";
pub const ACTION_FULLSCREEN: &str = "window.fullscreen";
pub const ACTION_CINEMA: &str = "window.cinema";
/// A grid zone's item: `window.zone.<id>` (the layout's zones are the
/// shell's; it adds their items to the menus it draws).
pub const ACTION_GRID_PREFIX: &str = "window.zone.";

/// The Window menu item id that moves a tab to grid zone `id`.
pub fn grid_action_id(id: u32) -> String {
    format!("{ACTION_GRID_PREFIX}{id}")
}

/// Compositor action the shell handles instead of forwarding `MenuAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Hide,
    Cycle,
    Zone(Zone),
}

/// One row of the default Window menu (not a divider).
#[derive(Debug, Clone, Copy)]
pub struct WindowMenuItem {
    pub id: &'static str,
    pub label: &'static str,
    pub shortcut: KeyChord,
    pub action: WindowAction,
}

/// Default Window menu, dividers included. Apps that want extras start
/// from [`window_menu`] and push more items; unknown ids still go to the
/// app as `MenuAction`.
#[derive(Debug, Clone, Copy)]
pub enum WindowMenuEntry {
    Item(WindowMenuItem),
    Divider,
}

const fn item(
    id: &'static str,
    label: &'static str,
    shortcut: KeyChord,
    action: WindowAction,
) -> WindowMenuEntry {
    WindowMenuEntry::Item(WindowMenuItem {
        id,
        label,
        shortcut,
        action,
    })
}

/// Canonical Window menu. Keep in lockstep with shell zoning keys.
pub const WINDOW_MENU_ENTRIES: &[WindowMenuEntry] = &[
    item(ACTION_HIDE, "Hide", KeyCode::H.meta(), WindowAction::Hide),
    item(
        ACTION_CYCLE,
        "Cycle Windows",
        KeyCode::GRAVE.meta(),
        WindowAction::Cycle,
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_FLOAT,
        "Float",
        kp(KeyCode::KP_MULTIPLY),
        WindowAction::Zone(Zone::Float),
    ),
    WindowMenuEntry::Divider,
    item(
        ACTION_FULLSCREEN,
        "Full",
        kp(KeyCode::KP_0),
        WindowAction::Zone(Zone::Fullscreen),
    ),
    item(
        ACTION_CINEMA,
        "Cinema",
        kp(KeyCode::KP_ENTER),
        WindowAction::Zone(Zone::Cinema),
    ),
];

const fn kp(keycode: KeyCode) -> KeyChord {
    KeyChord {
        keycode,
        meta: true,
        alt: false,
        ctrl: false,
        shift: false,
    }
}

/// Default Window menu for [`crate::app::BusSetup::app_menu_definition`].
pub fn window_menu() -> MenuDefinition {
    MenuDefinition {
        label: WINDOW_MENU_LABEL.into(),
        items: WINDOW_MENU_ENTRIES
            .iter()
            .map(|entry| match entry {
                WindowMenuEntry::Divider => MenuItem::Divider,
                WindowMenuEntry::Item(i) => MenuItem::Action {
                    id: i.id.into(),
                    label: i.label.into(),
                    shortcut: Some(i.shortcut),
                    disabled: false,
                    checked: false,
                },
            })
            .collect(),
    }
}

/// Map a menu action id to a compositor window action. `None` means the
/// id is not a kit Window item (forward to the app).
pub fn parse_window_action(id: &str) -> Option<WindowAction> {
    if let Some(n) = id.strip_prefix(ACTION_GRID_PREFIX) {
        return n.parse().ok().map(|n| WindowAction::Zone(Zone::Grid(n)));
    }
    WINDOW_MENU_ENTRIES.iter().find_map(|entry| match entry {
        WindowMenuEntry::Item(i) if i.id == id => Some(i.action),
        _ => None,
    })
}

/// Append the default Window menu when the payload does not already
/// declare one (so apps can replace it by publishing their own).
pub fn ensure_window_menu(mut payload: AppMenuPayload) -> AppMenuPayload {
    if payload.menus.iter().any(|m| m.label == WINDOW_MENU_LABEL) {
        payload
    } else {
        payload.menus.push(window_menu());
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_menu_covers_every_zone() {
        for (id, zone) in [
            (ACTION_FULLSCREEN, Zone::Fullscreen),
            (ACTION_CINEMA, Zone::Cinema),
            (ACTION_FLOAT, Zone::Float),
        ] {
            assert_eq!(parse_window_action(id), Some(WindowAction::Zone(zone)));
        }
        assert_eq!(
            parse_window_action(&grid_action_id(3)),
            Some(WindowAction::Zone(Zone::Grid(3)))
        );
        assert_eq!(parse_window_action("window.zone.x"), None);
    }

    #[test]
    fn hide_and_cycle_parse() {
        assert_eq!(parse_window_action(ACTION_HIDE), Some(WindowAction::Hide));
        assert_eq!(parse_window_action(ACTION_CYCLE), Some(WindowAction::Cycle));
        assert_eq!(parse_window_action("quit"), None);
        assert_eq!(parse_window_action("window.new"), None);
    }

    #[test]
    fn ensure_window_menu_appends_once() {
        let empty = AppMenuPayload {
            app_id: "firefox".into(),
            menus: vec![MenuDefinition {
                label: "Firefox".into(),
                items: vec![],
            }],
        };
        let once = ensure_window_menu(empty);
        assert_eq!(once.menus.len(), 2);
        assert_eq!(once.menus[1].label, WINDOW_MENU_LABEL);
        let twice = ensure_window_menu(once);
        assert_eq!(twice.menus.len(), 2);
    }

    #[test]
    fn ensure_window_menu_keeps_app_window_menu() {
        let custom = AppMenuPayload {
            app_id: "sola-browser".into(),
            menus: vec![MenuDefinition {
                label: WINDOW_MENU_LABEL.into(),
                items: vec![MenuItem::Action {
                    id: "window.new".into(),
                    label: "New Window".into(),
                    shortcut: None,
                    disabled: false,
                    checked: false,
                }],
            }],
        };
        let out = ensure_window_menu(custom);
        assert_eq!(out.menus.len(), 1);
        match &out.menus[0].items[0] {
            MenuItem::Action { id, .. } => assert_eq!(id, "window.new"),
            other => panic!("expected Action, got {other:?}"),
        }
    }

    #[test]
    fn window_menu_float_is_super_kp_multiply() {
        let chord = WINDOW_MENU_ENTRIES.iter().find_map(|e| match e {
            WindowMenuEntry::Item(i) if i.id == ACTION_FLOAT => Some(i.shortcut),
            _ => None,
        });
        let chord = chord.expect("float item");
        assert!(chord.meta);
        assert_eq!(chord.keycode, KeyCode::KP_MULTIPLY);
    }
}
