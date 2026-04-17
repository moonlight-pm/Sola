use serde::{Deserialize, Serialize};
pub use sola_core::KeyChord;

use crate::define_topics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub window_id: u32,
    pub app_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenUrlRequest {
    pub url: String,
    pub activate: bool,
}

/// Outcome of a `LaunchApp` spawn attempt, emitted by `sola`.
/// `ok=true` means the process was spawned; it does not guarantee the
/// process stayed alive. `error` is populated when `ok=false`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchResultPayload {
    pub command: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Emitted by `sola` whenever a user app process exits. Exactly one of
/// `code` or `signal` is set: `code` on normal exit, `signal` when killed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAppExitedPayload {
    pub command: String,
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// Z-ordered entry in the composition list. Bottom to top.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionEntry {
    pub window_id: u32,
}

/// Per-surface position and size. Applied immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameUpdate {
    pub window_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Which surface receives keyboard focus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusTarget {
    pub window_id: u32,
}

/// Output resolution, emitted by compositor on startup and hotplug.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputGeometry {
    pub width: i32,
    pub height: i32,
}

/// Emitted by compositor when pointer enters a different surface/window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEnteredPayload {
    pub window_id: u32,
}

/// App menu definition, emitted as sticky by apps at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMenuPayload {
    pub app_id: String,
    pub menus: Vec<MenuDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuDefinition {
    pub label: String,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MenuItem {
    Action {
        id: String,
        label: String,
        shortcut: Option<KeyChord>,
        disabled: bool,
        checked: bool,
    },
    Divider,
}

/// Dispatched by the shell when a shortcut or menu click maps to an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuActionPayload {
    pub app_id: String,
    pub action_id: String,
}

/// Keyboard chord the shell wants routed to it by sola-river.
/// Emitted (as a list) sticky by the shell on startup and whenever
/// the registered set changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RegisteredChord {
    pub keysym: u32,
    pub modifiers: u32,
}

/// A chord press, emitted by sola-river when a previously registered
/// chord fires on the seat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChordEvent {
    pub keysym: u32,
    pub modifiers: u32,
}

/// Mouse click targeted at a specific window, emitted by sola-river
/// for `window_interaction` seat events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseClickedPayload {
    pub window_id: u32,
}

/// Request that a specific window perform an edit action (copy or paste).
/// Emitted by the shell when a global clipboard chord fires; consumed by
/// the sola-app framework in the owning process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRequest {
    pub window_id: u32,
}

/// Named xkb keymap profile for the seat, emitted by the shell when
/// focus moves between Sola and non-Sola apps. Consumed by sola-river,
/// which pushes the corresponding keymap to River.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XkbProfilePayload {
    pub profile: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    TopMiddle,
    BottomMiddle,
    FullMiddle,
    Fullscreen,
}

impl Zone {
    /// Returns (x%, y%, width%, height%) as fractions of the output.
    pub fn rect(&self) -> (f64, f64, f64, f64) {
        match self {
            Zone::Left => (0.0, 0.0, 0.28, 1.0),
            Zone::Right => (0.72, 0.0, 0.28, 1.0),
            Zone::Top => (0.0, 0.0, 1.0, 0.7),
            Zone::Bottom => (0.0, 0.7, 1.0, 0.3),
            Zone::TopMiddle => (0.28, 0.0, 0.44, 0.7),
            Zone::BottomMiddle => (0.28, 0.7, 0.44, 0.3),
            Zone::FullMiddle => (0.28, 0.0, 0.44, 1.0),
            Zone::Fullscreen => (0.0, 0.0, 1.0, 1.0),
        }
    }
}

define_topics! {
    // Window management list (sticky)
    Apps(Vec<App>),
    LaunchApp(String),
    LaunchResult(LaunchResultPayload),
    UserAppExited(UserAppExitedPayload),

    // Composition authority (shell → sola-river)
    Composition(Vec<CompositionEntry>),
    Frame(FrameUpdate),
    Focus(FocusTarget),

    // Output
    OutputGeometry(OutputGeometry),

    // Mouse events (sola-river → shell)
    MouseEntered(MouseEnteredPayload),
    MouseLeft,
    MouseClicked(MouseClickedPayload),

    // Keyboard (shell ↔ sola-river)
    RegisteredChords(Vec<RegisteredChord>),
    Chord(ChordEvent),
    ChordReleased(ChordEvent),

    // Menus
    SetAppMenu(AppMenuPayload),
    MenuAction(MenuActionPayload),

    // Browser
    OpenUrl(OpenUrlRequest),

    // Global clipboard chords (shell → focused sola-app)
    Copy(EditRequest),
    Paste(EditRequest),

    // Focus-driven xkb profile (shell → sola-river, sticky)
    XkbProfile(XkbProfilePayload),

    // Lifecycle
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_topic_roundtrip() {
        let msg = Topic::Shutdown.to_message();
        assert_eq!(msg.topic, "Shutdown");
        assert!(msg.payload.is_none());

        let parsed = Topic::parse(&msg).unwrap();
        assert!(matches!(parsed, Topic::Shutdown));
    }

    #[test]
    fn payload_topic_roundtrip() {
        let apps = vec![App {
            window_id: 1,
            app_id: "zen".into(),
            title: "Browser".into(),
        }];
        let msg = Topic::Apps(apps).to_message();
        assert_eq!(msg.topic, "Apps");

        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::Apps(decoded) => {
                assert_eq!(decoded.len(), 1);
                assert_eq!(decoded[0].app_id, "zen");
                assert_eq!(decoded[0].window_id, 1);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn unknown_topic_returns_none() {
        let msg = crate::Message::new("SomeUnknownTopic");
        assert!(Topic::parse(&msg).is_none());
    }

    #[test]
    fn app_menu_roundtrip() {
        let payload = AppMenuPayload {
            app_id: "test-app".into(),
            menus: vec![MenuDefinition {
                label: "File".into(),
                items: vec![
                    MenuItem::Action {
                        id: "new".into(),
                        label: "New".into(),
                        shortcut: Some(sola_core::KeyCode::N.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                ],
            }],
        };
        let msg = Topic::SetAppMenu(payload).to_message();
        match Topic::parse(&msg) {
            Some(Topic::SetAppMenu(p)) => {
                assert_eq!(p.app_id, "test-app");
                assert_eq!(p.menus.len(), 1);
                assert_eq!(p.menus[0].items.len(), 2);
            }
            other => panic!("expected SetAppMenu, got {other:?}"),
        }
    }
}
