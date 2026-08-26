use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
pub use sola_core::applications::{AppKind, Application, ApplicationsConfig};
pub use sola_core::theme::{NamedTheme, Theme};
use sola_core::Encrypted;
pub use sola_core::KeyChord;

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

/// One open app to relaunch on next start. Owned by `sola-session` and
/// carried (as a list) by the persistent `Topic::SessionApps`. `command`
/// is the same launch string `sola-session` originally spawned, so restore
/// reuses the normal launch path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionApp {
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

/// Request that the shell omit an app's surfaces from composition
/// (River `hide`). Sticky and keyed by `app_id`: emit to hide, retract
/// to show again. Used by sola-arcade to park Steam's client UI while a
/// game runs under windowed gamescope — Sola has no taskbar minimize, so
/// the shell shows a menubar chip for each hidden app as the restore path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppHidden {
    pub app_id: String,
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
    /// When true, the compositor should enter true fullscreen state for
    /// this window (call `proxy.fullscreen(&output)`), not just resize
    /// it. Set by the shell for the Cinema zone so games receive a
    /// configure with the fullscreen bit, which per xdg-shell overrides
    /// their own internal max_size/work-area constraints. Exit is
    /// driven by either the client itself (`unset_fullscreen`) or by
    /// river auto-exiting on focus change.
    pub fullscreen: bool,
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

/// A window's live on-screen rectangle, emitted by sola-river whenever the
/// window's size (`river_window_v1.dimensions`) or position (`node.set_position`)
/// changes. Carried by the sticky `Topic::WindowGeometry`, keyed by `window_id`,
/// so a late subscriber (e.g. a per-window titlebar overlay) learns the current
/// rectangle without waiting for the next move. Retracted when the window closes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub window_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Whether a window is currently floating (shell → sola-river). Carried by the
/// sticky `Topic::WindowFloating`, keyed by `window_id`, so sola-river can gate
/// interactive move/resize on the window under the pointer without learning the
/// shell's zone vocabulary. The shell sets `floating: false` when the window
/// leaves the Float zone; sola-river also drops it when the window closes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFloating {
    pub window_id: u32,
    pub floating: bool,
}

/// A floating app's remembered rectangle, keyed by `app_id`. Carried by the
/// persistent `Topic::FloatGeometry` so a floating window restores to where it
/// last was across relaunch and across a full restart. Kept separate from the
/// `Zones:` map so `Zone` stays a clean unit enum and geometry churn doesn't
/// rewrite the zone assignments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloatGeometry {
    pub app_id: String,
    pub x: i32,
    pub y: i32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    TopMiddle,
    BottomMiddle,
    FullMiddle,
    /// Center + right columns combined (FullMiddle ∪ Right). Full height
    /// under the menubar — Meta+KP_Add.
    MiddleRight,
    /// Fullscreen *under* the menubar — the standard "max window".
    Fullscreen,
    /// True fullscreen including the menubar — the cinema / no-chrome
    /// view. The shell skips its menubar offset for this zone so the
    /// window covers the whole output.
    Cinema,
    /// App-sized / floating: positioned by the shell (centered, or a
    /// remembered location in later phases) but never force-resized. The
    /// window keeps the size it chooses for itself; the shell emits no
    /// sizing frame for it. Reuses the whole zone pipeline for
    /// designation + persistence.
    Float,
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
            // FullMiddle (0.28..0.72) + Right (0.72..1.0) → 0.28..1.0
            Zone::MiddleRight => (0.28, 0.0, 0.72, 1.0),
            Zone::Fullscreen => (0.0, 0.0, 1.0, 1.0),
            Zone::Cinema => (0.0, 0.0, 1.0, 1.0),
            // Float never goes through the sizing path; the value is unused
            // but the match must stay exhaustive.
            Zone::Float => (0.0, 0.0, 0.0, 0.0),
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
    /// Tab selected when the terminal last quit. Restored on boot so a
    /// restart lands on the same tab rather than whichever sticky
    /// `TerminalSession` arrives first.
    #[serde(default)]
    pub active_tab_id: Option<String>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 220,
            sidebar_collapsed: false,
            active_tab_id: None,
        }
    }
}

/// Orientation of a pane split. `Vertical` places the two panes
/// side-by-side with a vertical divider (the new pane lands to the
/// RIGHT, `⌘⇧→`); `Horizontal` stacks them with a horizontal divider
/// (the new pane lands BELOW, `⌘⇧↓`). Defined here rather than in
/// sola-terminal because the persisted [`PaneLayout`] wire type needs
/// it and sola-bus cannot depend on sola-terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDir {
    Vertical,
    Horizontal,
}

/// Serializable mirror of sola-terminal's in-memory pane tree, persisted
/// inside [`TerminalSession::layout`]. Each `Leaf` names the tmux
/// session backing that pane (the authoritative PTY id); `Split` carries
/// the divider orientation and pane `a`'s fraction of the split's main
/// axis (`ratio`, in `(0, 1)`). Runtime pane/split ids are NOT persisted
/// — only structure, tmux session names, cwd hints, and ratios; ids are
/// regenerated on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaneLayout {
    Leaf {
        tmux_session: String,
        #[serde(default)]
        cwd: Option<String>,
    },
    Split {
        dir: SplitDir,
        ratio: f32,
        a: Box<PaneLayout>,
        b: Box<PaneLayout>,
    },
}

/// One terminal tab as persisted on the bus. The `tmux_session` is the
/// authoritative identifier for the root/first pane's live PTY; `id` is
/// the sticky key — each tab has its own `(TerminalSession, [id])` slot.
/// `cwd` is a hint, refreshed via OSC 7. `ordinal` determines display
/// order in the tab strip — gaps are fine, JS sorts by ordinal. `layout`
/// carries the pane split tree once the tab has been split; `None`
/// (old records) restores as a single pane using `tmux_session`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalSession {
    pub id: String,
    pub tmux_session: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub ordinal: u32,
    #[serde(default)]
    pub layout: Option<PaneLayout>,
}

/// One persisted browser tab. Keyed by `id` (UUIDv4 generated at tab
/// creation). `ordinal` orders the tab strip; gaps are fine, JS sorts
/// by ordinal. `session_state` is the base64-encoded WebKit page
/// session blob (back/forward stack, scroll position, form state) and
/// is `None` until the tab has been navigated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub ordinal: u32,
    #[serde(default)]
    pub session_state: Option<String>,
}

/// Open paint tabs. Paths only — unsaved buffers are not persisted.
/// Missing files are skipped on restore and pruned on the next emit.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaintSession {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub selected: Option<PathBuf>,
}

/// Browser-wide singleton config. Headroom for future fields (default
/// search engine, zoom default, etc.) without breaking the schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserConfig {
    #[serde(default)]
    pub active_tab_id: Option<String>,
}

/// Monitor UI preferences. Persistent so the sticky-panel width
/// survives across monitor restarts and bus restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MonitorConfig {
    pub sidebar_width: u32,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self { sidebar_width: 240 }
    }
}

/// One visited URL. Cap and MRU policy are enforced by the browser
/// before emitting `BrowserHistory` (the singleton aggregate).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visits: u32,
}

/// Singleton browser history aggregate. The browser owns the cap and
/// MRU ordering; the bus persists the latest snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserHistory {
    #[serde(default)]
    pub entries: Vec<HistoryEntry>,
}

/// Capture request used by sola-river's screenshot path (call plane).
/// Not a bus topic — requests go through `sola-call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureScreenPayload {
    /// Where to write the PNG. `None` → auto-generate a path under
    /// `/tmp/sola/screenshots/<unix-ms>.png`.
    pub path: Option<PathBuf>,
    /// What to capture. `FullOutput` (default) captures the whole
    /// compositor output. `Window { app_id, title? }` captures the
    /// region currently occupied by that window. `Region` is an
    /// absolute rectangle on the first `wl_output` (V1 single-output).
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
    /// Absolute compositor coordinates on the first output.
    Region {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
}

/// Ask an image app to open a file.
/// Ephemeral — same pattern as [`OpenUrlRequest`].
///
/// Default dest (`app_id` missing) is **sola-paint** (MIME / `solactl open`).
/// Screenshots target **sola-preview** by setting `app_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenImageRequest {
    pub path: PathBuf,
    /// When true, the viewer should raise / take focus if it can.
    pub activate: bool,
    /// When set, only this `app_id` consumes the open. `None` means the
    /// default image dest (`sola-paint`).
    #[serde(default)]
    pub app_id: Option<String>,
}

impl OpenImageRequest {
    /// App that should open this path.
    pub fn target_app(&self) -> &str {
        self.app_id.as_deref().unwrap_or("sola-paint")
    }

    /// True when `app` should handle this request.
    pub fn for_app(&self, app: &str) -> bool {
        self.target_app() == app
    }
}

/// Pointer action for compositor input (call plane, not a bus topic).
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

/// Ephemeral menubar toast. Shell chrome only — no sound, no extra UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppToast {
    pub text: String,
}

/// Live inbox unread count. `sola-mail` emits this; `sola-shell` paints
/// a menubar chip. Not persisted — sticky so a restarting shell can
/// replay the last value while mail is still up.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailStatus {
    pub inbox_unread: u32,
}

define_topics! {
    // TopicKind is postcard-encoded in Subscribe. Inserting a variant *above*
    // existing ones shifts discriminants; a new client vs an old bus then
    // silently subscribes to the wrong topics (this broke Super+Tab).
    // Append new kinds at the **end**, or reuse an existing topic.
    //
    // Window management list. Sticky: latest list from sola-river is
    // replayed to new subscribers.
    #[sticky]
    Windows(Vec<Window>),
    LaunchApp(LaunchAppPayload),
    LaunchResult(LaunchResultPayload),
    UserAppExited(UserAppExitedPayload),
    CloseApp(String),

    // Hide app surfaces from composition (shell → river via Composition).
    // Sticky + keyed by app_id; retract to show. See `AppHidden` docs.
    #[sticky(keys = [app_id])]
    AppHidden(AppHidden),

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

    // A window's live rectangle (sola-river → shell). Sticky and keyed by
    // window_id so each window has its own retained slot and a late subscriber
    // learns the current geometry; sola-river retracts it on window close.
    #[sticky(keys = [window_id])]
    WindowGeometry(WindowGeometry),

    // Whether a window is floating (shell → sola-river). Sticky and keyed by
    // window_id so sola-river retains the bit per window and can gate
    // interactive move/resize at pointer-press time without a bus round-trip.
    #[sticky(keys = [window_id])]
    WindowFloating(WindowFloating),

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
    // can restore the menubar after a shell restart. Keyed by `app_id` so
    // each app has its own sticky slot — `(topic, [app_id])` — instead of
    // racing with other apps for a single shared SetAppMenu sticky.
    #[sticky(keys = [app_id])]
    SetAppMenu(AppMenuPayload),
    MenuAction(MenuActionPayload),

    // Zone assignments by app_id. Shell owns the map; emits a fresh
    // copy after each snap. Persistent so layouts survive restart.
    #[persistent]
    Zones(HashMap<String, Zone>),

    // Remembered rectangle of each floating app, keyed by app_id. Shell owns
    // it: records from Topic::WindowGeometry while a window is floating, restores
    // on relaunch. Persistent so float placement survives a restart. Separate
    // from Zones so geometry churn doesn't rewrite the zone map.
    #[persistent(keys = [app_id])]
    FloatGeometry(FloatGeometry),

    // Open user apps to restore on next start. sola-session owns the list
    // and emits a fresh copy whenever its child set changes. Persistent so
    // the open set survives a full restart and replays on subscribe.
    #[persistent]
    SessionApps(Vec<SessionApp>),

    // Mail account + filter rules. Edited by sola-settings, consumed by
    // sola-mail. Persistent — settings emits whenever the user saves.
    #[persistent]
    MailConfig(MailConfig),

    // Active theme tokens edited by sola-kit, consumed by every kit
    // consumer (sola-monitor, sola-settings, …). Persistent — survives
    // bus restart and sola sessions; replays on subscribe. Lives in its
    // own namespace file (`~/.config/sola/theme/current.yaml`) so theme
    // edits don't churn `state.yaml` and so the file loads even when
    // sola-kit isn't running.
    #[persistent(namespace = "theme/current")]
    Theme(Theme),

    // User-named theme presets persisted by sola-kit's storybook. Keyed
    // by `name` so each preset lives at
    // `~/.config/sola/theme/presets/<name>.yaml`. Emit to add/update,
    // retract to unlink. `name` is constrained to kebab-case (see
    // `sola_core::theme::is_valid_theme_name`) — it doubles as the
    // filename. The hardcoded "Default" preset is owned by Rust
    // constants and never travels through this topic.
    #[persistent(keys = [name], namespace = "theme/presets/:name")]
    CustomTheme(NamedTheme),

    // One launchable application (user-edited; built-ins live in code).
    // Keyed by `app_id` so each entry has its own item under
    // `Application:` in `state.yaml`: settings emits to add/update,
    // retracts to remove. Renaming `app_id` is a retract+emit pair.
    // sola-shell consumes for launcher search and switcher icon lookup.
    #[persistent(keys = [app_id])]
    Application(Application),

    // Terminal UI preferences (sidebar width / collapsed). Persistent
    // so terminal restarts restore the user's layout.
    #[persistent]
    TerminalConfig(TerminalConfig),

    // Monitor UI preferences (sticky-panel width). Persistent
    // so the user's chosen width survives across restarts.
    #[persistent]
    MonitorConfig(MonitorConfig),

    // One terminal tab as persisted on the bus. Keyed by `id` so each
    // tab has its own `(TerminalSession, [id])` slot — add a tab by
    // emitting; remove by retracting; reorder by re-emitting with new
    // ordinals.
    #[persistent(keys = [id])]
    TerminalSession(TerminalSession),

    // Browser singleton config (active tab id, future browser-wide
    // settings). Lives in its own namespace file so frequent active-tab
    // changes don't churn the shared state.yaml.
    #[persistent(namespace = "browser")]
    BrowserConfig(BrowserConfig),

    // Browser visited-URL history aggregate. Singleton; the browser
    // enforces the cap (1000) and MRU ordering before emitting. Lives
    // in its own namespace file.
    #[persistent(namespace = "browser/history")]
    BrowserHistory(BrowserHistory),

    // One persisted browser tab. Keyed by `id` so each tab has its
    // own `(BrowserTab, [id])` slot in the namespace
    // ~/.config/sola/browser/tabs/<id>.yaml.
    #[persistent(keys = [id], namespace = "browser/tabs/:id")]
    BrowserTab(BrowserTab),

    // Browser
    OpenUrl(OpenUrlRequest),

    // Paint tab strip. Singleton namespace so open/close does not churn
    // state.yaml. Paths that no longer exist are skipped on restore.
    #[persistent(namespace = "paint")]
    PaintSession(PaintSession),

    // Image open. Ephemeral; default dest is sola-paint. Screenshots
    // set `app_id` to sola-preview. Cold-start uses LaunchApp + path.
    OpenImage(OpenImageRequest),

    // Menubar toast. Ephemeral; shell shows `text` and expires it.
    // Operator-plain copy — the sender owns the words.
    AppToast(AppToast),

    // Lifecycle
    Shutdown,

    // Inbox unread for the menubar. Appended last so TopicKind postcard
    // discriminants stay stable. Sticky (not persistent): replay to late
    // subscribers; mail retracts on exit. Shell also hides the chip when
    // no sola-mail window is mapped.
    #[sticky]
    MailStatus(MailStatus),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_session_roundtrips_via_yaml() {
        let session = PaintSession {
            paths: vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.jpg")],
            selected: Some(PathBuf::from("/tmp/b.jpg")),
        };
        let topic = Topic::PaintSession(session.clone());
        let value = topic.to_yaml_value().expect("PaintSession is persistent");
        let restored = Topic::from_yaml_section(TopicKind::PaintSession, value)
            .expect("section should deserialize");
        match restored {
            Topic::PaintSession(back) => assert_eq!(back, session),
            other => panic!("expected PaintSession, got {other:?}"),
        }
    }

    fn open_image_defaults_to_paint() {
        let paint = OpenImageRequest {
            path: PathBuf::from("/tmp/a.png"),
            activate: true,
            app_id: None,
        };
        assert_eq!(paint.target_app(), "sola-paint");
        assert!(paint.for_app("sola-paint"));
        assert!(!paint.for_app("sola-preview"));

        let preview = OpenImageRequest {
            path: PathBuf::from("/tmp/shot.png"),
            activate: false,
            app_id: Some("sola-preview".into()),
        };
        assert!(preview.for_app("sola-preview"));
        assert!(!preview.for_app("sola-paint"));
    }

    #[test]
    fn mail_status_roundtrips_on_the_wire() {
        let topic = Topic::MailStatus(MailStatus { inbox_unread: 4 });
        let msg = topic.to_message();
        match Topic::parse(&msg) {
            Some(Topic::MailStatus(s)) => assert_eq!(s.inbox_unread, 4),
            other => panic!("expected MailStatus, got {other:?}"),
        }
        assert_eq!(TopicKind::MailStatus.as_str(), "MailStatus");
    }

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
        assert_eq!(TopicKind::MailStatus.behavior(), Behavior::Sticky);
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
    fn to_yaml_value_returns_none_for_non_persistent() {
        // Only persistent topics serialize to YAML; everything else
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
                t.to_yaml_value().is_none(),
                "expected None for {:?}",
                t.kind()
            );
        }
    }

    #[test]
    fn zones_roundtrips_via_yaml() {
        let mut zones: HashMap<String, Zone> = HashMap::new();
        zones.insert("sola-browser".into(), Zone::Left);
        zones.insert("sola-terminal".into(), Zone::Right);

        let topic = Topic::Zones(zones.clone());
        let value = topic
            .to_yaml_value()
            .expect("Zones is persistent; must serialize to YAML");

        match Topic::from_yaml_section(TopicKind::Zones, value) {
            Some(Topic::Zones(back)) => assert_eq!(back, zones),
            other => panic!("expected Zones, got {other:?}"),
        }
    }

    #[test]
    fn zone_float_rect_is_zero() {
        // Float never goes through the sizing path; rect() must stay
        // exhaustive but its value is unused.
        assert_eq!(Zone::Float.rect(), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn zone_middle_right_is_full_middle_union_right() {
        // FullMiddle (0.28, 0.44) ∪ Right (0.72, 0.28) → (0.28, 0.72)
        assert_eq!(Zone::MiddleRight.rect(), (0.28, 0.0, 0.72, 1.0));
    }

    #[test]
    fn zone_float_roundtrips_via_yaml() {
        let mut zones: HashMap<String, Zone> = HashMap::new();
        zones.insert("UnrealEditor".into(), Zone::Float);

        let value = Topic::Zones(zones.clone())
            .to_yaml_value()
            .expect("Zones is persistent; must serialize to YAML");

        match Topic::from_yaml_section(TopicKind::Zones, value) {
            Some(Topic::Zones(back)) => assert_eq!(back, zones),
            other => panic!("expected Zones, got {other:?}"),
        }
    }

    #[test]
    fn session_apps_is_persistent() {
        use crate::topic::Behavior;
        assert_eq!(TopicKind::SessionApps.behavior(), Behavior::Persistent);
    }

    #[test]
    fn float_geometry_is_persistent_and_roundtrips() {
        use crate::topic::Behavior;
        assert_eq!(TopicKind::FloatGeometry.behavior(), Behavior::Persistent);
        let fg = FloatGeometry {
            app_id: "UnrealEditor".into(),
            x: 10,
            y: 20,
            width: 1280,
            height: 800,
        };
        let value = Topic::FloatGeometry(fg.clone())
            .to_yaml_value()
            .expect("FloatGeometry is persistent; must serialize to YAML");
        match Topic::from_yaml_section(TopicKind::FloatGeometry, value) {
            Some(Topic::FloatGeometry(back)) => {
                assert_eq!(back.app_id, fg.app_id);
                assert_eq!(
                    (back.x, back.y, back.width, back.height),
                    (10, 20, 1280, 800)
                );
            }
            other => panic!("expected FloatGeometry, got {other:?}"),
        }
    }

    #[test]
    fn window_geometry_is_sticky_not_persistent() {
        use crate::topic::Behavior;
        assert_eq!(TopicKind::WindowGeometry.behavior(), Behavior::Sticky);
    }

    #[test]
    fn window_floating_is_sticky_and_keyed() {
        use crate::topic::Behavior;
        assert_eq!(TopicKind::WindowFloating.behavior(), Behavior::Sticky);
        let wf = WindowFloating {
            window_id: 7,
            floating: true,
        };
        assert_eq!(wf.window_id, 7);
        assert!(wf.floating);
    }

    #[test]
    fn session_apps_roundtrip_via_message() {
        let apps = vec![
            SessionApp {
                app_id: "helium".into(),
                command: "helium".into(),
            },
            SessionApp {
                app_id: "sola-terminal".into(),
                command: "/opt/sola/bin/sola-terminal".into(),
            },
        ];
        let msg = Topic::SessionApps(apps.clone()).to_message();
        assert_eq!(msg.topic, "SessionApps");
        match Topic::parse(&msg) {
            Some(Topic::SessionApps(back)) => assert_eq!(back, apps),
            other => panic!("expected SessionApps, got {other:?}"),
        }
    }

    #[test]
    fn session_apps_roundtrip_via_yaml() {
        let apps = vec![SessionApp {
            app_id: "helium".into(),
            command: "helium --restore".into(),
        }];
        let value = Topic::SessionApps(apps.clone())
            .to_yaml_value()
            .expect("SessionApps is persistent; must serialize to YAML");
        match Topic::from_yaml_section(TopicKind::SessionApps, value) {
            Some(Topic::SessionApps(back)) => assert_eq!(back, apps),
            other => panic!("expected SessionApps, got {other:?}"),
        }
    }

    #[test]
    fn from_yaml_section_returns_none_for_non_persistent() {
        let empty = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
        assert!(Topic::from_yaml_section(TopicKind::Windows, empty.clone()).is_none());
        assert!(Topic::from_yaml_section(TopicKind::Shutdown, empty).is_none());
    }

    #[test]
    fn application_roundtrips_via_yaml() {
        let app = Application {
            app_id: "steam".into(),
            label: "Steam".into(),
            command: "/run/current-system/sw/bin/steam".into(),
            icon: "simpleicons/steam".into(),
            ..Default::default()
        };
        let topic = Topic::Application(app);
        let value = topic
            .to_yaml_value()
            .expect("Application is persistent; must serialize to YAML");
        let restored = Topic::from_yaml_section(TopicKind::Application, value)
            .expect("section should deserialize");
        match restored {
            Topic::Application(back) => {
                assert_eq!(back.app_id, "steam");
                assert_eq!(back.command, "/run/current-system/sw/bin/steam");
            }
            other => panic!("expected Application, got {other:?}"),
        }
    }

    #[test]
    fn application_emits_app_id_as_key() {
        let app = Application {
            app_id: "Bitwarden".into(),
            label: "Bitwarden".into(),
            command: "/run/current-system/sw/bin/bitwarden".into(),
            icon: "simpleicons/bitwarden".into(),
            ..Default::default()
        };
        let msg = Topic::Application(app).to_message();
        assert_eq!(msg.keys, vec!["Bitwarden".to_string()]);
    }

    #[test]
    fn application_parse_accepts_pre_wrapper_postcard() {
        #[derive(serde::Serialize)]
        struct ApplicationV1 {
            app_id: String,
            label: String,
            command: String,
            icon: String,
        }
        let old = ApplicationV1 {
            app_id: "steam".into(),
            label: "Steam".into(),
            command: "/run/current-system/sw/bin/steam".into(),
            icon: "simpleicons/steam".into(),
        };
        let bytes = postcard::to_allocvec(&old).unwrap();
        let mut msg = crate::Message::with_payload("Application", bytes);
        msg.keys = vec!["steam".into()];
        msg.sticky = true;
        match Topic::parse(&msg) {
            Some(Topic::Application(a)) => {
                assert_eq!(a.app_id, "steam");
                assert_eq!(a.label, "Steam");
                assert_eq!(a.kind, AppKind::Command);
                assert_eq!(a.url, None);
            }
            other => panic!("expected Application, got {other:?}"),
        }
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
            active_tab_id: Some("tab-xyz".into()),
        };
        let topic = Topic::TerminalConfig(cfg.clone());
        let msg = topic.to_message();
        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::TerminalConfig(back) => {
                assert_eq!(back.sidebar_width, 312);
                assert!(back.sidebar_collapsed);
                assert_eq!(back.active_tab_id.as_deref(), Some("tab-xyz"));
            }
            other => panic!("expected TerminalConfig, got {other:?}"),
        }
    }

    #[test]
    fn terminal_config_roundtrips_via_yaml() {
        let cfg = TerminalConfig {
            sidebar_width: 240,
            sidebar_collapsed: false,
            active_tab_id: Some("tab-abc".into()),
        };
        let topic = Topic::TerminalConfig(cfg);
        let value = topic
            .to_yaml_value()
            .expect("persistent payload should serialize to TOML");
        let restored = Topic::from_yaml_section(TopicKind::TerminalConfig, value)
            .expect("section should deserialize");
        match restored {
            Topic::TerminalConfig(back) => {
                assert_eq!(back.sidebar_width, 240);
                assert!(!back.sidebar_collapsed);
                assert_eq!(back.active_tab_id.as_deref(), Some("tab-abc"));
            }
            other => panic!("expected TerminalConfig, got {other:?}"),
        }
    }

    #[test]
    fn terminal_config_yaml_without_active_tab_defaults_to_none() {
        // Old state.yaml records only had sidebar fields.
        let value = serde_yaml_ng::from_str("sidebar_width: 250\nsidebar_collapsed: false\n")
            .expect("yaml");
        let restored = Topic::from_yaml_section(TopicKind::TerminalConfig, value)
            .expect("section should deserialize");
        match restored {
            Topic::TerminalConfig(back) => {
                assert_eq!(back.sidebar_width, 250);
                assert!(back.active_tab_id.is_none());
            }
            other => panic!("expected TerminalConfig, got {other:?}"),
        }
    }

    #[test]
    fn monitor_config_roundtrips_via_postcard() {
        let cfg = MonitorConfig { sidebar_width: 312 };
        let topic = Topic::MonitorConfig(cfg.clone());
        let msg = topic.to_message();
        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::MonitorConfig(back) => {
                assert_eq!(back.sidebar_width, 312);
            }
            other => panic!("expected MonitorConfig, got {other:?}"),
        }
    }

    #[test]
    fn monitor_config_roundtrips_via_yaml() {
        let cfg = MonitorConfig { sidebar_width: 240 };
        let topic = Topic::MonitorConfig(cfg);
        let value = topic
            .to_yaml_value()
            .expect("persistent payload should serialize to TOML");
        let restored = Topic::from_yaml_section(TopicKind::MonitorConfig, value)
            .expect("section should deserialize");
        match restored {
            Topic::MonitorConfig(back) => {
                assert_eq!(back.sidebar_width, 240);
            }
            other => panic!("expected MonitorConfig, got {other:?}"),
        }
    }

    #[test]
    fn terminal_session_roundtrip_via_postcard() {
        let session = TerminalSession {
            id: "tab-1".into(),
            tmux_session: "sola-tab-1".into(),
            cwd: Some("/home/joshua".into()),
            ordinal: 0,
            layout: None,
        };
        let topic = Topic::TerminalSession(session.clone());
        let msg = topic.to_message();
        let parsed = Topic::parse(&msg).unwrap();
        match parsed {
            Topic::TerminalSession(back) => assert_eq!(back, session),
            other => panic!("expected TerminalSession, got {other:?}"),
        }
    }

    #[test]
    fn terminal_session_emits_id_as_key() {
        let session = TerminalSession {
            id: "abc-123".into(),
            tmux_session: "sola-x".into(),
            cwd: None,
            ordinal: 7,
            layout: None,
        };
        let topic = Topic::TerminalSession(session);
        assert_eq!(topic.keys_for(), vec!["abc-123".to_string()]);
    }

    #[test]
    fn terminal_session_roundtrip_via_yaml() {
        let session = TerminalSession {
            id: "x".into(),
            tmux_session: "sola-x".into(),
            cwd: Some("/tmp".into()),
            ordinal: 3,
            layout: None,
        };
        let topic = Topic::TerminalSession(session.clone());
        let value = topic
            .to_yaml_value()
            .expect("persistent payload should serialize to TOML");
        let restored = Topic::from_yaml_section(TopicKind::TerminalSession, value)
            .expect("section should deserialize");
        match restored {
            Topic::TerminalSession(back) => assert_eq!(back, session),
            other => panic!("expected TerminalSession, got {other:?}"),
        }
    }

    #[test]
    fn terminal_session_with_layout_roundtrips_via_yaml() {
        let session = TerminalSession {
            id: "split-tab".into(),
            tmux_session: "sola-a".into(),
            cwd: Some("/tmp".into()),
            ordinal: 1,
            layout: Some(PaneLayout::Split {
                dir: SplitDir::Vertical,
                ratio: 0.5,
                a: Box::new(PaneLayout::Leaf {
                    tmux_session: "sola-a".into(),
                    cwd: Some("/tmp".into()),
                }),
                b: Box::new(PaneLayout::Leaf {
                    tmux_session: "sola-b".into(),
                    cwd: None,
                }),
            }),
        };
        let topic = Topic::TerminalSession(session.clone());
        let value = topic
            .to_yaml_value()
            .expect("persistent payload should serialize to YAML");
        let restored = Topic::from_yaml_section(TopicKind::TerminalSession, value)
            .expect("section should deserialize");
        match restored {
            Topic::TerminalSession(back) => assert_eq!(back, session),
            other => panic!("expected TerminalSession, got {other:?}"),
        }
    }

    #[test]
    fn terminal_session_without_layout_field_defaults_to_none() {
        // An old persisted record predates the `layout` field; serde(default)
        // must restore it as a single-pane tab (layout == None).
        let yaml = "id: old-tab\ntmux_session: sola-old\ncwd: /home/joshua\nordinal: 2\n";
        let session: TerminalSession = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(session.id, "old-tab");
        assert_eq!(session.tmux_session, "sola-old");
        assert_eq!(session.ordinal, 2);
        assert_eq!(session.layout, None);
    }

    #[test]
    fn theme_roundtrips_via_yaml() {
        // Theme round-trips through the bus' TOML state path.
        let theme = sola_core::theme::Theme::default();
        let topic = Topic::Theme(theme.clone());
        let value = topic
            .to_yaml_value()
            .expect("Theme is persistent; must serialize to TOML");
        let restored =
            Topic::from_yaml_section(TopicKind::Theme, value).expect("section should deserialize");
        match restored {
            Topic::Theme(back) => assert_eq!(theme, back),
            other => panic!("expected Theme, got {other:?}"),
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
/// `to_yaml_value` / `from_yaml_section` until Phase 5 adds the first
/// real persistent topic.
#[cfg(test)]
mod persistent_yaml_tests {
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
    fn persistent_payload_roundtrips_via_yaml() {
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
            .to_yaml_value()
            .expect("persistent payload should serialize to YAML");

        let restored =
            Topic::from_yaml_section(TopicKind::Zones, value).expect("section should deserialize");

        match restored {
            Topic::Zones(decoded) => assert_eq!(decoded, zones),
            _ => panic!("wrong variant after round-trip"),
        }
    }

    #[test]
    fn non_persistent_kinds_do_not_deserialize() {
        let value = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
        assert!(Topic::from_yaml_section(TopicKind::Ping, value).is_none());
    }

    #[test]
    fn malformed_section_returns_none() {
        // Wrong shape for a HashMap<String, Zone>: a bare string.
        let value = serde_yaml_ng::Value::String("oops".into());
        assert!(Topic::from_yaml_section(TopicKind::Zones, value).is_none());
    }
}
