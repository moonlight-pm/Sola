use serde::{Deserialize, Serialize};

use crate::define_topics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub app_id: String,
    pub name: String,
    pub icon: String,
    pub window_count: u32,
}

/// A key event forwarded over the bus (Super+key combos).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEvent {
    pub code: u32,
    pub pressed: bool,
    pub super_held: bool,
    pub shift_held: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenUrlRequest {
    pub url: String,
    pub activate: bool,
}

/// Window geometry from sola-x for X11 window positioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub app_id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Output resolution, emitted by compositor on startup and hotplug.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputGeometry {
    pub width: i32,
    pub height: i32,
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
        shortcut: Option<String>,
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
    /// If true, compositor gives keyboard focus on map.
    pub auto_focus: bool,
    /// Fixed size for unzoned windows (width, height).
    #[serde(default)]
    pub size: Option<(i32, i32)>,
    /// Fixed position for unzoned windows (x, y).
    #[serde(default)]
    pub position: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Zone {
    Left,
    Right,
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
            Zone::TopMiddle => (0.28, 0.0, 0.44, 0.7),
            Zone::BottomMiddle => (0.28, 0.7, 0.44, 0.3),
            Zone::FullMiddle => (0.28, 0.0, 0.44, 1.0),
            Zone::Fullscreen => (0.0, 0.0, 1.0, 1.0),
        }
    }
}

define_topics! {
    // Input routing
    Key(KeyEvent),
    GrabInput(String),
    ReleaseInput,

    // App management
    ListApps,
    Apps(Vec<App>),
    RaiseApp(String),
    FocusChanged(String),
    LaunchApp(String),

    // Window management
    SetWindowPolicy(WindowPolicyPayload),
    SetWindowGeometry(WindowGeometry),
    OutputGeometry(OutputGeometry),

    // Menus
    SetAppMenu(AppMenuPayload),
    MenuAction(MenuActionPayload),

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
        let msg = Topic::ReleaseInput.to_message();
        assert_eq!(msg.topic, "ReleaseInput");
        assert!(msg.payload.is_none());

        let parsed = Topic::parse(&msg).unwrap();
        assert!(matches!(parsed, Topic::ReleaseInput));
    }

    #[test]
    fn payload_topic_roundtrip() {
        let apps = vec![App {
            app_id: "zen".into(),
            name: "Browser".into(),
            icon: "globe".into(),
            window_count: 2,
        }];
        let msg = Topic::Apps(apps).to_message();
        assert_eq!(msg.topic, "Apps");

        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::Apps(decoded) => {
                assert_eq!(decoded.len(), 1);
                assert_eq!(decoded[0].app_id, "zen");
                assert_eq!(decoded[0].window_count, 2);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn key_event_roundtrip() {
        let msg = Topic::Key(KeyEvent {
            code: 23,
            pressed: true,
            super_held: true,
            shift_held: false,
        })
        .to_message();
        assert_eq!(msg.topic, "Key");

        match Topic::parse(&msg).unwrap() {
            Topic::Key(k) => {
                assert_eq!(k.code, 23);
                assert!(k.pressed);
                assert!(k.super_held);
                assert!(!k.shift_held);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn grab_input_roundtrip() {
        let msg = Topic::GrabInput("sola-switcher".into()).to_message();

        match Topic::parse(&msg).unwrap() {
            Topic::GrabInput(target) => assert_eq!(target, "sola-switcher"),
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
                        shortcut: Some("Super+N".into()),
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
