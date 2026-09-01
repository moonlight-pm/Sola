# sola-app Trait-Based API — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace sola-app's builder API with a trait-based struct API where apps implement `SolaApp` and windows are first-class (any count, declared via `ctx.add_window`).

**Architecture:** One trait with default no-op methods (`SolaApp`), an effect handle (`AppCtx`) passed to every method, and `WindowHandle` values held as struct fields. `AppRuntime<A>` wraps `{app, ctx}` in `Rc<RefCell<_>>` so GTK/bus callbacks share disjoint `&mut` borrows via destructuring. New `AsyncDispatcher` replaces the old `AppHandler` plumbing for apps that need async command handling.

**Tech Stack:** Rust, GTK4, WebKit6, Smithay bus protocol, tokio (for async dispatch only).

**Scope:** sola-app crate + apps/terminal + apps/shell. sola-browser is NOT touched.

---

## File Structure

```
crates/sola-app/src/
  lib.rs              # SolaApp trait, run::<A>(), AppRuntime, module wiring
  ctx.rs              # AppCtx (effect handle: bus, gtk_app, windows)
  window.rs           # WindowConfig, WindowHandle, WindowInner
  async_dispatch.rs   # AppHandler trait, AsyncDispatcher (replaces old AppHandler plumbing)
  bridge.rs           # tokio → glib bridging for async replies (kept, refactored)
  webview.rs          # per-window webview/UCM helpers (refactored)
  assets.rs           # unchanged
  config.rs           # unchanged (JsonConfig utility)
  watcher.rs          # unchanged (binary self-watch)
  strip.rs            # unchanged

apps/shell/src/
  main.rs             # entry: sola_app::run::<ShellApp>()
  app.rs              # struct ShellApp + impl SolaApp (replaces state.rs)
  menu/               # stays; setup_menu_panel removed, menu.html becomes AssetBundle
  switcher/           # stays; setup_switcher_panel removed, overlay becomes AssetBundle
  keys.rs             # key controller, now attached to menubar WindowHandle
  zoning.rs           # unchanged
  util.rs             # unchanged (eval_js may be removed if superseded by WindowHandle::eval_js)

apps/terminal/src/
  main.rs             # struct TerminalApp + impl SolaApp + sola_app::run::<TerminalApp>()
  (other modules unchanged)
```

---

## Task 1: Rename old `SolaApp` struct to `SolaAppBuilder`

Frees the `SolaApp` name for the new trait. Intermediate state — both old builder API and new trait API will coexist until Task 11.

**Files:**
- Modify: `crates/sola-app/src/lib.rs`
- Modify: `apps/shell/src/main.rs`
- Modify: `apps/terminal/src/main.rs`

- [ ] **Step 1: Rename struct in `crates/sola-app/src/lib.rs`**

Replace `pub struct SolaApp { ... }` with `pub struct SolaAppBuilder { ... }` and `impl SolaApp { pub fn builder() -> Self { ... } ... }` with `impl SolaAppBuilder { pub fn new() -> Self { ... } ... }`. Drop the `builder()` factory — callers will use `SolaAppBuilder::new()`.

Also add `impl Default for SolaAppBuilder { fn default() -> Self { Self::new() } }`.

- [ ] **Step 2: Update `apps/shell/src/main.rs`**

Replace `use sola_app::{asset_bundle, SolaApp};` with `use sola_app::{asset_bundle, SolaAppBuilder};` and `SolaApp::builder()` with `SolaAppBuilder::new()`.

- [ ] **Step 3: Update `apps/terminal/src/main.rs`**

Same replacement as shell.

- [ ] **Step 4: Verify**

```bash
cd /home/joshua/Workspace/Sola/.worktrees/sola-shell
cargo check --workspace
```

Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-app/src/lib.rs apps/shell/src/main.rs apps/terminal/src/main.rs
git commit -m "$(cat <<'EOF'
refactor(sola-app): rename SolaApp struct to SolaAppBuilder

Frees the SolaApp name for an upcoming trait that replaces the builder
API. Behavior unchanged.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `WindowConfig`, `WindowHandle`, `WindowInner` types

Pure types, no GTK logic. Types are dead code at this point — that's expected.

**Files:**
- Create: `crates/sola-app/src/window.rs`
- Modify: `crates/sola-app/src/lib.rs` (add `pub mod window;`)

- [ ] **Step 1: Create `crates/sola-app/src/window.rs`**

```rust
use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use crate::assets::AssetBundle;

/// Declarative window configuration. Passed to `AppCtx::add_window`.
pub struct WindowConfig {
    pub title: String,
    pub size: (i32, i32),
    pub position: Option<(i32, i32)>,
    pub decorated: bool,
    pub transparent: bool,
    pub assets: &'static AssetBundle,
    pub initial_state: Option<String>,
    // WindowPolicy fields — auto-emitted to bus by sola-app after A::new
    pub zoned: bool,
    pub keyboard_target: bool,
}

/// Dispatcher installed per window by sola-app's runtime after A::new.
/// Converts UCM script messages into SolaApp::on_js_command calls.
pub type JsDispatcher = Box<dyn FnMut(&str, &Value)>;

/// Internal per-window state owned by sola-app.
pub(crate) struct WindowInner {
    pub(crate) title: String,
    pub(crate) webview: webkit6::WebView,
    pub(crate) gtk_window: gtk4::ApplicationWindow,
    pub(crate) dispatcher: RefCell<Option<JsDispatcher>>,
    pub(crate) zoned: bool,
    pub(crate) keyboard_target: bool,
    pub(crate) size: (i32, i32),
    pub(crate) position: Option<(i32, i32)>,
}

/// Cheap-clone handle to a window created via `AppCtx::add_window`.
#[derive(Clone)]
pub struct WindowHandle {
    pub(crate) inner: Rc<WindowInner>,
}

impl WindowHandle {
    pub fn title(&self) -> &str {
        &self.inner.title
    }

    pub fn eval_js(&self, script: &str) {
        use gio::Cancellable;
        use webkit6::prelude::WebViewExt;
        self.inner.webview.evaluate_javascript(
            script,
            None,
            None,
            None::<&Cancellable>,
            |_| {},
        );
    }

    pub fn send_to_js(&self, value: &Value) {
        let json = serde_json::to_string(value).unwrap_or_default();
        let script = format!("window.__solaRecv({json})");
        self.eval_js(&script);
    }

    pub(crate) fn gtk_window(&self) -> &gtk4::ApplicationWindow {
        &self.inner.gtk_window
    }
}

impl PartialEq for WindowHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for WindowHandle {}
```

- [ ] **Step 2: Add module to `crates/sola-app/src/lib.rs`**

Add near the other `pub mod` declarations (at the top of the file): `pub mod window;` and re-export at the bottom near `pub use assets::...`:

```rust
pub use window::{WindowConfig, WindowHandle};
```

- [ ] **Step 3: Verify**

```bash
cargo check --workspace
```

Expected: clean build, maybe a dead-code warning on `WindowInner` (acceptable — it's `pub(crate)` and will be used in Task 3).

- [ ] **Step 4: Commit**

```bash
git add crates/sola-app/src/window.rs crates/sola-app/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): add WindowConfig, WindowHandle, WindowInner types

Pure type definitions for the upcoming trait-based API. Not yet used.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Add `AppCtx` skeleton

Structure + `add_window` implementation that creates a GTK window + WebView + UCM per window.

**Files:**
- Create: `crates/sola-app/src/ctx.rs`
- Modify: `crates/sola-app/src/lib.rs` (add `pub mod ctx;`)
- Modify: `crates/sola-app/src/webview.rs` (add per-window helper)

- [ ] **Step 1: Add per-window UCM helper to `crates/sola-app/src/webview.rs`**

Add this function alongside the existing `create_content_manager` functions (do not remove the existing ones — they're still used by the old builder API):

```rust
/// Create a UserContentManager that forwards JSON messages to a shared slot.
/// The slot is expected to be filled after A::new returns.
pub(crate) fn create_ucm_for_window(
    dispatcher_slot: std::rc::Rc<std::cell::RefCell<Option<crate::window::JsDispatcher>>>,
) -> webkit6::UserContentManager {
    use webkit6::prelude::*;
    let ucm = webkit6::UserContentManager::new();
    ucm.register_script_message_handler("sola", None);
    ucm.connect_script_message_received(Some("sola"), move |_, value| {
        let js_value = match value.to_string() {
            s if !s.is_empty() => s,
            _ => return,
        };
        let parsed: serde_json::Value = match serde_json::from_str(&js_value) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("invalid JS command JSON: {e}");
                return;
            }
        };
        let cmd = parsed.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let args = parsed.get("args").cloned().unwrap_or(serde_json::json!({}));
        if let Some(dispatcher) = dispatcher_slot.borrow_mut().as_mut() {
            dispatcher(cmd, &args);
        } else {
            tracing::warn!(cmd, "JS command received before dispatcher installed");
        }
    });
    ucm
}
```

- [ ] **Step 2: Create `crates/sola-app/src/ctx.rs`**

```rust
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use webkit6::prelude::*;

use sola_bus::BusClient;
use sola_bus::topics::Topic;

use crate::assets;
use crate::webview;
use crate::window::{JsDispatcher, WindowConfig, WindowHandle, WindowInner};

/// Effect handle passed to every `SolaApp` trait method.
pub struct AppCtx {
    pub(crate) bus: Rc<RefCell<BusClient>>,
    pub(crate) gtk_app: gtk4::Application,
    pub(crate) windows: Vec<WindowHandle>,
    pub(crate) app_id: &'static str,
}

impl AppCtx {
    pub(crate) fn new(
        bus: Rc<RefCell<BusClient>>,
        gtk_app: gtk4::Application,
        app_id: &'static str,
    ) -> Self {
        Self { bus, gtk_app, windows: Vec::new(), app_id }
    }

    /// Create a new window. The returned handle can be stored as a field
    /// and used later to eval JS, send messages, etc.
    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle {
        let platform = Box::leak(Box::new(assets::platform_assets()));
        let html_raw = cfg
            .assets
            .find("/index.html")
            .map(|a| a.content.to_string())
            .unwrap_or_else(|| "<html><body>No index.html</body></html>".to_string());

        let html = if let Some(state_json) = cfg.initial_state.as_ref() {
            html_raw.replace("__RESTORED_STATE__", state_json)
        } else {
            html_raw
        };
        let html = crate::inject_import_map(&html);

        let web_context = webview::create_web_context(cfg.assets, platform, html);

        let dispatcher_slot: Rc<RefCell<Option<JsDispatcher>>> =
            Rc::new(RefCell::new(None));
        let ucm = webview::create_ucm_for_window(dispatcher_slot.clone());

        if cfg.transparent {
            let css = gtk4::CssProvider::new();
            css.load_from_data("window, window.background { background: transparent; }");
            gtk4::style_context_add_provider_for_display(
                &gdk4::Display::default().unwrap(),
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let gtk_window = gtk4::ApplicationWindow::new(&self.gtk_app);
        gtk_window.set_decorated(cfg.decorated);
        gtk_window.set_default_size(cfg.size.0, cfg.size.1);
        gtk_window.set_title(Some(&cfg.title));

        let webview = webkit6::WebView::builder()
            .web_context(&web_context)
            .user_content_manager(&ucm)
            .build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_developer_extras(true);
            settings.set_enable_write_console_messages_to_stdout(true);
        }
        webview.connect_context_menu(|_, _, _| true);
        if cfg.transparent {
            webview.set_background_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.0));
        }
        gtk_window.set_child(Some(&webview));
        webview.load_uri("app:///index.html");

        let inner = WindowInner {
            title: cfg.title,
            webview,
            gtk_window: gtk_window.clone(),
            dispatcher: RefCell::new(None),
            zoned: cfg.zoned,
            keyboard_target: cfg.keyboard_target,
            size: cfg.size,
            position: cfg.position,
        };

        // Attach dispatcher_slot to the inner so run<A>() can fill it after A::new.
        inner.dispatcher.swap(&RefCell::new(
            std::mem::replace(&mut *dispatcher_slot.borrow_mut(), None),
        ));
        // Actually: we want the SAME RefCell the UCM writes to. Swap doesn't do that.
        // See: keep dispatcher_slot as the source of truth; stash the Rc in WindowInner
        // so run<A>() can write into it and the UCM still reads from it.
        // Re-design: WindowInner.dispatcher stores the SHARED Rc, not its own RefCell.

        gtk_window.present();

        let handle = WindowHandle { inner: Rc::new(inner) };
        self.windows.push(handle.clone());
        handle
    }

    /// Remove a window. Closes its GTK window.
    pub fn remove_window(&mut self, handle: &WindowHandle) {
        handle.inner.gtk_window.close();
        self.windows.retain(|w| w != handle);
    }

    pub fn emit(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit(topic);
    }

    pub fn emit_sticky(&self, topic: Topic) {
        let _ = self.bus.borrow_mut().emit_sticky(topic);
    }
}
```

**Note the design flaw flagged in the comment above.** Fix it in the next step.

- [ ] **Step 3: Fix dispatcher slot sharing**

The UCM handler needs to read from the *same* RefCell that `run<A>()` writes into after `A::new`. The quick fix: change `WindowInner.dispatcher` to be the shared `Rc<RefCell<Option<JsDispatcher>>>`, not an owned one.

Edit `crates/sola-app/src/window.rs`:

Replace the field in `WindowInner`:
```rust
pub(crate) dispatcher: RefCell<Option<JsDispatcher>>,
```
with:
```rust
pub(crate) dispatcher: Rc<RefCell<Option<JsDispatcher>>>,
```

Then in `crates/sola-app/src/ctx.rs`, inside `add_window`, replace the `WindowInner` construction to use the shared slot directly:

```rust
let inner = WindowInner {
    title: cfg.title,
    webview,
    gtk_window: gtk_window.clone(),
    dispatcher: dispatcher_slot,  // shared with UCM
    zoned: cfg.zoned,
    keyboard_target: cfg.keyboard_target,
    size: cfg.size,
    position: cfg.position,
};
```

And remove the buggy swap block.

- [ ] **Step 4: Wire module and exports in `crates/sola-app/src/lib.rs`**

Add `pub mod ctx;` near other `pub mod` declarations. Add re-export near `pub use window::...`:

```rust
pub use ctx::AppCtx;
```

Also make `inject_import_map` `pub(crate)` instead of private (it's called from ctx.rs).

- [ ] **Step 5: Verify**

```bash
cargo check --workspace
```

Expected: clean build. Dead-code warnings on the new types are acceptable.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-app/src/ctx.rs crates/sola-app/src/window.rs crates/sola-app/src/lib.rs crates/sola-app/src/webview.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): add AppCtx with add_window implementation

Creates GTK window + WebView + UCM per call. UCM writes to a shared
dispatcher slot that run::<A>() will populate after A::new returns.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Define `SolaApp` trait and `run::<A>()`

Introduces the trait and the `run::<A>()` function. Bus wiring and dispatcher installation come in Task 5.

**Files:**
- Modify: `crates/sola-app/src/lib.rs`

- [ ] **Step 1: Add trait + AppRuntime + run function to `crates/sola-app/src/lib.rs`**

Near the top (after the existing `pub mod` declarations and re-exports, but before the old `SolaAppBuilder` struct), add:

```rust
use serde_json::Value;
use sola_bus::topics::Topic;

/// Trait implemented by every Sola app. Apps opt in to the methods
/// they need; unoverridden methods are no-ops.
pub trait SolaApp: 'static {
    const APP_ID: &'static str;

    fn new(ctx: &mut AppCtx) -> Self
    where
        Self: Sized;

    fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let _ = (topic, ctx);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        let _ = (cmd, args, source, ctx);
    }

    fn on_shutdown(&mut self, ctx: &mut AppCtx) {
        let _ = ctx;
    }
}

pub(crate) struct AppRuntime<A: SolaApp> {
    pub(crate) app: A,
    pub(crate) ctx: AppCtx,
}

/// Entry point. Bootstraps logging, Wayland wait, GTK, bus, and
/// drives the event loop.
pub fn run<A: SolaApp>() {
    let app_id = A::APP_ID;

    // --- Logging ---
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("{}=info", app_id.replace('-', "_")).into());

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("{app_id} starting");

    // --- Binary self-watch ---
    crate::watcher::watch_own_binary();

    // --- Wayland socket wait ---
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", "wayland-0") };
    }
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap();
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR must be set");
    let socket_path = std::path::PathBuf::from(&runtime_dir).join(&wayland_display);
    for attempt in 1..=20 {
        if socket_path.exists() {
            tracing::info!(path = %socket_path.display(), "wayland socket ready");
            break;
        }
        if attempt == 20 {
            tracing::error!(path = %socket_path.display(), "wayland socket not found after 10s");
            std::process::exit(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
    unsafe { std::env::set_var("GTK_A11Y", "none") };

    glib::set_prgname(Some(app_id));

    let gtk_app = gtk4::Application::new(None::<&str>, Default::default());

    gtk_app.connect_activate(move |gtk_app| {
        use std::cell::RefCell;
        use std::rc::Rc;

        // --- Bus ---
        let bus = Rc::new(RefCell::new(sola_bus::BusClient::new()));
        {
            let mut c = bus.borrow_mut();
            c.set_app_id(app_id);
            if let Err(e) = c.connect() {
                tracing::warn!("bus not available: {e}");
            }
        }

        // --- Build AppCtx, run A::new ---
        let mut ctx = AppCtx::new(bus.clone(), gtk_app.clone(), app_id);
        let app = A::new(&mut ctx);

        // --- Wrap runtime ---
        let runtime = Rc::new(RefCell::new(AppRuntime { app, ctx }));

        // (Remaining wiring — JS dispatchers, bus event loop, auto-emit policy
        // — added in Tasks 5–7.)

        tracing::info!("{app_id} ready");
    });

    gtk_app.run();
}
```

Make sure these imports are available at the top of `lib.rs`:
```rust
use serde_json::Value;
use sola_bus::topics::Topic;
```

(They may already be there; if `Topic` is in scope, fine; if a duplicate import appears, remove.)

- [ ] **Step 2: Verify**

```bash
cargo check --workspace
```

Expected: clean build. Unused-`app` warning is fine — it's consumed in Task 5.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-app/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): add SolaApp trait and run::<A>() skeleton

Trait with default no-op methods; run() bootstraps tracing + Wayland
socket wait + GTK + bus and calls A::new. JS dispatch, bus event loop,
and WindowPolicy auto-emit come next.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Wire JS dispatchers

After `A::new` returns, every window has a `dispatcher` slot that's `None`. Install dispatcher closures into each slot so JS commands route to `app.on_js_command`.

**Files:**
- Modify: `crates/sola-app/src/lib.rs`

- [ ] **Step 1: Replace the `TODO (Task 5/6/7)` block in `run::<A>()`**

Inside `gtk_app.connect_activate`, after `let runtime = Rc::new(...)`, add:

```rust
// --- Install per-window JS dispatchers ---
let window_handles: Vec<WindowHandle> = runtime.borrow().ctx.windows.clone();
for source in window_handles {
    let runtime_weak = Rc::downgrade(&runtime);
    let source_for_dispatch = source.clone();
    let dispatcher: crate::window::JsDispatcher = Box::new(move |cmd: &str, args: &Value| {
        let Some(runtime) = runtime_weak.upgrade() else { return };
        let mut rt = runtime.borrow_mut();
        let AppRuntime { app, ctx } = &mut *rt;
        app.on_js_command(cmd, args, &source_for_dispatch, ctx);
    });
    *source.inner.dispatcher.borrow_mut() = Some(dispatcher);
}
```

Weak reference to runtime avoids a cycle (windows → dispatcher → runtime → windows).

- [ ] **Step 2: Verify**

```bash
cargo check --workspace
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-app/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): wire per-window JS dispatchers after A::new

Each window's UCM message handler now routes through the runtime to
app.on_js_command(cmd, args, &source, ctx). Uses a weak reference to
break the window→dispatcher→runtime→window cycle.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Wire bus event loop + Shutdown handling

Register a GLib FD source on the bus notify fd. On each event, dispatch to `app.on_bus_event`. Intercept `Topic::Shutdown` before user code to guarantee clean exit.

**Files:**
- Modify: `crates/sola-app/src/lib.rs`

- [ ] **Step 1: Append to `run::<A>()`'s `connect_activate` closure**

After the dispatcher-install loop from Task 5, add:

```rust
// --- Bus event loop ---
let notify_fd = bus.borrow().notify_fd();
if let Some(fd) = notify_fd {
    let runtime = runtime.clone();
    let gtk_app = gtk_app.clone();
    let bus = bus.clone();
    glib::unix_fd_add_local(fd, glib::IOCondition::IN, move |_fd, _cond| {
        let client = bus.borrow();
        client.drain_notify();
        let mut messages = Vec::new();
        while let Some(msg) = client.try_recv() {
            messages.push(msg);
        }
        drop(client);

        for msg in messages {
            let Some(topic) = Topic::parse(&msg) else { continue };
            if matches!(topic, Topic::Shutdown) {
                {
                    let mut rt = runtime.borrow_mut();
                    let AppRuntime { app, ctx } = &mut *rt;
                    app.on_shutdown(ctx);
                }
                gtk_app.quit();
                return glib::ControlFlow::Continue;
            }
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            app.on_bus_event(&topic, ctx);
        }
        glib::ControlFlow::Continue
    });
} else {
    tracing::warn!("bus notify_fd unavailable; no bus events will be delivered");
}
```

- [ ] **Step 2: Verify**

```bash
cargo check --workspace
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/sola-app/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): wire bus event loop and Shutdown interception

Topic::Shutdown is caught before user code: calls app.on_shutdown(ctx),
then gtk_app.quit(). All other topics dispatch to app.on_bus_event.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Auto-emit `SetWindowPolicy`

After `A::new` returns and before installing JS dispatchers, convert each `WindowInner` to a `WindowPolicy` entry and emit one `Topic::SetWindowPolicy` sticky.

**Files:**
- Modify: `crates/sola-app/src/ctx.rs`
- Modify: `crates/sola-app/src/lib.rs`

- [ ] **Step 1: Add helper to `AppCtx` in `crates/sola-app/src/ctx.rs`**

Import at top:
```rust
use sola_bus::topics::{WindowPolicy, WindowPolicyPayload};
```

Add method:
```rust
pub(crate) fn emit_window_policy(&self) {
    let windows: Vec<WindowPolicy> = self
        .windows
        .iter()
        .map(|h| WindowPolicy {
            title: h.inner.title.clone(),
            zoned: h.inner.zoned,
            keyboard_target: h.inner.keyboard_target,
            size: Some(h.inner.size),
            position: h.inner.position,
        })
        .collect();
    self.emit_sticky(Topic::SetWindowPolicy(WindowPolicyPayload {
        app_id: self.app_id.to_string(),
        windows,
    }));
}
```

- [ ] **Step 2: Call it from `run::<A>()`**

In `crates/sola-app/src/lib.rs`, between the `A::new(&mut ctx)` call and `let runtime = Rc::new(...)`, add:

```rust
ctx.emit_window_policy();
```

Note: this must happen *before* ctx is moved into `AppRuntime`.

- [ ] **Step 3: Verify**

```bash
cargo check --workspace
```

Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-app/src/ctx.rs crates/sola-app/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): auto-emit SetWindowPolicy after A::new

Windows collected during A::new are converted to WindowPolicy entries
and emitted as one sticky SetWindowPolicy. Removes boilerplate that
every app would otherwise have to write.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: `AsyncDispatcher`

New module replacing the old `AppHandler`-in-lib.rs plumbing. Apps that need async command handling (terminal) construct one in `new`, forward from `on_js_command`, and send replies via `source.send_to_js`.

**Files:**
- Create: `crates/sola-app/src/async_dispatch.rs`
- Modify: `crates/sola-app/src/lib.rs` (add `pub mod async_dispatch;`)

- [ ] **Step 1: Create `crates/sola-app/src/async_dispatch.rs`**

```rust
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc as std_mpsc, Arc};
use std::time::Duration;

use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;

#[async_trait::async_trait]
pub trait AppHandler: Send + Sync + 'static {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value;
}

struct AsyncCmd {
    id: u64,
    cmd: String,
    args: Value,
}

type ReplyCallback = Box<dyn FnOnce(Value)>;

pub struct AsyncDispatcher {
    cmd_tx: tokio_mpsc::UnboundedSender<AsyncCmd>,
    pending: Rc<RefCell<HashMap<u64, ReplyCallback>>>,
    next_id: Rc<Cell<u64>>,
}

impl AsyncDispatcher {
    pub fn spawn<H: AppHandler>(handler: H) -> Self {
        let (cmd_tx, mut cmd_rx) = tokio_mpsc::unbounded_channel::<AsyncCmd>();
        let (reply_tx, reply_rx) = std_mpsc::channel::<(u64, Value)>();

        std::thread::spawn(move || {
            let rt = Runtime::new().expect("failed to create tokio runtime for AsyncDispatcher");
            rt.block_on(async move {
                let handler = Arc::new(handler);
                while let Some(AsyncCmd { id, cmd, args }) = cmd_rx.recv().await {
                    let handler = handler.clone();
                    let reply_tx = reply_tx.clone();
                    tokio::spawn(async move {
                        let result = handler.dispatch(&cmd, &args).await;
                        let _ = reply_tx.send((id, result));
                    });
                }
            });
        });

        let pending: Rc<RefCell<HashMap<u64, ReplyCallback>>> =
            Rc::new(RefCell::new(HashMap::new()));

        // Bridge reply channel to main loop with a 5ms poll (same pattern
        // as bridge.rs). Replies invoke the stored callback on main thread.
        let pending_for_bridge = pending.clone();
        glib::timeout_add_local(Duration::from_millis(5), move || {
            while let Ok((id, result)) = reply_rx.try_recv() {
                if let Some(cb) = pending_for_bridge.borrow_mut().remove(&id) {
                    cb(result);
                }
            }
            glib::ControlFlow::Continue
        });

        Self {
            cmd_tx,
            pending,
            next_id: Rc::new(Cell::new(0)),
        }
    }

    /// Dispatch a command. `reply` is invoked on the main thread with the
    /// handler's return value.
    pub fn dispatch(
        &self,
        cmd: String,
        args: Value,
        reply: impl FnOnce(Value) + 'static,
    ) {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.pending.borrow_mut().insert(id, Box::new(reply));
        if self.cmd_tx.send(AsyncCmd { id, cmd, args }).is_err() {
            // Runtime thread died — remove pending entry to avoid leak.
            self.pending.borrow_mut().remove(&id);
            tracing::error!("AsyncDispatcher runtime thread is dead");
        }
    }
}
```

- [ ] **Step 2: Add module + re-export in `crates/sola-app/src/lib.rs`**

Near other `pub mod` lines:
```rust
pub mod async_dispatch;
```

Re-export:
```rust
pub use async_dispatch::{AppHandler, AsyncDispatcher};
```

**Check for name collision:** the old `SolaAppBuilder` also has an `AppHandler` trait and its `handler()` method takes a closure-of-that-trait. To avoid collision during transition, **rename the old trait** (inside `lib.rs`) to `LegacyAppHandler`. Also rename any references inside the old `SolaAppBuilder` impl block. This is temporary — the old code is deleted in Task 11.

- [ ] **Step 3: Verify**

```bash
cargo check --workspace
```

Expected: clean build. Possible dead-code warnings on `AsyncDispatcher` — acceptable.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-app/src/async_dispatch.rs crates/sola-app/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(sola-app): add AsyncDispatcher for async command handling

Spawns a tokio runtime thread; dispatch() ships commands over an
unbounded channel and stores per-call reply callbacks keyed by id.
Replies return via a std mpsc channel bridged to the main loop so
callbacks run on the GTK thread.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Migrate `sola-terminal` to the new API

Terminal uses async handlers heavily (tmux commands), so this is the test case for `AsyncDispatcher`.

**Files:**
- Modify: `apps/terminal/src/main.rs`

- [ ] **Step 1: Rewrite `apps/terminal/src/main.rs`**

Read the current file first, then rewrite:

```bash
cat apps/terminal/src/main.rs
```

The new `main.rs` should:

```rust
use std::sync::Arc;

use serde_json::Value;
use sola_app::{asset_bundle, AppCtx, AppHandler, AsyncDispatcher, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic,
};

mod commands;
mod pty;
mod state;
mod tmux;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/terminal-pane.ts" => (include_str!("../web/src/terminal-pane.ts"), TypeScript),
    "/src/components/sidebar.ts" => (include_str!("../web/src/components/sidebar.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
    "/vendor/xterm.mjs" => (include_str!("../web/vendor/xterm.mjs"), JavaScript),
    "/vendor/xterm.css" => (include_str!("../web/vendor/xterm.css"), Css),
    "/vendor/addon-fit.mjs" => (include_str!("../web/vendor/addon-fit.mjs"), JavaScript),
    "/vendor/addon-web-links.mjs" => (include_str!("../web/vendor/addon-web-links.mjs"), JavaScript),
};

struct TerminalApp {
    main_window: WindowHandle,
    dispatcher: AsyncDispatcher,
    state: Arc<state::TerminalState>,
}

impl SolaApp for TerminalApp {
    const APP_ID: &'static str = "sola-terminal";

    fn new(ctx: &mut AppCtx) -> Self {
        tmux::cleanup_stale_socket();
        tmux::kill_orphaned_clients();
        tmux::reload_config();

        let restored_tabs = state::TerminalState::load_from_disk();
        let restored_json = serde_json::to_string(&restored_tabs).unwrap_or_default();

        let terminal_state = Arc::new(state::TerminalState::new());
        {
            let mut titles = terminal_state.custom_titles.try_write().unwrap();
            for tab in &restored_tabs {
                if let Some(ref title) = tab.custom_title {
                    titles.insert(tab.tmux_session.clone(), title.clone());
                }
            }
        }

        let main_window = ctx.add_window(WindowConfig {
            title: "terminal".into(),
            size: (1920, 1080),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(restored_json),
            zoned: true,
            keyboard_target: true,
        });

        // AsyncDispatcher needs an event sender for the handler.
        // Old design had event_tx for "send events to JS". Wire that via
        // a channel that lands in main_window.send_to_js.
        let (event_tx, mut event_rx) = std::sync::mpsc::channel::<String>();
        let mw_for_events = main_window.clone();
        // Poll the event channel on the main loop and forward to JS.
        glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(json_str) = event_rx.try_recv() {
                mw_for_events.eval_js(&format!(
                    "window.__solaRecv({json_str})"
                ));
            }
            glib::ControlFlow::Continue
        });

        let handler = commands::TerminalHandler {
            state: terminal_state.clone(),
            event_tx,
        };
        let dispatcher = AsyncDispatcher::spawn(handler);

        Self {
            main_window,
            dispatcher,
            state: terminal_state,
        }
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        _source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        let source = self.main_window.clone();
        let id = args.get("id").and_then(|v| v.as_u64());
        // Re-extract just the inner args payload that TerminalHandler expects
        let payload_args = args.get("args").cloned().unwrap_or(serde_json::json!({}));
        self.dispatcher
            .dispatch(cmd.to_string(), payload_args, move |result| {
                if let Some(id) = id {
                    source.send_to_js(&serde_json::json!({ "id": id, "result": result }));
                }
            });
    }

    fn on_bus_event(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic {
            if app_id != Self::APP_ID {
                return;
            }
            // Forward menu action into JS as an event (matches old behavior).
            self.main_window.send_to_js(&serde_json::json!({
                "type": "menu_action",
                "action_id": action_id,
            }));
        }
    }
}

fn main() {
    sola_app::run::<TerminalApp>();
}
```

**Notes:**
- The `_source` parameter is `&WindowHandle` for the window that sent the JS message. Terminal only has one window so we ignore it.
- Event_tx wiring is preserved from the old code — TerminalHandler pushes events via the mpsc; a glib timeout polls and forwards to the main window's JS.
- The existing logic for setting AppMenuPayload at startup (if any) might live inside `commands.rs` or elsewhere — preserve it if found. Check `apps/terminal/src/main.rs`'s current bus events & on_activate closures and port any missing logic.

**Verify the above replicates existing behavior** by reading `apps/terminal/src/main.rs` before writing and comparing:
- All `.on_activate` logic → move to `new`
- All `.on_bus_event` logic → move to `on_bus_event`
- `.handler(|tx| TerminalHandler { ... })` → `AsyncDispatcher::spawn(TerminalHandler { ... })`
- `.initial_state(&restored_json)` → `initial_state: Some(restored_json)` in WindowConfig

- [ ] **Step 2: Check for logic drift**

Look at the removed fragment:
```bash
git show HEAD:apps/terminal/src/main.rs | head -200
```
and make sure `new` captures everything the old `on_activate` did (menu registration, event poller, etc.).

- [ ] **Step 3: Verify**

```bash
cargo check -p sola-terminal
```

Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add apps/terminal/src/main.rs
git commit -m "$(cat <<'EOF'
refactor(terminal): migrate to SolaApp trait API

Replaces the SolaAppBuilder chain with struct TerminalApp + impl SolaApp.
Async commands now flow through AsyncDispatcher; window configuration
moves from builder methods to a single ctx.add_window call.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Migrate `sola-shell` to the new API

Shell has 3 windows (menubar, switcher, menu). This exercises multi-window end-to-end.

**Files:**
- Modify: `apps/shell/src/main.rs`
- Delete and recreate: `apps/shell/src/state.rs` → becomes `apps/shell/src/app.rs`
- Modify: `apps/shell/src/menu/panel.rs` (remove setup, keep open/close)
- Modify: `apps/shell/src/switcher/panel.rs` (remove setup)
- Modify: `apps/shell/src/keys.rs` (attach to menubar handle)
- Delete: `apps/shell/src/util.rs` (eval_js superseded by WindowHandle::eval_js)
- Create asset bundles: `apps/shell/src/menu/assets.rs`, `apps/shell/src/switcher/assets.rs`
- Possibly add: `apps/shell/web/src/menu.ts`, `apps/shell/web/src/overlay-impl.ts`

- [ ] **Step 1: Build per-window AssetBundles**

Shell's menubar already has an AssetBundle (`MENUBAR_ASSETS`). Switcher and menu currently use raw HTML with inline scripts. Convert them:

**a)** In `apps/shell/src/switcher/panel.rs`, the current code does `OVERLAY_HTML.replace("__OVERLAY_JS__", OVERLAY_JS)`. Replace that with an asset bundle: `apps/shell/web/overlay.html` needs a normal `<script type="module" src="/src/overlay.ts"></script>` tag (or equivalent). Rename `apps/shell/web/src/overlay.ts` to match whatever the script tag references.

Create `apps/shell/src/switcher/assets.rs`:
```rust
use sola_app::asset_bundle;

pub static SWITCHER_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/overlay.html"), Html),
    "/src/overlay.ts" => (include_str!("../../web/src/overlay.ts"), TypeScript),
};
```

Export from `apps/shell/src/switcher/mod.rs`:
```rust
pub mod assets;
pub use assets::SWITCHER_ASSETS;
```

Update `apps/shell/web/overlay.html` to reference `/src/overlay.ts` via a script tag (remove the `__OVERLAY_JS__` placeholder mechanism).

**b)** For menu: `apps/shell/web/menu.html` currently has inline JS. Extract to `apps/shell/web/src/menu.ts` (keeping the logic identical). Update `menu.html` to reference it via `<script type="module" src="/src/menu.ts"></script>`.

Create `apps/shell/src/menu/assets.rs`:
```rust
use sola_app::asset_bundle;

pub static MENU_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/menu.html"), Html),
    "/src/menu.ts" => (include_str!("../../web/src/menu.ts"), TypeScript),
};
```

Export from `apps/shell/src/menu/mod.rs`:
```rust
pub mod assets;
pub use assets::MENU_ASSETS;
```

**c)** For the menu, the current code uses `connect_notify_local("title", ...)` to capture JS→Rust messages (dismiss, action). Replace with proper `sola.send()`:

In the extracted `apps/shell/web/src/menu.ts`, wherever the old code set `document.title = "dismiss"` or `document.title = "action:<app_id>:<action_id>"`, replace with `sola.send("dismiss", {})` and `sola.send("action", { app_id, action_id })` respectively. (Import `sola` from `@sola/ipc`.)

- [ ] **Step 2: Rewrite shell as `ShellApp` struct**

Delete `apps/shell/src/state.rs`, create `apps/shell/src/app.rs` with:

```rust
use std::collections::HashSet;

use serde_json::Value;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    App, AppMenuPayload, CompositionEntry, FocusTarget, FrameUpdate, MenuDefinition,
    MenuItem, Topic,
};

use crate::menu::{self, state::MenuCache, MENU_ASSETS};
use crate::switcher::{self, state::SwitcherState, SWITCHER_ASSETS};
use crate::zoning::{self, ZoningState};

pub static MENUBAR_ASSETS: &sola_app::AssetBundle = &sola_app::asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/menubar.ts" => (include_str!("../web/src/menubar.ts"), TypeScript),
};

pub struct ShellApp {
    pub focused_app_id: Option<String>,
    pub mru_apps: Vec<String>,
    pub known_apps: Vec<App>,
    pub menus: MenuCache,
    pub zoning: ZoningState,
    pub switcher: SwitcherState,
    pub menu_open: bool,

    pub menubar: WindowHandle,
    pub switcher_win: WindowHandle,
    pub menu_win: WindowHandle,
}

impl SolaApp for ShellApp {
    const APP_ID: &'static str = "sola-shell";

    fn new(ctx: &mut AppCtx) -> Self {
        let menubar = ctx.add_window(WindowConfig {
            title: "menubar".into(),
            size: (1920, zoning::MENUBAR_HEIGHT),
            position: Some((0, 0)),
            decorated: false,
            transparent: true,
            assets: MENUBAR_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: true,
        });

        let switcher_win = ctx.add_window(WindowConfig {
            title: "switcher".into(),
            size: (800, 400),
            position: Some((560, 340)),
            decorated: false,
            transparent: true,
            assets: SWITCHER_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: false,
        });

        let menu_win = ctx.add_window(WindowConfig {
            title: "menu".into(),
            size: (220, 300),
            position: Some((0, zoning::MENUBAR_HEIGHT)),
            decorated: false,
            transparent: true,
            assets: MENU_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: false,
        });

        let mut menus = MenuCache::new();
        // Register system menu.
        menus.set_menu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![MenuDefinition {
                label: "Sola".into(),
                items: vec![MenuItem::Action {
                    id: "exit".into(),
                    label: "Exit Sola".into(),
                    shortcut: Some("Super+Shift+Backspace".into()),
                    disabled: false,
                    checked: false,
                }],
            }],
        });

        // Attach key controller to menubar's GTK window.
        crate::keys::setup_key_controller_on(menubar.clone());

        Self {
            focused_app_id: None,
            mru_apps: Vec::new(),
            known_apps: Vec::new(),
            menus,
            zoning: ZoningState::new(),
            switcher: SwitcherState::default(),
            menu_open: false,
            menubar,
            switcher_win,
            menu_win,
        }
    }

    fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        match topic {
            Topic::Apps(apps) => {
                self.handle_apps_update(apps.clone(), ctx);
                if self.switcher.active {
                    let json = serde_json::to_string(&self.switcher.apps).unwrap_or_default();
                    self.switcher_win.eval_js(&format!(
                        "renderSwitcher({}, {})",
                        json, self.switcher.selected
                    ));
                }
            }
            Topic::SetAppMenu(payload) => {
                self.menus.set_menu(payload.clone());
                if self.focused_app_id.as_deref() == Some(&payload.app_id) {
                    let app_name = payload
                        .menus
                        .first()
                        .map(|d| d.label.as_str())
                        .unwrap_or(&payload.app_id);
                    let menu_labels: Vec<String> =
                        payload.menus.iter().map(|d| d.label.clone()).collect();
                    self.menubar.send_to_js(&serde_json::json!({
                        "event": "focus",
                        "app_name": app_name,
                        "menu_labels": menu_labels,
                    }));
                }
            }
            Topic::OutputGeometry(geo) => {
                self.zoning.set_output_size(geo);
                self.emit_all_frames(ctx);
                self.emit_composition(ctx);
            }
            _ => {}
        }
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        match (source.title(), cmd) {
            ("menubar", "open_menu") => {
                let src = args.get("source").and_then(|v| v.as_str()).unwrap_or("app");
                let index = args.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let anchor_x = args.get("anchor_x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                self.open_menu(src, index, anchor_x, ctx);
            }
            ("menubar", "close_menu") => self.close_menu(ctx),
            ("menu", "dismiss") => self.close_menu(ctx),
            ("menu", "action") => {
                let app_id = args.get("app_id").and_then(|v| v.as_str()).unwrap_or("");
                let action_id = args.get("action_id").and_then(|v| v.as_str()).unwrap_or("");
                tracing::info!(app_id, action_id, "menu action");
                if app_id == Self::APP_ID && action_id == "exit" {
                    ctx.emit(Topic::Shutdown);
                } else {
                    ctx.emit(Topic::MenuAction(sola_bus::topics::MenuActionPayload {
                        app_id: app_id.to_string(),
                        action_id: action_id.to_string(),
                    }));
                }
                self.close_menu(ctx);
            }
            _ => {}
        }
    }
}

// All the methods that used to be on ShellState (set_focus,
// rebuild_switcher_apps, emit_composition, emit_all_frames,
// handle_apps_update) move here as `impl ShellApp` methods.
// Signatures change: `emit: &dyn Fn(Topic)` → `ctx: &mut AppCtx`
// and calls like `emit(Topic::Frame(...))` → `ctx.emit(Topic::Frame(...))`.

impl ShellApp {
    pub fn set_focus(&mut self, app_id: &str) {
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());

        if self.menu_open {
            self.menu_open = false;
            self.menu_win.eval_js("clearMenu()");
        }

        let menu = self.menus.get_menu(app_id);
        let app_name = menu
            .and_then(|m| m.menus.first())
            .map(|d| d.label.as_str())
            .unwrap_or(app_id);
        let menu_labels: Vec<String> = menu
            .map(|m| m.menus.iter().map(|d| d.label.clone()).collect())
            .unwrap_or_default();

        self.menubar.send_to_js(&serde_json::json!({
            "event": "focus",
            "app_name": app_name,
            "menu_labels": menu_labels,
        }));
    }

    pub fn rebuild_switcher_apps(&self) -> Vec<App> {
        let mut apps: Vec<App> = self
            .mru_apps
            .iter()
            .filter_map(|id| self.known_apps.iter().find(|a| &a.app_id == id))
            .cloned()
            .collect();
        for a in &self.known_apps {
            if a.app_id != Self::APP_ID && !self.mru_apps.contains(&a.app_id) {
                apps.push(a.clone());
            }
        }
        apps
    }

    pub fn emit_composition(&self, ctx: &mut AppCtx) {
        let mut entries = Vec::new();
        entries.push(CompositionEntry {
            app_id: Self::APP_ID.into(),
            title: Some("menubar".into()),
        });
        for app_id in self.mru_apps.iter().rev() {
            if app_id == Self::APP_ID {
                continue;
            }
            entries.push(CompositionEntry {
                app_id: app_id.clone(),
                title: None,
            });
        }
        for app in &self.known_apps {
            if app.app_id == Self::APP_ID {
                continue;
            }
            if !self.mru_apps.contains(&app.app_id) {
                entries.push(CompositionEntry {
                    app_id: app.app_id.clone(),
                    title: None,
                });
            }
        }
        if self.menu_open {
            entries.push(CompositionEntry {
                app_id: Self::APP_ID.into(),
                title: Some("menu".into()),
            });
        }
        if self.switcher.active {
            entries.push(CompositionEntry {
                app_id: Self::APP_ID.into(),
                title: Some("switcher".into()),
            });
        }
        ctx.emit(Topic::Composition(entries));
    }

    pub fn emit_all_frames(&self, ctx: &mut AppCtx) {
        if let Some(frame) = self.zoning.menubar_frame() {
            ctx.emit(Topic::Frame(frame));
        }
        for app in &self.known_apps {
            if app.app_id == Self::APP_ID {
                continue;
            }
            if let Some(frame) = self.zoning.app_frame(&app.app_id) {
                ctx.emit(Topic::Frame(frame));
            }
        }
    }

    pub fn handle_apps_update(&mut self, apps: Vec<App>, ctx: &mut AppCtx) {
        let old_ids: HashSet<&str> = self.known_apps.iter().map(|a| a.app_id.as_str()).collect();
        let new_ids: HashSet<&str> = apps.iter().map(|a| a.app_id.as_str()).collect();

        let added: Vec<String> = apps
            .iter()
            .filter(|a| !old_ids.contains(a.app_id.as_str()) && a.app_id != Self::APP_ID)
            .map(|a| a.app_id.clone())
            .collect();

        let removed: Vec<String> = self
            .known_apps
            .iter()
            .filter(|a| !new_ids.contains(a.app_id.as_str()) && a.app_id != Self::APP_ID)
            .map(|a| a.app_id.clone())
            .collect();

        self.known_apps = apps.clone();
        self.switcher.apps = apps
            .into_iter()
            .filter(|a| a.app_id != Self::APP_ID)
            .collect();

        for id in &removed {
            self.mru_apps.retain(|m| m != id);
        }

        for id in &added {
            if let Some(frame) = self.zoning.app_frame(id) {
                ctx.emit(Topic::Frame(frame));
            }
        }

        self.emit_composition(ctx);

        if let Some(id) = added.first() {
            self.set_focus(id);
            ctx.emit(Topic::Focus(FocusTarget {
                app_id: id.clone(),
                title: None,
            }));
            self.emit_composition(ctx);
        }
    }

    pub fn open_menu(&mut self, source: &str, menu_index: usize, anchor_x: f64, ctx: &mut AppCtx) {
        let app_id = if source == "system" {
            Self::APP_ID.to_string()
        } else {
            self.focused_app_id.clone().unwrap_or_default()
        };

        let menu = self.menus.get_menu(&app_id);
        let menu_def = menu.and_then(|m| m.menus.get(menu_index));
        let Some(menu_def) = menu_def else { return };

        let items: Vec<Value> = menu_def
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action {
                    id,
                    label,
                    shortcut,
                    disabled,
                    ..
                } => serde_json::json!({
                    "type": "action",
                    "id": id,
                    "app_id": app_id,
                    "label": label,
                    "shortcut": shortcut,
                    "disabled": disabled,
                }),
                MenuItem::Divider => serde_json::json!({ "type": "divider" }),
            })
            .collect();

        let json = serde_json::to_string(&items).unwrap_or_default();
        self.menu_win
            .eval_js(&format!("showMenu({}, {})", json, anchor_x));

        if let Some((ow, oh)) = self.zoning.output_size {
            ctx.emit(Topic::Frame(FrameUpdate {
                app_id: Self::APP_ID.into(),
                title: Some("menu".into()),
                x: 0,
                y: zoning::MENUBAR_HEIGHT,
                width: ow,
                height: oh - zoning::MENUBAR_HEIGHT,
            }));
        }

        self.menu_open = true;
        self.emit_composition(ctx);
    }

    pub fn close_menu(&mut self, ctx: &mut AppCtx) {
        if !self.menu_open {
            return;
        }
        self.menu_open = false;
        self.menu_win.eval_js("clearMenu()");
        self.menubar
            .send_to_js(&serde_json::json!({"event": "close_menu"}));
        self.emit_composition(ctx);
    }
}
```

- [ ] **Step 3: Replace `apps/shell/src/main.rs`**

```rust
mod app;
mod keys;
mod menu;
mod switcher;
mod zoning;

fn main() {
    sola_app::run::<app::ShellApp>();
}
```

- [ ] **Step 4: Delete obsolete files**

```bash
rm apps/shell/src/state.rs apps/shell/src/util.rs
```

- [ ] **Step 5: Strip panel files of the GTK setup logic**

**`apps/shell/src/menu/panel.rs`** — delete everything except the MENU_HTML constant and any helpers truly still used by `app.rs` (likely none, since `open_menu`/`close_menu` moved into `impl ShellApp`). Delete the file entirely if empty. Update `menu/mod.rs` exports.

**`apps/shell/src/switcher/panel.rs`** — delete the file entirely. Update `switcher/mod.rs` exports (drop `panel`, keep `state` + new `assets`).

- [ ] **Step 6: Update `apps/shell/src/keys.rs`**

The key controller today takes `window: &gtk4::ApplicationWindow, state: &Rc<RefCell<ShellState>>, bus: &Rc<RefCell<BusClient>>`. Replace with a function that takes a `WindowHandle` and installs the controller on its GTK window. Since keys.rs needs to update ShellApp state, it needs access to the runtime. Two options:

**Simplest:** in app.rs `new()`, after creating windows and storing them in `self`, attach the key controller via `menubar.gtk_window().add_controller(ctrl)`. The controller's callback closes over `Rc<RefCell<AppRuntime<ShellApp>>>` — but we don't have runtime at new-time.

**Workaround:** make `setup_key_controller_on` take just the menubar handle, and inside defer dispatching to app via a weak reference to a shared slot that gets filled post-`new`. Pattern mirrors the JS dispatcher slot in Task 3.

Alternatively — given keys.rs is sola-shell-specific and only operates on ShellApp, move it off the trait-friendly model: make it emit bus events/JS directly, not mutate ShellApp. But it currently mutates `switcher.active`, etc., so it can't.

**Chosen design:** add a `key_slot: Rc<RefCell<Option<Box<dyn FnMut(u32, bool)>>>>` that the key controller reads from. `new()` creates it empty, installs the controller, stores it. After the runtime exists (in `run::<A>`), there's no clean hook. Workaround: expose a new trait method `after_run(&mut self, runtime: Weak<RefCell<AppRuntime<Self>>>)` — too custom.

**Cleaner design:** expose `WindowHandle::gtk_window()` as `pub` so apps can attach controllers. The controller closure captures an `Rc<RefCell<ShellApp>>` — wait, we don't have one either.

**Final design:** the trait's `on_bus_event` dispatcher (Task 6) is the only place with access to `runtime`. Keys are GTK events, not bus events. We need a hook: extend the trait with:

```rust
fn after_runtime_ready(&mut self, runtime: std::rc::Weak<std::cell::RefCell<AppRuntime<Self>>>, ctx: &mut AppCtx)
  where Self: Sized {
    let _ = (runtime, ctx);
}
```

Default no-op. `run::<A>()` calls `app.after_runtime_ready(Rc::downgrade(&runtime), ctx)` after wiring dispatchers/bus.

ShellApp implements it: captures `runtime`, installs key controller on `self.menubar.gtk_window()` with a closure that upgrades the weak ref and destructures `AppRuntime` to dispatch into `self`.

Then keys.rs becomes:
```rust
pub fn install_on_menubar<A: SolaApp>(
    menubar: &WindowHandle,
    runtime: std::rc::Weak<std::cell::RefCell<AppRuntime<A>>>,
) where
    A: KeyHandler,
{
    // ...
}
```

Hmm, this gets tangled.

**Simpler chosen design (revised):** keys.rs stays specific to shell, lives inside shell. The `AppRuntime` type is sola-app internal, so shell can't name it. Instead: expose a generic mechanism via AppCtx. Add a method:

```rust
impl AppCtx {
    pub fn with_app<A: SolaApp>(&mut self, f: impl FnOnce(&mut A, &mut AppCtx)) {
        // Runtime must be accessible somehow.
    }
}
```

Still tangled.

**Simplest resolution — scope adjustment:** move the key controller into `apps/shell/src/keys.rs` as a function that takes `&mut ShellApp, &mut AppCtx, key_event` and call it from a GTK event controller attached during `new` via a shared slot. The slot pattern works because ShellApp methods take `&mut self`, and the key controller captures `Rc<RefCell<Option<KeyDispatcher>>>` — installed after `new` returns somewhere. But new returns... into run<A>().

**Concrete resolution:** add one more trait method, `after_windows_ready`, called right after `A::new` and before runtime is wrapped in Rc. Apps can attach GTK event controllers here using the concrete `&mut self` reference and a `&gtk4::ApplicationWindow` from their window handles. But they still can't dispatch into themselves from inside the callback without a Rc reference to self.

I'm going in circles. Let me pick the cleanest way and move on.

**Final design for key controller dispatch:**

- Add `fn setup_event_listeners(&mut self, ctx: &mut AppCtx)` — NO. Still no `Rc<Self>`.

- The reality: any GTK event callback that mutates `self` needs `Rc<RefCell<Self>>` access OR a way to route through the runtime. The only thing that has runtime access is sola-app's internals.

- Solution: **expose the runtime access as a post-new trait hook**. Add:

```rust
/// Called once after run::<A>() has built the AppRuntime. Use this to attach
/// GTK event controllers or other sources that need to mutate self via the
/// runtime. The Weak reference avoids cycles; upgrade it inside callbacks.
fn after_runtime_ready(
    &mut self,
    runtime: std::rc::Weak<std::cell::RefCell<AppRuntime<Self>>>,
    ctx: &mut AppCtx,
) where
    Self: Sized,
{
    let _ = (runtime, ctx);
}
```

And make `AppRuntime` public (it's fine — it's just the {app, ctx} pair).

Shell's impl:
```rust
fn after_runtime_ready(
    &mut self,
    runtime: std::rc::Weak<std::cell::RefCell<AppRuntime<Self>>>,
    _ctx: &mut AppCtx,
) {
    crate::keys::install(self.menubar.clone(), runtime);
}
```

And `apps/shell/src/keys.rs::install` takes the menubar handle and runtime weak ref; it attaches a GTK event controller to `menubar.gtk_window()`; the controller callback upgrades the weak ref, destructures `AppRuntime`, dispatches a key-pressed or key-released handler method on ShellApp passing ctx.

Add to trait/lib.rs:
```rust
pub use ctx::AppCtx;
pub use window::{WindowConfig, WindowHandle};

pub struct AppRuntime<A: SolaApp> {
    pub app: A,
    pub ctx: AppCtx,
}
```

Make `AppRuntime` `pub`, and move the field `ctx.windows` etc. visibility appropriately. `WindowHandle::gtk_window()` becomes `pub` too.

- [ ] **Step 6a: Apply the trait addition**

In `crates/sola-app/src/lib.rs`:
1. Make `AppRuntime` `pub` (not `pub(crate)`).
2. Add `fn after_runtime_ready(...)` to the trait with default no-op.
3. In `run::<A>()`, after the JS dispatcher-install loop, add:
```rust
{
    let mut rt = runtime.borrow_mut();
    let AppRuntime { app, ctx } = &mut *rt;
    app.after_runtime_ready(Rc::downgrade(&runtime), ctx);
}
```
But this reborrows while inside a mutable borrow. Actually `Rc::downgrade` takes `&Rc`, not `&mut`. So we can't borrow `runtime` (hold `&Rc`) AND hold `rt = runtime.borrow_mut()`. Do two-step:

```rust
let runtime_weak = Rc::downgrade(&runtime);
{
    let mut rt = runtime.borrow_mut();
    let AppRuntime { app, ctx } = &mut *rt;
    app.after_runtime_ready(runtime_weak, ctx);
}
```

`Rc::downgrade` takes `&Rc`, runtime is Rc, we borrow it immutably, downgrade returns Weak, done. Then we borrow_mut separately. Weak doesn't conflict with the mut borrow because it's a separate owned Weak, not a borrow.

- [ ] **Step 6b: Rewrite `apps/shell/src/keys.rs`**

```rust
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use sola_app::{AppRuntime, WindowHandle};
use sola_bus::topics::{FocusTarget, FrameUpdate, Topic};

use crate::app::ShellApp;
use crate::zoning;

mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

pub fn install(menubar: WindowHandle, runtime: Weak<RefCell<AppRuntime<ShellApp>>>) {
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let runtime = runtime.clone();
        move |_, _keyval, keycode, gtk_modifiers| {
            let shift = gtk_modifiers.contains(gdk4::ModifierType::SHIFT_MASK);
            let Some(runtime) = runtime.upgrade() else { return glib::Propagation::Proceed };
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            handle_key_pressed(app, ctx, keycode, shift)
        }
    });

    key_ctrl.connect_key_released({
        let runtime = runtime.clone();
        move |_, _keyval, keycode, _modifiers| {
            if keycode != keycode::SUPER_L {
                return;
            }
            let Some(runtime) = runtime.upgrade() else { return };
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            handle_super_released(app, ctx);
        }
    });

    menubar.gtk_window().add_controller(key_ctrl);
}

fn handle_key_pressed(
    app: &mut ShellApp,
    ctx: &mut sola_app::AppCtx,
    keycode: u32,
    shift_held: bool,
) -> glib::Propagation {
    if let Some(action) = app.menus.lookup_shortcut(keycode, shift_held, ShellApp::APP_ID) {
        tracing::info!(action_id = %action.action_id, "shell shortcut");
        if action.action_id == "exit" {
            ctx.emit(Topic::Shutdown);
        }
        return glib::Propagation::Stop;
    }

    if keycode == keycode::TAB && !app.switcher.active {
        tracing::info!("activating switcher");
        app.switcher.apps = app.rebuild_switcher_apps();
        app.switcher.active = true;
        app.switcher.selected = if app.switcher.apps.len() > 1 { 1 } else { 0 };
        let json = serde_json::to_string(&app.switcher.apps).unwrap_or_default();
        app.switcher_win
            .eval_js(&format!("renderSwitcher({}, {})", json, app.switcher.selected));

        if let Some((ow, oh)) = app.zoning.output_size {
            ctx.emit(Topic::Frame(FrameUpdate {
                app_id: ShellApp::APP_ID.into(),
                title: Some("switcher".into()),
                x: (ow - 800) / 2,
                y: (oh - 400) / 2,
                width: 800,
                height: 400,
            }));
        }
        app.emit_composition(ctx);
        return glib::Propagation::Stop;
    }

    if app.switcher.active {
        match keycode {
            keycode::TAB | keycode::RIGHT => {
                app.switcher.select_next();
                let sel = app.switcher.selected;
                app.switcher_win.eval_js(&format!("setSelection({sel})"));
                return glib::Propagation::Stop;
            }
            keycode::LEFT => {
                app.switcher.select_prev();
                let sel = app.switcher.selected;
                app.switcher_win.eval_js(&format!("setSelection({sel})"));
                return glib::Propagation::Stop;
            }
            _ => {}
        }
    }

    if let Some(frame) = app.zoning.handle_key(keycode) {
        ctx.emit(Topic::Frame(frame));
        return glib::Propagation::Stop;
    }

    if let Some(focused) = app.focused_app_id.clone() {
        if let Some(action) = app.menus.lookup_shortcut(keycode, shift_held, &focused) {
            tracing::info!(
                app_id = %action.app_id,
                action_id = %action.action_id,
                "menu shortcut matched"
            );
            ctx.emit(Topic::MenuAction(action));
            return glib::Propagation::Stop;
        }
    }

    glib::Propagation::Proceed
}

fn handle_super_released(app: &mut ShellApp, ctx: &mut sola_app::AppCtx) {
    if !app.switcher.active {
        return;
    }

    let app_id = app.switcher.selected_app_id().map(String::from);
    tracing::info!(app_id = ?app_id, "deactivating switcher");

    app.switcher.active = false;
    app.switcher_win.eval_js("clear()");

    if let Some(ref app_id) = app_id {
        app.set_focus(app_id);
        ctx.emit(Topic::Focus(FocusTarget {
            app_id: app_id.clone(),
            title: None,
        }));
    }
    app.emit_composition(ctx);
}
```

Note: `WindowHandle::gtk_window()` must be `pub`. Update `crates/sola-app/src/window.rs` accordingly.

- [ ] **Step 6c: Add `after_runtime_ready` to `ShellApp`**

In `apps/shell/src/app.rs`, add to the `impl SolaApp for ShellApp` block:

```rust
fn after_runtime_ready(
    &mut self,
    runtime: std::rc::Weak<std::cell::RefCell<sola_app::AppRuntime<Self>>>,
    _ctx: &mut AppCtx,
) {
    crate::keys::install(self.menubar.clone(), runtime);
}
```

Also remove the `crate::keys::setup_key_controller_on(menubar.clone());` call from inside `new` — the install happens in `after_runtime_ready` instead.

- [ ] **Step 7: Update `apps/shell/src/menu/mod.rs` and `apps/shell/src/switcher/mod.rs`**

```rust
// apps/shell/src/menu/mod.rs
pub mod assets;
pub mod state;

pub use assets::MENU_ASSETS;
pub use state::MenuCache;
```

```rust
// apps/shell/src/switcher/mod.rs
pub mod assets;
pub mod state;

pub use assets::SWITCHER_ASSETS;
pub use state::SwitcherState;
```

- [ ] **Step 8: Verify**

```bash
cargo check -p sola-shell
```

Expected: clean build. If anything in `app.rs` references `crate::util::eval_js` (old helper), replace with `window_handle.eval_js(script)`.

- [ ] **Step 9: Commit**

```bash
git add -A apps/shell crates/sola-app
git commit -m "$(cat <<'EOF'
refactor(shell): migrate to SolaApp trait API

Drops setup_switcher_panel and setup_menu_panel — sola-app now creates
all 3 windows via ctx.add_window. The title-property JS→Rust hack in
the menu webview is replaced by proper sola.send("action", ...) flowing
through on_js_command with (source.title(), cmd) tuple matching.

Auto-emitted SetWindowPolicy replaces the manual emit in activate. Key
controller attaches via after_runtime_ready, which receives a weak
reference to the runtime so its callbacks can mutate ShellApp.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Remove old `SolaAppBuilder` and dead code

All consumers now use the trait API. Delete the legacy.

**Files:**
- Modify: `crates/sola-app/src/lib.rs`
- Modify: `crates/sola-app/src/webview.rs`
- Modify: `crates/sola-app/src/bridge.rs`

- [ ] **Step 1: Delete from `crates/sola-app/src/lib.rs`**

Remove:
- `pub struct SolaAppBuilder { ... }` and its impl.
- Old `LegacyAppHandler` trait (renamed in Task 8).
- `dispatch_loop` free function.
- Anything else only reachable from the builder path.

Keep:
- `SolaApp` trait
- `run::<A>()`
- `AppRuntime`
- `inject_import_map` helper
- Module declarations
- Re-exports

- [ ] **Step 2: Clean up `crates/sola-app/src/webview.rs`**

Delete `create_content_manager` and `create_content_manager_with_handler` (only used by the old builder). Keep `create_web_context` and `create_ucm_for_window`.

- [ ] **Step 3: Clean up `crates/sola-app/src/bridge.rs`**

If `setup_event_poller` is now unused (AsyncDispatcher has its own bridging), delete it and the file. If parts are still referenced, keep only what's needed.

**Check usage:**
```bash
grep -r "setup_event_poller\|create_content_manager" crates/ apps/
```

- [ ] **Step 4: Verify workspace clean**

```bash
cargo check --workspace
cargo clippy --workspace -- -W warnings 2>&1 | head -50
```

Expected: clean build. Minimal warnings.

- [ ] **Step 5: Commit**

```bash
git add -A crates/sola-app
git commit -m "$(cat <<'EOF'
refactor(sola-app): remove legacy SolaAppBuilder API

All apps (shell, terminal) have migrated to the SolaApp trait. The old
builder, its AppHandler plumbing, dispatch_loop, and the non-per-window
content-manager helpers are removed.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Parity Verification (after Task 11)

These are checks to eyeball before calling the migration done:

1. **Compile:** `cargo check --workspace` — clean across the workspace.
2. **Shell window policy:** inspect the shell's auto-emitted `SetWindowPolicy` payload (e.g., log it from compositor side) — it should contain 3 entries (menubar/switcher/menu) with the same sizes/positions/zoned/keyboard_target as before the migration.
3. **Terminal async commands:** static review — `TerminalHandler::dispatch` is still invoked via `AsyncDispatcher`; reply JSON shape is `{ id, result }` as before.
4. **No hanging builder references:** `grep -r "SolaAppBuilder\|builder()" crates/ apps/` — no hits.
5. **No raw GTK panel setup in shell:** `grep -r "setup_switcher_panel\|setup_menu_panel" apps/` — no hits.
6. **Menu `title` hack gone:** `grep -r "connect_notify_local" apps/shell` — no hits; the `menu.ts` frontend uses `sola.send("action", ...)` instead.

## Out of scope

Runtime testing (actual install + visual verification) requires explicit user permission per project policy and is not part of this plan's execution.

## Known caveats

- The `bridge.rs` poll pattern (5ms timer) continues in `AsyncDispatcher` — not ideal but matches existing behavior. A future improvement could use eventfd/pipe + `glib::unix_fd_add_local`.
- `WindowHandle::gtk_window()` is `pub` for controller attachment. Some consumers (shell's keys.rs) depend on this. If undesired, introduce typed event subscription APIs on `WindowHandle` in a follow-up.
- `after_runtime_ready` is a small escape hatch specifically for cases (like keys.rs) that need access to the full runtime for GTK event callbacks. If multiple escape hatches accumulate, revisit the design.
