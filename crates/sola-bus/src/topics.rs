use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
pub use sola_core::KeyChord;
use sola_core::Encrypted;

use crate::define_topics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub window_id: u32,
    pub app_id: String,
    pub title: String,
    /// PID of the process that owns the surface, as reported by the
    /// compositor. May be absent for windows where the compositor has no
    /// way to attribute a process (non-Wayland edge cases, early frames).
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenUrlRequest {
    pub url: String,
    pub activate: bool,
}

/// Payload for a `LaunchApp` bus message, emitted by the shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchAppPayload {
    pub app_id: String,
    pub command: String,
}

/// Outcome of a `LaunchApp` spawn attempt, emitted by `sola`.
/// `ok=true` means the process was spawned; it does not guarantee the
/// process stayed alive. `error` is populated when `ok=false`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchResultPayload {
    pub app_id: String,
    pub command: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Emitted by `sola` whenever a user app process exits. Exactly one of
/// `code` or `signal` is set: `code` on normal exit, `signal` when killed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAppExitedPayload {
    pub app_id: String,
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

/// Mail account + filter rules. Edited by sola-settings, consumed by
/// sola-mail. Persisted as a sticky bus topic; the password field is
/// encrypted on disk via [`Encrypted`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MailConfig {
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: Encrypted<String>,
    pub rules: Vec<MailRule>,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            email: String::new(),
            imap_host: String::new(),
            imap_port: 993,
            smtp_host: String::new(),
            smtp_port: 587,
            username: String::new(),
            password: Encrypted(String::new()),
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailRule {
    pub name: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    pub conditions: Vec<MailRuleCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailRuleCondition {
    pub field: String,
    #[serde(rename = "match")]
    pub match_type: String,
    pub value: String,
}

/// Per-window UI preferences for sola-terminal. Persistent so they
/// survive across terminal restarts and bus restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TerminalConfig {
    pub sidebar_width: u32,
    pub sidebar_collapsed: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 220,
            sidebar_collapsed: false,
        }
    }
}

/// One terminal tab as persisted on the bus. The `tmux_session` is the
/// authoritative identifier for the live PTY; `id` is a stable handle
/// used by the JS side. `cwd` is a hint, refreshed via OSC 7.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalTab {
    pub id: String,
    pub tmux_session: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Live tab list for sola-terminal. Persistent so tabs survive across
/// terminal/bus restarts; reconciled against live tmux on startup.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TerminalSessions {
    pub tabs: Vec<TerminalTab>,
}

/// Ask a specific Sola app to evaluate a JS expression in one of its
/// WebViews. The app's framework wraps the expression, runs it, and
/// emits an `Evaluation` event with the JSON-encoded result. Multiple
/// concurrent `Evaluate` events to the same app race against each
/// other — `sola-debug` is a one-at-a-time tool and doesn't try to
/// correlate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatePayload {
    pub target_app: String,
    /// Window title; `None` selects the first window.
    pub window: Option<String>,
    pub expr: String,
}

/// Result of an evaluation. Source app is on `Message::source`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationPayload {
    /// `Ok(json)` — the JSON-encoded value. `Err(msg)` — runtime or
    /// serialization error from the WebView.
    pub result: Result<String, String>,
}

/// Ask sola-river to capture the compositor output. Answered with a
/// `Screenshot` event whose `result` carries the path on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureScreenPayload {
    /// Where to write the PNG. `None` → auto-generate a path under
    /// `/tmp/sola/screenshots/<unix-ms>.png`.
    pub path: Option<PathBuf>,
    /// What to capture. `FullOutput` (default) captures the whole
    /// compositor output. `Window { app_id, title? }` captures the
    /// region currently occupied by that window.
    #[serde(default)]
    pub target: CaptureTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum CaptureTarget {
    #[default]
    FullOutput,
    Window {
        app_id: String,
        title: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotPayload {
    pub result: Result<PathBuf, String>,
}

/// Synthesize a pointer event on the seat. Handled by sola-river via
/// `wlr-virtual-pointer-unstable-v1`. Coordinates are absolute in
/// compositor space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatePointerPayload {
    pub action: PointerAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PointerAction {
    /// Move pointer to absolute (x, y) on the primary output.
    Move { x: i32, y: i32 },
    /// Move and click (press + release).
    Click {
        button: PointerButton,
        x: i32,
        y: i32,
    },
    /// Press button at current pointer location.
    Press { button: PointerButton },
    /// Release button at current pointer location.
    Release { button: PointerButton },
    /// Scroll. Positive `dy` = scroll down. `dx` for horizontal.
    Scroll { dx: f64, dy: f64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

/// Synthesize a single keystroke (press + release) with the given
/// modifiers. Handled by sola-river via the existing virtual-keyboard
/// protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulateKeyPayload {
    pub chord: KeyChord,
}

define_topics! {
    // Window management list. Sticky: latest list from sola-river is
    // replayed to new subscribers.
    #[sticky]
    Windows(Vec<Window>),
    LaunchApp(LaunchAppPayload),
    LaunchResult(LaunchResultPayload),
    UserAppExited(UserAppExitedPayload),
    CloseApp(String),

    // Lifecycle / presence
    ClientConnected(String),
    ClientDisconnected(String),

    // Composition authority (shell → sola-river)
    Composition(Vec<CompositionEntry>),
    Frame(FrameUpdate),
    Focus(FocusTarget),

    // Output geometry. Sticky so late-joining apps learn the current
    // resolution without waiting for a hotplug event.
    #[sticky]
    OutputGeometry(OutputGeometry),

    // Mouse events (sola-river → shell)
    MouseEntered(MouseEnteredPayload),
    MouseLeft,
    MouseClicked(MouseClickedPayload),

    // Keyboard (shell ↔ sola-river). Shell emits the registered-chord
    // set sticky so sola-river can restore bindings after restart.
    #[sticky]
    RegisteredChords(Vec<RegisteredChord>),
    Chord(ChordEvent),
    ChordReleased(ChordEvent),

    // App menu. Each app emits its menu sticky on startup so the shell
    // can restore the menubar after a shell restart.
    #[sticky]
    SetAppMenu(AppMenuPayload),
    MenuAction(MenuActionPayload),

    // Zone assignments by app_id. Shell owns the map; emits a fresh
    // copy after each snap. Persistent so layouts survive restart.
    #[persistent]
    Zones(HashMap<String, Zone>),

    // Mail account + filter rules. Edited by sola-settings, consumed by
    // sola-mail. Persistent — settings emits whenever the user saves.
    #[persistent]
    MailConfig(MailConfig),

    // Terminal UI preferences (sidebar width / collapsed). Persistent
    // so terminal restarts restore the user's layout.
    #[persistent]
    TerminalConfig(TerminalConfig),

    // Live tab list for sola-terminal. Persistent so tabs survive
    // terminal-app and bus restarts.
    #[persistent]
    TerminalSessions(TerminalSessions),

    // Browser
    OpenUrl(OpenUrlRequest),

    // Global clipboard chords. Shell broadcasts when Meta+C/V fires;
    // sola-app handles for its own windows, sola-river handles for
    // foreign (non-sola) windows by synthesizing Ctrl+C / Ctrl+V.
    Copy(EditRequest),
    Paste(EditRequest),

    // Debug introspection (sola-debug ↔ apps via sola-app framework).
    Evaluate(EvaluatePayload),
    Evaluation(EvaluationPayload),

    // Screenshot capture (sola-debug → sola-river).
    CaptureScreen(CaptureScreenPayload),
    Screenshot(ScreenshotPayload),

    // Synthetic input (sola-debug → sola-river).
    SimulatePointer(SimulatePointerPayload),
    SimulateKey(SimulateKeyPayload),

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
        let windows = vec![Window {
            window_id: 1,
            app_id: "zen".into(),
            title: "Browser".into(),
            pid: None,
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
    fn topic_kind_matches_variant() {
        let t = Topic::Shutdown;
        assert_eq!(t.kind(), TopicKind::Shutdown);
    }

    #[test]
    fn topic_kind_all_includes_shutdown_and_windows() {
        assert!(TopicKind::ALL.iter().any(|k| k.as_str() == "Shutdown"));
        assert!(TopicKind::ALL.iter().any(|k| k.as_str() == "Windows"));
    }

    #[test]
    fn behavior_reflects_annotations() {
        use crate::topic::Behavior;
        // Persistent variants
        assert_eq!(TopicKind::Zones.behavior(), Behavior::Persistent);
        // Sticky variants
        assert_eq!(TopicKind::Windows.behavior(), Behavior::Sticky);
        assert_eq!(TopicKind::OutputGeometry.behavior(), Behavior::Sticky);
        assert_eq!(TopicKind::RegisteredChords.behavior(), Behavior::Sticky);
        assert_eq!(TopicKind::SetAppMenu.behavior(), Behavior::Sticky);
        // Ephemeral variants
        assert_eq!(TopicKind::LaunchApp.behavior(), Behavior::Ephemeral);
        assert_eq!(TopicKind::Frame.behavior(), Behavior::Ephemeral);
        assert_eq!(TopicKind::Shutdown.behavior(), Behavior::Ephemeral);
        assert_eq!(TopicKind::MouseLeft.behavior(), Behavior::Ephemeral);
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

    #[test]
    fn topic_kind_from_str_roundtrip() {
        for kind in TopicKind::ALL {
            let name = kind.as_str();
            assert_eq!(TopicKind::from_str(name), Some(*kind), "kind {name}");
        }
    }

    #[test]
    fn topic_kind_from_str_unknown_is_none() {
        assert_eq!(TopicKind::from_str("NotARealTopic"), None);
    }

    #[test]
    fn to_toml_value_returns_none_for_non_persistent() {
        // Only persistent topics serialize to TOML; everything else
        // must return None regardless of behavior (ephemeral/sticky).
        let samples: Vec<Topic> = vec![
            Topic::Shutdown,
            Topic::Windows(vec![]),
            Topic::OutputGeometry(OutputGeometry {
                width: 1,
                height: 1,
            }),
            Topic::RegisteredChords(vec![]),
        ];
        for t in samples {
            assert!(
                t.to_toml_value().is_none(),
                "expected None for {:?}",
                t.kind()
            );
        }
    }

    #[test]
    fn zones_roundtrips_via_toml() {
        let mut zones: HashMap<String, Zone> = HashMap::new();
        zones.insert("sola-browser".into(), Zone::Left);
        zones.insert("sola-terminal".into(), Zone::Right);

        let topic = Topic::Zones(zones.clone());
        let value = topic
            .to_toml_value()
            .expect("Zones is persistent; must serialize to TOML");

        match Topic::from_toml_section(TopicKind::Zones, value) {
            Some(Topic::Zones(back)) => assert_eq!(back, zones),
            other => panic!("expected Zones, got {other:?}"),
        }
    }

    #[test]
    fn from_toml_section_returns_none_for_non_persistent() {
        let empty = toml::Value::Table(toml::map::Map::new());
        assert!(Topic::from_toml_section(TopicKind::Windows, empty.clone()).is_none());
        assert!(Topic::from_toml_section(TopicKind::Shutdown, empty).is_none());
    }

    #[test]
    fn mail_config_to_json_serializes_object() {
        // Regression: monitor's old hand-written `topic_to_json` had no
        // arm for MailConfig and fell through to Value::Null. The macro-
        // generated `to_json_value` now covers every variant.
        let topic = Topic::MailConfig(MailConfig::default());
        let v = topic.to_json_value();
        assert!(v.is_object(), "expected object, got {v:?}");
        assert!(v.get("email").is_some());
    }

    #[test]
    fn terminal_config_roundtrips_via_postcard() {
        let cfg = TerminalConfig {
            sidebar_width: 312,
            sidebar_collapsed: true,
        };
        let topic = Topic::TerminalConfig(cfg.clone());
        let msg = topic.to_message();
        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::TerminalConfig(back) => {
                assert_eq!(back.sidebar_width, 312);
                assert!(back.sidebar_collapsed);
            }
            other => panic!("expected TerminalConfig, got {other:?}"),
        }
    }

    #[test]
    fn terminal_config_roundtrips_via_toml() {
        let cfg = TerminalConfig {
            sidebar_width: 240,
            sidebar_collapsed: false,
        };
        let topic = Topic::TerminalConfig(cfg);
        let value = topic
            .to_toml_value()
            .expect("persistent payload should serialize to TOML");
        let restored = Topic::from_toml_section(TopicKind::TerminalConfig, value)
            .expect("section should deserialize");
        match restored {
            Topic::TerminalConfig(back) => {
                assert_eq!(back.sidebar_width, 240);
                assert!(!back.sidebar_collapsed);
            }
            other => panic!("expected TerminalConfig, got {other:?}"),
        }
    }

    #[test]
    fn terminal_sessions_roundtrip_via_postcard() {
        let sessions = TerminalSessions {
            tabs: vec![
                TerminalTab {
                    id: "tab-1".into(),
                    tmux_session: "sola-tab-1".into(),
                    cwd: Some("/home/joshua".into()),
                },
                TerminalTab {
                    id: "tab-2".into(),
                    tmux_session: "sola-tab-2".into(),
                    cwd: None,
                },
            ],
        };
        let topic = Topic::TerminalSessions(sessions.clone());
        let msg = topic.to_message();
        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::TerminalSessions(back) => assert_eq!(back, sessions),
            other => panic!("expected TerminalSessions, got {other:?}"),
        }
    }

    #[test]
    fn terminal_sessions_roundtrip_via_toml() {
        let sessions = TerminalSessions {
            tabs: vec![TerminalTab {
                id: "x".into(),
                tmux_session: "sola-x".into(),
                cwd: Some("/tmp".into()),
            }],
        };
        let topic = Topic::TerminalSessions(sessions.clone());
        let value = topic
            .to_toml_value()
            .expect("persistent payload should serialize to TOML");
        let restored = Topic::from_toml_section(TopicKind::TerminalSessions, value)
            .expect("section should deserialize");
        match restored {
            Topic::TerminalSessions(back) => assert_eq!(back, sessions),
            other => panic!("expected TerminalSessions, got {other:?}"),
        }
    }

    #[test]
    fn mail_config_roundtrips_via_postcard_in_clear() {
        let cfg = MailConfig {
            email: "u@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            username: "u".into(),
            password: Encrypted("hunter2".into()),
            rules: vec![],
        };
        let topic = Topic::MailConfig(cfg.clone());
        let msg = topic.to_message();
        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::MailConfig(back) => {
                assert_eq!(back.email, cfg.email);
                // Password travels in clear over the postcard wire.
                assert_eq!(back.password.0, "hunter2");
            }
            other => panic!("expected MailConfig, got {other:?}"),
        }
    }
}

/// TOML round-trip tests. Runs its own `define_topics!` invocation
/// with a `#[persistent]` variant so we can exercise the generated
/// `to_toml_value` / `from_toml_section` until Phase 5 adds the first
/// real persistent topic.
#[cfg(test)]
mod persistent_toml_tests {
    #[allow(dead_code)]
    mod fixture {
        use serde::{Deserialize, Serialize};

        use crate::define_topics;

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
        pub struct Zone {
            pub name: String,
        }

        define_topics! {
            Ping,
            #[persistent]
            Zones(std::collections::HashMap<String, Zone>),
        }
    }

    use fixture::{Topic, TopicKind, Zone};
    use std::collections::HashMap;

    #[test]
    fn persistent_payload_roundtrips_via_toml() {
        let mut zones = HashMap::new();
        zones.insert(
            "sola-browser".into(),
            Zone {
                name: "Left".into(),
            },
        );
        zones.insert(
            "sola-terminal".into(),
            Zone {
                name: "Right".into(),
            },
        );

        let topic = Topic::Zones(zones.clone());
        let value = topic
            .to_toml_value()
            .expect("persistent payload should serialize to TOML");

        let restored =
            Topic::from_toml_section(TopicKind::Zones, value).expect("section should deserialize");

        match restored {
            Topic::Zones(decoded) => assert_eq!(decoded, zones),
            _ => panic!("wrong variant after round-trip"),
        }
    }

    #[test]
    fn non_persistent_kinds_do_not_deserialize() {
        let value = toml::Value::Table(toml::map::Map::new());
        assert!(Topic::from_toml_section(TopicKind::Ping, value).is_none());
    }

    #[test]
    fn malformed_section_returns_none() {
        // Wrong shape for a HashMap<String, Zone>: a bare string.
        let value = toml::Value::String("oops".into());
        assert!(Topic::from_toml_section(TopicKind::Zones, value).is_none());
    }
}
