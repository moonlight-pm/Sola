//! Sola-bus integration for the browser chrome.
//!
//! Wires the running browser into the Sola bus the way kit apps do (see
//! `sola-monitor`). The connect + subscribe + app-menu publish happens once in
//! `run()` via `sola_kit::app::BusSetup`; this module is the receive side.
//! It reacts to:
//!
//! - `Topic::OpenUrl` — open a fresh tab (focused per `activate`),
//! - `Topic::MenuAction` from the published "Browser" menu — the
//!   keyboard-shortcut mechanism (only ⌘/meta items fire; non-meta keys reach
//!   the page),
//! - `Topic::Theme` — restyle the chrome live (handled by the kit helper),
//! - self-addressed quit (`MenuAction "quit"` / `CloseApp`).
//!
//! The `Topic` → `BrowserIntent` mapping is pure so it can be unit-tested
//! without a running bus.

use std::sync::Arc;

use iced::Task;
use sola_bus::topics::{OpenUrlRequest, Topic, TopicKind};
use sola_bus::Message;
use sola_core::{KeyChord, KeyCode};

use crate::app::{App, BLANK_URL, Msg};
use crate::engine::{EditCmd, Engine};

// Menu action ids — shared between the published menu and the handler so the
// two never drift.
pub const ACTION_NEW_TAB: &str = "new-tab";
pub const ACTION_CLOSE_TAB: &str = "close-tab";
pub const ACTION_RELOAD: &str = "reload";
pub const ACTION_FOCUS_URL: &str = "focus-url";
pub const ACTION_BACK: &str = "back";
pub const ACTION_FORWARD: &str = "forward";
pub const ACTION_QUIT: &str = "quit";
pub const ACTION_EDIT_UNDO: &str = "edit-undo";
pub const ACTION_EDIT_REDO: &str = "edit-redo";
pub const ACTION_EDIT_CUT: &str = "edit-cut";
pub const ACTION_EDIT_COPY: &str = "edit-copy";
pub const ACTION_EDIT_PASTE: &str = "edit-paste";
pub const ACTION_EDIT_SELECT_ALL: &str = "edit-select-all";

/// Topics the browser subscribes to. Theme/OpenUrl/MenuAction are the live
/// inputs; CloseApp is the shell's "quit this app" signal (via `is_self_quit`).
pub const SUBSCRIBE: &[TopicKind] = &[
    TopicKind::Theme,
    TopicKind::OpenUrl,
    TopicKind::MenuAction,
    TopicKind::CloseApp,
];

/// The "Browser" app-menu published to the shell at startup. Each entry is
/// `(action_id, label, chord)`; chords are meta-bound. The shell binds them
/// globally and routes `Topic::MenuAction` back when one is pressed.
pub const MENU_ITEMS: [(&str, &str, KeyChord); 7] = [
    (ACTION_NEW_TAB, "New Tab", KeyCode::T.meta()),
    (ACTION_CLOSE_TAB, "Close Tab", KeyCode::W.meta()),
    (ACTION_RELOAD, "Reload", KeyCode::R.meta()),
    (ACTION_FOCUS_URL, "Focus URL", KeyCode::L.meta()),
    (ACTION_BACK, "Back", KeyCode::LEFT.meta()),
    (ACTION_FORWARD, "Forward", KeyCode::RIGHT.meta()),
    (ACTION_QUIT, "Quit Browser", KeyCode::Q.meta()),
];

/// The "Edit" app-menu published alongside "Browser". Meta-bound so the
/// shell grabs them globally and routes `Topic::MenuAction` back; the
/// browser then routes each to the focused surface (web content or URL bar).
pub const EDIT_MENU_ITEMS: [(&str, &str, KeyChord); 6] = [
    (ACTION_EDIT_UNDO, "Undo", KeyCode::Z.meta()),
    (ACTION_EDIT_REDO, "Redo", KeyCode::Z.meta_shift()),
    (ACTION_EDIT_CUT, "Cut", KeyCode::X.meta()),
    (ACTION_EDIT_COPY, "Copy", KeyCode::C.meta()),
    (ACTION_EDIT_PASTE, "Paste", KeyCode::V.meta()),
    (ACTION_EDIT_SELECT_ALL, "Select All", KeyCode::A.meta()),
];

/// Stable widget id for the chrome URL field, so the `Focus URL` action can
/// move keyboard focus to it.
pub fn url_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("browser-url-bar")
}

/// What the browser should do in response to a bus event. A plain enum keeps
/// the mapping pure and unit-testable without a bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserIntent {
    /// Open a tab loading `url`, focused per `activate` (bus-driven OpenUrl).
    NewTab { url: String, activate: bool },
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
    Quit,
    None,
}

/// Which surface an `Edit` intent acts on, chosen by `url_bar_focused`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTarget {
    /// The web content (full fidelity — honors the page's text selection).
    Engine,
    /// The chrome URL bar (best-effort: whole-field, no partial selection).
    UrlBar,
}

/// Route an `Edit` intent: the URL bar when it holds focus, else the page.
pub fn edit_target(url_bar_focused: bool) -> EditTarget {
    if url_bar_focused {
        EditTarget::UrlBar
    } else {
        EditTarget::Engine
    }
}

/// Map an `OpenUrlRequest` to an intent: always a fresh tab, focused per
/// `activate` (matches the retired GTK browser's behaviour).
pub fn intent_for_open_url(req: &OpenUrlRequest) -> BrowserIntent {
    BrowserIntent::NewTab { url: req.url.clone(), activate: req.activate }
}

/// Map a menu `action_id` to an intent. Unknown ids are ignored.
pub fn intent_for_menu_action(action_id: &str) -> BrowserIntent {
    match action_id {
        ACTION_NEW_TAB => BrowserIntent::NewBlankTab,
        ACTION_CLOSE_TAB => BrowserIntent::CloseActiveTab,
        ACTION_RELOAD => BrowserIntent::Reload,
        ACTION_FOCUS_URL => BrowserIntent::FocusUrl,
        ACTION_BACK => BrowserIntent::Back,
        ACTION_FORWARD => BrowserIntent::Forward,
        ACTION_QUIT => BrowserIntent::Quit,
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
pub fn handle_bus<E: Engine>(app: &mut App<E>, message: Arc<Message>, app_id: &'static str) -> Task<Msg> {
    // Theme first: restyle the chrome live (also installs the font roles).
    if sola_kit::app::apply_theme_update(&message, &mut app.theme) {
        return Task::none();
    }
    // Self-addressed quit (MenuAction "quit" or CloseApp).
    if sola_kit::app::is_self_quit(&message, app_id) {
        return iced::exit();
    }
    match Topic::parse(&message) {
        Some(Topic::OpenUrl(req)) => run_intent(app, intent_for_open_url(&req)),
        Some(Topic::MenuAction(m)) if m.app_id == app_id => {
            run_intent(app, intent_for_menu_action(&m.action_id))
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
            app.url_field.clear();
            app.last_seen_url = BLANK_URL.to_string();
            focus_url_bar()
        }
        BrowserIntent::CloseActiveTab => {
            let id = app.cached_active;
            app.update(Msg::CloseTab(id))
        }
        BrowserIntent::Reload => app.update(Msg::NavReload),
        BrowserIntent::Back => app.update(Msg::NavBack),
        BrowserIntent::Forward => app.update(Msg::NavForward),
        BrowserIntent::FocusUrl => focus_url_bar(),
        BrowserIntent::Edit(_) => Task::none(),
        BrowserIntent::Quit => iced::exit(),
        BrowserIntent::None => Task::none(),
    }
}

/// Move keyboard focus to the chrome URL field.
fn focus_url_bar() -> Task<Msg> {
    iced::advanced::widget::operate(
        iced::advanced::widget::operation::focusable::focus::<Msg>(url_input_id()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_actions_map_to_intents() {
        assert_eq!(intent_for_menu_action(ACTION_RELOAD), BrowserIntent::Reload);
        assert_eq!(intent_for_menu_action(ACTION_CLOSE_TAB), BrowserIntent::CloseActiveTab);
        assert_eq!(intent_for_menu_action(ACTION_BACK), BrowserIntent::Back);
        assert_eq!(intent_for_menu_action(ACTION_FORWARD), BrowserIntent::Forward);
        assert_eq!(intent_for_menu_action(ACTION_FOCUS_URL), BrowserIntent::FocusUrl);
        assert_eq!(intent_for_menu_action(ACTION_QUIT), BrowserIntent::Quit);
    }

    #[test]
    fn new_tab_action_opens_blank_tab() {
        assert_eq!(intent_for_menu_action(ACTION_NEW_TAB), BrowserIntent::NewBlankTab);
    }

    #[test]
    fn edit_actions_map_to_edit_intents() {
        use crate::engine::EditCmd;
        assert_eq!(intent_for_menu_action(ACTION_EDIT_COPY), BrowserIntent::Edit(EditCmd::Copy));
        assert_eq!(intent_for_menu_action(ACTION_EDIT_CUT), BrowserIntent::Edit(EditCmd::Cut));
        assert_eq!(intent_for_menu_action(ACTION_EDIT_PASTE), BrowserIntent::Edit(EditCmd::Paste));
        assert_eq!(intent_for_menu_action(ACTION_EDIT_SELECT_ALL), BrowserIntent::Edit(EditCmd::SelectAll));
        assert_eq!(intent_for_menu_action(ACTION_EDIT_UNDO), BrowserIntent::Edit(EditCmd::Undo));
        assert_eq!(intent_for_menu_action(ACTION_EDIT_REDO), BrowserIntent::Edit(EditCmd::Redo));
    }

    #[test]
    fn edit_target_routes_by_focus() {
        assert_eq!(edit_target(true), EditTarget::UrlBar);
        assert_eq!(edit_target(false), EditTarget::Engine);
    }

    #[test]
    fn edit_menu_items_cover_all_actions() {
        let ids: Vec<&str> = EDIT_MENU_ITEMS.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(
            ids,
            vec![
                ACTION_EDIT_UNDO, ACTION_EDIT_REDO, ACTION_EDIT_CUT,
                ACTION_EDIT_COPY, ACTION_EDIT_PASTE, ACTION_EDIT_SELECT_ALL,
            ]
        );
    }

    #[test]
    fn unknown_action_is_none() {
        assert_eq!(intent_for_menu_action("bogus"), BrowserIntent::None);
    }

    #[test]
    fn open_url_honors_activate_and_url() {
        let req = OpenUrlRequest { url: "https://slate.auto".into(), activate: false };
        assert_eq!(
            intent_for_open_url(&req),
            BrowserIntent::NewTab { url: "https://slate.auto".into(), activate: false }
        );
    }
}
