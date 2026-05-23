# sola-shell → iced kit Port — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace CEF-based `sola-shell` with iced-native shell built on the new `sola-kit`, preserving every bus contract and behavior of today's four-window shell.

**Architecture:** One iced multi-window app process; four windows (menubar, launcher, switcher, menu); one shared `Shell` state struct with per-window sub-state. Bus integration via `sola_kit::app::bus_subscription()`. Theme via `sola_kit::theme::from_bus_theme`. Window visibility is composition-only (windows stay at final geometry, included/excluded from `Topic::Composition`).

**Tech Stack:** Rust 2024 edition, iced 0.14 (multi-window), sola-kit (iced), sola-bus, sola-core. Workspace-excluded crate (same as sola-monitor / sola-kit, because iced flips `wayland-sys` into dlopen mode).

**Spec:** `docs/specs/2026-05-22-sola-shell-iced-port-design.md` (read before starting any task).

**Worktree:** All work happens in this worktree (`.worktrees/sola-shell-iced-port/`). Branch `shell-iced-port`.

## Standing rules for every implementer subagent

1. **Worktree cwd.** All file paths in this plan are relative to the worktree root `/home/joshua/Workspace/Sola/.worktrees/sola-shell-iced-port/`. Subagents must cd there before doing anything.
2. **Do NOT install.** Never run `cargo make install` (or `cargo install`, or copy binaries to `/opt/sola/bin/`). Building (`cargo make build` / `cargo build`) is fine and expected. Installation is the user's call, not ours.
3. **Reference, don't reinvent.** This is a *port*, not a new feature. Read the corresponding files in `crates/sola-shell-legacy/` (after Task 1) and the iced patterns in `crates/sola-monitor/` and `crates/sola-kit/src/storybook/` before writing new code.
4. **No speculative abstractions.** Implement what the current task says. If you discover a pattern that would be useful elsewhere, leave a comment noting it; don't refactor to extract it now.
5. **No legacy code in the new crate.** The new `crates/sola-shell/` has zero web/, zero JS, no CEF, no `sola-app`, no `sola-kit-legacy` deps. The legacy CEF shell is preserved as a separate crate.
6. **Test what's testable.** Pure-data modules (filter, MenuCache, zoning math) get unit tests. UI behavior is verified by `cargo make build` + visual smoke at the end. No mock harness for iced widgets — out of scope per spec.
7. **Commit per task.** Each task ends with a single commit on branch `shell-iced-port`. Use the existing commit message style (look at `git log --oneline -20`).
8. **Don't merge to master.** No PRs, no merge commits. The branch stays in the worktree until the user explicitly merges.

---

## Task 1: Rename `sola-shell` → `sola-shell-legacy`

**Files:**
- Rename: `crates/sola-shell/` → `crates/sola-shell-legacy/`
- Modify: `crates/sola-shell-legacy/Cargo.toml` ([package].name + [[bin]].name → `sola-shell-legacy`)
- Modify: `crates/sola-core/src/applications.rs` (add builtin entry for `sola-shell-legacy`)
- Verify-only: workspace root `Cargo.toml` (no changes needed — legacy crate stays in workspace)

- [ ] **Step 1:** `git mv crates/sola-shell crates/sola-shell-legacy`

- [ ] **Step 2:** Edit `crates/sola-shell-legacy/Cargo.toml`:
  - `[package].name = "sola-shell-legacy"`
  - `[[bin]].name = "sola-shell-legacy"` (path stays `src/main.rs`)

- [ ] **Step 3:** Update any `path = "../sola-shell"` references in other crates (likely none — search with `grep -r 'path = "../sola-shell"' crates/`). If found, point to `sola-shell-legacy`.

- [ ] **Step 4:** In `crates/sola-core/src/applications.rs`, add a builtin entry just after the existing Kit (Legacy) entry — copy the pattern:

```rust
        Application {
            app_id: "sola-shell-legacy".into(),
            label: "Shell (Legacy)".into(),
            command: "/opt/sola/bin/sola-shell-legacy".into(),
            icon: "lucide/layout".into(),
        },
```

- [ ] **Step 5:** Build to verify nothing broke: `cargo make build`. Expected: clean.

- [ ] **Step 6:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
refactor(shell): rename current sola-shell crate to sola-shell-legacy

Prepares for the iced-based rewrite to take the sola-shell name. The
CEF/Remix v3 shell remains buildable and installable as sola-shell-legacy
for side-by-side comparison during the port. No code changes other than
package + binary rename + new launcher builtin entry.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Create new `sola-shell` crate skeleton (iced)

**Files:**
- Create: `crates/sola-shell/Cargo.toml`
- Create: `crates/sola-shell/build.rs` (copy from `crates/sola-monitor/build.rs` verbatim)
- Create: `crates/sola-shell/src/main.rs`
- Create: `crates/sola-shell/src/app.rs` (Shell struct skeleton)
- Modify: workspace `Cargo.toml` (add `crates/sola-shell` to `exclude`)

- [ ] **Step 1:** Read `crates/sola-monitor/Cargo.toml` and `crates/sola-monitor/src/main.rs` to absorb the iced-app pattern.

- [ ] **Step 2:** Create `crates/sola-shell/Cargo.toml`:

```toml
[package]
name = "sola-shell"
version = "0.1.0"
edition = "2024"

# Iced-native sola desktop shell. Successor to the CEF/Remix v3 shell
# (now `sola-shell-legacy`). Four-window shell: menubar, launcher,
# switcher, menu. Workspace-excluded because iced's transitive
# smithay-clipboard flips wayland-sys into dlopen mode which would
# unify across the workspace and break sola-river's direct wayland
# linkage.

[[bin]]
name = "sola-shell"
path = "src/main.rs"

[dependencies]
iced = { version = "0.14", default-features = false, features = ["wgpu", "tokio", "wayland", "svg"] }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
sola-kit = { path = "../sola-kit" }
sola-assets = { path = "../sola-assets" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
tokio = { version = "1", features = ["time"] }
```

- [ ] **Step 3:** Copy build.rs verbatim: `cp crates/sola-monitor/build.rs crates/sola-shell/build.rs`.

- [ ] **Step 4:** Add `"crates/sola-shell"` to the workspace `exclude` list in the root `Cargo.toml`. Insert it before `"crates/sola-shell-legacy"` ordering doesn't matter, but stay near the other shell entry if one is there.

- [ ] **Step 5:** Create `crates/sola-shell/src/main.rs`:

```rust
//! sola-shell — iced-native desktop shell. Replaces the CEF/Remix v3
//! shell (preserved as `sola-shell-legacy`). Four windows on one
//! iced multi-window application.

use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, startup, window_settings};
use sola_kit::fonts::{self, F_NORMAL};

mod app;

const APP_ID: &str = "sola-shell";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Shell", [("quit", "Quit Shell", KeyCode::Q.meta())])
        .install();

    let mut iced_app = iced::application(app::Shell::default, app::Shell::update, app::Shell::view)
        .title(app::Shell::title)
        .subscription(app::Shell::subscription)
        .theme(app::Shell::theme)
        .default_font(F_NORMAL)
        .window(window_settings(APP_ID));
    for bytes in fonts::load_all() {
        iced_app = iced_app.font(bytes);
    }
    iced_app.run()
}
```

- [ ] **Step 6:** Create `crates/sola-shell/src/app.rs` with a minimal placeholder `Shell` so the binary compiles and a single window opens:

```rust
//! Shell — central state for the iced shell. This is the skeleton;
//! per-window state and bus dispatch land in subsequent tasks.

use std::sync::Arc;

use iced::widget::{container, text};
use iced::{Element, Length, Subscription};

use sola_kit::theme;

#[derive(Clone, Debug)]
pub enum Msg {
    Bus(Arc<sola_bus::Message>),
    Noop,
}

pub struct Shell {
    theme: iced::Theme,
}

impl Shell {
    pub fn default() -> Self {
        Self { theme: theme::default_theme() }
    }

    pub fn title(&self) -> String {
        "sola-shell".to_string()
    }

    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        sola_kit::app::bus_subscription().map(Msg::Bus)
    }

    pub fn update(&mut self, _msg: Msg) {}

    pub fn view(&self) -> Element<'_, Msg> {
        container(text("sola-shell (iced) — skeleton"))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }
}
```

- [ ] **Step 7:** Build the new crate: `cargo make build sola-shell`. Expected: clean.

- [ ] **Step 8:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): scaffold new iced-based sola-shell crate

Empty single-window skeleton — builds clean, opens one placeholder
window when launched. Subsequent tasks port the four-window shell on
top of this base. Workspace-excluded for the same wayland-sys/dlopen
reason as sola-kit and sola-monitor.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Port framework-agnostic modules

Carry over pure-logic modules from `sola-shell-legacy/` into the new crate. These are framework-agnostic — no iced, no CEF, no kit-specific code. Mostly copy with light editing where bus types are referenced directly.

**Files:**
- Create: `crates/sola-shell/src/keys.rs` (port from `crates/sola-shell-legacy/src/keys.rs`)
- Create: `crates/sola-shell/src/zoning.rs` (port from `crates/sola-shell-legacy/src/zoning.rs`)
- Create: `crates/sola-shell/src/menu/mod.rs` + `state.rs` (port `MenuCache`, `synthesized_menu` only — no window logic yet)
- Create: `crates/sola-shell/src/launcher/state.rs` (port filter logic only — no window logic)
- Create: `crates/sola-shell/src/switcher/state.rs` (port `SwitcherState` only — no window logic)
- Modify: `crates/sola-shell/src/app.rs` (declare new modules)
- Create: `crates/sola-shell/src/menu/mod.rs` (initially: `pub mod state;` only)

- [ ] **Step 1:** Read each legacy source before porting:
  - `crates/sola-shell-legacy/src/keys.rs`
  - `crates/sola-shell-legacy/src/zoning.rs`
  - `crates/sola-shell-legacy/src/menu/state.rs`
  - `crates/sola-shell-legacy/src/launcher/state.rs`
  - `crates/sola-shell-legacy/src/switcher/state.rs`

- [ ] **Step 2:** Port `keys.rs`. Strip any references to `sola_kit::*` (the legacy kit), CEF, or window-specific iced concepts. Keep KeyChord types, keymap building, chord dispatch *logic*. Anywhere the legacy version calls back into `App` methods, replace with TODO comments noting "wired in Task 7 (chord dispatch into Shell::update)". Keep the structure intact so the wiring task is mechanical.

- [ ] **Step 3:** Port `zoning.rs`. The legacy version has `ZoningState` with methods that take `&mut self` and return data — keep that shape. Anywhere it expects to call `ctx.emit(...)` or similar, change to return values the caller can emit. The shell's update fn will emit on its behalf.

- [ ] **Step 4:** Port `menu/state.rs`. `MenuCache` and `synthesized_menu` — pure data, copy directly.

- [ ] **Step 5:** Port `launcher/state.rs`. `LauncherState { active, prior_focus, query, filtered_ids, selected }` + `apply_query()`. Pure data. Carry over the case-insensitive substring filter test if one exists; add one if not.

- [ ] **Step 6:** Port `switcher/state.rs`. `SwitcherState { active, apps, selected }` + `select_next`/`select_prev`. Pure data.

- [ ] **Step 7:** Create `crates/sola-shell/src/menu/mod.rs` with `pub mod state;` only (window logic lands in Task 7).

- [ ] **Step 8:** Add module declarations to `src/app.rs`:

```rust
pub mod keys;
pub mod launcher;  // create launcher/mod.rs with `pub mod state;`
pub mod menu;
pub mod switcher;  // create switcher/mod.rs with `pub mod state;`
pub mod zoning;
```

Plus the corresponding tiny `mod.rs` files for launcher and switcher.

- [ ] **Step 9:** Add unit tests for the pure-data modules. At minimum:
  - `LauncherState::apply_query` filters correctly on substring, case-insensitive, preserves order.
  - `MenuCache::synthesized_menu` produces the expected single-item "Quit \<App\>" structure.
  - One test per zone in `zoning::ZoningState::geometry` confirming the (x%, y%, w%, h%) for each zone matches the legacy version.

- [ ] **Step 10:** Build and run tests: `cargo make build sola-shell && cargo test --manifest-path crates/sola-shell/Cargo.toml`. Expected: clean.

- [ ] **Step 11:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): port framework-agnostic modules from sola-shell-legacy

Carry over pure-data and pure-logic modules — KeyChord dispatch types,
ZoningState math, MenuCache + synthesized menus, launcher filter,
switcher state. No iced wiring yet; the shell's update fn glues them
together in subsequent tasks. Unit tests cover filter, synthesized
menu, and zone geometry to lock in parity with the legacy shell.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Bus integration + theme + Shell state shape

Build out the `Shell` struct with all the shared state from the spec, and wire the bus dispatch skeleton so subsequent tasks plug in handlers.

**Files:**
- Modify: `crates/sola-shell/src/app.rs` (expand Shell struct, update fn, theme handling)
- Create: `crates/sola-shell/src/bus.rs` (Topic dispatch table)

- [ ] **Step 1:** Read `crates/sola-shell-legacy/src/app.rs:46-78` for the `ShellApp` struct shape (focus tracking, MRU, window registry, app catalog, menu cache, focus-hover timer). Carry the shape over to `Shell`.

- [ ] **Step 2:** Expand `crates/sola-shell/src/app.rs::Shell` to hold:
```rust
pub struct Shell {
    pub theme: iced::Theme,

    // Focus
    pub focused_app_id: Option<String>,
    pub focused_window_id: Option<u32>,

    // MRU
    pub mru_apps: Vec<String>,
    pub mru_window_by_app: HashMap<String, u32>,

    // Window registry (from Topic::Windows)
    pub known_windows: Vec<sola_bus::topics::Window>,
    pub window_id_by_key: HashMap<(String, String), u32>,

    // Application catalog
    pub applications: sola_core::applications::ApplicationsConfig,

    // Menu cache (built up from Topic::SetAppMenu)
    pub menus: menu::state::MenuCache,

    // Output geometry (from Topic::OutputGeometry)
    pub output_size: Option<(u32, u32)>,

    // Per-window state
    pub menu_open: bool,
    pub menu_anchor_x: f32,
    pub switcher: switcher::state::SwitcherState,
    pub launcher: launcher::state::LauncherState,
    pub zoning: zoning::ZoningState,

    // Focus-hover generation counter (replaces legacy AppRuntimeHandle)
    pub pending_focus_generation: u64,
}
```

- [ ] **Step 3:** Create `crates/sola-shell/src/bus.rs`:

```rust
//! Bus topic dispatch — parses an incoming `sola_bus::Message` and
//! routes to the right `Shell` method. Each topic handler lives here;
//! window-specific reactions are dispatched out from these methods.

use sola_bus::topics::{Topic, TopicKind};

use crate::app::{Msg, Shell};

impl Shell {
    pub fn handle_bus(&mut self, message: &sola_bus::Message) {
        let Some(topic) = Topic::parse(message) else { return; };
        match topic {
            Topic::Theme(t) => self.on_theme(t),
            Topic::OutputGeometry(g) => self.on_output_geometry(g),
            Topic::Windows(w) => self.on_windows(w),
            Topic::SetAppMenu(m) => self.on_set_app_menu(m),
            Topic::Application(a) => self.on_application(a),
            Topic::Chord(c) => self.on_chord(c),
            Topic::ChordReleased(c) => self.on_chord_released(c),
            Topic::MouseEntered(e) => self.on_mouse_entered(e),
            Topic::MouseClicked(e) => self.on_mouse_clicked(e),
            Topic::MouseLeft(e) => self.on_mouse_left(e),
            Topic::LaunchResult(r) => self.on_launch_result(r),
            Topic::UserAppExited(e) => self.on_user_app_exited(e),
            Topic::Zones(z) => self.on_zones(z),
            _ => {}
        }
    }

    fn on_theme(&mut self, t: sola_core::theme::Theme) {
        self.theme = sola_kit::theme::from_bus_theme(&t);
    }

    fn on_output_geometry(&mut self, g: sola_bus::topics::OutputGeometry) {
        self.output_size = Some((g.width, g.height));
        // TODO Task 5+: emit Topic::Frame to position windows
    }

    // Stubs for the rest — each task wires them up as it lands.
    fn on_windows(&mut self, _w: sola_bus::topics::WindowsPayload) {}
    fn on_set_app_menu(&mut self, _m: sola_bus::topics::AppMenuPayload) {}
    fn on_application(&mut self, _a: sola_bus::topics::Application) {}
    fn on_chord(&mut self, _c: sola_bus::topics::ChordPayload) {}
    fn on_chord_released(&mut self, _c: sola_bus::topics::ChordPayload) {}
    fn on_mouse_entered(&mut self, _e: sola_bus::topics::MouseEvent) {}
    fn on_mouse_clicked(&mut self, _e: sola_bus::topics::MouseEvent) {}
    fn on_mouse_left(&mut self, _e: sola_bus::topics::MouseEvent) {}
    fn on_launch_result(&mut self, _r: sola_bus::topics::LaunchResult) {}
    fn on_user_app_exited(&mut self, _e: sola_bus::topics::UserAppExited) {}
    fn on_zones(&mut self, _z: sola_bus::topics::ZonesPayload) {}
}
```

(Adjust topic-payload type names to match what `sola_bus::topics` actually exports — read the source.)

- [ ] **Step 4:** Wire `Msg::Bus(arc)` in `Shell::update` to call `self.handle_bus(&arc)`.

- [ ] **Step 5:** Seed `Topic::Theme` at startup. In `Shell::default()`, emit the kit default theme so the bus has a sticky value:
```rust
let theme = theme::default_theme();
let bus_theme = sola_kit::theme::to_bus_theme(&theme);  // add helper if missing
let _ = sola_kit::app::bus().lock().unwrap().emit(Topic::Theme(bus_theme));
```
(If `to_bus_theme` doesn't exist in sola-kit, add it as the inverse of `from_bus_theme`.)

- [ ] **Step 6:** Build: `cargo make build sola-shell`. Expected: clean.

- [ ] **Step 7:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): wire bus dispatch + Shell state + theme seeding

Shell struct now holds the full shared-state shape from the legacy
shell — focus, MRU, window registry, app catalog, menu cache, per-
window sub-state. bus.rs dispatches Topic::parse into method stubs;
each subsequent task fills in handlers as its window comes online.
Topic::Theme is seeded at startup so other kit apps have a value to
replay against on connect.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Menubar window

Multi-window iced enters here. Switch the iced builder from single-window to multi-window, create the menubar window, port its layout and state.

**Files:**
- Modify: `crates/sola-shell/src/main.rs` (multi-window iced builder)
- Modify: `crates/sola-shell/src/app.rs` (per-window view dispatch on `window::Id`)
- Create: `crates/sola-shell/src/menubar/mod.rs` (window state + open)
- Create: `crates/sola-shell/src/menubar/view.rs` (iced view fn)
- Create: `crates/sola-shell/src/components/clock.rs` (clock widget)
- Create: `crates/sola-shell/src/components/toast.rs` (toast widget)

- [ ] **Step 1:** Read `crates/sola-shell-legacy/src/menubar/mod.rs` and `crates/sola-shell-legacy/web/components/menubar/menubar.{tsx,css}` to understand the layout: left cluster (logo button → app name → menu labels from index 1), `Length::Fill` spacer, right cluster (toast + clock).

- [ ] **Step 2:** Read iced 0.14 docs (via context7 if needed) for multi-window via `iced::application` + `iced::window::open`. The `view` signature becomes `fn view(&self, window: window::Id) -> Element<'_, Msg>` so the same Shell renders different content per window.

- [ ] **Step 3:** Restructure `main.rs` to open the menubar window via `iced::window::open` after the app starts. Track its `window::Id` in `Shell` (e.g. `menubar_window_id: Option<window::Id>`). Pattern: use `Shell::default()` → `(Self, Command)` shape if iced 0.14 supports it (it does via the older `iced::application::application(...)` API), and the Command opens the menubar window.

- [ ] **Step 4:** Create `crates/sola-shell/src/menubar/mod.rs`:
  - `MenubarState { toast: Option<Toast>, toast_generation: u64, clock_now: chrono::DateTime<chrono::Local>, label_positions: Vec<f32> }`
  - Methods to push a toast (`push_toast(msg)` → bumps generation, sets toast).
  - `WINDOW_HEIGHT: u32 = 28` constant.

- [ ] **Step 5:** Create `crates/sola-shell/src/menubar/view.rs::view(shell: &Shell) -> Element<Msg>`:
  - `row![system_menu_button, app_title, menu_labels, Length::Fill, toast_overlay, clock]`.
  - `system_menu_button`: `button(text("●"))` for now — replaced by icon in Task 6.
  - `app_title`: bold text of focused app's display name (look up via `applications` from `focused_app_id`).
  - `menu_labels`: iterate over the focused app's `AppMenuPayload.menus[1..]` and render each as a clickable label with `mouse_area::on_press` → `Msg::OpenMenu { index }` and `mouse_area::on_enter` → `Msg::HoverMenu { index }`.
  - `clock`: format `shell.menubar.clock_now` as `HH:MM Wed YYYY-MM-DD`.

- [ ] **Step 6:** Add a clock subscription: in `Shell::subscription`, batch `bus_subscription()` with `iced::time::every(Duration::from_secs(10)).map(|_| Msg::ClockTick)`. `ClockTick` updates `menubar.clock_now`.

- [ ] **Step 7:** Toast: when `on_launch_result(Err(..))` or `on_user_app_exited` fires, call `menubar.push_toast(...)` and `Command::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| Msg::ToastExpire(gen))`. `ToastExpire(gen)` clears toast only if `gen == menubar.toast_generation`.

- [ ] **Step 8:** Wire `on_set_app_menu` to update `Shell.menus` (delegate to `MenuCache::set`).

- [ ] **Step 9:** Wire `on_windows` to maintain the `known_windows` registry + derive focus changes (the legacy version does this — port the logic).

- [ ] **Step 10:** Build: `cargo make build sola-shell`. Expected: clean. (Visual verification deferred to Task 11 — running shell needs all four windows.)

- [ ] **Step 11:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): menubar window — layout, clock, toast, app-menu labels

First of four windows lands. Multi-window iced builder set up with
per-window view dispatch on window::Id. Menubar renders left cluster
(system-menu button placeholder, app title, menu labels) and right
cluster (toast overlay, clock). Clock ticks every 10s via iced::time
subscription; toast auto-hides after 5s via generation-cancel pattern.
Topic::SetAppMenu populates the menu cache; Topic::Windows drives
focus tracking. Visual smoke deferred until all windows land.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Icon primitive in sola-kit

Replace the placeholder system-menu button with a real SVG icon. Pull the resolution into `sola-kit` so other consumers (launcher rows, switcher cards) reuse it.

**Files:**
- Create: `crates/sola-kit/src/components/icon.rs`
- Modify: `crates/sola-kit/src/components/mod.rs` (export `icon`)
- Modify: `crates/sola-shell/src/menubar/view.rs` (use `icon("sola/pillars")`)
- Verify: `crates/sola-assets` has lucide icons available (look at how legacy shell consumed them)

- [ ] **Step 1:** Read `crates/sola-assets/src/lib.rs` to understand how assets are exposed. Determine whether SVG bytes can be looked up by `lucide/<name>` or `sola/<name>` paths. If lookup doesn't exist, add it.

- [ ] **Step 2:** Create `crates/sola-kit/src/components/icon.rs`:

```rust
//! Icon — resolves a name like "lucide/settings" or "sola/pillars" to
//! an iced Svg widget themed with the current text color via
//! `iced::widget::svg::Style { color: Some(palette.text), ... }`.
//!
//! Used by sola-shell for system-menu logo, launcher rows, switcher
//! cards. Static lookup against sola-assets; no fs I/O at render
//! time.

use iced::widget::svg;
use iced::{Element, Length};

pub fn icon<'a, Msg: 'a>(name: &str, size: u16) -> Element<'a, Msg> {
    let bytes = sola_assets::lookup_svg(name).unwrap_or_default();
    let handle = svg::Handle::from_memory(bytes.to_vec());
    svg(handle)
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .style(|theme: &iced::Theme, _status| svg::Style {
            color: Some(theme.extended_palette().background.base.text),
        })
        .into()
}
```

(If `sola_assets::lookup_svg` doesn't exist, add it. If the asset format differs, adapt — the goal is "name → bytes".)

- [ ] **Step 3:** Export `icon` from `sola_kit::components`.

- [ ] **Step 4:** Replace the menubar's placeholder button with `icon("sola/pillars", 18)` (or `lucide/menu` if pillars isn't available as an asset). Confirm the SVG renders at 18×18 with the theme text color.

- [ ] **Step 5:** Build: `cargo make build sola-kit && cargo make build sola-shell`. Expected: clean.

- [ ] **Step 6:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(kit): icon component — name → themed SVG

sola_kit::components::icon resolves "lucide/<name>" or "sola/<name>"
against sola-assets and returns an iced Svg widget tinted with the
active theme's text color. First consumer: sola-shell menubar system-
menu button. Launcher rows and switcher cards consume it in later
tasks.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Menu window + anchor positioning

The menu dropdown window. Hardest single visual task because it needs cross-window coordinate translation (menubar label position → menu dropdown left edge).

**Files:**
- Modify: `crates/sola-shell/src/main.rs` (open menu window)
- Modify: `crates/sola-shell/src/app.rs` (track menu_window_id, dispatch view by id)
- Create: `crates/sola-shell/src/menu/view.rs`
- Modify: `crates/sola-shell/src/menubar/view.rs` (report label X positions via responsive widget, store in `Shell.menubar.label_positions`)
- Modify: `crates/sola-shell/src/bus.rs` (wire mouse_clicked dismiss)

- [ ] **Step 1:** Read `crates/sola-shell-legacy/src/menu/mod.rs` and `crates/sola-shell-legacy/web/components/menu/menu.{tsx,css}` for the dropdown layout: item rows (label left, shortcut right), disabled muted, dividers.

- [ ] **Step 2:** Open the menu window at startup. Size is full-overlay (output_width × (output_height - 28)). Initially hidden via composition (don't emit it in composition until `menu_open == true`). Position (0, 28).

- [ ] **Step 3:** Implement anchor X tracking. In `menubar/view.rs`, when laying out menu labels, wrap each label in `iced::widget::container` with an `Id` and use `iced::widget::responsive` to capture its laid-out X. Post a `Msg::MenuLabelPosition { index, x }` for each. Store in `shell.menubar.label_positions` (Vec indexed by label index).

  *If `responsive` doesn't give us position data:* fall back to font-metric measurement using `iced::advanced::text::Paragraph` (or `cosmic_text` directly if iced re-exports). Compute cumulative widths from labels + paddings.

- [ ] **Step 4:** Create `crates/sola-shell/src/menu/view.rs::view(shell: &Shell) -> Element<Msg>`:
  - Full-overlay transparent backdrop.
  - Stack the dropdown card at `padding::Padding { top: 0, left: shell.menu_anchor_x, ... }`.
  - Card body: `column` of menu items rendered from `shell.menus.get(focused_app_id).menus[open_menu_index]`.
  - Each item: action row (label + shortcut), disabled style, divider as `Rule::horizontal(1)`.

- [ ] **Step 5:** Implement `OpenMenu { source, index, anchor_x }` and `CloseMenu` messages. `OpenMenu` sets `menu_open = true`, sets `menu_anchor_x`, emits a `Topic::Composition` that includes the menu window. `CloseMenu` sets `menu_open = false`, emits composition without it.

- [ ] **Step 6:** Implement `MenuAction { app_id, action_id }`:
  - If `app_id == "sola-shell"` and `action_id == "exit"`: emit `Topic::Shutdown`.
  - If `action_id == "_close"`: emit `Topic::CloseApp` for focused app.
  - Otherwise: emit `Topic::MenuAction`.
  - Always close menu after.

- [ ] **Step 7:** Dismiss handlers:
  - Escape chord: `CloseMenu`.
  - `MouseClicked` on non-shell window (via Topic::MouseClicked): `CloseMenu`.
  - Focus change (in `on_windows` handler): if `menu_open`, `CloseMenu`.
  - Backdrop click (menu window's own background): `CloseMenu`.

- [ ] **Step 8:** Hover-sweep behavior: in `HoverMenu { index }`, if `menu_open` and `current_open_index != index`, close current + open new. Otherwise no-op.

- [ ] **Step 9:** Build: `cargo make build sola-shell`. Expected: clean.

- [ ] **Step 10:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): menu window — dropdown, anchor positioning, dismiss

Menu window renders the focused app's menu items at a label-anchored
X coordinate computed from the menubar's laid-out label positions
(captured via iced::widget::responsive; falls back to font-metric
math if needed). MenuAction routes to Shutdown/CloseApp/MenuAction
topics per the legacy contract. Dismiss handlers cover Escape,
outside-click via Topic::MouseClicked, focus change, and backdrop.
Hover-sweep guard preserved (reopen only if a different menu is up).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Launcher window

**Files:**
- Modify: `crates/sola-shell/src/main.rs` (open launcher window)
- Modify: `crates/sola-shell/src/app.rs` (dispatch launcher view, launcher messages)
- Create: `crates/sola-shell/src/launcher/view.rs`
- Modify: `crates/sola-shell/src/launcher/mod.rs` (mod.rs wires open/close, render glue)

- [ ] **Step 1:** Read `crates/sola-shell-legacy/src/launcher/{mod.rs,state.rs}` and `crates/sola-shell-legacy/web/components/launcher/launcher.{tsx,css}` for behavior reference.

- [ ] **Step 2:** Open launcher window at startup. Size full-overlay below menubar. Hidden via composition until activated.

- [ ] **Step 3:** Create `crates/sola-shell/src/launcher/view.rs::view(shell: &Shell)`:
  - Full-overlay transparent backdrop with `mouse_area::on_press(Msg::CloseLauncher)` (outside-click).
  - Centered card via `container::center_x()` + padding-top 33% of height.
  - Card body:
    - `text_input("", &launcher.query).on_input(Msg::LauncherQuery).id(text_input::Id::new("launcher-query"))`.
    - `Rule::horizontal(1)`.
    - `scrollable(column(rows))` where rows iterate over `launcher.filtered_ids`. Each row is a `button(row![icon, text(label)])` styled differently if selected. Empty state: text "No matching applications.".

- [ ] **Step 4:** On `OpenLauncher`: snapshot `prior_focus`, set `active = true`, emit `Topic::Composition` including launcher window, emit `Topic::Focus { window_id: launcher_window }`, focus the text input via `text_input::focus(Id)`.

- [ ] **Step 5:** On `CloseLauncher`: `active = false`, emit composition without launcher, emit `Topic::Focus { window_id: prior_focus }`.

- [ ] **Step 6:** Keyboard:
  - `LauncherQuery(text)` → `launcher.apply_query(text)`.
  - `LauncherNav { dir }` → `launcher.selected ±= 1` (clamped).
  - `Launch` (Enter or click): emit `Topic::LaunchApp { app_id, command }` from `applications`, then `CloseLauncher`.

- [ ] **Step 7:** Chord wiring (handled in `on_chord` later in Task 10): Meta+Space toggles launcher; Escape closes; arrows nav when active; Enter launches when active.

- [ ] **Step 8:** Build: `cargo make build sola-shell`. Expected: clean.

- [ ] **Step 9:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): launcher window — search input, filter, launch

Launcher renders centered search card over a transparent backdrop.
Substring filter on application labels; arrow-key navigation; Enter
launches via Topic::LaunchApp; Escape closes; outside-click dismisses
via backdrop mouse_area. Focus routing: Topic::Focus snaps keyboard
to the launcher on open and restores prior focus on close.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Switcher window

**Files:**
- Modify: `crates/sola-shell/src/main.rs` (open switcher window)
- Modify: `crates/sola-shell/src/app.rs` (dispatch switcher view, switcher messages)
- Create: `crates/sola-shell/src/switcher/view.rs`
- Create: `crates/sola-shell/src/components/switcher_card.rs`
- Modify: `crates/sola-shell/src/switcher/mod.rs` (open/close glue)

- [ ] **Step 1:** Read `crates/sola-shell-legacy/src/switcher/{mod.rs,state.rs}` and `crates/sola-shell-legacy/web/components/switcher/switcher{,-card}.{tsx,css}`.

- [ ] **Step 2:** Open switcher window at startup. Hidden via composition until `switcher.active`.

- [ ] **Step 3:** Create `crates/sola-shell/src/switcher/view.rs::view(shell: &Shell)`:
  - Full-overlay transparent.
  - Card auto-centered (`container::center_x().center_y()`).
  - Card body: `row` of `switcher_card(app)` per app in `switcher.apps`.
  - Each card: `column![icon(app.icon, 52), text(app.label)]`, with highlighted background if `index == switcher.selected`.

- [ ] **Step 4:** `select_next` / `select_prev` already in `SwitcherState`. Wire them to `Msg::SwitcherNav { dir }`.

- [ ] **Step 5:** `Msg::SwitcherHover { index }` (mouse_area::on_enter on each card) sets `switcher.selected = index`.

- [ ] **Step 6:** `Msg::SwitcherConfirm` (Super_L release): emit `Topic::Focus { window_id }` for the MRU window of the selected app, then deactivate.

- [ ] **Step 7:** `Msg::SwitcherCancel` (Escape): just deactivate, no focus change.

- [ ] **Step 8:** Build: `cargo make build sola-shell`. Expected: clean.

- [ ] **Step 9:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): switcher window — alt-tab equivalent

Switcher renders a centered row of app cards over a transparent
overlay. Meta+Tab/Left/Right navigates; Super_L release confirms by
emitting Topic::Focus to the MRU window of the selected app;
Escape cancels. Mouse hover selects without confirming.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Chord wiring + remaining bus topics + composition

Finish wiring the bus contract — chord dispatch, composition emission, focus routing, zoning chord handling. After this task, the shell is functionally complete.

**Files:**
- Modify: `crates/sola-shell/src/bus.rs` (fill in `on_chord`, `on_chord_released`, `on_windows`, `on_application`, `on_zones`)
- Modify: `crates/sola-shell/src/app.rs` (compose composition stack, emit `Topic::Composition`, emit `Topic::Frame` on geometry changes, emit `Topic::RegisteredChords` on overlay state changes)
- Modify: `crates/sola-shell/src/keys.rs` (final wiring: chord → Msg)

- [ ] **Step 1:** Read `crates/sola-shell-legacy/src/app.rs` for:
  - Composition stack ordering (`crate_composition`, the function that builds the bottom-to-top stack — find it).
  - `RegisteredChords` emission logic (lines around 501-532).
  - Chord dispatch (look for `on_chord` or the legacy equivalent).
  - Focus update logic (`set_focus`, focused_app derivation).

- [ ] **Step 2:** Port composition emission. Whenever any of `known_windows`, `mru_apps`, `menu_open`, `switcher.active`, `launcher.active` changes, rebuild the composition stack and emit `Topic::Composition`.

- [ ] **Step 3:** Port `Topic::RegisteredChords` emission. Base set + `Escape` only when an overlay is active. Re-emit on every overlay-state transition.

- [ ] **Step 4:** Port chord dispatch in `on_chord`:
  - Meta+Space: toggle launcher.
  - Meta+Tab/Right: switcher_next + activate switcher if inactive.
  - Meta+Left: switcher_prev.
  - Escape: close whichever overlay is up.
  - Meta+Numpad{0..9}: zoning snap (delegate to `zoning.handle_key`, emit resulting `Topic::Frame`, take `take_zones_update()` and emit `Topic::Zones` if dirty).
  - Other chords: forward to the focused app's MenuAction shortcut lookup via `MenuCache::shortcut_to_action`.

- [ ] **Step 5:** Port `on_chord_released`:
  - Super_L release while switcher active: confirm selection.

- [ ] **Step 6:** Port `on_windows`:
  - Update `known_windows` and `window_id_by_key`.
  - Derive focus changes: if focused window disappeared, pick a new one (MRU order).
  - Update `switcher.apps` from the window list when switcher is active.
  - Apply config zone for any newly-appearing window of an app with a saved zone.

- [ ] **Step 7:** Port `on_zones` (sticky replay from sola-session): seed `zoning.app_zone_config`.

- [ ] **Step 8:** Port `on_application` (sticky replay from sola-session): append to `applications`, re-filter launcher if active.

- [ ] **Step 9:** Port `on_user_app_exited` and `on_launch_result` to push toasts to menubar (already wired in Task 5; this task verifies they fire).

- [ ] **Step 10:** Emit `Topic::Frame` for each window on every `Topic::OutputGeometry` update (to position all four windows correctly when output size changes).

- [ ] **Step 11:** Build: `cargo make build sola-shell`. Expected: clean.

- [ ] **Step 12:** Commit:
```
git add -A
git commit -m "$(cat <<'EOF'
feat(shell): chord dispatch, composition emission, zone handling

Final wiring task. Chord handler routes Meta+Space / Meta+Tab /
Escape / Meta+Numpad to launcher / switcher / dismiss / zoning,
plus app-menu shortcut lookup for arbitrary chords. Composition
stack rebuilt and re-emitted on every overlay or window-registry
change. Topic::RegisteredChords re-emitted on overlay transitions
to manage the Escape grab dynamically. Topic::Frame emitted per
window on OutputGeometry to handle output size changes. Shell is
functionally complete; smoke test is the next task.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Smoke test + side-by-side comparison (USER step)

This task is the user's, not a subagent's. The subagent harness ends after Task 10. Document what to verify so the user can do the manual A/B.

**Files:** none changed. This is a runbook task.

- [ ] **Step 1:** Build both shells: `cargo make build sola-shell sola-shell-legacy`.

- [ ] **Step 2:** Install both (user does this explicitly): `cargo make install sola-shell sola-shell-legacy`.

- [ ] **Step 3:** From a TTY, launch sola normally. The process manager runs `/opt/sola/bin/sola-shell` (the new iced one).

- [ ] **Step 4:** Verify, in order:
  - Menubar appears at top, shows logo, clock updates after 10s.
  - Launch an app from any source — menubar shows app title + menu labels for focused app.
  - Meta+Space opens launcher; typing filters; arrow keys nav; Enter launches; Escape closes.
  - Meta+Tab opens switcher; arrows cycle; Super_L release confirms; Escape cancels.
  - Click a menubar label opens menu dropdown anchored under that label.
  - Click outside menu dismisses it.
  - Menu action routes to focused app correctly (test with `sola-monitor` "Quit" action).
  - Meta+Numpad zone snap works for sola-* apps.
  - Toast appears on a failed launch (try launching a bogus path).
  - Theme reload works (run `sola-kit` storybook, change theme, shell updates).

- [ ] **Step 5:** Compare with legacy. From a fresh sola, manually replace `/opt/sola/bin/sola-shell` with the legacy build temporarily (`cp /opt/sola/bin/sola-shell-legacy /opt/sola/bin/sola-shell`) and verify visual parity. (Optional — only if anything looks off.)

- [ ] **Step 6:** Report any divergences as discrete tasks against this branch; do not consider the port "done" until visual + behavioral parity is achieved.

---

## Done criteria

The port is complete when:

1. All ten implementer tasks above are committed on branch `shell-iced-port`.
2. Task 11 manual verification passes for every checklist item.
3. The user explicitly merges (or instructs Claude to merge) `shell-iced-port` → `master`.
4. `cargo make install sola-shell` is the only step needed to flip the production shell to iced.

`sola-shell-legacy` stays in tree as a fallback for one release; retirement is a separate small task at the user's call.
