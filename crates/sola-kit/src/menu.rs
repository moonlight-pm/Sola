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
use std::sync::atomic::{AtomicBool, Ordering};

use sola_bus::topics::{AppMenuPayload, KeyActions, MenuActionPayload, MenuDefinition, MenuItem, Topic, Workspaces, Zone, ZoneLayout};
use sola_core::{KeyChord, KeyCode};

/// This process's latest published menus.
static APP_MENU: RwLock<Option<AppMenuPayload>> = RwLock::new(None);

/// Menus published for other app ids (a web app's), as declared, so they
/// follow your key changes too.
static HOSTED: RwLock<Vec<AppMenuPayload>> = RwLock::new(Vec::new());

/// The zone layout (`Topic::ZoneLayout`), for the Window menu's grid
/// zones. `None`: the default.
static LAYOUT: RwLock<Option<ZoneLayout>> = RwLock::new(None);

/// The workspaces (`Topic::Workspaces`), for the Window menu's Send to
/// items. `None`: not said yet.
static WORKSPACES: RwLock<Option<Workspaces>> = RwLock::new(None);

/// [`current`]'s answer until something it reads changes.
static CURRENT: RwLock<Option<AppMenuPayload>> = RwLock::new(None);

/// Learn the zone layout and the workspaces from a bus message (the kit's
/// poller shows it every one).
pub(crate) fn observe(message: &sola_bus::Message) {
    if message.topic != "ZoneLayout" && message.topic != "Workspaces" {
        return;
    }
    match Topic::parse(message) {
        Some(Topic::ZoneLayout(l)) => {
            *LAYOUT.write().unwrap_or_else(|p| p.into_inner()) = message.sticky.then_some(l);
            forget_current();
        }
        Some(Topic::Workspaces(w)) => {
            *WORKSPACES.write().unwrap_or_else(|p| p.into_inner()) = message.sticky.then_some(w);
            forget_current();
        }
        _ => {}
    }
}

/// The zone layout now (the default until the shell says), for the
/// titlebar's zone picker.
pub(crate) fn zone_layout() -> ZoneLayout {
    LAYOUT
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default()
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

/// Give a Window menu one Send to item per workspace, at its end, named
/// after the workspace; their keys come with the rest. The showing one is
/// checked, as where the window is, unless the window is pinned: it is on
/// them all, and sending it to one unpins it there.
pub fn fill_workspaces(menu: &mut MenuDefinition, workspaces: &Workspaces, pinned: bool) {
    if workspaces.workspaces.is_empty() {
        return;
    }
    menu.items.push(MenuItem::Divider);
    menu.items
        .extend(workspaces.workspaces.iter().enumerate().map(|(index, ws)| MenuItem::Action {
            id: workspace_action_id(index),
            label: format!("Send to {}", ws.name),
            shortcut: None,
            disabled: false,
            checked: !pinned && ws.id == workspaces.showing,
        }));
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

/// Keys changed since this process last sent its menus.
static STALE: AtomicBool = AtomicBool::new(false);

/// Keys changed: the titlebars' menus are made again at once, and the
/// menus are sent again when the poller has caught up with the bus
/// ([`publish_stale`]), once for a burst.
pub(crate) fn keys_changed() {
    forget_current();
    STALE.store(true, Ordering::Release);
}

/// Send the menus again if keys changed since they were last sent.
pub(crate) fn publish_stale() {
    if STALE.swap(false, Ordering::AcqRel) {
        republish();
    }
}

/// Send every menu this process published again, with its keys now (your
/// key changes, or the shell's, arrived).
fn republish() {
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

/// An app this process hosted is gone for good (a removed web app): take
/// back its menus and the keys it declared, which outlive the process
/// otherwise (`KeyActions` is kept on disk for Settings).
pub fn forget_for(app_id: &str) {
    HOSTED
        .write()
        .unwrap_or_else(|p| p.into_inner())
        .retain(|p| p.app_id != app_id);
    let Some(bus) = crate::app::try_bus() else {
        return;
    };
    if let Ok(mut bus) = bus.lock() {
        let _ = bus.retract(Topic::SetAppMenu(AppMenuPayload {
            app_id: app_id.to_string(),
            menus: Vec::new(),
        }));
        let _ = bus.retract(Topic::KeyActions(KeyActions {
            owner: app_id.to_string(),
            label: String::new(),
            actions: Vec::new(),
        }));
    }
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
    let layout = zone_layout();
    let workspaces = WORKSPACES
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default();
    if let Some(menu) = payload.menus.iter_mut().find(|m| m.label == WINDOW_MENU_LABEL) {
        let pinned = crate::pinned::focus_is_pinned();
        fill_window_menu(menu, &layout);
        fill_workspaces(menu, &workspaces, pinned);
        check_pin(menu, pinned);
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
            } if *k == key && !is_shell_action(id) => Some(id.clone()),
            _ => None,
        })
}

/// Check the Window menu's Pin item, or not.
pub fn check_pin(menu: &mut MenuDefinition, pinned: bool) {
    for item in &mut menu.items {
        if let MenuItem::Action { id, checked, .. } = item {
            if id == ACTION_PIN {
                *checked = pinned;
            }
        }
    }
}

/// The focused window was pinned or unpinned, or another took focus: the
/// check on the Pin item.
pub(crate) fn focus_pin_changed() {
    forget_current();
}

/// [`current`] made again on its next call (the menus, the keys or the
/// layout changed).
fn forget_current() {
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Act on a titlebar menu choice. Window items and Close Window are the
/// shell's (it acts on the focused window); the rest go to this app as its
/// own `MenuAction`.
pub fn choose(app_id: &str, action_id: &str) {
    let topic = Topic::MenuAction(MenuActionPayload {
        app_id: app_id.to_string(),
        action_id: action_id.to_string(),
    });
    if is_shell_action(action_id) {
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
/// The titlebar's yellow light: hide this window alone (Hide takes its
/// whole app). Not a Window menu item.
pub const ACTION_HIDE_WINDOW: &str = "window.hide-window";
pub const ACTION_CYCLE: &str = "window.cycle";
pub const ACTION_FLOAT: &str = "window.float";
pub const ACTION_FULLSCREEN: &str = "window.fullscreen";
pub const ACTION_CINEMA: &str = "window.cinema";
/// On every workspace, or back on the showing one alone.
pub const ACTION_PIN: &str = "window.pin";
/// A grid zone's item: `window.zone.<id>` (the layout's zones are the
/// shell's; it adds their items to the menus it draws).
pub const ACTION_GRID_PREFIX: &str = "window.zone.";

/// A workspace's item: `window.workspace.<index>` (the workspaces are the
/// shell's; it adds their items to the menus it draws).
pub const ACTION_WORKSPACE_PREFIX: &str = "window.workspace.";

/// The Window menu item id that moves a tab to grid zone `id`.
pub fn grid_action_id(id: u32) -> String {
    format!("{ACTION_GRID_PREFIX}{id}")
}

/// The Window menu item id that sends a window to workspace `index`.
pub fn workspace_action_id(index: usize) -> String {
    format!("{ACTION_WORKSPACE_PREFIX}{index}")
}

/// Compositor action the shell handles instead of forwarding `MenuAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Hide,
    /// The focused window alone ([`ACTION_HIDE_WINDOW`]).
    HideWindow,
    Cycle,
    Pin,
    Zone(Zone),
    SendToWorkspace(usize),
}

/// One row of the default Window menu (not a divider).
#[derive(Debug, Clone, Copy)]
pub struct WindowMenuItem {
    pub id: &'static str,
    pub label: &'static str,
    pub shortcut: Option<KeyChord>,
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
        shortcut: Some(shortcut),
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
    WindowMenuEntry::Item(WindowMenuItem {
        id: ACTION_PIN,
        label: "Pin to All Workspaces",
        shortcut: None,
        action: WindowAction::Pin,
    }),
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

/// The menu action that closes the focused window, which the shell carries
/// out for every app: a tab's window through its app, any other as the bar's
/// close does. An app's own menu lists it ([`close_window_item`]). Editor
/// alone leaves its key out: Super is Meta there, and there only.
pub const ACTION_CLOSE_WINDOW: &str = "_close_window";

/// The key for [`ACTION_CLOSE_WINDOW`].
pub const CLOSE_WINDOW_KEY: KeyChord = KeyCode::W.meta();

/// **Close Window** (Super+W), for [`crate::app::BusSetup::app_menu`].
pub const fn close_window_item() -> (&'static str, &'static str, KeyChord) {
    (ACTION_CLOSE_WINDOW, "Close Window", CLOSE_WINDOW_KEY)
}

/// [`close_window_item`] as a [`MenuItem`].
pub fn close_window_menu_item() -> MenuItem {
    MenuItem::Action {
        id: ACTION_CLOSE_WINDOW.into(),
        label: "Close Window".into(),
        shortcut: Some(CLOSE_WINDOW_KEY),
        disabled: false,
        checked: false,
    }
}

/// The key for Quit: the system's Close App, which every Quit item shows
/// (it follows your change of it, [`crate::keys`]).
pub const QUIT_KEY: KeyChord = KeyCode::Q.meta();

/// **Quit** (Super+Q), for [`crate::app::BusSetup::app_menu`]; `label` reads
/// "Quit Monitor".
pub const fn quit_item(label: &'static str) -> (&'static str, &'static str, KeyChord) {
    (crate::app::QUIT_ACTION_ID, label, QUIT_KEY)
}

/// [`quit_item`] as a [`MenuItem`].
pub fn quit_menu_item(label: &str) -> MenuItem {
    MenuItem::Action {
        id: crate::app::QUIT_ACTION_ID.into(),
        label: label.into(),
        shortcut: Some(QUIT_KEY),
        disabled: false,
        checked: false,
    }
}

/// A menu action the shell carries out rather than the app.
fn is_shell_action(id: &str) -> bool {
    id == ACTION_CLOSE_WINDOW || parse_window_action(id).is_some()
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
                    shortcut: i.shortcut,
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
    if id == ACTION_HIDE_WINDOW {
        return Some(WindowAction::HideWindow);
    }
    if let Some(n) = id.strip_prefix(ACTION_GRID_PREFIX) {
        return n.parse().ok().map(|n| WindowAction::Zone(Zone::Grid(n)));
    }
    if let Some(n) = id.strip_prefix(ACTION_WORKSPACE_PREFIX) {
        return n.parse().ok().map(WindowAction::SendToWorkspace);
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
    use sola_bus::topics::Workspace;

    fn two_workspaces() -> Workspaces {
        let workspace = |id: &str, name: &str| Workspace {
            id: id.into(),
            name: name.into(),
        };
        Workspaces {
            workspaces: vec![workspace("a", "Main"), workspace("b", "Games")],
            showing: "b".into(),
        }
    }

    fn sends(menu: &MenuDefinition) -> Vec<(String, String, bool)> {
        menu.items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { id, label, checked, .. } if id.starts_with(ACTION_WORKSPACE_PREFIX) => {
                    Some((id.clone(), label.clone(), *checked))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn one_send_item_per_workspace_with_the_showing_one_checked() {
        let mut menu = window_menu();
        fill_workspaces(&mut menu, &two_workspaces(), false);
        assert_eq!(
            sends(&menu),
            [
                ("window.workspace.0".to_string(), "Send to Main".to_string(), false),
                ("window.workspace.1".to_string(), "Send to Games".to_string(), true),
            ]
        );
    }

    #[test]
    fn a_pinned_window_is_on_no_one_workspace() {
        let mut menu = window_menu();
        fill_workspaces(&mut menu, &two_workspaces(), true);
        assert!(sends(&menu).iter().all(|(_, _, checked)| !checked));
    }

    #[test]
    fn no_workspaces_known_leaves_the_menu_alone() {
        let mut menu = window_menu();
        let before = menu.items.len();
        fill_workspaces(&mut menu, &Workspaces::default(), false);
        assert_eq!(menu.items.len(), before);
    }

    #[test]
    fn a_send_item_is_the_shell_s() {
        assert_eq!(
            parse_window_action(&workspace_action_id(2)),
            Some(WindowAction::SendToWorkspace(2))
        );
        assert!(is_shell_action("window.workspace.0"));
        assert_eq!(parse_window_action("window.workspace.x"), None);
    }

    #[test]
    fn the_pin_item_is_the_shell_s_and_shows_whether_the_window_is_pinned() {
        assert_eq!(parse_window_action(ACTION_PIN), Some(WindowAction::Pin));
        assert!(is_shell_action(ACTION_PIN));
        let mut menu = window_menu();
        let checked = |menu: &MenuDefinition| {
            menu.items.iter().find_map(|i| match i {
                MenuItem::Action { id, checked, .. } if id == ACTION_PIN => Some(*checked),
                _ => None,
            })
        };
        assert_eq!(checked(&menu), Some(false));
        check_pin(&mut menu, true);
        assert_eq!(checked(&menu), Some(true));
        check_pin(&mut menu, false);
        assert_eq!(checked(&menu), Some(false));
    }

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
        assert_eq!(parse_window_action(ACTION_HIDE_WINDOW), Some(WindowAction::HideWindow));
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
            WindowMenuEntry::Item(i) if i.id == ACTION_FLOAT => i.shortcut,
            _ => None,
        });
        let chord = chord.expect("float item");
        assert!(chord.meta);
        assert_eq!(chord.keycode, KeyCode::KP_MULTIPLY);
    }
}
