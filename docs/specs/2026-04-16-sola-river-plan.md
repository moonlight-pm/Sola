# sola-river Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `sola-compositor` (Smithay) with `sola-river`: a small Wayland client that supervises `/usr/bin/river` and translates the Sola bus to river's `river-window-management-v1` protocol.

**Architecture:** `sola-river` spawns River via `std::process::Command`, waits for its wayland socket, then connects as a client. It binds `river_window_manager_v1`, `river_xkb_bindings_v1`, and `wl_seat`. A `Translator` maps bus topics to and from river protocol requests/events. A `PendingUpdate` struct batches changes and flushes per calloop tick as manage/render sequences. All other Sola apps connect to River directly via `wayland-0` inherited from `sola`.

**Tech Stack:** Rust 2024, `wayland-client` + `wayland-scanner` (compile-time binding generation from vendored XML), `calloop` (event loop), `sola-bus` (existing), `tracing`.

**Scope guardrails (do not expand):**
- No multi-output handling, layer-shell, IME, libinput config, session restore, decorations, or MRU logic changes in shell beyond what the plan specifies.
- `sola-shell`, `sola-terminal`, `sola-monitor` get surgical edits only. Do not refactor adjacent code.
- `Windows` topic is renamed to `Apps` with the same payload shape (sans `parent_window_id`, which drops — River's `parent` event handling is deferred).

---

## File Structure

**New crate: `crates/sola-river/`**

```
crates/sola-river/
├── Cargo.toml
├── build.rs                        # wayland-scanner codegen
├── protocols/
│   ├── river-window-management-v1.xml  # vendored from /usr/share/river-protocols
│   └── river-xkb-bindings-v1.xml
└── src/
    ├── main.rs                     # entry point
    ├── supervisor.rs               # spawn /usr/bin/river, wait for socket, watch child pid
    ├── bus.rs                      # wrapper over sola_bus::BusClient
    ├── translator.rs               # bus <-> river mapping
    ├── registry.rs                 # WindowRegistry, ChordRegistry
    ├── pending.rs                  # PendingUpdate struct
    ├── protocol.rs                 # re-exports for wayland-scanner-generated modules
    └── client/
        ├── mod.rs                  # Wayland connection + AppData dispatch state
        ├── window.rs               # river_window_v1 event handlers
        ├── manage.rs               # manage_start / render_start handlers
        ├── seat.rs                 # river_seat_v1 pointer_enter/leave/window_interaction
        └── binding.rs              # river_xkb_binding_v1 pressed handler
```

**Bus changes:** `crates/sola-bus/src/topics.rs` — rename `Windows` to `Apps`, remove `SetWindowPolicy` + `WindowPolicyPayload` + `WindowPolicy`, remove `ShellKeyBindings` (replaced by `RegisteredChords`), add `RegisteredChords`, `Chord`, `MouseClicked`. Keep `MouseEntered`, add `MouseLeft` (unit). Drop `parent_window_id` from the payload.

**Process manager:** `crates/sola/src/main.rs` — `MANAGED` list swaps `sola-compositor` to `sola-river`.

**Shell:** `apps/shell/src/app.rs` and related — switch topic names, replace `ShellKeyBindings` emission with `RegisteredChords`, track MRU window per app, handle `MouseClicked`.

**Terminal:** `apps/terminal/src/**` — delete `SetWindowPolicy` emission sites.

**Monitor:** `apps/monitor/src/**` — delete `SetWindowPolicy` emission sites.

**App framework (`crates/sola-app/`):** the `WindowConfig` struct currently has flags that map to `SetWindowPolicy`. Strip the `SetWindowPolicy` emission; keep the struct fields for now (removing them would be a deeper refactor).

**Deletion:** entire `crates/sola-compositor/` directory.

---

## Phase 0: Vendor protocol XML + scaffold crate

### Task 0.1: Create crate skeleton

**Files:**
- Create: `crates/sola-river/Cargo.toml`
- Create: `crates/sola-river/build.rs`
- Create: `crates/sola-river/src/main.rs`
- Create: `crates/sola-river/protocols/river-window-management-v1.xml`
- Create: `crates/sola-river/protocols/river-xkb-bindings-v1.xml`
- Modify: workspace `Cargo.toml` (members)

- [ ] **Step 1: Copy vendored protocol XMLs**

```bash
cp /tmp/river-window-management-v1.xml crates/sola-river/protocols/
cp /tmp/river-xkb-bindings-v1.xml      crates/sola-river/protocols/
```

- [ ] **Step 2: Write `crates/sola-river/Cargo.toml`**

```toml
[package]
name = "sola-river"
version = "0.1.0"
edition = "2024"

[dependencies]
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
tracing-appender = { workspace = true }
wayland-client = "0.31"
wayland-backend = "0.3"
calloop = "0.14"
calloop-wayland-source = "0.4"
libc = "0.2"
thiserror = "1"

[build-dependencies]
wayland-scanner = "0.31"
```

Check workspace `Cargo.toml` and existing `sola-compositor/Cargo.toml` for pinned versions of `tracing`, `calloop`, etc., and match them. Add the new crate to `[workspace] members`.

- [ ] **Step 3: Write `crates/sola-river/build.rs`**

```rust
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for (xml, stem) in [
        ("protocols/river-window-management-v1.xml", "river_window_management_v1"),
        ("protocols/river-xkb-bindings-v1.xml",      "river_xkb_bindings_v1"),
    ] {
        println!("cargo:rerun-if-changed={xml}");
        wayland_scanner::generate_code(
            xml,
            out_dir.join(format!("{stem}_client.rs")),
            wayland_scanner::Side::Client,
        );
    }
}
```

- [ ] **Step 4: Write placeholder `src/main.rs`**

```rust
fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("sola-river starting (scaffold)");
}
```

- [ ] **Step 5: Verify the workspace compiles**

Run: `cargo check -p sola-river`
Expected: clean build of the scaffold.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-river/ Cargo.toml
git commit -m "feat(sola-river): scaffold crate and vendor river protocol XML"
```

---

## Phase 1: Bus protocol changes

Change the bus types first; the Rust compiler will point out every site that needs updating.

### Task 1.1: Rewrite `topics.rs`

**Files:**
- Modify: `crates/sola-bus/src/topics.rs`

- [ ] **Step 1: Apply these changes**

Rename `WindowInfo` to `App`, drop `parent_window_id`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub window_id: u32,
    pub app_id: String,
    pub title: String,
}
```

Delete `WindowPolicyPayload`, `WindowPolicy`, `ShellKeyBindingsPayload`.

Add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredChord {
    pub keysym: u32,
    pub modifiers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChordEvent {
    pub keysym: u32,
    pub modifiers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseClickedPayload {
    pub window_id: u32,
}
```

Keep `MouseEnteredPayload` as-is.

Update the `define_topics!` macro invocation:

```rust
define_topics! {
    Apps(Vec<App>),
    LaunchApp(String),

    Composition(Vec<CompositionEntry>),
    Frame(FrameUpdate),
    Focus(FocusTarget),

    OutputGeometry(OutputGeometry),

    MouseEntered(MouseEnteredPayload),
    MouseLeft,
    MouseClicked(MouseClickedPayload),

    RegisteredChords(Vec<RegisteredChord>),
    Chord(ChordEvent),

    SetAppMenu(AppMenuPayload),
    MenuAction(MenuActionPayload),

    OpenUrl(OpenUrlRequest),

    Shutdown,
}
```

Update tests — rename `Topic::Windows` to `Topic::Apps`, `WindowInfo` to `App`, drop `parent_window_id`.

- [ ] **Step 2: Verify bus compiles**

Run: `cargo check -p sola-bus`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "refactor(bus): rename Windows->Apps; add RegisteredChords/Chord/MouseClicked/MouseLeft; remove WindowPolicy and ShellKeyBindings"
```

---

## Phase 2: Fix every compile error the bus change produced

A broad sweep; each edit is small. Batch into one commit at the end.

### Task 2.1: sola-app framework

**Files:**
- Modify: `crates/sola-app/src/**/*.rs` wherever `SetWindowPolicy`, `WindowInfo`, `Topic::Windows`, `ShellKeyBindings` appear

- [ ] **Step 1: Find all sites**

Use Grep to enumerate. Delete the `SetWindowPolicy` emission sites. Keep the `WindowConfig` struct fields (`zoned`, `keyboard_target`, `decorated`, `transparent`) — they may still drive app-local behavior.

- [ ] **Step 2: Rename `Topic::Windows` to `Topic::Apps` and `WindowInfo` to `App`** in any consumer code.

- [ ] **Step 3: Remove `ShellKeyBindings` imports and emission** (framework-side, if any).

- [ ] **Step 4: Verify**

Run: `cargo check -p sola-app`
Expected: clean.

### Task 2.2: sola-shell

**Files:**
- Modify: `apps/shell/src/app.rs`
- Modify: `apps/shell/src/keys.rs`
- Modify: `apps/shell/src/zoning.rs` (if references `WindowInfo`)
- Modify: `apps/shell/src/switcher/state.rs` (if references `WindowInfo`)
- Modify: `apps/shell/src/menubar/mod.rs` (if relevant)

- [ ] **Step 1: Imports**

Replace `WindowInfo` with `App` and `Topic::Windows(windows)` with `Topic::Apps(apps)` everywhere. Rename `handle_windows_update` to `handle_apps_update`, `known_windows` to `known_apps`.

Remove imports of `ShellKeyBindingsPayload`. Add imports for `RegisteredChord`, `ChordEvent`, `MouseClickedPayload`.

- [ ] **Step 2: Replace `emit_shell_key_bindings` with `emit_registered_chords`**

Emit `Topic::RegisteredChords` (sticky). Build `Vec<RegisteredChord>` from `self.menus.key_bindings()` + hardcoded shell chords (Meta+Tab, Meta+Space, Meta+numpad zoning keys). Map each `KeyChord` to a `RegisteredChord` via helpers in `shell/src/keys.rs`:

```rust
pub fn to_registered(chord: &KeyChord) -> RegisteredChord {
    RegisteredChord {
        keysym: keycode_to_keysym(chord.keycode),
        modifiers: river_modifiers(chord),
    }
}

fn river_modifiers(c: &KeyChord) -> u32 {
    let mut m = 0u32;
    if c.shift { m |= 1; }   // shift
    if c.ctrl  { m |= 4; }   // ctrl
    if c.alt   { m |= 8; }   // mod1 (alt)
    if c.meta  { m |= 64; }  // mod4 (super)
    m
}
```

`keycode_to_keysym`: inspect `sola-core::KeyCode`. If its numeric values already correspond to xkbcommon keysyms, `chord.keycode as u32` works directly. Otherwise implement a minimal match.

- [ ] **Step 3: Rename field and method**

`self.known_windows` becomes `self.known_apps`. `handle_windows_update(windows: Vec<WindowInfo>)` becomes `handle_apps_update(apps: Vec<App>)`. Drop any references to `parent_window_id`.

- [ ] **Step 4: Handle `Topic::Chord` in `on_bus_event`**

```rust
Topic::Chord(chord) => self.handle_chord(chord, ctx),
```

Move the existing chord-matching logic (currently in `keys.rs` listening via a compositor-specific channel) into `fn handle_chord(&mut self, chord: ChordEvent, ctx: &mut AppCtx)`. Reuse `self.menus.lookup_shortcut(&our_chord)` to fire `Topic::MenuAction`. Activate switcher / launcher / zoning the same way.

- [ ] **Step 5: Handle `Topic::MouseClicked`**

```rust
Topic::MouseClicked(MouseClickedPayload { window_id }) => {
    let info = self.known_apps.iter().find(|w| w.window_id == *window_id);
    let Some(info) = info else { return };
    if info.app_id == Self::APP_ID { return; }
    if self.menu_open || self.switcher.active || self.launcher.active { return; }
    let app_id = info.app_id.clone();
    self.set_focus(&app_id);
    self.focused_window_id = Some(*window_id);
    ctx.emit(Topic::Focus(FocusTarget { window_id: *window_id }));
    self.emit_composition(ctx);
}
```

- [ ] **Step 6: Handle `Topic::MouseLeft`**

No-op. Do not match or log.

- [ ] **Step 7: MRU window-per-app tracking**

Add `pub mru_window_by_app: HashMap<String, u32>`. Update it whenever we emit `Topic::Focus` for a user-app window:

```rust
fn remember_focus(&mut self, app_id: &str, window_id: u32) {
    self.mru_window_by_app.insert(app_id.to_string(), window_id);
}
```

Look up via `self.mru_window_by_app.get(app_id)` when activating an app via Super+Tab.

- [ ] **Step 8: Delete compositor-channel wiring in `keys.rs`**

If `apps/shell/src/keys.rs` installs a callback that listened to the compositor's input channel, delete that `install` function. If `app.rs::after_runtime_ready` calls `keys::install`, remove the call. Keep pure helpers (`to_registered`, `from_registered`, shortcut lookup) only.

- [ ] **Step 9: Verify**

Run: `cargo check -p sola-shell`
Expected: clean.

### Task 2.3: sola-terminal and sola-monitor

**Files:**
- Modify: `apps/terminal/src/**/*.rs`
- Modify: `apps/monitor/src/**/*.rs`

- [ ] **Step 1: Grep and delete `SetWindowPolicy` emission sites**

Delete each occurrence. Drop orphaned imports and helpers.

- [ ] **Step 2: Rename any `Topic::Windows` / `WindowInfo` references**

- [ ] **Step 3: Verify**

Run: `cargo check -p sola-terminal -p sola-monitor`
Expected: clean.

### Task 2.4: sola binary (process manager)

**Files:**
- Modify: `crates/sola/src/main.rs`

- [ ] **Step 1: Update MANAGED**

```rust
const MANAGED: &[&str] = &[
    "sola-bus",
    "sola-river",
    "sola-shell",
    "sola-terminal",
];
```

- [ ] **Step 2: Verify**

Run: `cargo check -p sola`

### Task 2.5: Delete sola-compositor

**Files:**
- Delete: `crates/sola-compositor/`
- Modify: workspace `Cargo.toml`

- [ ] **Step 1: Remove workspace member entry**
- [ ] **Step 2: Remove directory**

```bash
rm -rf crates/sola-compositor
```

- [ ] **Step 3: Verify workspace**

Run: `cargo check --workspace`

- [ ] **Step 4: Commit the Phase 2 sweep**

```bash
git add -A
git commit -m "refactor: migrate consumers to new bus topics; delete sola-compositor"
```

---

## Phase 3: Pure logic — registries and pending update (TDD)

Pure in-memory data structures. Easy to test without Wayland.

### Task 3.1: WindowRegistry

**Files:**
- Create: `crates/sola-river/src/registry.rs`
- Create: `crates/sola-river/src/lib.rs`

- [ ] **Step 1: Write failing tests in `registry.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_assigns_monotonic_ids() {
        let mut r = WindowRegistry::new();
        assert_eq!(r.mint(), 1);
        assert_eq!(r.mint(), 2);
    }

    #[test]
    fn set_and_get_app_id() {
        let mut r = WindowRegistry::new();
        let id = r.mint();
        r.set_app_id(id, "zen".into());
        assert_eq!(r.get(id).unwrap().app_id.as_deref(), Some("zen"));
    }

    #[test]
    fn remove_drops_entry() {
        let mut r = WindowRegistry::new();
        let id = r.mint();
        r.remove(id);
        assert!(r.get(id).is_none());
    }

    #[test]
    fn as_apps_returns_only_fully_populated() {
        let mut r = WindowRegistry::new();
        let a = r.mint();
        r.set_app_id(a, "zen".into());
        r.set_title(a, "Browser".into());
        let b = r.mint();
        r.set_app_id(b, "pending".into());
        let apps = r.as_apps();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].window_id, a);
        assert_eq!(apps[0].app_id, "zen");
    }
}
```

- [ ] **Step 2: Implement**

```rust
use sola_bus::topics::App;
use std::collections::HashMap;

#[derive(Default)]
pub struct WindowRegistry {
    next_id: u32,
    by_id: HashMap<u32, Entry>,
}

pub struct Entry {
    pub app_id: Option<String>,
    pub title: Option<String>,
}

impl WindowRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn mint(&mut self) -> u32 {
        self.next_id += 1;
        self.by_id.insert(self.next_id, Entry { app_id: None, title: None });
        self.next_id
    }

    pub fn get(&self, id: u32) -> Option<&Entry> { self.by_id.get(&id) }

    pub fn set_app_id(&mut self, id: u32, value: String) {
        if let Some(e) = self.by_id.get_mut(&id) { e.app_id = Some(value); }
    }

    pub fn set_title(&mut self, id: u32, value: String) {
        if let Some(e) = self.by_id.get_mut(&id) { e.title = Some(value); }
    }

    pub fn remove(&mut self, id: u32) { self.by_id.remove(&id); }

    pub fn as_apps(&self) -> Vec<App> {
        let mut v: Vec<App> = self.by_id.iter()
            .filter_map(|(id, e)| {
                let (Some(app_id), Some(title)) = (e.app_id.clone(), e.title.clone()) else {
                    return None;
                };
                Some(App { window_id: *id, app_id, title })
            })
            .collect();
        v.sort_by_key(|a| a.window_id);
        v
    }
}
```

Create `src/lib.rs`:

```rust
pub mod pending;
pub mod registry;
```

- [ ] **Step 3: Verify tests pass**

Run: `cargo test -p sola-river`

### Task 3.2: PendingUpdate

**Files:**
- Create: `crates/sola-river/src/pending.rs`

- [ ] **Step 1: Tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_marks_manage_and_render_dirty() {
        let mut p = PendingUpdate::default();
        p.frame(1, 100, 200, 800, 600);
        assert_eq!(p.manage.get(&1).copied(), Some((800, 600)));
        assert_eq!(p.render_positions.get(&1).copied(), Some((100, 200)));
        assert!(p.manage_dirty);
        assert!(p.render_dirty);
    }

    #[test]
    fn composition_replaces_z_order_and_marks_render_dirty() {
        let mut p = PendingUpdate::default();
        p.set_composition(vec![3, 1, 2]);
        assert_eq!(p.composition.as_deref(), Some([3u32, 1, 2].as_slice()));
        assert!(p.render_dirty);
    }

    #[test]
    fn clear_resets_everything() {
        let mut p = PendingUpdate::default();
        p.frame(1, 0, 0, 10, 10);
        p.set_composition(vec![1]);
        p.clear();
        assert!(p.manage.is_empty());
        assert!(p.render_positions.is_empty());
        assert!(p.composition.is_none());
        assert!(!p.manage_dirty);
        assert!(!p.render_dirty);
    }
}
```

- [ ] **Step 2: Implement**

```rust
use std::collections::HashMap;

#[derive(Default)]
pub struct PendingUpdate {
    pub manage: HashMap<u32, (i32, i32)>,
    pub render_positions: HashMap<u32, (i32, i32)>,
    pub composition: Option<Vec<u32>>,
    pub focus: Option<FocusAction>,
    pub manage_dirty: bool,
    pub render_dirty: bool,
}

pub enum FocusAction {
    Window(u32),
    None,
}

impl PendingUpdate {
    pub fn frame(&mut self, id: u32, x: i32, y: i32, w: i32, h: i32) {
        self.manage.insert(id, (w, h));
        self.render_positions.insert(id, (x, y));
        self.manage_dirty = true;
        self.render_dirty = true;
    }

    pub fn set_composition(&mut self, order: Vec<u32>) {
        self.composition = Some(order);
        self.render_dirty = true;
    }

    pub fn set_focus(&mut self, action: FocusAction) {
        self.focus = Some(action);
        self.render_dirty = true;
    }

    pub fn clear(&mut self) {
        self.manage.clear();
        self.render_positions.clear();
        self.composition = None;
        self.focus = None;
        self.manage_dirty = false;
        self.render_dirty = false;
    }
}
```

- [ ] **Step 3: Verify**

Run: `cargo test -p sola-river`

### Task 3.3: ChordRegistry diff

**Files:**
- Modify: `crates/sola-river/src/registry.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn chord_diff_added_and_removed() {
    let old: Vec<(u32, u32)> = vec![(0x61, 64), (0x62, 64)];
    let new: Vec<(u32, u32)> = vec![(0x62, 64), (0x63, 64)];
    let (added, removed) = chord_diff(&old, &new);
    assert_eq!(added,   vec![(0x63, 64)]);
    assert_eq!(removed, vec![(0x61, 64)]);
}
```

- [ ] **Step 2: Implement**

```rust
pub fn chord_diff(old: &[(u32, u32)], new: &[(u32, u32)])
    -> (Vec<(u32, u32)>, Vec<(u32, u32)>)
{
    use std::collections::HashSet;
    let old_set: HashSet<(u32, u32)> = old.iter().copied().collect();
    let new_set: HashSet<(u32, u32)> = new.iter().copied().collect();
    let mut added: Vec<_> = new_set.difference(&old_set).copied().collect();
    let mut removed: Vec<_> = old_set.difference(&new_set).copied().collect();
    added.sort();
    removed.sort();
    (added, removed)
}
```

- [ ] **Step 3: Verify + commit**

```bash
cargo test -p sola-river
git add crates/sola-river/src/
git commit -m "feat(sola-river): WindowRegistry, PendingUpdate, chord diff with tests"
```

---

## Phase 4: Supervisor

### Task 4.1: Implement supervisor

**Files:**
- Create: `crates/sola-river/src/supervisor.rs`
- Modify: `crates/sola-river/src/lib.rs` (add `pub mod supervisor;`)
- Modify: `crates/sola-river/src/main.rs`

- [ ] **Step 1: `supervisor.rs`**

```rust
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

pub struct RiverSupervisor {
    child: Child,
    socket_path: PathBuf,
}

impl RiverSupervisor {
    pub fn spawn(log_path: &Path) -> io::Result<Self> {
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/1000"));
        let socket_path = runtime_dir.join("wayland-0");

        let log = std::fs::OpenOptions::new()
            .create(true).append(true).open(log_path)?;
        let log_err = log.try_clone()?;

        let child = unsafe {
            Command::new("/usr/bin/river")
                .args(["-log-level", "info"])
                .stdout(Stdio::from(log))
                .stderr(Stdio::from(log_err))
                .pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    libc::setsid();
                    Ok(())
                })
                .spawn()?
        };

        info!(pid = child.id(), "spawned river");
        Ok(Self { child, socket_path })
    }

    pub fn wait_for_socket(&self) -> io::Result<()> {
        let start = Instant::now();
        let total_cap = Duration::from_secs(30);
        let mut delay = Duration::from_millis(10);
        let cap = Duration::from_secs(1);
        loop {
            if self.socket_path.exists() {
                info!(path = %self.socket_path.display(), "river socket appeared");
                return Ok(());
            }
            if start.elapsed() > total_cap {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("river socket {} did not appear within 30s", self.socket_path.display()),
                ));
            }
            std::thread::sleep(delay);
            delay = std::cmp::min(delay * 2, cap);
        }
    }

    pub fn pid(&self) -> u32 { self.child.id() }

    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub fn shutdown(&mut self) {
        let pid = self.child.id() as i32;
        unsafe { libc::kill(pid, libc::SIGTERM); }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() >= deadline => {
                    warn!(pid, "river did not exit after SIGTERM; sending SIGKILL");
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    error!(%e, pid, "error waiting on river");
                    return;
                }
            }
        }
    }

    pub fn socket_path(&self) -> &Path { &self.socket_path }
}
```

- [ ] **Step 2: Wire `main.rs`**

```rust
use std::path::Path;
use std::process::exit;
use std::time::Duration;
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod bus;
mod client;
mod pending;
mod protocol;
mod registry;
mod supervisor;
mod translator;

fn main() {
    init_tracing();
    info!("sola-river starting");

    let mut sup = match supervisor::RiverSupervisor::spawn(Path::new("/opt/sola/log/river.log")) {
        Ok(s) => s,
        Err(e) => { error!(%e, "failed to spawn river"); exit(1); }
    };

    if let Err(e) = sup.wait_for_socket() {
        error!(%e, "river socket never appeared");
        sup.shutdown();
        exit(1);
    }

    // Phase 5 wires the wayland client here.
    loop {
        match sup.try_wait() {
            Ok(Some(status)) => { error!(?status, "river exited; sola-river exiting"); exit(1); }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => { error!(%e, "try_wait failed"); exit(1); }
        }
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sola_river=info".into());
    let _ = std::fs::create_dir_all("/opt/sola/log");
    let file_appender = tracing_appender::rolling::never("/opt/sola/log", "sola-river.log");
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(file_appender);
    tracing_subscriber::registry().with(filter).with(stderr_layer).with(file_layer).init();
}
```

Create empty stubs `bus.rs`, `translator.rs`, `client/mod.rs`, `protocol.rs` so the `mod` declarations compile.

- [ ] **Step 3: Verify**

Run: `cargo check -p sola-river`

- [ ] **Step 4: Commit**

```bash
git add crates/sola-river/
git commit -m "feat(sola-river): supervisor spawns /usr/bin/river and waits for socket"
```

---

## Phase 5: Wayland client — connection, globals, event dispatch

Broken by responsibility. A single `AppData` struct implements each needed `Dispatch<..>` trait.

### Task 5.1: Protocol module + globals binding

**Files:**
- Modify: `crates/sola-river/src/protocol.rs`
- Modify: `crates/sola-river/src/client/mod.rs`

- [ ] **Step 1: `protocol.rs` — include generated bindings**

```rust
#![allow(non_snake_case, non_camel_case_types, clippy::all)]

pub mod river_window_management_v1 {
    use wayland_client;
    use wayland_client::protocol::*;
    include!(concat!(env!("OUT_DIR"), "/river_window_management_v1_client.rs"));
}

pub mod river_xkb_bindings_v1 {
    use wayland_client;
    use wayland_client::protocol::*;
    use crate::protocol::river_window_management_v1::*;
    include!(concat!(env!("OUT_DIR"), "/river_xkb_bindings_v1_client.rs"));
}
```

If the generated code needs different `use` imports, inspect `target/debug/build/sola-river-*/out/` after the first build and adjust.

- [ ] **Step 2: `client/mod.rs` — connection + globals binding**

```rust
use std::collections::HashMap;

use tracing::{error, info, warn};
use wayland_client::{
    backend::ObjectId,
    protocol::{wl_output, wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};

use crate::protocol::river_window_management_v1::{
    river_node_v1::RiverNodeV1,
    river_output_v1::RiverOutputV1,
    river_seat_v1::RiverSeatV1,
    river_window_manager_v1::RiverWindowManagerV1,
    river_window_v1::RiverWindowV1,
};
use crate::protocol::river_xkb_bindings_v1::{
    river_xkb_binding_v1::RiverXkbBindingV1,
    river_xkb_bindings_seat_v1::RiverXkbBindingsSeatV1,
    river_xkb_bindings_v1::RiverXkbBindingsV1,
};

pub struct AppData {
    pub wm: Option<RiverWindowManagerV1>,
    pub xkb_bindings: Option<RiverXkbBindingsV1>,
    pub seat: Option<RiverSeatV1>,
    pub wl_seat: Option<wl_seat::WlSeat>,
    pub registry: crate::registry::WindowRegistry,
    pub pending: crate::pending::PendingUpdate,
    pub chords: crate::registry::ChordRegistry,
    pub bus: crate::bus::BusClient,
    pub windows_by_object: HashMap<ObjectId, u32>,
    pub windows_by_id: HashMap<u32, RiverWindowV1>,
    pub nodes_by_window: HashMap<u32, RiverNodeV1>,
    pub qh: Option<QueueHandle<Self>>,
}

impl AppData {
    pub fn new(bus: crate::bus::BusClient) -> Self {
        Self {
            wm: None,
            xkb_bindings: None,
            seat: None,
            wl_seat: None,
            registry: crate::registry::WindowRegistry::new(),
            pending: crate::pending::PendingUpdate::default(),
            chords: crate::registry::ChordRegistry::default(),
            bus,
            windows_by_object: HashMap::new(),
            windows_by_id: HashMap::new(),
            nodes_by_window: HashMap::new(),
            qh: None,
        }
    }
}

pub fn connect(bus: crate::bus::BusClient)
    -> Result<(Connection, EventQueue<AppData>, AppData), Box<dyn std::error::Error>>
{
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut queue = conn.new_event_queue::<AppData>();
    let qh = queue.handle();
    let _registry = display.get_registry(&qh, ());

    let mut data = AppData::new(bus);
    queue.roundtrip(&mut data)?;
    queue.roundtrip(&mut data)?;

    if data.wm.is_none() {
        return Err("river_window_manager_v1 not advertised — is River 0.4.2+ running?".into());
    }
    data.qh = Some(qh);
    info!("bound river_window_manager_v1");
    Ok((conn, queue, data))
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "river_window_manager_v1" => {
                    let wm: RiverWindowManagerV1 = proxy.bind(name, version.min(4), qh, ());
                    state.wm = Some(wm);
                }
                "river_xkb_bindings_v1" => {
                    let xb: RiverXkbBindingsV1 = proxy.bind(name, version.min(2), qh, ());
                    state.xkb_bindings = Some(xb);
                }
                "wl_seat" => {
                    let s: wl_seat::WlSeat = proxy.bind(name, version.min(7), qh, ());
                    state.wl_seat = Some(s);
                }
                _ => {}
            }
        }
    }
}

// Stub dispatches — expanded in subsequent tasks.
impl Dispatch<wl_seat::WlSeat, ()> for AppData {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<wl_output::WlOutput, ()> for AppData {
    fn event(_: &mut Self, _: &wl_output::WlOutput, _: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<RiverOutputV1, ()> for AppData {
    fn event(_: &mut Self, _: &RiverOutputV1, _: <RiverOutputV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<RiverNodeV1, ()> for AppData {
    fn event(_: &mut Self, _: &RiverNodeV1, _: <RiverNodeV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<RiverXkbBindingsV1, ()> for AppData {
    fn event(_: &mut Self, _: &RiverXkbBindingsV1, _: <RiverXkbBindingsV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<RiverXkbBindingsSeatV1, ()> for AppData {
    fn event(_: &mut Self, _: &RiverXkbBindingsSeatV1, _: <RiverXkbBindingsSeatV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
```

Tasks 5.3–5.6 replace the relevant stubs with full implementations.

- [ ] **Step 3: Verify**

Run: `cargo check -p sola-river`

- [ ] **Step 4: Commit**

```bash
git add crates/sola-river/src/
git commit -m "feat(sola-river): wayland client connection and global binding"
```

### Task 5.2: Bus client wrapper

**Files:**
- Modify: `crates/sola-river/src/bus.rs`

- [ ] **Step 1: Implement**

```rust
use sola_bus::{topics::Topic, Message};

pub struct BusClient {
    inner: sola_bus::BusClient,
}

impl BusClient {
    pub fn new() -> Self {
        let mut inner = sola_bus::BusClient::new();
        inner.set_app_id("sola-river");
        Self { inner }
    }

    pub fn ensure_connected(&mut self) {
        if !self.inner.is_connected() { let _ = self.inner.connect(); }
    }
    pub fn try_recv(&mut self) -> Option<Message> { self.inner.try_recv() }
    pub fn emit(&mut self, topic: Topic) { self.inner.emit(topic); }
    pub fn emit_sticky(&mut self, topic: Topic) { self.inner.emit_sticky(topic); }

    pub fn subscribe(&mut self) {
        self.inner.subscribe("Composition");
        self.inner.subscribe("Frame");
        self.inner.subscribe("Focus");
        self.inner.subscribe("RegisteredChords");
        self.inner.subscribe("Shutdown");
    }
}
```

Cross-check method names against `crates/sola-bus/src/client.rs` and adjust if different.

- [ ] **Step 2: Verify + commit**

```bash
cargo check -p sola-river
git add crates/sola-river/src/bus.rs
git commit -m "feat(sola-river): bus client wrapper"
```

### Task 5.3: Window lifecycle dispatch

**Files:**
- Modify: `crates/sola-river/src/client/mod.rs`
- Create: `crates/sola-river/src/client/window.rs`

- [ ] **Step 1: Full `Dispatch<RiverWindowManagerV1, ()>`**

Replace the stub:

```rust
impl Dispatch<RiverWindowManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _wm: &RiverWindowManagerV1,
        event: <RiverWindowManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_window_manager_v1::Event;
        match event {
            Event::Window { window } => {
                let id = state.registry.mint();
                state.windows_by_object.insert(window.id(), id);
                let node = window.get_node(qh, ());
                state.nodes_by_window.insert(id, node);
                state.windows_by_id.insert(id, window);
                info!(window_id = id, "new river window");
            }
            Event::ManageStart { serial } => crate::client::manage::handle_manage_start(state, serial),
            Event::RenderStart { serial } => crate::client::manage::handle_render_start(state, serial),
            Event::Seat { seat } => { if state.seat.is_none() { state.seat = Some(seat); } }
            Event::Unavailable => { error!("river_window_manager_v1 unavailable"); }
            Event::Finished => { warn!("river_window_manager_v1 finished"); }
            _ => {}
        }
    }
}
```

- [ ] **Step 2: `Dispatch<RiverWindowV1, ()>` — look up id via object**

```rust
impl Dispatch<RiverWindowV1, ()> for AppData {
    fn event(
        state: &mut Self,
        window: &RiverWindowV1,
        event: <RiverWindowV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_window_v1::Event;
        let Some(&window_id) = state.windows_by_object.get(&window.id()) else {
            warn!(object = ?window.id(), "event for unknown window object");
            return;
        };
        let mut apps_dirty = false;
        match event {
            Event::AppId { app_id } => { state.registry.set_app_id(window_id, app_id); apps_dirty = true; }
            Event::Title { title }  => { state.registry.set_title(window_id, title); apps_dirty = true; }
            Event::Closed => {
                state.registry.remove(window_id);
                state.windows_by_object.retain(|_, v| *v != window_id);
                state.windows_by_id.remove(&window_id);
                state.nodes_by_window.remove(&window_id);
                window.destroy();
                apps_dirty = true;
            }
            _ => {}
        }
        if apps_dirty { crate::translator::emit_apps(state); }
    }
}
```

- [ ] **Step 3: Verify + commit**

```bash
cargo check -p sola-river
git add crates/sola-river/src/
git commit -m "feat(sola-river): window lifecycle dispatch emits Apps topic"
```

### Task 5.4: Manage/render sequences + translator

**Files:**
- Modify: `crates/sola-river/src/translator.rs`
- Create: `crates/sola-river/src/client/manage.rs`
- Modify: `crates/sola-river/src/client/mod.rs` (add `pub mod manage;`)

- [ ] **Step 1: `translator.rs`**

```rust
use sola_bus::topics::Topic;
use tracing::debug;

use crate::client::AppData;

pub fn emit_apps(state: &mut AppData) {
    let apps = state.registry.as_apps();
    debug!(count = apps.len(), "emitting Apps");
    state.bus.emit_sticky(Topic::Apps(apps));
}
```

- [ ] **Step 2: `client/manage.rs`**

```rust
use tracing::debug;

use crate::client::AppData;
use crate::pending::FocusAction;

pub fn handle_manage_start(state: &mut AppData, serial: u32) {
    let Some(wm) = state.wm.clone() else { return };

    for (&window_id, &(w, h)) in &state.pending.manage {
        if let Some(proxy) = state.windows_by_id.get(&window_id) {
            proxy.propose_dimensions(w as u32, h as u32);
            proxy.set_borders(0);
        }
    }

    wm.manage_finish(serial);
    debug!(serial, "manage_finish sent");
}

pub fn handle_render_start(state: &mut AppData, serial: u32) {
    let Some(wm) = state.wm.clone() else { return };

    if let Some(order) = &state.pending.composition {
        for &window_id in order {
            if let Some(node) = state.nodes_by_window.get(&window_id) {
                node.place_top();
            }
        }
    }

    for (&window_id, &(x, y)) in &state.pending.render_positions {
        if let Some(node) = state.nodes_by_window.get(&window_id) {
            node.set_position(x, y);
        }
    }

    if let Some(focus) = state.pending.focus.as_ref() {
        if let Some(seat) = state.seat.as_ref() {
            match focus {
                FocusAction::Window(id) => {
                    if let Some(proxy) = state.windows_by_id.get(id) {
                        seat.focus_window(proxy);
                    }
                }
                FocusAction::None => seat.clear_focus(),
            }
        }
    }

    wm.render_finish(serial);
    state.pending.clear();
    debug!(serial, "render_finish sent");
}
```

- [ ] **Step 3: Verify + commit**

```bash
cargo check -p sola-river
git add crates/sola-river/src/
git commit -m "feat(sola-river): manage and render sequence handling"
```

### Task 5.5: Seat dispatch

**Files:**
- Create: `crates/sola-river/src/client/seat.rs`
- Modify: `crates/sola-river/src/client/mod.rs` (replace stub)

- [ ] **Step 1: Implement**

```rust
impl Dispatch<RiverSeatV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _seat: &RiverSeatV1,
        event: <RiverSeatV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_window_management_v1::river_seat_v1::Event;
        use sola_bus::topics::{MouseClickedPayload, MouseEnteredPayload, Topic};
        match event {
            Event::PointerEnter { window } => {
                if let Some(&id) = state.windows_by_object.get(&window.id()) {
                    state.bus.emit(Topic::MouseEntered(MouseEnteredPayload { window_id: id }));
                }
            }
            Event::PointerLeave => { state.bus.emit(Topic::MouseLeft); }
            Event::WindowInteraction { window } => {
                if let Some(&id) = state.windows_by_object.get(&window.id()) {
                    state.bus.emit(Topic::MouseClicked(MouseClickedPayload { window_id: id }));
                }
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 2: Verify + commit**

```bash
cargo check -p sola-river
git add -A
git commit -m "feat(sola-river): emit MouseEntered/Left/Clicked from seat events"
```

### Task 5.6: Xkb bindings

**Files:**
- Modify: `crates/sola-river/src/registry.rs` (add `ChordRegistry`)
- Modify: `crates/sola-river/src/translator.rs` (add `update_registered_chords`)
- Create: `crates/sola-river/src/client/binding.rs`

- [ ] **Step 1: `ChordRegistry`**

```rust
use std::collections::HashMap;
use wayland_client::backend::ObjectId;
use crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::RiverXkbBindingV1;

#[derive(Default)]
pub struct ChordRegistry {
    pub by_chord: HashMap<(u32, u32), RiverXkbBindingV1>,
    pub by_object: HashMap<ObjectId, (u32, u32)>,
}
```

- [ ] **Step 2: `translator::update_registered_chords`**

```rust
pub fn update_registered_chords(
    state: &mut AppData,
    new: Vec<sola_bus::topics::RegisteredChord>,
) {
    let Some(qh) = state.qh.clone() else { return };
    let (Some(xb), Some(river_seat)) = (state.xkb_bindings.as_ref().cloned(), state.seat.as_ref().cloned()) else {
        return;
    };

    let new_pairs: Vec<(u32, u32)> = new.iter().map(|c| (c.keysym, c.modifiers)).collect();
    let old_pairs: Vec<(u32, u32)> = state.chords.by_chord.keys().copied().collect();
    let (added, removed) = crate::registry::chord_diff(&old_pairs, &new_pairs);

    for pair in removed {
        if let Some(b) = state.chords.by_chord.remove(&pair) {
            state.chords.by_object.retain(|_, v| *v != pair);
            b.disable();
            b.destroy();
        }
    }

    for (keysym, modifiers) in added {
        let binding = xb.get_xkb_binding(&river_seat, keysym, modifiers, &qh, (keysym, modifiers));
        binding.enable();
        state.chords.by_object.insert(binding.id(), (keysym, modifiers));
        state.chords.by_chord.insert((keysym, modifiers), binding);
    }
}
```

Verify `get_xkb_binding`'s seat argument type against the generated bindings — if it takes `wl_seat` instead of `river_seat_v1`, pass `state.wl_seat`.

- [ ] **Step 3: `Dispatch<RiverXkbBindingV1, (u32, u32)>` in `binding.rs`**

```rust
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::client::AppData;
use crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::RiverXkbBindingV1;

impl Dispatch<RiverXkbBindingV1, (u32, u32)> for AppData {
    fn event(
        state: &mut Self,
        _: &RiverXkbBindingV1,
        event: <RiverXkbBindingV1 as Proxy>::Event,
        &(keysym, modifiers): &(u32, u32),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_xkb_bindings_v1::river_xkb_binding_v1::Event;
        if let Event::Pressed = event {
            state.bus.emit(sola_bus::topics::Topic::Chord(
                sola_bus::topics::ChordEvent { keysym, modifiers }
            ));
        }
    }
}
```

Add `pub mod binding;` in `client/mod.rs`.

- [ ] **Step 4: Verify + commit**

```bash
cargo check -p sola-river
git add -A
git commit -m "feat(sola-river): xkb bindings — register on shell demand, emit Chord events"
```

### Task 5.7: Main loop wires bus + wayland via calloop

**Files:**
- Modify: `crates/sola-river/src/main.rs`
- Modify: `crates/sola-river/src/client/mod.rs` (add `bus_tick`)

- [ ] **Step 1: `bus_tick` function in `client/mod.rs`**

```rust
pub fn bus_tick(state: &mut AppData) {
    state.bus.ensure_connected();
    while let Some(msg) = state.bus.try_recv() {
        let Some(topic) = sola_bus::topics::Topic::parse(&msg) else { continue };
        match topic {
            sola_bus::topics::Topic::Composition(entries) => {
                let ids: Vec<u32> = entries.into_iter().map(|e| e.window_id).collect();
                state.pending.set_composition(ids);
            }
            sola_bus::topics::Topic::Frame(f) => {
                state.pending.frame(f.window_id, f.x, f.y, f.width, f.height);
            }
            sola_bus::topics::Topic::Focus(t) => {
                state.pending.set_focus(crate::pending::FocusAction::Window(t.window_id));
            }
            sola_bus::topics::Topic::RegisteredChords(chords) => {
                crate::translator::update_registered_chords(state, chords);
            }
            sola_bus::topics::Topic::Shutdown => std::process::exit(0),
            _ => {}
        }
    }

    if (state.pending.manage_dirty || state.pending.render_dirty) && state.wm.is_some() {
        state.wm.as_ref().unwrap().manage_dirty();
    }
}
```

- [ ] **Step 2: `main.rs` with calloop**

Replace the busy-wait loop with:

```rust
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;

// ... after sup.wait_for_socket() succeeds:
unsafe { std::env::set_var("WAYLAND_DISPLAY", "wayland-0"); }

let mut bus = bus::BusClient::new();
bus.ensure_connected();
bus.subscribe();

let (conn, queue, mut data) = match client::connect(bus) {
    Ok(x) => x,
    Err(e) => { error!(%e, "wayland connect failed"); sup.shutdown(); exit(1); }
};

let mut event_loop: EventLoop<client::AppData> = EventLoop::try_new()
    .expect("calloop");
let handle = event_loop.handle();

WaylandSource::new(conn, queue).insert(handle.clone())
    .expect("wayland source insert");

handle.insert_source(
    calloop::timer::Timer::from_duration(std::time::Duration::from_millis(20)),
    |_, _, state: &mut client::AppData| {
        client::bus_tick(state);
        calloop::timer::TimeoutAction::ToDuration(std::time::Duration::from_millis(20))
    },
).expect("bus timer");

// Child-death watch every 500ms.
let (tx, rx) = std::sync::mpsc::channel::<()>();
std::thread::spawn(move || loop {
    std::thread::sleep(Duration::from_millis(500));
    if tx.send(()).is_err() { return; }
});

event_loop.run(Duration::from_millis(500), &mut data, |_state| {
    if let Ok(()) = rx.try_recv() {
        // TODO: supervisor check hooked via shared state.
    }
}).expect("event loop");

sup.shutdown();
```

If hooking the supervisor child-watch into calloop proves fiddly, a simpler approach: spawn a background thread that polls `sup.try_wait()` every 500ms and calls `process::exit(1)` on detecting exit. Main loop stays focused on wayland + bus.

- [ ] **Step 3: Verify + commit**

```bash
cargo check -p sola-river
git add -A
git commit -m "feat(sola-river): main loop wires bus and wayland via calloop"
```

---

## Phase 6: Verification

### Task 6.1: Workspace check and tests

- [ ] **Step 1: Full workspace check**

```
cargo check --workspace --all-targets
```

- [ ] **Step 2: Tests**

```
cargo test --workspace
```

### Task 6.2: Release build

- [ ] **Step 1: Build**

```
cargo make build --release
```

`sola-make`'s discovery picks up `sola-river` automatically via `crates/sola-river/src/main.rs`. `sola-compositor` is gone.

### Task 6.3: Handoff note

- [ ] **Step 1: Write `docs/specs/2026-04-16-sola-river-handoff.md`**

One page: what was implemented, what needs TTY install testing (River startup sequencing, first-window timing, chord events, mouse events, focus sequencing, decoration-off behavior for third-party apps), and the rollback command (`git worktree remove .worktrees/sola-river`).

---

## Self-review checklist

Before declaring done:

- [ ] `grep -rn SetWindowPolicy crates apps` returns nothing.
- [ ] `grep -rn "Topic::Windows\b" crates apps` returns nothing.
- [ ] `grep -rn "ShellKeyBindings" crates apps` returns nothing.
- [ ] `crates/sola-compositor/` does not exist.
- [ ] `crates/sola-river/` builds clean.
- [ ] `cargo check --workspace --all-targets` is green.
- [ ] `cargo test --workspace` is green.
- [ ] `MANAGED` in `crates/sola/src/main.rs` lists `sola-river`, not `sola-compositor`.
- [ ] Workspace `Cargo.toml` members list includes `crates/sola-river` and excludes `crates/sola-compositor`.
