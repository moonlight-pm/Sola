use serde::{Deserialize, Serialize};
pub use sola_core::KeyChord;

use crate::define_topics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub window_id: u32,
    pub app_id: String,
    pub title: String,
    /// If this window is a child of another (X11 transient_for), the
    /// parent's window_id. The shell should not independently frame
    /// or zone transient windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_window_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenUrlRequest {
    pub url: String,
    pub activate: bool,
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

/// Declares how an app's windows should be managed by the compositor.
/// Emitted as sticky by apps at startup, before mapping surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowPolicyPayload {
    pub app_id: String,
    pub windows: Vec<WindowPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowPolicy {
    /// Matches xdg_toplevel title for surface identification.
    pub title: String,
    /// If true, the shell manages position/size via zones.
    pub zoned: bool,
    /// If true, compositor routes Meta+key events to this surface.
    #[serde(default)]
    pub keyboard_target: bool,
    /// Fixed size for unzoned windows (width, height).
    #[serde(default)]
    pub size: Option<(i32, i32)>,
    /// Fixed position for unzoned windows (x, y).
    #[serde(default)]
    pub position: Option<(i32, i32)>,
}

/// Declares which key chords the shell wants intercepted.
/// Emitted as sticky by the shell; compositor uses this as a routing allowlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellKeyBindingsPayload {
    pub app_id: String,
    pub bindings: Vec<KeyChord>,
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
    // Window management list
    Windows(Vec<WindowInfo>),
    LaunchApp(String),

    // Composition authority (shell → compositor)
    Composition(Vec<CompositionEntry>),
    Frame(FrameUpdate),
    Focus(FocusTarget),

    // Window management
    SetWindowPolicy(WindowPolicyPayload),
    OutputGeometry(OutputGeometry),
    MouseEntered(MouseEnteredPayload),

    // Menus
    SetAppMenu(AppMenuPayload),
    MenuAction(MenuActionPayload),

    // Shell input routing
    ShellKeyBindings(ShellKeyBindingsPayload),

    // Browser
    OpenUrl(OpenUrlRequest),

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
        let windows = vec![WindowInfo {
            window_id: 1,
            app_id: "zen".into(),
            title: "Browser".into(),
        }];
        let msg = Topic::Windows(windows).to_message();
        assert_eq!(msg.topic, "Windows");

        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::Windows(decoded) => {
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
