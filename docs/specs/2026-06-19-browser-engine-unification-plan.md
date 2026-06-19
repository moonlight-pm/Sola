# Browser Engine Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the two duplicated browser crates into one shared chrome library (`sola-browser-core`) behind an `Engine` trait, two thin engine binaries, and a featherweight `sola-browser` dispatcher that `exec`s the chosen engine.

**Architecture:** `sola-browser-core` holds all engine-agnostic chrome (`App<E>`, `Msg`, `view`/`update`, bus integration, input mapping, shared types, `FrameSlot<F>`, and `run::<E>()`), generic over an `Engine` trait. `sola-browser-wpe` and `sola-browser-cef` each implement `Engine` and shrink to a one-line `main`. `sola-browser` selects an engine (`--engine`/env/default) and `execv`s the sibling binary; it depends on neither engine. Both engines report `app_id = "sola-browser"`.

**Tech Stack:** Rust 2024, iced 0.14 (wgpu/wayland), wgpu 27 + wgpu-hal + ash, `sola-bus`/`sola-core`/`sola-kit`, WPEWebKit FFI (bindgen/cc/pkg-config) and the `cef` crate. Build via `cargo make build`.

**Design spec:** `docs/specs/2026-06-19-browser-engine-unification-design.md`

## Global Constraints

- **NEVER run `cargo make install` (or any variant) without explicit per-call user permission.** Verify with `cargo make build` only. (Project CLAUDE.md.)
- **Build with `cargo make build [target]`** — never raw `cargo build`/`cp` for deliverable verification. (A `cargo build -p <lib>` is acceptable only as a quick library typecheck where noted.)
- **Use Serena symbolic tools** (`get_symbols_overview`, `find_symbol`, `replace_symbol_body`, `insert_*`, `replace_content`) for all code files; built-in Edit/Read only where Serena cannot express the change or for non-code files (Cargo.toml, .desktop, .md).
- **`app_id = "sola-browser"`** for both engines (passed to `run::<E>("sola-browser")`).
- **`sola-browser-core` depends on no engine lib**; **`sola-browser` (dispatcher) depends on no engine crate and no engine lib** — std + `sola-core` only. Violating either re-introduces the 1.34 GB libcef load cost or a dependency cycle.
- New crates live under `crates/` and are auto-included by `members = ["crates/*"]`. **No edits to the root `Cargo.toml` `exclude` list.**
- Commit after each task. End commit messages with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
- Each engine binary keeps its name (`sola-browser-wpe`, `sola-browser-cef`); the dispatcher is `sola-browser`. All install to `/opt/sola/bin/`.
- This is a **refactor — engine swap only**. Relocated code moves verbatim except for the explicit signature/generic changes named per task. Do not change render behaviour, the dma-buf import strategy, or engine semantics.
- **Smoke runs are user-performed** (GUI + install permission). Tasks that need a live window end at "build clean" + a written smoke checklist for the user; do not block on running the GUI yourself.

## File Structure

```
crates/
  sola-browser-core/                 # NEW library
    Cargo.toml
    src/lib.rs                        # module wiring + pub re-exports
    src/engine.rs                     # Engine trait + Cmd/NavCmd/InputEvent/TabId/TabInfo/TaggedFrame<F>/FrameSlot<F> + handle aliases
    src/util.rs                       # truncate, normalize_url (pure; unit-tested)
    src/app.rs                        # App<E>, Msg, update/view/subscription, consts
    src/integration.rs               # bus receive side (SUBSCRIBE, MENU_ITEMS, handle_bus, intent_for_open_url, run_intent)
    src/input.rs                      # iced event -> InputEvent
    src/run.rs                        # run::<E>(app_id) -> ExitCode + frame_stream helper

  sola-browser-wpe/                  # THIN bin (existing crate, gutted)
    Cargo.toml                        # + sola-browser-core dep
    src/main.rs                       # fn main() -> ExitCode { sola_browser_core::run::<WpeEngine>("sola-browser") }
    src/engine.rs                     # WpeEngine + impl Engine (was wpe.rs)
    src/frame.rs                      # WpeFrame + WpeProgram/WpePrimitive/WpePipeline + dma-buf import (was shader.rs + wgpu_import.rs)
    src/lib.rs, wpe_sys.rs, sola_wpe.{c,h}, wpe_wrapper.h, bin/*.rs   # unchanged
    build.rs                          # unchanged

  sola-browser-cef/                  # THIN bin (existing crate, gutted)
    Cargo.toml                        # + sola-browser-core dep
    src/main.rs                       # fn main() -> ExitCode { sola_browser_core::run::<CefEngine>("sola-browser") }
    src/engine.rs                     # CefEngine + impl Engine (was cef.rs, incl. dispatch_subprocess)
    src/frame.rs                      # CefFrame + CefProgram/CefPrimitive/CefPipeline + import (was shader.rs + cpu_import.rs)
    src/lib.rs                        # unchanged

  sola-browser/                      # NEW dispatcher bin
    Cargo.toml
    src/main.rs                       # engine select + arg filter + fallback + execv
    dist/applications/sola-browser.desktop
```

Modified outside the browser crates:
- `crates/sola-shell/src/builtins.rs` — launcher entries.
- `crates/solactl/src/open.rs` and `crates/sola-make/src/install.rs` — stale `crates/sola-browser/...` doc references (now real).

---

## Task 1: `sola-browser-core` skeleton — `Engine` trait, shared types, pure helpers

Creates the library and the engine-agnostic types both engines already share (verified type-identical against `wpe.rs`/`cef.rs`). No engine code yet — the generic surface compiles standalone against trait bounds.

**Files:**
- Create: `crates/sola-browser-core/Cargo.toml`
- Create: `crates/sola-browser-core/src/lib.rs`
- Create: `crates/sola-browser-core/src/engine.rs`
- Create: `crates/sola-browser-core/src/util.rs`
- Create: `crates/sola-browser-core/src/app.rs` (**Msg + consts only** in this task; `App<E>` + methods are added in Task 2)

**Interfaces:**
- Produces: `Engine` trait; `Cmd`, `NavCmd`, `InputEvent`, `TabId(pub u64)`, `TabInfo{id,url,title}`, `TaggedFrame<F>{tab_id,frame}`, `FrameSlot<E>{pending,releaser,last_size,cursor}`; aliases `TabsHandle = Arc<Mutex<Vec<TabInfo>>>`, `ActiveHandle = Arc<AtomicU64>`, `CursorHandle = Arc<AtomicU32>`; `app::Msg` + chrome consts; `util::truncate`, `util::normalize_url`.

> **Compile ordering:** `engine.rs`'s `Engine::Program` bound is `shader::Program<crate::app::Msg>`, so `app::Msg` must exist in this task. `Msg` depends only on core types (`TabId`, `sola_bus::Message`), so it ships here; the rest of `app.rs` (`App<E>`, `update`/`view`/…) arrives in Task 2.

- [ ] **Step 1: Create the crate manifest**

`crates/sola-browser-core/Cargo.toml`:
```toml
[package]
name = "sola-browser-core"
version = "0.1.0"
edition = "2024"

# Shared iced chrome for the Sola browsers. Generic over the `Engine`
# trait so sola-browser-wpe / sola-browser-cef supply only their engine
# body. Depends on NO web-engine library — neither libWPEWebKit nor
# libcef — so neither leaks into the other's process image.

[lib]
path = "src/lib.rs"

[dependencies]
iced = { version = "0.14", default-features = false, features = ["wgpu", "tokio", "wayland"] }
sola-bus = { path = "../sola-bus" }
sola-core = { path = "../sola-core" }
sola-kit = { path = "../sola-kit" }
tracing = "0.1"
wgpu = { version = "27", features = ["vulkan-portability"] }
wgpu-hal = { version = "27", features = ["vulkan"] }
ash = "0.38"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros"] }
```

- [ ] **Step 2: Write `engine.rs` — shared types + the `Engine` trait**

`crates/sola-browser-core/src/engine.rs` (the enums/structs are copied verbatim from `sola-browser-wpe/src/wpe.rs` lines 61–176 and `shader.rs` 26–43; only `TaggedFrame` and `FrameSlot` gain a generic `F`):
```rust
//! Engine-agnostic types shared by every Sola browser engine, plus the
//! `Engine` trait the shared chrome is generic over.

use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

#[derive(Debug, Clone)]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub enum NavCmd {
    Back,
    Forward,
    Reload,
    Stop,
    LoadUrl(String),
}

#[derive(Debug, Clone)]
pub enum InputEvent {
    PointerMove { x: f64, y: f64, delta_x: f64, delta_y: f64, modifiers: u32, time_ms: u32 },
    PointerButton { down: bool, x: f64, y: f64, button: u32, modifiers: u32, time_ms: u32 },
    Scroll { x: f64, y: f64, delta_x: f64, delta_y: f64, precise: bool, modifiers: u32, time_ms: u32 },
    Key { down: bool, keyval: u32, keycode: u32, modifiers: u32, time_ms: u32 },
}

/// Commands the chrome sends to the engine worker. `Release` carries an
/// engine-specific token, so it is generic over the engine's token type.
pub enum Cmd<Tok> {
    Resize { width: u32, height: u32 },
    Release { token: Tok },
    Input(InputEvent),
    Focus(bool),
    Nav(NavCmd),
    OpenTab { id: TabId, url: String },
    CloseTab(TabId),
    SetActiveTab(TabId),
    Quit,
}

/// One frame as it crosses the worker→chrome boundary.
pub struct TaggedFrame<F> {
    pub tab_id: TabId,
    pub frame: F,
}

/// Shared between `App` (fills `pending`) and the engine's shader Program
/// (drains it on next prepare). `releaser` goes back to the engine worker.
pub struct FrameSlot<E: Engine> {
    pub pending: Mutex<Option<E::Frame>>,
    pub releaser: Sender<Cmd<E::Token>>,
    pub last_size: Mutex<(u32, u32)>,
    pub cursor: Arc<AtomicU32>,
}

pub type TabsHandle = Arc<Mutex<Vec<TabInfo>>>;
pub type ActiveHandle = Arc<AtomicU64>;
pub type CursorHandle = Arc<AtomicU32>;
pub type FrameReceiver<F> = Arc<Mutex<Receiver<TaggedFrame<F>>>>;

/// A browser engine. Both `WpeEngine` and `CefEngine` already expose this
/// exact surface (7 methods + the CEF subprocess gate); the trait names it.
pub trait Engine: Sized + Send + Sync + 'static {
    /// Engine-specific raw frame (WPE: dma-buf fd; CEF: dma-buf or CPU buffer).
    type Frame: Send + 'static;
    /// Opaque buffer-recycle token returned via `Cmd::Release`.
    type Token: Send + 'static;
    /// The iced shader Program that imports `Self::Frame` and samples it.
    type Program: iced::widget::shader::Program<crate::app::Msg> + 'static;

    /// CEF subprocess gate; runs first in `run()`, before logging/Wayland
    /// init. WPE returns `None`; CEF dispatches `--type=` workers and
    /// returns `Some(exit_code)`.
    fn dispatch_subprocess(_app_id: &'static str) -> Option<std::process::ExitCode> {
        None
    }

    /// Bring the engine up. Encapsulates ALL engine-specific startup
    /// quirks (e.g. WPE's WEBKIT_EXEC_PATH + WAYLAND_DISPLAY dance).
    fn spawn(app_id: &'static str, url: &str, w: u32, h: u32) -> Self;

    fn alloc_tab_id(&self) -> TabId;
    fn cmd_sender(&self) -> Sender<Cmd<Self::Token>>;
    fn tabs_handle(&self) -> TabsHandle;
    fn active_tab_handle(&self) -> ActiveHandle;
    fn cursor_handle(&self) -> CursorHandle;
    fn frames(&self) -> FrameReceiver<Self::Frame>;
    fn make_program(slot: Arc<FrameSlot<Self>>) -> Self::Program;
    fn shutdown(self);
}
```

> Note on `Cmd<Tok>`/`FrameSlot<E>`: today `Cmd` is non-generic because each crate has its own `ResourceToken`. Unifying requires either a generic token (shown) or moving the token type into the trait. The generic-token form keeps `Cmd` in core while letting each engine keep its own token. The engine's `Cmd` alias becomes `Cmd<Self::Token>`.

- [ ] **Step 3: Write `util.rs` with a failing test first**

`crates/sola-browser-core/src/util.rs` — copy the bodies of `truncate` and `normalize_url` verbatim from `sola-browser-wpe/src/main.rs` (functions `truncate`, `normalize_url`), make them `pub`, then add tests that assert their *actual* current behaviour. First read the two bodies with `find_symbol` and transcribe them, then write tests matching what they do. Skeleton:
```rust
//! Pure string helpers for the browser chrome.

/// (verbatim body of main.rs `truncate`)
pub fn truncate(s: &str, max: usize) -> String { /* paste verbatim */ }

/// (verbatim body of main.rs `normalize_url`)
pub fn normalize_url(input: &str) -> String { /* paste verbatim */ }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_strings_untouched() {
        assert_eq!(truncate("hi", 20), "hi");
    }

    #[test]
    fn truncate_shortens_long_strings_within_budget() {
        let out = truncate("a very long tab title indeed", 10);
        assert!(out.chars().count() <= 10, "got {out:?}");
    }

    #[test]
    fn normalize_url_adds_scheme_to_bare_host() {
        assert!(normalize_url("example.com").starts_with("http"));
    }
}
```
> If `truncate`'s ellipsis pushes the count to `max` exactly, the `<= 10` assertion holds; if the real function uses a different budget convention, adjust the literal to match the transcribed body (do not change the body).

- [ ] **Step 4: Write `app.rs` — `Msg` + consts (skeleton)**

`crates/sola-browser-core/src/app.rs` — in this task it holds ONLY the `Msg` enum (transcribed verbatim from `sola-browser-wpe/src/main.rs` lines 172–196, made `pub`) and the chrome consts. Task 2 adds `App<E>` and the methods to this same file.
```rust
//! Browser chrome message type + layout constants. `App<E>` and its
//! update/view methods are added in Task 2.
use std::sync::Arc;

use crate::engine::TabId;

pub const DEFAULT_URL: &str = "https://slate.auto";
pub const VIEW_W: u32 = 1280;
pub const VIEW_H: u32 = 800;
pub const CHROME_HEIGHT: f32 = 38.0;
pub const SIDEBAR_W_DEFAULT: f32 = 200.0;
pub const SIDEBAR_W_MIN: f32 = 120.0;
pub const SIDEBAR_W_MAX: f32 = 420.0;

#[derive(Debug, Clone)]
pub enum Msg {
    NewFrame,
    NavBack,
    NavForward,
    NavReload,
    UrlInput(String),
    UrlSubmit,
    CloseTab(TabId),
    ActivateTab(TabId),
    Tick,
    Bus(Arc<sola_bus::Message>),
    DividerPress,
    CursorMoved(f32),
    CursorReleased,
    TabHover(Option<usize>),
}
```
> `sola-bus` is already a dependency (Step 1). `CHROME_HEIGHT`/`SIDEBAR_W_*` are unused until Task 2's `view`; add `#[allow(dead_code)]` on the consts if the compiler warns, or accept the warning until Task 2 (it resolves there).

- [ ] **Step 5: Write `lib.rs`**

`crates/sola-browser-core/src/lib.rs`:
```rust
//! Shared iced chrome for the Sola browsers, generic over `Engine`.
pub mod app;
pub mod engine;
pub mod util;
// Added in Task 2:
// pub mod input;
// pub mod integration;
// pub mod run;

pub use engine::{
    ActiveHandle, Cmd, CursorHandle, Engine, FrameReceiver, FrameSlot, InputEvent, NavCmd, TabId,
    TabInfo, TabsHandle, TaggedFrame,
};
pub use app::Msg;
// pub use run::run;   // re-enable in Task 2
```
> `input`/`integration`/`run` arrive in Task 2; their `mod`/`pub use` lines stay commented until then so the crate builds with `app`+`engine`+`util`.

- [ ] **Step 6: Build + test**

Run: `cargo build -p sola-browser-core` (library typecheck) then `cargo test -p sola-browser-core`
Expected: compiles; 3 tests pass.

- [ ] **Step 7: Commit**
```bash
git add crates/sola-browser-core
git commit -m "feat(sola-browser-core): scaffold shared types + Engine trait"
```

---

## Task 2: Move the generic chrome into `sola-browser-core`

Relocates the chrome that is identical across the two crates into core, made generic over `E: Engine`. The key transformation is eliminating the three process-wide `OnceLock` statics (`ENGINE`, `SLOT_FOR_STREAM`, `ACTIVE_TAB_FOR_STREAM`) — illegal as `static OnceLock<E>` — by having `App<E>` own `engine: E` and build the frame subscription from owned `Arc`s.

**Files:**
- Modify: `crates/sola-browser-core/src/app.rs` (add `App<E>` + methods; `Msg`/consts already present from Task 1)
- Create: `crates/sola-browser-core/src/integration.rs`
- Create: `crates/sola-browser-core/src/input.rs`
- Create: `crates/sola-browser-core/src/run.rs`
- Modify: `crates/sola-browser-core/src/lib.rs` (re-enable `input`/`integration`/`run`)

**Interfaces:**
- Consumes: `Engine`, `Cmd`, `FrameSlot`, `TabId`, `TabInfo`, `app::Msg`, the chrome consts (all from Task 1).
- Produces: `App<E>` (fields below) + `App::<E>::new(...)`, `run::<E>(app_id: &'static str) -> ExitCode`, `run::frame_stream::<E>(...)`.

- [ ] **Step 1: Create `input.rs` (verbatim move)**

Move `sola-browser-wpe/src/input.rs` into `crates/sola-browser-core/src/input.rs` verbatim, changing only its imports to pull `InputEvent` etc. from `crate::engine` instead of `crate::wpe`/`sola_browser_wpe::wpe`. This file maps iced events → `InputEvent`; it has no engine dependency. (The CEF copy is discarded; reconcile any divergence by keeping the WPE version, which is the primary.)

- [ ] **Step 2: Create `integration.rs` (verbatim move, generic app)**

Move `sola-browser-wpe/src/integration.rs` into `crates/sola-browser-core/src/integration.rs`. Change its imports to `crate::*`. Wherever it names the concrete `App`, make the relevant functions generic: `pub fn handle_bus<E: Engine>(app: &mut App<E>, message: &Arc<Message>) -> Task<Msg>` and `pub fn run_intent<E: Engine>(app: &mut App<E>, intent: BrowserIntent) -> Task<Msg>`. `SUBSCRIBE`, `MENU_ITEMS`, `intent_for_open_url` are non-generic consts/fns and move unchanged. (Verified earlier: this file is byte-identical between the two crates after engine-name normalization.)

- [ ] **Step 3: Extend `app.rs` — add `App<E>` + methods**

`Msg` and the consts are already in `app.rs` (Task 1) — do **not** re-add them. Move the `App` struct and `impl App` (`active_tab_info`, `update`, `open_tab`, `theme`, `pick_new_active_after_close`, `view`, `view_tab_sidebar`, `view_nav_bar`, `subscription`) from `sola-browser-wpe/src/main.rs` into `crates/sola-browser-core/src/app.rs`, and add the `App::<E>::new(...)` constructor. Apply these exact changes:

1. `struct App` → `struct App<E: Engine>` and add a field `pub engine: E,`. Change `slot: Arc<FrameSlot>` → `slot: Arc<FrameSlot<E>>`. Keep existing fields (`releaser` now `Sender<Cmd<E::Token>>`, `tabs_handle`, `active_handle`, `cached_tabs`, `cached_active`, `url_field`, `last_seen_url`, `theme`, `sidebar_w`, `dragging_divider`, `last_cursor_x`, `drag_anchor`, `hovered_tab`).
2. `impl App` → `impl<E: Engine> App<E>`.
3. In `open_tab`, replace any `ENGINE.get().expect(...).alloc_tab_id()` with `self.engine.alloc_tab_id()`.
4. In `view`, replace `Shader::new(WpeProgram { slot: self.slot.clone() })` with `Shader::new(E::make_program(self.slot.clone()))`.
5. Replace `subscription` body to drop the static-based `Subscription::run(frame_stream)` and build the frame stream from owned `Arc`s:
```rust
fn subscription(&self) -> Subscription<Msg> {
    let frames = self.engine.frames();
    let slot = self.slot.clone();
    let active = self.active_handle.clone();
    Subscription::batch(vec![
        Subscription::run_with_id("web-frames", crate::run::frame_stream::<E>(frames, slot, active)),
        iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
        sola_kit::app::bus_subscription().map(Msg::Bus),
        event::listen_with(|event, _, _| match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => Some(Msg::CursorMoved(position.x)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Msg::CursorReleased),
            _ => None,
        }),
    ])
}
```
6. `view_tab_sidebar`, `view_nav_bar`, `update`, `pick_new_active_after_close`, `active_tab_info`, `theme` move unchanged except `Msg`/types now resolve to `crate::*`. Imports for kit widgets (`vertical_tabs`, `toolbar_button`, `horizontal_divider`, `vertical_divider`, `TabDescriptor`) move unchanged.

- [ ] **Step 4: Create `run.rs` — `frame_stream` + `run::<E>`**

`crates/sola-browser-core/src/run.rs`. `frame_stream` is the existing `main.rs` `frame_stream` body, parameterised by owned `Arc`s instead of statics:
```rust
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use iced::futures::{SinkExt, Stream};
use iced::stream;

use crate::app::{App, Msg, DEFAULT_URL, VIEW_H, VIEW_W};
use crate::engine::{ActiveHandle, Engine, FrameSlot, TaggedFrame};

pub fn frame_stream<E: Engine>(
    frames: Arc<Mutex<Receiver<TaggedFrame<E::Frame>>>>,
    slot: Arc<FrameSlot<E>>,
    active: ActiveHandle,
) -> impl Stream<Item = Msg> {
    stream::channel(64, async move |mut output| {
        loop {
            let tagged = match tokio::task::spawn_blocking({
                let frames = frames.clone();
                move || frames.lock().unwrap().recv().ok()
            })
            .await
            {
                Ok(Some(f)) => f,
                _ => break,
            };
            if tagged.tab_id.0 != active.load(Ordering::Relaxed) {
                continue;
            }
            *slot.pending.lock().unwrap() = Some(tagged.frame);
            if output.send(Msg::NewFrame).await.is_err() {
                break;
            }
        }
    })
}

pub fn run<E: Engine>(app_id: &'static str) -> ExitCode {
    if let Some(code) = E::dispatch_subprocess(app_id) {
        return code;
    }
    sola_core::log::init(app_id);
    tracing::info!("{app_id} starting");
    let _ = sola_core::env::activate_wayland_session(10_000);

    let url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_string());
    tracing::info!(%url, "loading url");
    let engine = E::spawn(app_id, &url, VIEW_W, VIEW_H);

    let releaser = engine.cmd_sender();
    let tabs_handle = engine.tabs_handle();
    let active_handle = engine.active_tab_handle();
    let cursor = engine.cursor_handle();

    let slot = Arc::new(FrameSlot::<E> {
        pending: Mutex::new(None),
        releaser: releaser.clone(),
        last_size: Mutex::new((VIEW_W, VIEW_H)),
        cursor,
    });

    sola_kit::app::BusSetup::new(app_id)
        .subscribe(crate::integration::SUBSCRIBE)
        .app_menu("Browser", crate::integration::MENU_ITEMS)
        .install();

    let result = iced::application(
        move || App::<E>::new(engine, slot.clone(), releaser.clone(), tabs_handle.clone(), active_handle.clone(), url.clone()),
        App::<E>::update,
        App::<E>::view,
    )
    .title(|app: &App<E>| match app.active_tab_info() {
        Some(t) if !t.title.is_empty() => format!("{app_id} — {}", t.title),
        Some(t) if !t.url.is_empty() => format!("{app_id} — {}", t.url),
        _ => app_id.to_string(),
    })
    .subscription(App::<E>::subscription)
    .theme(App::<E>::theme)
    .default_font(sola_kit::fonts::ui())
    .window(iced::window::Settings {
        decorations: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: app_id.to_string(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    })
    .run();

    if let Err(e) = result {
        tracing::error!("iced::application returned: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
```
Add a constructor `App::<E>::new(engine, slot, releaser, tabs_handle, active_handle, url)` to `app.rs` that fills the struct (cached_tabs empty, cached_active from `active_handle.load`, url_field/last_seen_url from `url`, theme default, sidebar defaults, drag fields `None`/`false`). This replaces the inline `App { .. }` literal the old `main` used. Note `engine` is *moved into* the App (keeps the worker alive for the program's life — no static needed).

- [ ] **Step 5: Re-enable modules in `lib.rs`** (uncomment `pub mod input;`, `pub mod integration;`, `pub mod run;`, and `pub use run::run;` — `app`/`engine`/`util` are already enabled from Task 1).

- [ ] **Step 6: Typecheck the generic library**

Run: `cargo build -p sola-browser-core`
Expected: compiles. (Generic code typechecks against the `Engine` bound with no concrete engine present.) Fix any leftover references to the deleted statics or `WpeProgram`/`sola_browser_wpe::*` paths.

- [ ] **Step 7: Commit**
```bash
git add crates/sola-browser-core
git commit -m "feat(sola-browser-core): generic chrome (App<E>, run, integration, input)"
```

---

## Task 3: Port `sola-browser-wpe` onto the core

Reduce the WPE crate to its engine body + frame import + a one-line `main`, implementing `Engine` for `WpeEngine`. Build clean; user smokes.

**Files:**
- Modify: `crates/sola-browser-wpe/Cargo.toml` (add core dep)
- Replace: `crates/sola-browser-wpe/src/main.rs` (one-liner)
- Rename/Modify: `crates/sola-browser-wpe/src/wpe.rs` → `src/engine.rs`
- Rename/Modify: `crates/sola-browser-wpe/src/shader.rs` (+ fold `wgpu_import.rs`) → `src/frame.rs`
- Modify: `crates/sola-browser-wpe/src/lib.rs`

**Interfaces:**
- Consumes: `sola_browser_core::{Engine, Cmd, FrameSlot, TabId, TabInfo, run, app::Msg, ...}`.
- Produces: `WpeEngine: Engine<Frame = WpeFrame, Token = ResourceToken, Program = WpeProgram>`.

- [ ] **Step 1: Add the core dependency**

`crates/sola-browser-wpe/Cargo.toml` `[dependencies]`: add `sola-browser-core = { path = "../sola-browser-core" }`. Remove the now-shared `iced`/`sola-kit` direct deps only if nothing in the gutted crate still uses them directly — the engine body still uses `wgpu`/`wgpu-hal`/`ash`/`iced` (for the `shader::Program` impl), so keep those.

- [ ] **Step 2: Move `wpe.rs` → `engine.rs`; delete the now-shared types**

Rename `wpe.rs` to `engine.rs`. Delete the type definitions now living in core (`Cmd`, `NavCmd`, `InputEvent`, `TabId`, `TabInfo`, `TaggedFrame`) and import them from `sola_browser_core`. Keep `WpeFrame`, `ResourceToken`, `WpeEngine`, `WorkerCtx`, `TabState`, `TabSignalCtx` and all `worker_main`/`process_cmd`/`apply_resize`/etc. functions. Change `Cmd` usages to `Cmd<ResourceToken>`.

- [ ] **Step 3: Implement `Engine` for `WpeEngine`**

Replace the inherent `impl WpeEngine` accessor methods with an `impl Engine for WpeEngine` carrying the same bodies, plus move the env dance from the old `main` into `spawn`:
```rust
impl sola_browser_core::Engine for WpeEngine {
    type Frame = WpeFrame;
    type Token = ResourceToken;
    type Program = crate::frame::WpeProgram;

    fn spawn(_app_id: &'static str, url: &str, w: u32, h: u32) -> Self {
        // Moved verbatim from the old main(): set WEBKIT_EXEC_PATH, hide
        // WAYLAND_DISPLAY across engine bring-up, restore it after.
        unsafe { std::env::set_var("WEBKIT_EXEC_PATH", env!("WEBKIT_EXEC_PATH")) };
        let saved = std::env::var("WAYLAND_DISPLAY").ok();
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
        let engine = WpeEngine::spawn_inner(url, w, h); // the old `spawn` body, renamed
        if let Some(d) = saved {
            unsafe { std::env::set_var("WAYLAND_DISPLAY", d) };
        }
        engine
    }
    fn alloc_tab_id(&self) -> sola_browser_core::TabId { /* old body */ }
    fn cmd_sender(&self) -> std::sync::mpsc::Sender<sola_browser_core::Cmd<ResourceToken>> { self.cmd_tx.clone() }
    fn tabs_handle(&self) -> sola_browser_core::TabsHandle { self.tabs.clone() }
    fn active_tab_handle(&self) -> sola_browser_core::ActiveHandle { self.active_tab.clone() }
    fn cursor_handle(&self) -> sola_browser_core::CursorHandle { self.cursor.clone() }
    fn frames(&self) -> sola_browser_core::FrameReceiver<WpeFrame> { self.frames.clone() }
    fn make_program(slot: std::sync::Arc<sola_browser_core::FrameSlot<Self>>) -> Self::Program {
        crate::frame::WpeProgram { slot }
    }
    fn shutdown(self) { /* old body */ }
}
```
Rename the existing public `spawn(url, w, h)` to a private `spawn_inner(url, w, h)`. (`dispatch_subprocess` uses the trait default — WPE has no typed subprocesses.)

- [ ] **Step 4: Move `shader.rs` (+ `wgpu_import.rs`) → `frame.rs`**

Rename `shader.rs` to `frame.rs`. Change `FrameSlot` references to `sola_browser_core::FrameSlot<WpeEngine>`. Change `WpeProgram`'s `shader::Program<Msg>` impl to `shader::Program<sola_browser_core::app::Msg>`. The `WpePrimitive::prepare` resize-feedback already sends `Cmd::Resize` via `slot.releaser` — keep verbatim (now `Cmd<ResourceToken>`). Keep `wgpu_import.rs` as a sibling module or inline it into `frame.rs`; either is fine — pick the smaller diff.

- [ ] **Step 5: Replace `main.rs` and `lib.rs`**

`crates/sola-browser-wpe/src/main.rs`:
```rust
//! sola-browser-wpe — WPE engine over the shared sola-browser-core chrome.
use sola_browser_wpe::engine::WpeEngine;

fn main() -> std::process::ExitCode {
    sola_browser_core::run::<WpeEngine>("sola-browser")
}
```
`crates/sola-browser-wpe/src/lib.rs`: expose `pub mod engine; pub mod frame;` plus the existing FFI modules (`wpe_sys`, etc.). Remove any `mod integration;` / chrome modules now in core.

- [ ] **Step 6: Build**

Run: `cargo make build sola-browser-wpe`
Expected: compiles clean, zero warnings. Then verify linkage is unchanged (still no libcef):
`patchelf --print-needed target/debug/sola-browser-wpe | grep -iE 'wpe|cef'` → only `libWPEWebKit-2.0.so.1`.

- [ ] **Step 7: Commit + write the user smoke checklist**
```bash
git add crates/sola-browser-wpe
git commit -m "refactor(sola-browser-wpe): thin bin over sola-browser-core"
```
Smoke (user, after `cargo make install sola-browser-wpe` with permission): launch, open a tab, switch tabs, drag the divider, close a tab, confirm theme follows `Topic::Theme`.

---

## Task 4: Port `sola-browser-cef` onto the core

Same shape as Task 3 for CEF; additionally override `dispatch_subprocess`.

**Files:**
- Modify: `crates/sola-browser-cef/Cargo.toml` (add core dep)
- Replace: `crates/sola-browser-cef/src/main.rs` (one-liner)
- Rename/Modify: `crates/sola-browser-cef/src/cef.rs` → `src/engine.rs`
- Rename/Modify: `crates/sola-browser-cef/src/shader.rs` (+ fold `cpu_import.rs`) → `src/frame.rs`
- Modify: `crates/sola-browser-cef/src/lib.rs`

**Interfaces:**
- Produces: `CefEngine: Engine<Frame = CefFrame, Token = <cef token>, Program = CefProgram>` with `dispatch_subprocess` overridden.

- [ ] **Step 1: Add core dep** (mirror Task 3 Step 1 in `sola-browser-cef/Cargo.toml`; keep the `cef`, `wgpu*`, `ash`, `libc` deps).

- [ ] **Step 2: `cef.rs` → `engine.rs`; delete shared types, import from core** (mirror Task 3 Step 2; keep `CefFrame`, `CefEngine`, `CefThreadState`, `CefTabState`, worker fns, `initialize_cef`). The CEF token type used in `Cmd::Release` becomes `Self::Token`.

- [ ] **Step 3: Implement `Engine` for `CefEngine`** with the same 7 methods, `make_program` returning `crate::frame::CefProgram`, **plus** override the subprocess gate (move the existing associated fn body verbatim):
```rust
fn dispatch_subprocess(app_id: &'static str) -> Option<std::process::ExitCode> {
    CefEngine::dispatch_subprocess_inner(app_id) // existing body (cef::execute_process)
}
fn spawn(app_id: &'static str, url: &str, w: u32, h: u32) -> Self {
    CefEngine::spawn_inner(app_id, url, w, h) // existing body; browser_subprocess_path = current_exe()
}
```
(Keep `browser_subprocess_path = current_exe()` — after the dispatcher `exec`s this binary, `current_exe()` is `sola-browser-cef`, so `--type=` workers re-exec correctly.)

- [ ] **Step 4: `shader.rs` (+ `cpu_import.rs`) → `frame.rs`** (mirror Task 3 Step 4; `FrameSlot<CefEngine>`, `shader::Program<sola_browser_core::app::Msg>`).

- [ ] **Step 5: Replace `main.rs` + `lib.rs`**

`crates/sola-browser-cef/src/main.rs`:
```rust
//! sola-browser-cef — CEF engine over the shared sola-browser-core chrome.
use sola_browser_cef::engine::CefEngine;

fn main() -> std::process::ExitCode {
    sola_browser_core::run::<CefEngine>("sola-browser")
}
```

- [ ] **Step 6: Build + linkage check**

Run: `cargo make build sola-browser-cef`
Expected: clean. `patchelf --print-needed target/debug/sola-browser-cef | grep -iE 'wpe|cef'` → only `libcef.so` (no `libWPEWebKit`).

- [ ] **Step 7: Commit + smoke checklist**
```bash
git add crates/sola-browser-cef
git commit -m "refactor(sola-browser-cef): thin bin over sola-browser-core"
```
Smoke (user): same checklist as Task 3, plus confirm CEF subprocesses spawn (no `--type=` crash) and a page renders.

---

## Task 5: `sola-browser` dispatcher

A dependency-light bin that selects an engine and `execv`s the sibling binary. Pure selection/arg logic is unit-tested; the `exec` itself is the thin shell around it.

**Files:**
- Create: `crates/sola-browser/Cargo.toml`
- Create: `crates/sola-browser/src/main.rs`
- Create: `crates/sola-browser/dist/applications/sola-browser.desktop`

**Interfaces:**
- Produces: binary `sola-browser`; pure fns `pick_engine(args, env) -> &'static str`, `passthrough(args) -> Vec<OsString>`, `resolve_target(dir, engine) -> Option<PathBuf>`.

- [ ] **Step 1: Manifest**

`crates/sola-browser/Cargo.toml`:
```toml
[package]
name = "sola-browser"
version = "0.1.0"
edition = "2024"

# Featherweight engine dispatcher. Depends on NO web engine — it only
# selects sola-browser-{wpe,cef} and execs it. This is what keeps the
# 1.34 GB libcef out of the WPE launch path.

[[bin]]
name = "sola-browser"
path = "src/main.rs"

[dependencies]
sola-core = { path = "../sola-core" }
```

- [ ] **Step 2: Write failing tests for the pure logic**

`crates/sola-browser/src/main.rs` (tests first):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn s(v: &[&str]) -> Vec<OsString> { v.iter().map(OsString::from).collect() }

    #[test]
    fn default_engine_is_wpe() {
        assert_eq!(pick_engine(&s(&[]), None), "wpe");
    }
    #[test]
    fn flag_selects_cef() {
        assert_eq!(pick_engine(&s(&["--engine", "cef"]), None), "cef");
    }
    #[test]
    fn flag_eq_form_selects_cef() {
        assert_eq!(pick_engine(&s(&["--engine=cef"]), None), "cef");
    }
    #[test]
    fn env_selects_when_no_flag() {
        assert_eq!(pick_engine(&s(&[]), Some("cef".into())), "cef");
    }
    #[test]
    fn flag_overrides_env() {
        assert_eq!(pick_engine(&s(&["--engine", "wpe"]), Some("cef".into())), "wpe");
    }
    #[test]
    fn unknown_engine_falls_back_to_wpe() {
        assert_eq!(pick_engine(&s(&["--engine", "lynx"]), None), "wpe");
    }
    #[test]
    fn passthrough_strips_engine_flag_keeps_url() {
        assert_eq!(passthrough(&s(&["--engine", "cef", "https://x.test"])), s(&["https://x.test"]));
    }
    #[test]
    fn passthrough_strips_eq_form() {
        assert_eq!(passthrough(&s(&["--engine=cef", "--app", "https://x.test"])), s(&["--app", "https://x.test"]));
    }
}
```

- [ ] **Step 3: Run tests — expect failure**

Run: `cargo test -p sola-browser`
Expected: FAIL (functions not defined).

- [ ] **Step 4: Implement**

```rust
//! sola-browser — selects an engine and execs sola-browser-{wpe,cef}.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const ENGINES: [&str; 2] = ["wpe", "cef"];
const DEFAULT_ENGINE: &str = "wpe";

/// Resolve the engine name from `--engine <x>` / `--engine=x`, then
/// `$SOLA_BROWSER_ENGINE`, else the default. Unknown names fall back to default.
fn pick_engine(args: &[OsString], env: Option<String>) -> &'static str {
    let mut chosen: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let a = a.to_string_lossy();
        if let Some(v) = a.strip_prefix("--engine=") {
            chosen = Some(v.to_string());
        } else if a == "--engine" {
            if let Some(v) = it.next() {
                chosen = Some(v.to_string_lossy().to_string());
            }
        }
    }
    let want = chosen.or(env).unwrap_or_else(|| DEFAULT_ENGINE.to_string());
    ENGINES.into_iter().find(|e| *e == want).unwrap_or(DEFAULT_ENGINE)
}

/// Args to forward to the engine binary: everything except `--engine`/value.
fn passthrough(args: &[OsString]) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let s = a.to_string_lossy();
        if s == "--engine" {
            let _ = it.next(); // drop its value
        } else if s.starts_with("--engine=") {
            // drop
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// Path to `sola-browser-<engine>` next to this dispatcher; falls back to
/// the other engine if the requested one is missing.
fn resolve_target(dir: &Path, engine: &str) -> Option<PathBuf> {
    let primary = dir.join(format!("sola-browser-{engine}"));
    if primary.exists() {
        return Some(primary);
    }
    let other = if engine == "wpe" { "cef" } else { "wpe" };
    let fallback = dir.join(format!("sola-browser-{other}"));
    fallback.exists().then_some(fallback)
}

fn main() -> ExitCode {
    use std::os::unix::process::CommandExt;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let engine = pick_engine(&args, std::env::var("SOLA_BROWSER_ENGINE").ok());
    let dir = match std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        Some(d) => d,
        None => {
            eprintln!("sola-browser: cannot resolve own directory");
            return ExitCode::FAILURE;
        }
    };
    let Some(target) = resolve_target(&dir, engine) else {
        eprintln!("sola-browser: no engine binary found in {}", dir.display());
        return ExitCode::FAILURE;
    };
    let err = std::process::Command::new(&target).args(passthrough(&args)).exec();
    eprintln!("sola-browser: exec {} failed: {err}", target.display());
    ExitCode::FAILURE
}
```

- [ ] **Step 5: Run tests — expect pass**

Run: `cargo test -p sola-browser`
Expected: 8 tests pass.

- [ ] **Step 6: `.desktop` handler**

`crates/sola-browser/dist/applications/sola-browser.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Sola Browser
Exec=/opt/sola/bin/sola-browser %u
Terminal=false
Categories=Network;WebBrowser;
MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;
```

- [ ] **Step 7: Build the workspace (confirms membership pickup)**

Run: `cargo make build sola-browser`
Expected: clean. (Auto-included via `members = ["crates/*"]`.)

- [ ] **Step 8: Commit**
```bash
git add crates/sola-browser
git commit -m "feat(sola-browser): engine dispatcher + .desktop handler"
```

---

## Task 6: Launcher entries, stale-ref cleanup, install verification

Unify the user-facing identity and fix the orphaned references the dispatcher now satisfies.

**Files:**
- Modify: `crates/sola-shell/src/builtins.rs`
- Modify: `crates/solactl/src/open.rs` (doc comment)
- Modify: `crates/sola-make/src/install.rs` (doc comment)

- [ ] **Step 1: Update the launcher entries**

In `crates/sola-shell/src/builtins.rs::builtin_apps`, replace the two browser entries (`Browser (WPE)` / `Browser (CEF)`) with one default entry plus an explicit CEF entry, both under `app_id = "sola-browser"` (the unified id both engines now report):
```rust
Application {
    // One Browser; the dispatcher picks the engine. Both engines report
    // app_id "sola-browser", so the shell associates either window here.
    app_id: "sola-browser".into(),
    label: "Browser".into(),
    command: "/opt/sola/bin/sola-browser".into(),
    icon: "lucide/globe".into(),
},
Application {
    app_id: "sola-browser".into(),
    label: "Browser (CEF)".into(),
    command: "/opt/sola/bin/sola-browser --engine cef".into(),
    icon: "lucide/earth".into(),
},
```
> Verify the launcher splits `command` on whitespace into argv (so `--engine cef` is passed as an argument). If it execs the whole string as one path, change the CEF entry to rely on `SOLA_BROWSER_ENGINE` or a dedicated arg-aware launch — check `sola-shell`'s app-launch path (`launcher`/`session`) before relying on the space form.

- [ ] **Step 2: Fix stale doc references**

- `crates/solactl/src/open.rs` line ~5: the comment referencing `crates/sola-browser/dist/applications/sola-browser.desktop` is now accurate — adjust wording from "retired/none" framing to point at the real path.
- `crates/sola-make/src/install.rs` line ~43: same — the example `crates/sola-browser/dist/applications/foo.desktop` is now a live crate; update the comment to name `sola-browser.desktop` if helpful.

(Use `replace_content` for these comment-only edits.)

- [ ] **Step 3: Build the shell + whole workspace**

Run: `cargo make build sola-shell` then `cargo make build`
Expected: both clean, zero warnings.

- [ ] **Step 4: Verify install discovery (no install performed)**

Confirm `sola-make` will install the dispatcher binary and the `.desktop`:
- `sola-browser` is a workspace binary → picked up by `discover_binaries()` (used by `install`).
- `crates/sola-browser/dist/applications/sola-browser.desktop` → picked up by `install_dist_files()`.

Inspect (read-only): `grep -n 'discover_binaries\|install_dist_files' crates/sola-make/src/install.rs` and confirm the dispatcher and dist path fall in scope. Do **not** run `cargo make install`.

- [ ] **Step 5: Commit**
```bash
git add crates/sola-shell crates/solactl crates/sola-make
git commit -m "feat(sola-shell): unified Browser launcher entry; fix sola-browser refs"
```

- [ ] **Step 6: Final user verification (with explicit install permission)**

Hand off to the user: `cargo make install` (or the three browser targets + shell), then:
1. Launcher "Browser" opens the WPE engine; window `app_id` is `sola-browser` (`solactl apps`).
2. "Browser (CEF)" opens the CEF engine.
3. `SOLA_BROWSER_ENGINE=cef /opt/sola/bin/sola-browser` opens CEF; `--engine wpe` overrides it.
4. `xdg-open https://example.com` (or the `.desktop` handler) routes through `sola-browser`.
5. Removing/renaming the CEF binary makes `--engine cef` fall back to WPE with a warning.

---

## Self-Review

**Spec coverage:** Three crates (Tasks 1–2 core, 3 WPE, 4 CEF, 5 dispatcher) ✓; `Engine` trait (Task 1) ✓; `exec` dispatcher with flag/env/fallback (Task 5) ✓; app_id unification (Tasks 3/4 set it, Task 6 wires launcher) ✓; `.desktop` + stale-ref fix (Tasks 5/6) ✓; `sola-make` membership/dist (Task 5 Step 7, Task 6 Step 4) ✓; engine isolation preserved (linkage checks in Tasks 3/4) ✓.

**Known deferral (call out to the user before starting):** the per-engine shader `Program`/`Primitive`/`Pipeline` (~500 lines each) stays in its engine bin in this plan — only `FrameSlot`/types/chrome are shared. The spec's "render WGSL + resize-feedback → core" is a *follow-on*: factoring the common Program behind a `FrameImport` trait (engine provides `import(frame) -> texture`; core owns prepare/render/WGSL). It is the riskiest piece (generic wgpu) and is separable; do it as a 7th task only after Tasks 1–6 land and both engines smoke clean. Flagged here so "shared chrome" is not mistaken for "shared renderer."

**Type consistency:** `Cmd` is generic (`Cmd<Tok>`) end-to-end — `FrameSlot.releaser: Sender<Cmd<E::Token>>`, engine `cmd_sender() -> Sender<Cmd<Self::Token>>`, `Engine::Token` associated type. `FrameSlot<E: Engine>` (carries `E::Frame`) is used identically in `app.rs`, `run.rs`, and both `frame.rs`. `app_id` is `&'static str` from `main` → `run` → `BusSetup`/window settings. Handle aliases (`TabsHandle`/`ActiveHandle`/`CursorHandle`/`FrameReceiver`) match the verified `Arc<...>` return types.

**Placeholder scan:** code shown for all new surfaces; bulk relocations specify exact source ranges + the precise signature/generic deltas; verbatim transcription of `truncate`/`normalize_url`/`spawn`/`dispatch_subprocess` bodies is called out where I do not reproduce them inline (they move unchanged).
