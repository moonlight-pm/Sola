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
use crate::engine::Engine;

// Menu action ids — shared between the published menu and the handler so the
// two never drift.
pub const ACTION_NEW_TAB: &str = "new-tab";
pub const ACTION_CLOSE_TAB: &str = "close-tab";
pub const ACTION_RELOAD: &str = "reload";
pub const ACTION_FOCUS_URL: &str = "focus-url";
pub const ACTION_BACK: &str = "back";
pub const ACTION_FORWARD: &str = "forward";
pub const ACTION_QUIT: &str = "quit";

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
    Quit,
    None,
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
