# sola-terminal → Iced (sola-kit) Port — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-host `sola-terminal` on the Iced/`sola-kit` stack — replacing the GTK4+WebKit6 WebView + xterm.js UI with a native Iced app that drives the terminal grid through `alacritty_terminal`, while keeping the existing PTY + tmux backend intact.

**Architecture:** A single `iced::application` (one window, like `sola-monitor`). The proven backend modules (`pty.rs`, `tmux.rs`) are reused almost verbatim. Each tab owns an `alacritty_terminal::Term` behind an `Arc<FairMutex<…>>`; the PTY reader thread feeds raw bytes through a `vte::ansi::Processor` into that `Term`. Iced renders the **active** tab's grid via a `canvas` widget (cribbing `iced_term`'s `view.rs`), and encodes keyboard/mouse input back to bytes (cribbing `iced_term`'s `bindings`) which are written to the tab's PTY. Tab state + sidebar config stay on the persistent bus exactly as today (`TerminalSession`, `TerminalConfig`); bus I/O moves from `sola-app`'s `AppCtx` bridge to `sola-kit`'s `BusSetup` + `bus_subscription()`.

**Tech Stack:** Rust, `iced` 0.14 (wgpu/tokio/wayland), `sola-kit`, `sola-bus`, `sola-core`, `alacritty_terminal` `=0.26.x`, `vte` `0.15`, existing `nix`/`libc` PTY layer, `tmux` (socket `sola`, `sola-tmux.service`).

**Source research:** `docs/specs/2026-06-03-sola-terminal-iced-engine-research.md` (engine decision + integration sketch). Reference implementation to crib (NOT depend on): `Harzu/iced_term` `0.8.0` — its `src/view.rs` (canvas renderer) and `src/bindings`/input modules. License: alacritty_terminal is Apache-2.0; iced_term is MIT — both fine to read and re-author.

---

## ⚠️ Open Questions (flagged inline, resolve during execution — not blockers to starting)

These are called out at the exact task where they bite. None block Phase 1.

1. **Canvas render perf on the NVIDIA box (Task 2.1, the spike).** Per-cell `frame.fill_text` through cosmic-text may not hold a full-screen scrolling redraw at 60fps. The spike *measures* this and decides whether Phase 2 ships the canvas renderer as-is or needs a glyphon/instanced-quad pass. **Do the spike first.**
2. **Scrollback authority (Task 2.4).** tmux already keeps history *and* alacritty's `Grid` keeps history. Plan adopts: **alacritty `Grid` = live viewport + local scrollback; tmux = session persistence.** We still capture tmux scrollback on attach (as today) and feed it into the `Term` as initial bytes. Confirm this doesn't double-render on reattach.
3. **Exact `alacritty_terminal` 0.26 API surface (Tasks 2.2–2.3).** `iced_term` 0.8.0 pins `0.25.1`; we target `0.26.x`. Type/field names on `Term::new`, `renderable_content()`, `EventListener`, `Selection`, `Config` **must be re-derived against 0.26 docs** at implementation time — do not copy 0.25 signatures blind.
4. **Input encoding completeness (Task 2.3).** Mouse SGR modes, kitty-keyboard, bracketed paste must be validated against real TUIs (vim, htop, fzf). The spike covers basic keys; full coverage is its own task.
5. **`alacritty_terminal` pulls `rustix-openpty` behind a cfg (Task 1.1).** It should be inert (we never call alacritty's `tty`), but confirm no link/symbol conflict with our `nix::openpty` path.

---

## File Structure

New crate layout (`crates/sola-terminal/`), Rust-only — the entire `web/` tree is deleted at the end (Task 5.3):

| File | Responsibility | Origin |
|---|---|---|
| `Cargo.toml` | Deps: swap `sola-app`+`gtk4` → `sola-kit`+`iced`+`alacritty_terminal`+`vte`. Keep `nix`/`libc`/`tokio`/`base64`/`uuid`/`serde`. | Modify |
| `src/main.rs` | `iced::application` wiring (`startup` → `BusSetup` → builder), `App` struct, `Msg`, `update`/`view`/`subscription`/`theme`. | Rewrite |
| `src/pty.rs` | PTY spawn/attach/read/write/resize/close + tmux child. | **Reuse verbatim** (one change: reader feeds emulator, not base64 channel — Task 2.2). |
| `src/tmux.rs` | tmux server lifecycle, scrollback, cwd, session list. | **Reuse verbatim.** |
| `src/menu.rs` | App-menu definition (tab count → menu). | **Reuse** (drop the `sola_app::SolaApp` import; inline the `APP_ID` const). |
| `src/state.rs` | `TabEntry` + tab vec. Extend with the live `Tab` runtime (emulator handle, pty backend). | Modify/extend |
| `src/session.rs` | Bus-side reconciliation (replay `TerminalSession`/`TerminalConfig`, retract stale-vs-tmux). | New (extracted from old `main.rs`) |
| `src/emulator.rs` | Per-tab `alacritty_terminal::Term` owner: construct, feed bytes, expose `renderable_content`, resize, selection, the `EventListener` impl + `PtyWrite` back-channel. | New |
| `src/term_view.rs` | Iced `canvas::Program` rendering a `Term` grid (glyphs, bg rects, cursor, selection). Cribbed from `iced_term::view`. | New |
| `src/input.rs` | Keyboard/mouse `iced::Event` → terminal bytes (cribbed from `iced_term` bindings). | New |
| `src/sidebar.rs` | Iced sidebar widget: tab list, collapse, resize-drag, reorder-drag. Mirrors `web/components/sidebar.ts` + `sola-monitor`'s divider-drag. | New |

---

## Phase 0 — Prep

### Task 0.1: Branch + baseline build

**Files:** none (git only)

- [ ] **Step 1: Confirm the legacy crate builds today (baseline).**

Run: `cargo make build sola-terminal`
Expected: PASS (current `sola-app`-based build compiles).

- [ ] **Step 2: Record the feature checklist we must preserve.**

No code. Confirm this list against `web/src/app.ts`, `terminal-pane.ts`, `sidebar.ts` (already done in this plan):
PTY spawn/attach, raw data + scrollback render, input→write, resize→cols/rows, OSC 0/2 title, selection+copy, paste, OSC-8 link→`open_url`, cursor blink, exit→close-tab, tab CRUD, active-tab switch (refit+focus), bus `state` reconcile, `cwd_update`, menu `new_tab`/`select_tab`/`close_tab`/`copy`/`paste`, restore-on-boot, new-tab-inherits-active-cwd, sidebar collapse/resize/reorder persisted via `TerminalConfig`.

- [ ] **Step 3: Commit a marker (empty) so the port has a clean base.**

```bash
git commit --allow-empty -m "chore(sola-terminal): begin iced port (baseline marker)"
```

---

## Phase 1 — Re-host skeleton on sola-kit (no terminal rendering yet)

Goal: a compiling Iced `sola-terminal` that boots, connects to the bus via `sola-kit`, reuses `pty.rs`/`tmux.rs`, reconciles persisted tabs against tmux, publishes the app menu, and shows a **placeholder** body (sidebar list + empty pane). No grid yet.

### Task 1.1: Cargo.toml — swap legacy deps for the kit + engine

**Files:** Modify `crates/sola-terminal/Cargo.toml`

- [ ] **Step 1: Replace the `[dependencies]` block.**

```toml
[dependencies]
sola-kit = { path = "../sola-kit" }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
iced = { version = "0.14", default-features = false, features = ["wgpu", "tokio", "wayland", "canvas", "advanced", "svg"] }
alacritty_terminal = "=0.26.0"   # OPEN QUESTION #3: verify latest 0.26.x at impl time
vte = "0.15"
tokio = { version = "1", features = ["rt-multi-thread", "sync", "io-util", "macros", "time"] }
nix = { version = "0.30", features = ["process", "term", "signal"] }
libc = "0.2"
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
```

Removed: `sola-app`, `gtk4`, `async-trait`. (`base64`/`serde_json` may become unused later — keep until Phase 2 confirms.)

- [ ] **Step 2: Build (will fail — `main.rs` still references `sola_app`). That's expected; next task rewrites it.**

Run: `cargo build -p sola-terminal 2>&1 | head -5`
Expected: FAIL referencing `sola_app` / `gtk4`. **OPEN QUESTION #5:** scan the failure for any `rustix-openpty` symbol/link error — there should be none.

### Task 1.2: Decouple `menu.rs` from `sola_app`

**Files:** Modify `crates/sola-terminal/src/menu.rs`

- [ ] **Step 1: Replace the `sola_app::SolaApp` dependency with a local const.**

In `menu.rs`, delete `use crate::TerminalApp;` and `use sola_app::SolaApp;`. Replace `TerminalApp::APP_ID.into()` with `crate::APP_ID.into()`. (The `APP_ID` const is defined in Task 1.3.)

- [ ] **Step 2: Run menu unit tests (they don't touch `sola_app`).**

Run: `cargo test -p sola-terminal --lib menu`
Expected: PASS (after 1.3 compiles) — `empty_menu_has_no_tab_items`, `nine_tabs_get_shortcuts`, etc.

### Task 1.3: `main.rs` skeleton — Iced application boot

**Files:** Rewrite `crates/sola-terminal/src/main.rs`

- [ ] **Step 1: Write the boot + module wiring + `App`/`Msg` skeleton.**

```rust
//! sola-terminal — native Iced terminal. One window; tabs are
//! alacritty_terminal emulators fed by our tmux-backed PTYs. The
//! active tab's grid renders via a canvas widget; sidebar lists tabs.
//! Backend (pty.rs/tmux.rs) and bus topics (TerminalSession/Config)
//! are unchanged from the legacy WebView build.

mod emulator;
mod input;
mod menu;
mod pty;
mod session;
mod sidebar;
mod state;
mod term_view;
mod tmux;

use std::sync::Arc;

use iced::{Element, Subscription, Task, Theme};
use sola_bus::Message;
use sola_bus::topics::{Topic, TopicKind};
use sola_kit::app::{BusSetup, bus_subscription, startup, window_settings};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

pub const APP_ID: &str = "sola-terminal";

fn main() -> iced::Result {
    startup(APP_ID);

    // tmux server must be up before we replay persisted tabs.
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::TerminalConfig,
            TopicKind::TerminalSession,
        ])
        .app_menu_definition(menu::terminal_menu(0).menus.into_iter().next().unwrap_or_else(|| {
            // terminal_menu returns multiple menus; publish the full set instead.
            unreachable!("use the multi-menu publish below")
        }))
        .install();
    // NOTE: BusSetup::app_menu_definition takes ONE MenuDefinition, but our
    // menu has four (Terminal/Shell/Edit/Tabs). Publish the full AppMenuPayload
    // directly instead — see Step 2.

    let mut app = iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .window(window_settings(APP_ID));
    for bytes in fonts::load_all() {
        app = app.font(bytes);
    }
    app.run()
}

struct App {
    tabs: state::Tabs,
    active: Option<String>,
    config: sola_bus::topics::TerminalConfig,
    /// tmux sessions live at startup; used once to retract stale tabs.
    live_tmux_at_startup: Option<std::collections::HashSet<String>>,
    theme: Theme,
    sidebar: sidebar::SidebarState,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    /// A tab's emulator produced new output → redraw.
    PtyOutput(String),
    /// A tab's shell exited.
    PtyExit(String),
    // Sidebar
    SelectTab(String),
    CloseTab(String),
    NewTab,
    ToggleCollapse,
    SidebarDragStart,
    SidebarDragMove(f32),
    SidebarDragEnd,
    ReorderStart(usize),
    ReorderMove(f32),
    ReorderEnd,
    // Terminal surface
    Input(iced::Event),
    Resized(iced::Size),
    Tick,
}

impl App {
    fn new() -> (Self, Task<Msg>) {
        let app = Self {
            tabs: state::Tabs::default(),
            active: None,
            config: Default::default(),
            live_tmux_at_startup: tmux::list_sessions().map(|v| v.into_iter().collect()),
            theme: default_theme(),
            sidebar: sidebar::SidebarState::default(),
        };
        (app, Task::none())
    }

    fn title(&self) -> String { "Terminal".into() }
    fn theme(&self) -> Theme { self.theme.clone() }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(|tab_id| Msg::PtyOutput(tab_id)),
            iced::event::listen().map(Msg::Input),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            // remaining arms added in later tasks
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        use iced::widget::{row, container, text};
        // Placeholder body until Task 2.5 wires term_view.
        row![
            sidebar::view(&self.sidebar, &self.tabs, self.active.as_deref(), &self.config),
            container(text("terminal pane (placeholder)")).padding(8),
        ]
        .into()
    }
}
```

> **Note for implementer:** the `BusSetup::app_menu_definition` call above is a deliberate dead-end comment — `terminal_menu` yields a 4-menu `AppMenuPayload`, but `BusSetup` only takes one `MenuDefinition`. Use the direct-publish in Step 2 instead and drop `.app_menu_definition(...)` from the builder chain.

- [ ] **Step 2: Publish the full multi-menu payload directly (BusSetup can't).**

Replace the `.app_menu_definition(...)` line with nothing, and after `.install()` add:

```rust
    // BusSetup publishes at most one menu; the terminal has four.
    // Publish the real payload straight onto the bus client.
    if let Err(e) = sola_kit::app::bus()
        .lock()
        .map(|c| c.emit(Topic::SetAppMenu(menu::terminal_menu(0))))
    {
        tracing::warn!("initial app-menu publish failed: {e:?}");
    }
```

- [ ] **Step 3: Build.**

Run: `cargo build -p sola-terminal 2>&1 | tail -20`
Expected: FAIL only on not-yet-written modules (`emulator`, `term_view`, `input`, `sidebar`, `session`, and `state::Tabs`). Those are the next tasks. No `sola_app`/`gtk4` errors.

### Task 1.4: `state.rs` — runtime tab model

**Files:** Modify `crates/sola-terminal/src/state.rs`, Test: same file `#[cfg(test)]`

- [ ] **Step 1: Write a failing test for ordinal-sorted insert + remove.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upsert_keeps_sorted_by_ordinal() {
        let mut tabs = Tabs::default();
        tabs.upsert_meta(meta("b", 2));
        tabs.upsert_meta(meta("a", 1));
        assert_eq!(tabs.ids_in_order(), vec!["a", "b"]);
    }
    #[test]
    fn remove_drops_the_tab() {
        let mut tabs = Tabs::default();
        tabs.upsert_meta(meta("a", 1));
        tabs.remove("a");
        assert!(tabs.ids_in_order().is_empty());
    }
    fn meta(id: &str, ord: u32) -> TabMeta {
        TabMeta { id: id.into(), tmux_session: format!("sola-{id}"), cwd: None, ordinal: ord }
    }
}
```

- [ ] **Step 2: Run it (fails — `Tabs`/`TabMeta` undefined).**

Run: `cargo test -p sola-terminal --lib state`
Expected: FAIL (compile error).

- [ ] **Step 3: Implement the model.**

```rust
use std::collections::BTreeMap;
use crate::emulator::Emulator;
use crate::pty::PtyBackend;

/// Persisted-on-bus metadata for one tab (mirrors `TerminalSession`).
#[derive(Clone, Debug, PartialEq)]
pub struct TabMeta {
    pub id: String,
    pub tmux_session: String,
    pub cwd: Option<String>,
    pub ordinal: u32,
}

/// Live runtime for one tab: the emulator + the PTY backend handle.
/// `None` until the PTY is spawned/attached (Task 2.2).
pub struct TabRuntime {
    pub emulator: Emulator,
    pub backend: PtyBackend,
}

#[derive(Default)]
pub struct Tabs {
    meta: BTreeMap<String, TabMeta>,
    pub runtime: std::collections::HashMap<String, TabRuntime>,
}

impl Tabs {
    pub fn upsert_meta(&mut self, m: TabMeta) { self.meta.insert(m.id.clone(), m); }
    pub fn remove(&mut self, id: &str) { self.meta.remove(id); self.runtime.remove(id); }
    pub fn get(&self, id: &str) -> Option<&TabMeta> { self.meta.get(id) }
    pub fn len(&self) -> usize { self.meta.len() }
    pub fn is_empty(&self) -> bool { self.meta.is_empty() }
    /// Ids sorted by ordinal then id (stable).
    pub fn ids_in_order(&self) -> Vec<String> {
        let mut v: Vec<&TabMeta> = self.meta.values().collect();
        v.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then(a.id.cmp(&b.id)));
        v.into_iter().map(|m| m.id.clone()).collect()
    }
    pub fn ordered_meta(&self) -> Vec<TabMeta> {
        self.ids_in_order().into_iter().filter_map(|id| self.meta.get(&id).cloned()).collect()
    }
}
```

> The test references only `upsert_meta`/`ids_in_order`/`remove`, which don't touch `TabRuntime`/`Emulator`/`PtyBackend`. To compile the test before Phase 2, gate the `runtime`/`TabRuntime` lines behind the real `emulator`/`pty` types existing — implement `emulator::Emulator` + `pty::PtyBackend` stubs first (Task 2.2) **or** temporarily comment the `runtime` field + `TabRuntime` struct and re-add in Task 2.2. Prefer the latter to keep Phase 1 green.

- [ ] **Step 4: Run tests.**

Run: `cargo test -p sola-terminal --lib state`
Expected: PASS.

### Task 1.5: `session.rs` — bus reconciliation (ported from old `main.rs`)

**Files:** Create `crates/sola-terminal/src/session.rs`, Test: same file

This ports the old `on_terminal_session` / `on_terminal_config` / startup-tmux reconciliation logic — the part that decides which persisted `TerminalSession`s are admitted vs retracted against the live tmux snapshot.

- [ ] **Step 1: Write a failing test for stale-tab reconciliation.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn admits_tab_when_tmux_alive() {
        let live: Option<HashSet<String>> = Some(["sola-a".into()].into_iter().collect());
        assert_eq!(reconcile_admit(&live, "sola-a"), Admit::Yes);
    }
    #[test]
    fn retracts_tab_when_tmux_gone() {
        let live: Option<HashSet<String>> = Some(HashSet::new());
        assert_eq!(reconcile_admit(&live, "sola-a"), Admit::Retract);
    }
    #[test]
    fn admits_everything_when_tmux_unknown() {
        let live: Option<HashSet<String>> = None;
        assert_eq!(reconcile_admit(&live, "sola-a"), Admit::Yes);
    }
}
```

- [ ] **Step 2: Run it (fails).**

Run: `cargo test -p sola-terminal --lib session`
Expected: FAIL.

- [ ] **Step 3: Implement `reconcile_admit` + the `Admit` enum.**

```rust
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub enum Admit { Yes, Retract }

/// Decide whether a replayed persisted tab should be admitted or
/// retracted, given the live-at-startup tmux session set. `None`
/// (tmux unknown) admits everything — never nuke tabs on a transient
/// tmux glitch (preserves the old behavior exactly).
pub fn reconcile_admit(live_tmux: &Option<HashSet<String>>, tmux_session: &str) -> Admit {
    match live_tmux {
        Some(set) if !set.contains(tmux_session) => Admit::Retract,
        _ => Admit::Yes,
    }
}
```

- [ ] **Step 4: Run tests.**

Run: `cargo test -p sola-terminal --lib session`
Expected: PASS.

### Task 1.6: Wire `on_bus` in `main.rs`

**Files:** Modify `crates/sola-terminal/src/main.rs`

- [ ] **Step 1: Implement `App::on_bus` handling Theme / quit / TerminalConfig / TerminalSession.**

```rust
impl App {
    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        // Live theme reload.
        if sola_kit::app::apply_theme_update(m, &mut self.theme) {
            return Task::none();
        }
        // Self-quit (Cmd+Q via MenuAction, or CloseApp addressed to us).
        if sola_kit::app::is_self_quit(m, APP_ID) {
            return iced::exit();
        }
        match Topic::parse(m) {
            Some(Topic::TerminalConfig(cfg)) => { self.config = cfg; Task::none() }
            Some(Topic::TerminalSession(s)) => {
                let retracted = m.is_retracted(); // confirm method name on Message/Delivery
                if retracted {
                    self.tabs.remove(&s.id);
                    self.republish_menu();
                    return Task::none();
                }
                use session::Admit;
                if session::reconcile_admit(&self.live_tmux_at_startup, &s.tmux_session) == Admit::Retract {
                    tracing::info!(id=%s.id, tmux=%s.tmux_session, "retracting stale tab");
                    let _ = sola_kit::app::bus().lock().map(|c| c.retract(Topic::TerminalSession(s)));
                    return Task::none();
                }
                let was_present = self.tabs.get(&s.id).is_some();
                self.tabs.upsert_meta(state::TabMeta {
                    id: s.id.clone(), tmux_session: s.tmux_session.clone(),
                    cwd: s.cwd.clone(), ordinal: s.ordinal,
                });
                if self.active.is_none() { self.active = Some(s.id.clone()); }
                self.republish_menu();
                if !was_present {
                    // New/replayed tab → spawn-or-attach its PTY (Task 2.2).
                    return self.attach_tab(&s.id);
                }
                Task::none()
            }
            Some(Topic::MenuAction(p)) if p.app_id == APP_ID => self.on_menu_action(&p.action_id),
            _ => Task::none(),
        }
    }

    fn republish_menu(&self) {
        let payload = menu::terminal_menu(self.tabs.len());
        let _ = sola_kit::app::bus().lock().map(|c| c.emit(Topic::SetAppMenu(payload)));
    }

    // attach_tab + on_menu_action stubbed here, implemented in Phase 2/3.
    fn attach_tab(&mut self, _id: &str) -> Task<Msg> { Task::none() }
    fn on_menu_action(&mut self, _action: &str) -> Task<Msg> { Task::none() }
}
```

> **OPEN QUESTION (minor):** confirm the retract-flag accessor on a delivered `Message` (old code used `delivery.retracted`). In the kit's `Topic::parse(message)` path, verify how a retract is signalled — it may be a field on `Message` or require the lower-level delivery. Adjust `m.is_retracted()` accordingly.

- [ ] **Step 2: Build (sidebar/term_view stubs still missing → expected partial fail).**

Run: `cargo build -p sola-terminal 2>&1 | tail -20`
Expected: FAIL only on `sidebar`, `term_view`, `emulator`, `input` (next tasks).

### Task 1.7: Minimal `sidebar.rs` (list only) to make Phase 1 compile + run

**Files:** Create `crates/sola-terminal/src/sidebar.rs`

- [ ] **Step 1: Implement a read-only sidebar (tab list + active highlight). Drag/resize added in Phase 3.**

```rust
use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Length};
use sola_bus::topics::TerminalConfig;
use crate::Msg;
use crate::state::Tabs;

#[derive(Default)]
pub struct SidebarState {
    pub dragging_divider: bool,
    pub drag_anchor: Option<(f32, f32)>,
    pub reorder: Option<(usize, f32)>,
}

/// cwd basename → tab label, falling back to "shell".
pub fn tab_label(cwd: &Option<String>) -> String {
    match cwd.as_deref() {
        Some("/") => "/".into(),
        Some(p) if !p.is_empty() => p.trim_end_matches('/').rsplit('/').next().unwrap_or("shell").to_string(),
        _ => "shell".into(),
    }
}

pub fn view<'a>(
    _state: &SidebarState,
    tabs: &'a Tabs,
    active: Option<&str>,
    config: &TerminalConfig,
) -> Element<'a, Msg> {
    let width = if config.sidebar_collapsed { 36.0 } else { config.sidebar_width as f32 };
    let mut list = column![].spacing(2);
    for (i, m) in tabs.ordered_meta().into_iter().enumerate() {
        let is_active = active == Some(m.id.as_str());
        let label = format!("{}  {}", i + 1, tab_label(&m.cwd));
        list = list.push(
            button(text(label))
                .width(Length::Fill)
                .on_press(Msg::SelectTab(m.id.clone()))
                .style(move |t, s| crate::sidebar::tab_button_style(t, s, is_active)),
        );
    }
    let new_btn = button(text("+ New Tab")).width(Length::Fill).on_press(Msg::NewTab);
    container(column![scrollable(list).height(Length::Fill), new_btn])
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .padding(4)
        .into()
}

pub fn tab_button_style(theme: &iced::Theme, _status: button::Status, active: bool) -> button::Style {
    let p = theme.extended_palette();
    button::Style {
        background: Some(if active { p.background.weak.color } else { p.background.base.color }.into()),
        text_color: p.background.base.text,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basename_label() {
        assert_eq!(tab_label(&Some("/home/joshua/Workspace".into())), "Workspace");
        assert_eq!(tab_label(&Some("/".into())), "/");
        assert_eq!(tab_label(&None), "shell");
    }
}
```

- [ ] **Step 2: Temporarily stub `term_view`/`emulator`/`input` so the crate links.**

Create one-line stub modules returning placeholders so Phase 1 runs:

`src/emulator.rs`:
```rust
use iced::Subscription;
/// Stream of tab-ids that produced output. Stub in Phase 1; real in Task 2.2.
pub fn output_subscription() -> Subscription<String> { Subscription::none() }
```
`src/term_view.rs`: `// placeholder; implemented in Phase 2`
`src/input.rs`: `// placeholder; implemented in Phase 2`

(Remove the `Input`/`Resized`/`Tick`/`PtyOutput` arms' dependence on these until Phase 2 — they already fall through `_ => Task::none()`.)

- [ ] **Step 3: Build the whole crate.**

Run: `cargo make build sola-terminal`
Expected: PASS.

- [ ] **Step 4: Sidebar test.**

Run: `cargo test -p sola-terminal --lib sidebar`
Expected: PASS (`basename_label`).

- [ ] **Step 5: Commit Phase 1.**

```bash
git add crates/sola-terminal
git commit -m "feat(sola-terminal): re-host on sola-kit/iced skeleton (no grid yet)"
```

> **DO NOT install.** Per project rules, installing requires explicit per-call user permission. Phase 1 verification is `cargo make build` + unit tests only. If you want to see it boot, ask the user to run the install.

---

## Phase 2 — Terminal emulator + canvas renderer (the core)

### Task 2.1: 🔬 SPIKE — alacritty_terminal → iced canvas, on the NVIDIA box

**This is OPEN QUESTION #1. Do it first. Throwaway code — a separate `examples/` binary, not the app.**

**Files:** Create `crates/sola-terminal/examples/spike_render.rs`

- [ ] **Step 1: Build a minimal example: open a raw PTY running `bash` (no tmux), construct an `alacritty_terminal::Term`, feed bytes via `vte::ansi::Processor::advance`, render `term.renderable_content()` in an `iced::canvas` (per-cell `fill_text`, batched bg rects, cursor). Crib `iced_term/src/view.rs`.**

Acceptance: the example shows a live shell you can type into.

- [ ] **Step 2: Benchmark the worst case: `yes | head -c 5000000` or `find /` scrolling full-screen. Measure FPS / frame time on the RTX 3090 Ti box.**

Record the result in `docs/specs/2026-06-03-sola-terminal-iced-engine-research.md` under a new "Spike result" heading.

- [ ] **Step 3: DECISION GATE.**
  - If frame time is acceptable (≲16ms full-screen scroll): **Phase 2 ships the canvas renderer** (Tasks 2.5/2.6 as written).
  - If not: **add Task 2.7 (glyphon atlas / instanced-quad renderer)** before shipping, and keep canvas as the fallback. Update the plan's Task 2.5 to note the chosen path.

- [ ] **Step 4: Delete the spike (or keep under `examples/` as a perf regression harness — implementer's call). Do not let it block the real modules.**

### Task 2.2: `emulator.rs` — per-tab Term owner + reader→Term feed + output subscription

**Files:** Rewrite `crates/sola-terminal/src/emulator.rs`; Modify `src/pty.rs` (reader feeds emulator)

- [ ] **Step 1: Define the `EventListener` + `Emulator` (construct, feed, renderable snapshot, resize, selection).**

Real API names re-derived against alacritty_terminal 0.26 (OPEN QUESTION #3). Shape:

```rust
use std::sync::Arc;
use alacritty_terminal::Term;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::grid::Dimensions;

/// Forwards Term events. PtyWrite (DSR/DA replies, OSC-52) MUST be
/// written back to the PTY or some TUIs hang (OPEN QUESTION: keep this).
#[derive(Clone)]
pub struct Listener {
    pub tab_id: String,
    pub pty_write: std::sync::mpsc::Sender<(String, Vec<u8>)>,
    pub notify: std::sync::mpsc::Sender<String>, // wakes iced (output_subscription)
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::PtyWrite(text) => { let _ = self.pty_write.send((self.tab_id.clone(), text.into_bytes())); }
            TermEvent::Wakeup => { let _ = self.notify.send(self.tab_id.clone()); }
            // Title/Bell/ClipboardStore → forward to App via notify/bus as needed.
            _ => {}
        }
    }
}

pub struct Emulator {
    pub term: Arc<FairMutex<Term<Listener>>>,
    parser: vte::ansi::Processor,
}

impl Emulator {
    pub fn new(tab_id: String, cols: u16, rows: u16, listener: Listener) -> Self {
        let dims = SizeInfo::new(cols, rows); // impl Dimensions; see Step 2
        let term = Term::new(TermConfig::default(), &dims, listener);
        Self { term: Arc::new(FairMutex::new(term)), parser: vte::ansi::Processor::new() }
    }
    /// Feed raw PTY bytes; drives the grid. Called from the reader thread.
    pub fn advance(&mut self, bytes: &[u8]) {
        let mut term = self.term.lock();
        self.parser.advance(&mut *term, bytes);
    }
}
```

- [ ] **Step 2: Implement a `SizeInfo`/`Dimensions` impl (cols/rows/cell px) — required by `Term::new`/`resize`. Crib field names from iced_term's `TermSize`/alacritty docs.**

- [ ] **Step 3: Replace `pty.rs`'s reader-thread payload: instead of `event_tx.send(PtyEvent::Data{..})` (base64 to JS), feed bytes straight into the tab's `Emulator::advance` and send only a `notify(tab_id)` wakeup.** The 64KB `OutputBuffer` is no longer needed (alacritty's grid holds scrollback) — remove it. Scrollback-on-attach (Task 2.4) still feeds captured tmux bytes through `advance`.

- [ ] **Step 4: Implement `output_subscription()` — a `Subscription<String>` over the `notify` channel (same polling-thread pattern as `sola_kit::app::bus_stream`).**

```rust
pub fn output_subscription() -> Subscription<String> {
    Subscription::run(output_stream) // forwards notify-channel tab-ids into iced
}
```

- [ ] **Step 5: Unit test the parser feed (headless, no iced).**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advance_writes_cells_into_grid() {
        let (ptx, _prx) = std::sync::mpsc::channel();
        let (ntx, _nrx) = std::sync::mpsc::channel();
        let mut e = Emulator::new("t".into(), 80, 24,
            Listener { tab_id: "t".into(), pty_write: ptx, notify: ntx });
        e.advance(b"hi");
        let term = e.term.lock();
        // assert the first two cells contain 'h','i' via renderable_content/grid
        // (exact accessor per 0.26 API)
        assert!(format!("{:?}", term.grid()).contains('h') || true); // refine to real assert
    }
}
```

Run: `cargo test -p sola-terminal --lib emulator`
Expected: PASS (refine the assert to the real 0.26 grid accessor — placeholder `|| true` MUST be removed before commit).

### Task 2.3: `input.rs` — iced events → terminal bytes

**Files:** Create `crates/sola-terminal/src/input.rs`; Test: same file

- [ ] **Step 1: Port iced_term's key/mouse → bytes encoder. Cover: printable chars, Enter/Tab/Backspace/Esc, arrows + Home/End/PgUp/PgDn (CSI), Ctrl-letters, Alt-prefix, bracketed paste, mouse SGR per `term.mode()`.** (OPEN QUESTION #4 — full coverage validated in Task 4.2.)

- [ ] **Step 2: Unit test the simple, deterministic cases.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Key, key::Named};
    #[test]
    fn enter_is_cr() { assert_eq!(encode_key(&Key::Named(Named::Enter), Mods::NONE), Some(b"\r".to_vec())); }
    #[test]
    fn ctrl_c_is_etx() { assert_eq!(encode_char('c', Mods::CTRL), Some(vec![0x03])); }
    #[test]
    fn up_arrow_csi() { assert_eq!(encode_key(&Key::Named(Named::ArrowUp), Mods::NONE), Some(b"\x1b[A".to_vec())); }
}
```

Run: `cargo test -p sola-terminal --lib input`
Expected: PASS.

### Task 2.4: PTY spawn/attach + scrollback-on-attach wired to the emulator

**Files:** Modify `crates/sola-terminal/src/pty.rs`, `src/state.rs` (`TabRuntime`), `src/main.rs` (`attach_tab`)

- [ ] **Step 1: Introduce `PtyBackend` (master_fd, child_pid, tmux_session) — the per-tab handle stored in `TabRuntime`. Move `write`/`resize`/`sigwinch`/`close` onto it (from today's `PtyManager`, which managed a map; now one-per-tab).**

- [ ] **Step 2: `attach_tab(&mut self, id)` in `main.rs`: look up `TabMeta`, build the `Emulator` + `Listener`, spawn/attach the PTY (`tmux new-session -A -s <tmux_session>`), capture tmux scrollback (`tmux::capture_scrollback`) and feed it via `emulator.advance(scrollback_bytes)` BEFORE the reader thread starts (no race — same ordering guarantee the old code had), then start the reader thread feeding `advance` + `notify`. Store `TabRuntime` in `tabs.runtime`.**

> **OPEN QUESTION #2 (scrollback authority):** confirm feeding tmux scrollback into alacritty's grid on attach doesn't conflict with alacritty's own history when the same pane later scrolls. Adopt "Grid = viewport+local history, tmux = persistence"; the captured scrollback is a one-shot seed.

- [ ] **Step 3: New-tab path (`Msg::NewTab`): mint a uuid, spawn fresh tmux session, emit `TerminalSession` (persistent) + republish menu, inherit active tab's cwd as the start dir.** (Ports old `cmd_spawn_pty` non-restore branch.)

- [ ] **Step 4: Close-tab path (`Msg::CloseTab` / menu / shell exit): `backend.close()` (kills tmux session + child), `tabs.remove(id)`, retract `TerminalSession`, pick a new active tab, republish menu.** (Ports old `cmd_close_pty`.)

- [ ] **Step 5: Build + existing tests.**

Run: `cargo make build sola-terminal && cargo test -p sola-terminal --lib`
Expected: PASS.

### Task 2.5: `term_view.rs` — canvas renderer (or glyphon, per spike gate)

**Files:** Rewrite `crates/sola-terminal/src/term_view.rs`

- [ ] **Step 1: Implement an `iced::widget::canvas::Program<Msg>` that locks the active tab's `Term`, reads `renderable_content()`, and draws: batched contiguous-bg rects (`frame.fill`), per-cell glyphs (`frame.fill_text` with per-cell weight/style/color, `Shaping::Advanced`), underlines (`frame.stroke`), cursor block, and selection highlight. Use a geometry `Cache` invalidated on the `notify` tick.** Crib `iced_term/src/view.rs` structure; re-derive `RenderableContent`/`Cell` field names against 0.26.

- [ ] **Step 2: Compute cell metrics from the mono font at the configured px; expose `cols/rows = floor(size / cell)` for resize (Task 2.6).**

- [ ] **Step 3: Mouse selection: translate canvas-local cursor → grid cell; drive `term.selection` (start/update); `Cmd+C` copy reads `term.selection_to_string()`.**

- [ ] **Step 4: Wire `App::view` to render `term_view` for the active tab instead of the placeholder text. Build.**

Run: `cargo make build sola-terminal`
Expected: PASS.

> If the spike (2.1) failed the gate, this task targets the glyphon/instanced-quad renderer instead; same external contract (reads `renderable_content`, emits `Msg` for selection).

### Task 2.6: Resize plumbing

**Files:** Modify `src/main.rs` (`Msg::Resized`, `Msg::Tick`), `src/term_view.rs`, `src/emulator.rs`

- [ ] **Step 1: On window/pane resize (`Msg::Resized` from a `canvas` size or `iced::window::resize_events`): compute new cols/rows; call `emulator.resize(dims)`, `backend.resize(cols,rows)` (ioctl TIOCSWINSZ), `tmux::resize_window`, and `backend.sigwinch()`.** Mirrors old `cmd_resize_pty`. Drive resize into BOTH alacritty and tmux (OPEN QUESTION #2 — tmux owns wrapping authority).

- [ ] **Step 2: Build + commit Phase 2.**

```bash
cargo make build sola-terminal
git add crates/sola-terminal && git commit -m "feat(sola-terminal): alacritty emulator + canvas grid renderer"
```

---

## Phase 3 — Sidebar parity (collapse, resize, reorder) + tab switching

### Task 3.1: Tab switch / focus

**Files:** Modify `src/main.rs`

- [ ] **Step 1: `Msg::SelectTab(id)` → set `self.active = Some(id)`; the canvas re-reads the now-active tab's `Term` next frame. Keyboard input routes to the active tab's PTY.** (Inactive tabs keep their emulator alive and keep consuming PTY bytes — instant switch, warm scrollback.)
- [ ] **Step 2: Menu `select_tab_<n>` / `new_tab` / `close_tab` → map to the same handlers.** (Ports old `on_menu_action`.)

### Task 3.2: Sidebar collapse + resize-drag (persisted to `TerminalConfig`)

**Files:** Modify `src/sidebar.rs`, `src/main.rs`

- [ ] **Step 1: Collapse toggle (`Msg::ToggleCollapse`): flip `config.sidebar_collapsed`, emit `Topic::TerminalConfig` (persistent), refit active tab.** (Ports `handleToggleCollapse` + `set_sidebar`.)
- [ ] **Step 2: Resize drag — reuse `sola-monitor`'s anchor-based divider pattern (`DividerPress`/`CursorMoved`/`CursorReleased`, clamp MIN 80 / MAX 250). On release, emit `Topic::TerminalConfig`.** Add a unit test for the clamp + anchor math (copy monitor's approach).
- [ ] **Step 3: Build + test.**

Run: `cargo make build sola-terminal && cargo test -p sola-terminal --lib sidebar`
Expected: PASS.

### Task 3.3: Tab drag-reorder

**Files:** Modify `src/sidebar.rs`, `src/main.rs`

- [ ] **Step 1: Port the reorder interaction (`ReorderStart`/`ReorderMove`/`ReorderEnd`, 5px threshold, drop-target highlight). On drop, renumber ordinals and emit one `Topic::TerminalSession` per changed tab.** (Ports `handleReorder` + old `cmd_reorder_tabs`.) Unit-test the renumber-by-order pure function.
- [ ] **Step 2: Build + commit Phase 3.**

```bash
cargo make build sola-terminal && git add crates/sola-terminal && git commit -m "feat(sola-terminal): sidebar parity — collapse, resize, reorder, switch"
```

---

## Phase 4 — Menu actions, copy/paste, links, theme, polish

### Task 4.1: Copy / paste / open-url

**Files:** Modify `src/main.rs`, `src/term_view.rs`

- [ ] **Step 1: Menu `copy` → `term.selection_to_string()` → wayland clipboard (via `iced::clipboard::write` Task).** Menu `paste` → bracketed-paste-encode the clipboard text → write to active PTY. (Ports the old `copy`/`paste` bus handlers + `dispatch_copy`/`dispatch_paste`.)
- [ ] **Step 2: OSC-8 hyperlink click → emit `Topic::OpenUrl(OpenUrlRequest{url,activate:true})`.** (Ports the `WebLinksAddon` → `open_url`.)

### Task 4.2: Input-encoding validation against real TUIs (OPEN QUESTION #4)

**Files:** none (manual verification) — ask the user to run, per install rules

- [ ] **Step 1: Document a manual test matrix: `vim` (arrows, Esc, Ctrl), `htop` (mouse), `fzf`/`less` (PgUp/Dn, search), wide chars (`echo 世界`), truecolor (`printf` 24-bit), bracketed paste.** The user runs the installed build and reports breakage; fix `input.rs` accordingly.

### Task 4.3: Title + cwd tracking

**Files:** Modify `src/main.rs`, `src/emulator.rs`

- [ ] **Step 1: OSC 0/2 title → `TermEvent::Title` forwarded from `Listener` → update tab (used for window title / optional sidebar tooltip).**
- [ ] **Step 2: cwd tracking — port the delayed `pane_current_path` query on Enter (`refresh_cwd`): on a `\r` write, spawn a `tokio` task after 150ms, query tmux, and if changed, update the tab + emit `Topic::TerminalSession`.** (Sidebar label already derives from cwd.)

### Task 4.4: Theme application to the grid

**Files:** Modify `src/term_view.rs`, `src/emulator.rs`

- [ ] **Step 1: Map the iced theme palette + `Topic::Theme` colors onto the terminal's default fg/bg/cursor/selection + the 16 ANSI colors.** `apply_theme_update` already swaps `self.theme`; the renderer reads palette from it. Confirm named-color cells (`NamedColor`) map to the themed ANSI set, truecolor cells render literally.

### Task 4.5: Retire the legacy stack

**Files:** Delete `crates/sola-terminal/web/` (entire tree); confirm no `sola-app` references remain repo-wide for the terminal

- [ ] **Step 1: `git rm -r crates/sola-terminal/web`.**
- [ ] **Step 2: Grep for stragglers.**

Run: `grep -rn "sola_app\|sola-app\|xterm\|webkit" crates/sola-terminal/`
Expected: no matches.

- [ ] **Step 3: Full workspace build + clippy.**

Run: `cargo make build && cargo clippy -p sola-terminal --all-targets`
Expected: PASS, no warnings in the new modules.

- [ ] **Step 4: Commit + (ask user before any install/smoke).**

```bash
git add -A crates/sola-terminal && git commit -m "feat(sola-terminal): complete iced port; remove legacy webview stack"
```

> **Final smoke is the user's to run** (install rule): suggest the user `cargo make install sola-terminal` and exercise the Task 4.2 matrix from a TTY. Do not install autonomously.

---

## Self-Review (against the Phase 0 feature checklist)

| Feature (legacy) | Covered by |
|---|---|
| PTY spawn/attach (tmux) | 2.4 |
| Render data + scrollback | 2.2, 2.4, 2.5 |
| Input → write_pty | 2.3, 3.1 |
| Resize → cols/rows (+tmux) | 2.6 |
| OSC 0/2 title | 4.3 |
| Selection + copy | 2.5, 4.1 |
| Paste (bracketed) | 4.1 |
| OSC-8 link → open_url | 4.1 |
| Cursor (blink) | 2.5 |
| Shell exit → close tab | 2.4 |
| Tab CRUD + active switch | 2.4, 3.1 |
| Bus `state`/session reconcile | 1.5, 1.6 |
| Persisted `TerminalConfig` (sidebar) | 3.2 |
| cwd_update (tmux poll) | 4.3 |
| Menu new/select/close/copy/paste | 3.1, 4.1 |
| Restore-on-boot (sticky replay) | 1.6, 2.4 |
| New-tab inherits active cwd | 2.4 |
| Sidebar collapse/resize/reorder | 3.2, 3.3 |
| Theme | 4.4 |

**Type-consistency note for the implementer:** `Tabs`/`TabMeta`/`TabRuntime` (state.rs), `Emulator`/`Listener` (emulator.rs), `PtyBackend` (pty.rs), and the `Msg` variants are defined once in Phase 1/early-2 and referenced thereafter — keep those exact names. The single largest source of drift will be **alacritty_terminal 0.26 vs iced_term's 0.25 API** — re-derive every `Term`/`RenderableContent`/`Config`/`Selection` signature against 0.26 docs (OPEN QUESTION #3) rather than copying iced_term verbatim.

---

**Plan saved to:** `docs/specs/2026-06-03-sola-terminal-iced-port-plan.md`
