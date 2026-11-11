//! Sola-bus integration for the browser chrome.
//!
//! Wires the running browser into the Sola bus the way kit apps do (see
//! `sola-monitor`). The connect + subscribe + app-menu publish happens once in
//! `run()` via `sola_kit::app::BusSetup`; this module is the receive side.
//! It reacts to:
//!
//! - `Topic::OpenUrl` — open a fresh tab, a window of its own (in front
//!   per `activate`). Our own echo is ignored.
//! - `Topic::MenuAction` from published menus — keyboard shortcuts and
//!   menubar clicks,
//! - `Topic::Chord` / `ChordReleased` — Super held (River steals Super_L
//!   from the focused client; ⌘-click needs that bit),
//! - `Topic::Theme` — restyle the chrome live (handled by the kit helper),
//! - self-addressed quit (`MenuAction "quit"` / `CloseApp`).
//!
//! The `Topic` → `BrowserIntent` mapping is pure so it can be unit-tested
//! without a running bus.

use std::sync::Arc;

use iced::Task;
use sola_bus::Message;
use sola_bus::topics::{
    AppMenuPayload, MenuDefinition, MenuItem, OpenUrlRequest, Topic, TopicKind,
};
use sola_core::{KeyChord, KeyCode};

use crate::app::{App, BLANK_URL, Msg};
use crate::engine::{EditCmd, Engine};

// Menu action ids — shared between the published menu and the handler so the
// two never drift.
pub const ACTION_NEW_TAB: &str = "new-tab";
pub const ACTION_CLOSE_TAB: &str = "close-tab";
pub const ACTION_REOPEN_TAB: &str = "reopen-tab";
pub const ACTION_RELOAD: &str = "reload";
pub const ACTION_HARD_RELOAD: &str = "hard-reload";
pub const ACTION_FOCUS_URL: &str = "focus-url";
pub const ACTION_BACK: &str = "back";
pub const ACTION_FORWARD: &str = "forward";
pub const ACTION_QUIT: &str = "quit";
pub const ACTION_DEVTOOLS: &str = "devtools";
// Edit ids are the kit's shared ones (`sola_kit::keys::COMMON_ACTIONS`):
// an app id on a shared key loses the key, and the shell stops grabbing it.
pub const ACTION_EDIT_UNDO: &str = "undo";
pub const ACTION_EDIT_REDO: &str = "redo";
pub const ACTION_EDIT_CUT: &str = "cut";
pub const ACTION_EDIT_COPY: &str = "copy";
pub const ACTION_EDIT_PASTE: &str = "paste";
pub const ACTION_EDIT_SELECT_ALL: &str = "select_all";
pub const ACTION_FIND: &str = "find";
pub const ACTION_FIND_NEXT: &str = "find-next";
pub const ACTION_FIND_PREV: &str = "find-prev";
/// Browser → Wrap This Page…: the page becomes a web app.
pub const ACTION_WRAP_PAGE: &str = "wrap-page";
/// Adopt Workspace: the window's page runs the showing workspace's profile.
/// In a window's titlebar menu only while it runs another (a web app's, a
/// pinned page's).
pub const ACTION_ADOPT_WORKSPACE: &str = "adopt-workspace";
/// Topics the browser subscribes to. Theme/MenuAction are the live inputs;
/// CloseApp is the shell's "quit this app" signal (via `is_self_quit`).
///
/// `OpenUrl` is subscribed for dogfood / `solactl emit OpenUrl` control of a
/// running sola-browser. System http/https defaults go to sola-browser
/// (D3) until we flip MIME; this does not change that default by itself.
///
/// Chord / ChordReleased: River does not deliver bound Super_L to the
/// focused surface. The shell registers bare Super_L so switcher confirm
/// works; we listen so ⌘-click still sees Super.
pub const SUBSCRIBE: &[TopicKind] = &[
    TopicKind::Theme,
    TopicKind::MenuAction,
    TopicKind::CloseApp,
    TopicKind::WindowPlace,
    TopicKind::OpenUrl,
    TopicKind::Chord,
    TopicKind::ChordReleased,
    TopicKind::NotificationActivate,
    // Which window each page is, whether it is on screen, and which
    // workspace it is on (`WindowPlace`; Next).
    TopicKind::CloseTab,
    // Which workspace shows and where each window is: a page runs its
    // workspace's profile (Next slice 5).
    TopicKind::Workspaces,
    // The catalog: web apps this process hosts.
    TopicKind::Application,
    // The output's size: the shape Use as Desktop Background cuts to.
    TopicKind::OutputGeometry,
    // Light or dark for the pages (`Appearance.light`).
    TopicKind::Appearance,
    // What was picked in a page's right-click menu (the shell draws it).
    TopicKind::DeskMenuPick,
];

/// The "Browser" app-menu published to the shell at startup. Each entry is
/// `(action_id, label, chord)`; chords are meta-bound. The shell binds them
/// globally and routes `Topic::MenuAction` back when one is pressed.
pub const MENU_ITEMS: [(&str, &str, KeyChord); 10] = [
    (ACTION_NEW_TAB, "New Tab", KeyCode::T.meta()),
    (ACTION_CLOSE_TAB, "Close Tab", KeyCode::W.meta()),
    (
        ACTION_REOPEN_TAB,
        "Reopen Closed Tab",
        KeyCode::T.meta_shift(),
    ),
    (ACTION_RELOAD, "Reload", KeyCode::R.meta()),
    (ACTION_HARD_RELOAD, "Hard Reload", KeyCode::R.meta_shift()),
    (ACTION_FOCUS_URL, "Focus URL", KeyCode::L.meta()),
    (ACTION_BACK, "Back", KeyCode::LEFT.meta()),
    (ACTION_FORWARD, "Forward", KeyCode::RIGHT.meta()),
    (ACTION_DEVTOOLS, "Developer Tools", KeyCode::I.meta().alt()),
    (ACTION_QUIT, "Quit Browser", KeyCode::Q.meta()),
];

/// The "Edit" app-menu published alongside "Browser". Meta-bound so the
/// shell grabs them globally and routes `Topic::MenuAction` back; the
/// browser then routes each to the focused surface (web content, the URL
/// bar, the vault panel). The page view swallows a raw ⌘C/X/V/A/Z, so a
/// key the shell does not grab does nothing. Undo / Redo act on the page.
pub const EDIT_MENU_ITEMS: [(&str, &str, KeyChord); 9] = [
    (ACTION_EDIT_UNDO, "Undo", KeyCode::Z.meta()),
    (ACTION_EDIT_REDO, "Redo", KeyCode::Z.meta_shift()),
    (ACTION_EDIT_CUT, "Cut", KeyCode::X.meta()),
    (ACTION_EDIT_COPY, "Copy", KeyCode::C.meta()),
    (ACTION_EDIT_PASTE, "Paste", KeyCode::V.meta()),
    (ACTION_EDIT_SELECT_ALL, "Select All", KeyCode::A.meta()),
    (ACTION_FIND, "Find", KeyCode::F.meta()),
    (ACTION_FIND_NEXT, "Find Next", KeyCode::G.meta()),
    (ACTION_FIND_PREV, "Find Previous", KeyCode::G.meta_shift()),
];

/// Full menubar for the browser (Browser + Edit). No Profiles menu: a
/// workspace has its own profile, so where a page is says which it runs;
/// `adopt` adds Adopt Workspace, for a window whose page runs another.
pub fn browser_app_menu(app_id: &str, adopt: bool) -> AppMenuPayload {
    let mut browser = menu_from_items("Browser", &MENU_ITEMS);
    // Before Quit: the page becomes its own app.
    let quit = browser.items.pop();
    browser.items.push(MenuItem::Divider);
    browser.items.push(MenuItem::Action {
        id: ACTION_WRAP_PAGE.into(),
        label: "Wrap This Page…".into(),
        shortcut: None,
        disabled: false,
        checked: false,
    });
    if adopt {
        browser.items.push(adopt_workspace_item());
    }
    browser.items.push(MenuItem::Divider);
    browser.items.extend(quit);
    AppMenuPayload {
        app_id: app_id.into(),
        menus: vec![
            browser,
            menu_from_items("Edit", &EDIT_MENU_ITEMS),
        ],
    }
}

/// Adopt Workspace, as a menu item.
pub fn adopt_workspace_item() -> MenuItem {
    MenuItem::Action {
        id: ACTION_ADOPT_WORKSPACE.into(),
        label: "Adopt Workspace".into(),
        shortcut: None,
        disabled: false,
        checked: false,
    }
}

fn menu_from_items(label: &str, items: &[(&str, &str, KeyChord)]) -> MenuDefinition {
    MenuDefinition {
        label: label.into(),
        items: items
            .iter()
            .map(|(id, item_label, chord)| MenuItem::Action {
                id: (*id).into(),
                label: (*item_label).into(),
                shortcut: Some(chord.clone()),
                disabled: false,
                checked: false,
            })
            .collect(),
    }
}

/// Each menu item's id and key, to compare a menu with what the keybinding
/// rules leave of it.
#[cfg(test)]
pub(crate) fn menu_keys(menu: &AppMenuPayload) -> Vec<(String, Option<KeyChord>)> {
    menu.menus
        .iter()
        .flat_map(|m| &m.items)
        .filter_map(|i| match i {
            MenuItem::Action { id, shortcut, .. } => Some((id.clone(), *shortcut)),
            _ => None,
        })
        .collect()
}

/// Stable widget id for the chrome URL field, so the `Focus URL` action can
/// move keyboard focus to it.
pub fn url_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("browser-url-bar")
}

pub fn find_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("browser-find")
}

/// What the browser should do in response to a bus event. A plain enum keeps
/// the mapping pure and unit-testable without a bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserIntent {
    /// Open a tab loading `url`, focused per `activate` (bus-driven OpenUrl).
    NewTab {
        url: String,
        activate: bool,
    },
    /// Open a fresh blank tab, focus it, and move keyboard focus to the empty
    /// URL bar ready for typing (⌘T).
    NewBlankTab,
    CloseActiveTab,
    ReopenClosedTab,
    Reload,
    HardReload,
    Back,
    Forward,
    FocusUrl,
    /// Run an editing command, routed to the focused surface.
    Edit(EditCmd),
    /// Open DevTools (console) for the active tab.
    ShowDevTools,
    Find,
    FindNext,
    FindPrev,
    /// The active page becomes a web app (the Wrap sheet).
    WrapPage,
    /// The active page runs the showing workspace's profile.
    AdoptWorkspace,
    Quit,
    None,
}

/// True when this `OpenUrl` came from this chrome (it opens its own pages;
/// never open them twice).
pub fn open_url_is_self_echo(source: &str, app_id: &str) -> bool {
    source == app_id
}

/// Map an `OpenUrlRequest` to an intent: always a fresh tab, focused per
/// `activate` (matches the retired GTK browser's behaviour).
pub fn intent_for_open_url(req: &OpenUrlRequest) -> BrowserIntent {
    BrowserIntent::NewTab {
        url: req.url.clone(),
        activate: req.activate,
    }
}

/// Map a menu `action_id` to an intent. Unknown ids are ignored.
pub fn intent_for_menu_action(action_id: &str) -> BrowserIntent {
    match action_id {
        ACTION_NEW_TAB => BrowserIntent::NewBlankTab,
        ACTION_CLOSE_TAB => BrowserIntent::CloseActiveTab,
        ACTION_REOPEN_TAB => BrowserIntent::ReopenClosedTab,
        ACTION_FIND => BrowserIntent::Find,
        ACTION_FIND_NEXT => BrowserIntent::FindNext,
        ACTION_FIND_PREV => BrowserIntent::FindPrev,
        ACTION_RELOAD => BrowserIntent::Reload,
        ACTION_HARD_RELOAD => BrowserIntent::HardReload,
        ACTION_FOCUS_URL => BrowserIntent::FocusUrl,
        ACTION_BACK => BrowserIntent::Back,
        ACTION_FORWARD => BrowserIntent::Forward,
        ACTION_QUIT => BrowserIntent::Quit,
        ACTION_WRAP_PAGE => BrowserIntent::WrapPage,
        ACTION_ADOPT_WORKSPACE => BrowserIntent::AdoptWorkspace,
        ACTION_DEVTOOLS => BrowserIntent::ShowDevTools,
        ACTION_EDIT_UNDO => BrowserIntent::Edit(EditCmd::Undo),
        ACTION_EDIT_REDO => BrowserIntent::Edit(EditCmd::Redo),
        ACTION_EDIT_CUT => BrowserIntent::Edit(EditCmd::Cut),
        ACTION_EDIT_COPY => BrowserIntent::Edit(EditCmd::Copy),
        ACTION_EDIT_PASTE => BrowserIntent::Edit(EditCmd::Paste),
        ACTION_EDIT_SELECT_ALL => BrowserIntent::Edit(EditCmd::SelectAll),
        _ => BrowserIntent::None,
    }
}

/// Handle one bus message. Returns a `Task` so intents that need one (focus,
/// exit) can produce it.
pub fn handle_bus<E: Engine>(
    app: &mut App<E>,
    message: Arc<Message>,
    app_id: &'static str,
) -> Task<Msg> {
    app.float.update(&message);
    // Theme first: restyle the chrome live (also installs the font roles).
    if sola_kit::app::apply_theme_update(&message, &mut app.theme) {
        return Task::none();
    }
    // Self-addressed quit (MenuAction "quit" or CloseApp).
    if sola_kit::app::is_self_quit(&message, app_id) {
        app.persist_session();
        return iced::exit();
    }
    // Web apps: the catalog, and what is addressed to an app hosted here.
    if let Some(task) = app.on_web_app_bus(&message) {
        return task;
    }
    match Topic::parse(&message) {
        Some(Topic::OpenUrl(req)) => {
            // Ignore our own echo or we would open the tab twice.
            if open_url_is_self_echo(&message.source, app_id) {
                tracing::debug!(url = %req.url, "OpenUrl self-echo — tab already opened");
                return Task::none();
            }
            tracing::info!(url = %req.url, activate = req.activate, "OpenUrl bus");
            run_intent(app, intent_for_open_url(&req))
        }
        Some(Topic::MenuAction(m)) if m.app_id == app_id => {
            tracing::info!(
                action_id = %m.action_id,
                profile = %crate::profiles::active().id,
                "menu action received"
            );
            run_intent(app, intent_for_menu_action(&m.action_id))
        }
        Some(Topic::Chord(c)) => {
            if crate::input::apply_super_chord(true, c.keysym) {
                tracing::info!(keysym = c.keysym, "super down (bus chord)");
            }
            Task::none()
        }
        Some(Topic::ChordReleased(c)) => {
            if crate::input::apply_super_chord(false, c.keysym) {
                tracing::info!(keysym = c.keysym, "super up (bus chord)");
            }
            Task::none()
        }
        Some(Topic::DeskMenuPick(_)) => match sola_kit::desk_menu::pick_of(&message) {
            Some(pick) => app.on_desk_pick(pick),
            None => Task::none(),
        },
        Some(Topic::NotificationActivate(a)) if a.app_id == app_id => {
            if let Some(id) = a.tab_id {
                let tid = crate::engine::TabId(id);
                if app.cached_tabs.iter().any(|t| t.id == tid) {
                    app.switch_active_tab(tid);
                }
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

pub fn run_intent<E: Engine>(app: &mut App<E>, intent: BrowserIntent) -> Task<Msg> {
    match intent {
        BrowserIntent::NewTab { url, activate } => {
            app.open_outside_link(url, activate);
            Task::none()
        }
        BrowserIntent::NewBlankTab => {
            let tab = app.open_tab(BLANK_URL.to_string(), true);
            app.url_bar_on_focus = Some(tab);
            // Empty the URL bar and move keyboard focus to it so the user can
            // type a URL or search immediately. The blank tab's "about:blank"
            // is suppressed from the field (see `Msg::Tick`); seeding
            // `last_seen_url` here avoids a one-frame flash of it.
            //
            // Prefer URL-bar focus for a blank tab so typing starts immediately.
            app.url_field.clear();
            app.last_seen_url = BLANK_URL.to_string();
            app.set_url_bar_focused(true);
            Task::batch([focus_url_bar(), select_url_bar()])
        }
        BrowserIntent::CloseActiveTab => {
            let id = app.cached_active;
            app.update(Msg::CloseTab(id))
        }
        BrowserIntent::ReopenClosedTab => app.update(Msg::ReopenClosedTab),
        BrowserIntent::Find => app.update(Msg::FindOpen),
        BrowserIntent::FindNext => app.update(Msg::FindNext),
        BrowserIntent::FindPrev => app.update(Msg::FindPrev),
        BrowserIntent::Reload => app.update(Msg::NavReloadOrStop),
        BrowserIntent::HardReload => app.update(Msg::NavHardReload),
        BrowserIntent::Back => app.update(Msg::NavBack),
        BrowserIntent::Forward => app.update(Msg::NavForward),
        BrowserIntent::FocusUrl => {
            // ⌘L: focus the URL bar and select its contents (browser-standard)
            // so the next keystroke replaces the whole URL.
            app.set_url_bar_focused(true);
            Task::batch([focus_url_bar(), select_url_bar()])
        }
        BrowserIntent::Edit(cmd) => {
            // Idle omnibox unmounts the field, so `is_focused(url_input_id)`
            // yields Outcome::None and ⌘V would vanish. The tracked flag is
            // set on click / ⌘L / typing and cleared when the page takes
            // the press.
            tracing::debug!(?cmd, url_bar = app.url_bar_focused, "edit intent");
            app.update(Msg::EditRouted {
                cmd,
                url_bar_focused: app.url_bar_focused,
            })
        }
        BrowserIntent::ShowDevTools => {
            let _ = app.cmd_tx.send(crate::engine::Cmd::ShowDevTools {
                panel: "console".into(),
                inspect_x: None,
                inspect_y: None,
            });
            Task::none()
        }
        BrowserIntent::WrapPage => app.open_wrap_sheet(),
        BrowserIntent::AdoptWorkspace => {
            let tab = app.cached_active;
            app.adopt_workspace(tab);
            Task::none()
        }
        BrowserIntent::Quit => iced::exit(),
        BrowserIntent::None => Task::none(),
    }
}

/// Move keyboard focus to the chrome URL field.
pub(crate) fn focus_url_bar() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus::<Msg>(
        url_input_id(),
    ))
}

/// Drop iced widget focus (URL bar, dialogs) so key events reach the
/// webview shader after a click into the page.
pub(crate) fn unfocus_chrome() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus::<Msg>())
}

/// Select all text in the chrome URL field.
pub(crate) fn select_url_bar() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::text_input::select_all::<
        Msg,
    >(url_input_id()))
}

/// Query the chrome URL field's live focus state. Used to decide whether a
/// click just *gained* focus (→ select-all). Edit chords use the tracked
/// `url_bar_focused` flag — this query is `Outcome::None` while the idle
/// label is showing (the field is unmounted). The result arrives as a
/// message the caller chooses.
pub(crate) fn url_bar_is_focused<F>(to_msg: F) -> Task<Msg>
where
    F: Fn(bool) -> Msg + Send + 'static,
{
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::is_focused(
        url_input_id(),
    ))
    .map(to_msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribes_to_super_chords() {
        assert!(SUBSCRIBE.contains(&TopicKind::Chord));
        assert!(SUBSCRIBE.contains(&TopicKind::ChordReleased));
        assert!(SUBSCRIBE.contains(&TopicKind::MenuAction));
    }

    #[test]
    fn menu_actions_map_to_intents() {
        assert_eq!(intent_for_menu_action(ACTION_RELOAD), BrowserIntent::Reload);
        assert_eq!(
            intent_for_menu_action(ACTION_HARD_RELOAD),
            BrowserIntent::HardReload
        );
        assert_eq!(
            intent_for_menu_action(ACTION_CLOSE_TAB),
            BrowserIntent::CloseActiveTab
        );
        assert_eq!(
            intent_for_menu_action(ACTION_REOPEN_TAB),
            BrowserIntent::ReopenClosedTab
        );
        assert_eq!(intent_for_menu_action(ACTION_BACK), BrowserIntent::Back);
        assert_eq!(
            intent_for_menu_action(ACTION_FORWARD),
            BrowserIntent::Forward
        );
        assert_eq!(
            intent_for_menu_action(ACTION_FOCUS_URL),
            BrowserIntent::FocusUrl
        );
        assert_eq!(intent_for_menu_action(ACTION_FIND), BrowserIntent::Find);
        assert_eq!(
            intent_for_menu_action(ACTION_FIND_NEXT),
            BrowserIntent::FindNext
        );
        assert_eq!(
            intent_for_menu_action(ACTION_FIND_PREV),
            BrowserIntent::FindPrev
        );
        assert_eq!(intent_for_menu_action(ACTION_QUIT), BrowserIntent::Quit);
        assert_eq!(
            intent_for_menu_action(ACTION_DEVTOOLS),
            BrowserIntent::ShowDevTools
        );
    }

    #[test]
    fn new_tab_action_opens_blank_tab() {
        assert_eq!(
            intent_for_menu_action(ACTION_NEW_TAB),
            BrowserIntent::NewBlankTab
        );
    }

    #[test]
    fn reopen_closed_is_on_the_browser_menu() {
        let ids: Vec<&str> = MENU_ITEMS.iter().map(|(id, _, _)| *id).collect();
        assert!(ids.contains(&ACTION_REOPEN_TAB));
        assert_eq!(MENU_ITEMS[2].0, ACTION_REOPEN_TAB);
        // Groups went with the tab strip (a window per page).
        assert!(!ids.contains(&"new-group"));
        assert!(ids.contains(&ACTION_HARD_RELOAD));
        assert_eq!(MENU_ITEMS[4].0, ACTION_HARD_RELOAD);
    }

    #[test]
    fn edit_actions_map_to_edit_intents() {
        use crate::engine::EditCmd;
        assert_eq!(
            intent_for_menu_action(ACTION_EDIT_COPY),
            BrowserIntent::Edit(EditCmd::Copy)
        );
        assert_eq!(
            intent_for_menu_action(ACTION_EDIT_CUT),
            BrowserIntent::Edit(EditCmd::Cut)
        );
        assert_eq!(
            intent_for_menu_action(ACTION_EDIT_PASTE),
            BrowserIntent::Edit(EditCmd::Paste)
        );
        assert_eq!(
            intent_for_menu_action(ACTION_EDIT_SELECT_ALL),
            BrowserIntent::Edit(EditCmd::SelectAll)
        );
        assert_eq!(
            intent_for_menu_action(ACTION_EDIT_UNDO),
            BrowserIntent::Edit(EditCmd::Undo)
        );
        assert_eq!(
            intent_for_menu_action(ACTION_EDIT_REDO),
            BrowserIntent::Edit(EditCmd::Redo)
        );
    }

    /// Every key on the browser's menus survives the keybinding rules: an
    /// item that clashed with a shared key would lose it, and the shell
    /// would stop grabbing it (⌘V then reached nothing).
    #[test]
    fn menu_keys_survive_the_shared_keys() {
        let menu = browser_app_menu("sola-browser", true);
        assert_eq!(menu_keys(&sola_kit::keys::apply(&menu)), menu_keys(&menu));
    }

    /// Adopt Workspace is on a window's Browser menu only while its page
    /// runs another workspace's profile, beside Wrap This Page….
    #[test]
    fn adopt_workspace_follows_wrap_this_page_when_asked_for() {
        let ids = |adopt| -> Vec<String> {
            browser_app_menu("sola-browser", adopt).menus[0]
                .items
                .iter()
                .filter_map(|i| match i {
                    MenuItem::Action { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        assert!(!ids(false).iter().any(|id| id == ACTION_ADOPT_WORKSPACE));
        let with = ids(true);
        let wrap = with.iter().position(|id| id == ACTION_WRAP_PAGE).unwrap();
        assert_eq!(with[wrap + 1], ACTION_ADOPT_WORKSPACE);
        assert_eq!(
            intent_for_menu_action(ACTION_ADOPT_WORKSPACE),
            BrowserIntent::AdoptWorkspace
        );
    }

    #[test]
    fn edit_menu_items_cover_all_actions() {
        let ids: Vec<&str> = EDIT_MENU_ITEMS.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(
            ids,
            vec![
                ACTION_EDIT_UNDO,
                ACTION_EDIT_REDO,
                ACTION_EDIT_CUT,
                ACTION_EDIT_COPY,
                ACTION_EDIT_PASTE,
                ACTION_EDIT_SELECT_ALL,
                ACTION_FIND,
                ACTION_FIND_NEXT,
                ACTION_FIND_PREV,
            ]
        );
    }

    #[test]
    fn unknown_action_is_none() {
        assert_eq!(intent_for_menu_action("bogus"), BrowserIntent::None);
    }

    #[test]
    fn open_url_self_echo_is_detected() {
        assert!(open_url_is_self_echo("sola-browser", "sola-browser"));
        assert!(!open_url_is_self_echo("sola-mail", "sola-browser"));
        assert!(!open_url_is_self_echo("", "sola-browser"));
    }

    #[test]
    fn open_url_honors_activate_and_url() {
        let req = OpenUrlRequest {
            url: "https://slate.auto".into(),
            activate: false,
        };
        assert_eq!(
            intent_for_open_url(&req),
            BrowserIntent::NewTab {
                url: "https://slate.auto".into(),
                activate: false
            }
        );
    }
}
