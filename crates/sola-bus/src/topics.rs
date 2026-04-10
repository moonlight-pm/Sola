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

/// Window geometry from sola-x for X11 window positioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub app_id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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
    SetWindowGeometry(WindowGeometry),

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
}
