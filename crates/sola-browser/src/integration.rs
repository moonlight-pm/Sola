//! Sola-bus integration for the browser chrome.
//!
//! Wires the running browser into the Sola bus the way kit apps do (see
//! `sola-monitor`). The connect + subscribe + app-menu publish happens once in
//! `run()` via `sola_kit::app::BusSetup`; this module is the receive side.
//! It reacts to:
//!
//! - `Topic::OpenUrl` — open a fresh tab (focused per `activate`);
//!   a `chrome.sock` handoff re-emits this so the shell can raise the
//!   window. Our own echo is ignored.
//! - `Topic::MenuAction` from published menus — keyboard shortcuts and
//!   menubar clicks (Profiles switch / manage included),
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

use crate::app::{App, BLANK_URL, Msg, ProfileDialog};
use crate::engine::{EditCmd, Engine};
use crate::profiles;

// Menu action ids — shared between the published menu and the handler so the
// two never drift.
pub const ACTION_NEW_TAB: &str = "new-tab";
pub const ACTION_CLOSE_TAB: &str = "close-tab";
pub const ACTION_RELOAD: &str = "reload";
pub const ACTION_FOCUS_URL: &str = "focus-url";
pub const ACTION_BACK: &str = "back";
pub const ACTION_FORWARD: &str = "forward";
pub const ACTION_QUIT: &str = "quit";
pub const ACTION_DEVTOOLS: &str = "devtools";
pub const ACTION_EDIT_CUT: &str = "edit-cut";
pub const ACTION_EDIT_COPY: &str = "edit-copy";
pub const ACTION_EDIT_PASTE: &str = "edit-paste";
pub const ACTION_EDIT_SELECT_ALL: &str = "edit-select-all";
pub const ACTION_PROFILE_NEW: &str = "profile-new";
pub const ACTION_PROFILE_RENAME: &str = "profile-rename";
pub const ACTION_PROFILE_DELETE: &str = "profile-delete";
/// Prefix for per-profile switch actions: `profile-switch:<uuid>`.
pub const ACTION_PROFILE_SWITCH_PREFIX: &str = "profile-switch:";
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
    TopicKind::WindowFloating,
    TopicKind::OpenUrl,
    TopicKind::Chord,
    TopicKind::ChordReleased,
    TopicKind::NotificationActivate,
];

/// The "Browser" app-menu published to the shell at startup. Each entry is
/// `(action_id, label, chord)`; chords are meta-bound. The shell binds them
/// globally and routes `Topic::MenuAction` back when one is pressed.
pub const MENU_ITEMS: [(&str, &str, KeyChord); 8] = [
    (ACTION_NEW_TAB, "New Tab", KeyCode::T.meta()),
    (ACTION_CLOSE_TAB, "Close Tab", KeyCode::W.meta()),
    (ACTION_RELOAD, "Reload", KeyCode::R.meta()),
    (ACTION_FOCUS_URL, "Focus URL", KeyCode::L.meta()),
    (ACTION_BACK, "Back", KeyCode::LEFT.meta()),
    (ACTION_FORWARD, "Forward", KeyCode::RIGHT.meta()),
    (ACTION_DEVTOOLS, "Developer Tools", KeyCode::I.meta().alt()),
    (ACTION_QUIT, "Quit Browser", KeyCode::Q.meta()),
];

/// The "Edit" app-menu published alongside "Browser". Meta-bound so the
/// shell grabs them globally and routes `Topic::MenuAction` back; the
/// browser then routes each to the focused surface (web content or URL bar).
/// Undo/Redo are intentionally omitted — in a browser they only act on the
/// editable text field that currently has focus, which is too narrow to earn
/// a top-level menu slot here.
pub const EDIT_MENU_ITEMS: [(&str, &str, KeyChord); 4] = [
    (ACTION_EDIT_CUT, "Cut", KeyCode::X.meta()),
    (ACTION_EDIT_COPY, "Copy", KeyCode::C.meta()),
    (ACTION_EDIT_PASTE, "Paste", KeyCode::V.meta()),
    (ACTION_EDIT_SELECT_ALL, "Select All", KeyCode::A.meta()),
];

/// Full menubar for the browser (Browser + Edit + Profiles). Rebuilt when
/// the profile list changes so switcher checkmarks stay accurate.
pub fn browser_app_menu(app_id: &str) -> AppMenuPayload {
    AppMenuPayload {
        app_id: app_id.into(),
        menus: vec![
            menu_from_items("Browser", &MENU_ITEMS),
            menu_from_items("Edit", &EDIT_MENU_ITEMS),
            profiles_menu(),
        ],
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

/// Profiles menubar: list (checked **registry** active) + New / Rename / Delete.
pub fn profiles_menu() -> MenuDefinition {
    let active_id = profiles::registry_active_id();
    let entries = profiles::list();
    let only_one = entries.len() <= 1;

    let mut items: Vec<MenuItem> = entries
        .into_iter()
        .map(|p| {
            let is_active = p.id == active_id;
            MenuItem::Action {
                id: format!("{ACTION_PROFILE_SWITCH_PREFIX}{}", p.id),
                label: p.name,
                shortcut: None,
                disabled: false,
                checked: is_active,
            }
        })
        .collect();

    items.push(MenuItem::Divider);
    items.push(MenuItem::Action {
        id: ACTION_PROFILE_NEW.into(),
        label: "New Profile…".into(),
        shortcut: None,
        disabled: false,
        checked: false,
    });
    items.push(MenuItem::Action {
        id: ACTION_PROFILE_RENAME.into(),
        label: "Rename Profile…".into(),
        shortcut: None,
        disabled: false,
        checked: false,
    });
    items.push(MenuItem::Action {
        id: ACTION_PROFILE_DELETE.into(),
        label: "Delete Profile…".into(),
        shortcut: None,
        disabled: only_one,
        checked: false,
    });

    MenuDefinition {
        label: "Profiles".into(),
        items,
    }
}

/// Re-publish Browser + Edit + Profiles menus (after create/rename/delete).
pub fn republish_menus(app_id: &str) {
    if let Ok(mut client) = sola_kit::app::bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(browser_app_menu(app_id))) {
            tracing::warn!(error = %e, "republish browser menus failed");
        }
    }
}

/// Stable widget id for the chrome URL field, so the `Focus URL` action can
/// move keyboard focus to it.
pub fn url_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("browser-url-bar")
}

pub fn profile_name_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("browser-profile-name")
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
    Reload,
    Back,
    Forward,
    FocusUrl,
    /// Run an editing command, routed to the focused surface.
    Edit(EditCmd),
    /// Switch to profile `id` (save session, set active, same window).
    SwitchProfile {
        id: String,
    },
    /// Open the new-profile name dialog.
    NewProfile,
    /// Open rename dialog for the active profile.
    RenameProfile,
    /// Open delete confirmation for the active profile.
    DeleteProfile,
    /// Open DevTools (console) for the active tab.
    ShowDevTools,
    Quit,
    None,
}

/// True when this `OpenUrl` is the chrome re-broadcasting a `chrome.sock`
/// handoff so the shell will raise the window. The tab is already open.
pub fn open_url_is_self_echo(source: &str, app_id: &str) -> bool {
    source == app_id
}

/// Re-broadcast a sock-handoff as `Topic::OpenUrl` so the shell raises
/// the existing window (same path as mail / bus opens). The chrome
/// ignores its own echo in [`handle_bus`].
pub fn emit_open_url_for_raise(url: &str) {
    if let Ok(mut bus) = sola_kit::app::bus().lock() {
        let _ = bus.emit(Topic::OpenUrl(OpenUrlRequest {
            url: url.to_string(),
            activate: true,
        }));
    }
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
    if let Some(id) = action_id.strip_prefix(ACTION_PROFILE_SWITCH_PREFIX) {
        return BrowserIntent::SwitchProfile { id: id.to_string() };
    }
    match action_id {
        ACTION_NEW_TAB => BrowserIntent::NewBlankTab,
        ACTION_CLOSE_TAB => BrowserIntent::CloseActiveTab,
        ACTION_RELOAD => BrowserIntent::Reload,
        ACTION_FOCUS_URL => BrowserIntent::FocusUrl,
        ACTION_BACK => BrowserIntent::Back,
        ACTION_FORWARD => BrowserIntent::Forward,
        ACTION_QUIT => BrowserIntent::Quit,
        ACTION_DEVTOOLS => BrowserIntent::ShowDevTools,
        ACTION_EDIT_CUT => BrowserIntent::Edit(EditCmd::Cut),
        ACTION_EDIT_COPY => BrowserIntent::Edit(EditCmd::Copy),
        ACTION_EDIT_PASTE => BrowserIntent::Edit(EditCmd::Paste),
        ACTION_EDIT_SELECT_ALL => BrowserIntent::Edit(EditCmd::SelectAll),
        ACTION_PROFILE_NEW => BrowserIntent::NewProfile,
        ACTION_PROFILE_RENAME => BrowserIntent::RenameProfile,
        ACTION_PROFILE_DELETE => BrowserIntent::DeleteProfile,
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
    match Topic::parse(&message) {
        Some(Topic::OpenUrl(req)) => {
            // Sock handoff re-emits OpenUrl so the shell can raise us.
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
                profile = %crate::profiles::active().name,
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
            app.open_tab(url, activate);
            Task::none()
        }
        BrowserIntent::NewBlankTab => {
            app.open_tab(BLANK_URL.to_string(), true);
            // Empty the URL bar and move keyboard focus to it so the user can
            // type a URL or search immediately. The blank tab's "about:blank"
            // is suppressed from the field (see `Msg::Tick`); seeding
            // `last_seen_url` here avoids a one-frame flash of it.
            //
            // Prefer URL-bar focus for a blank tab so typing starts immediately.
            app.url_field.clear();
            app.last_seen_url = BLANK_URL.to_string();
            app.url_bar_focused = true;
            Task::batch([focus_url_bar(), select_url_bar()])
        }
        BrowserIntent::CloseActiveTab => {
            let id = app.cached_active;
            app.update(Msg::CloseTab(id))
        }
        BrowserIntent::Reload => app.update(Msg::NavReloadOrStop),
        BrowserIntent::Back => app.update(Msg::NavBack),
        BrowserIntent::Forward => app.update(Msg::NavForward),
        BrowserIntent::FocusUrl => {
            // ⌘L: focus the URL bar and select its contents (browser-standard)
            // so the next keystroke replaces the whole URL.
            app.url_bar_focused = true;
            Task::batch([focus_url_bar(), select_url_bar()])
        }
        BrowserIntent::Edit(cmd) => {
            // Route by the URL bar's *live* focus. iced doesn't surface focus
            // as state, and a click into the field is captured by `text_input`
            // before any wrapper widget can observe it — so a tracked bool
            // can't be kept honest. Query the real focus via an operation and
            // finish the routing in `Msg::EditRouted`.
            tracing::debug!(?cmd, "edit intent — querying live URL-bar focus");
            url_bar_is_focused(move |url_bar_focused| Msg::EditRouted {
                cmd,
                url_bar_focused,
            })
        }
        BrowserIntent::SwitchProfile { id } => app.switch_profile(&id),
        BrowserIntent::NewProfile => {
            app.open_profile_dialog(ProfileDialog::New);
            focus_profile_name()
        }
        BrowserIntent::RenameProfile => {
            app.open_profile_dialog(ProfileDialog::Rename);
            focus_profile_name()
        }
        BrowserIntent::DeleteProfile => {
            app.open_profile_dialog(ProfileDialog::DeleteConfirm);
            Task::none()
        }
        BrowserIntent::ShowDevTools => {
            let _ = app.cmd_tx.send(crate::engine::Cmd::ShowDevTools {
                panel: "console".into(),
                inspect_x: None,
                inspect_y: None,
            });
            Task::none()
        }
        BrowserIntent::Quit => iced::exit(),
        BrowserIntent::None => Task::none(),
    }
}

/// Move keyboard focus to the chrome URL field.
fn focus_url_bar() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus::<Msg>(
        url_input_id(),
    ))
}

/// Drop iced widget focus (URL bar, dialogs) so key events reach the
/// webview shader after a click into the page.
pub(crate) fn unfocus_chrome() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus::<Msg>())
}

fn focus_profile_name() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus::<Msg>(
        profile_name_input_id(),
    ))
}

/// Select all text in the chrome URL field.
pub(crate) fn select_url_bar() -> Task<Msg> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::text_input::select_all::<
        Msg,
    >(url_input_id()))
}

/// Query the chrome URL field's live focus state. Used to route Edit
/// actions (⌘C/⌘X/⌘V/⌘A) to the field vs. the page, and to decide whether a
/// click just *gained* focus (→ select-all). The result arrives as a message
/// the caller chooses.
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
            intent_for_menu_action(ACTION_CLOSE_TAB),
            BrowserIntent::CloseActiveTab
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
    }

    #[test]
    fn edit_menu_items_cover_all_actions() {
        // Undo/Redo are intentionally absent (see EDIT_MENU_ITEMS).
        let ids: Vec<&str> = EDIT_MENU_ITEMS.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(
            ids,
            vec![
                ACTION_EDIT_CUT,
                ACTION_EDIT_COPY,
                ACTION_EDIT_PASTE,
                ACTION_EDIT_SELECT_ALL,
            ]
        );
    }

    #[test]
    fn profile_actions_map() {
        assert_eq!(
            intent_for_menu_action(ACTION_PROFILE_NEW),
            BrowserIntent::NewProfile
        );
        assert_eq!(
            intent_for_menu_action(ACTION_PROFILE_RENAME),
            BrowserIntent::RenameProfile
        );
        assert_eq!(
            intent_for_menu_action(ACTION_PROFILE_DELETE),
            BrowserIntent::DeleteProfile
        );
        assert_eq!(
            intent_for_menu_action("profile-switch:abc-123"),
            BrowserIntent::SwitchProfile {
                id: "abc-123".into()
            }
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
