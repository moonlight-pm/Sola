# sola-app Trait-Based API — Design

## Context

sola-app today exposes a builder API that assumes one primary window per app:

```rust
SolaApp::builder()
    .app_id("...")
    .window_size(w, h)
    .decorated(b)
    .transparent(b)
    .web_assets(&ASSETS)
    .on_js_command(|cmd, args| ...)
    .on_bus_event(|topic, send, emit| ...)
    .on_activate(|window, webview, bus| ...)
    .run()
```

Handlers are closures; state is shared via `Rc<RefCell<AppState>>` cloned into each closure. Apps that need multiple windows (sola-shell: menubar + switcher + menu) create the extras with raw GTK + webkit6 after sola-app hands them the primary window. The browser app bypasses sola-app entirely (~481 lines of its own bootstrap) — a signal that the primary-window assumption is too limiting.

## Goal

Replace the builder + callbacks API with a trait-based struct API:

- Apps define a struct and implement `SolaApp`.
- Windows are first-class: any number, each with its own assets, declared during construction.
- `&mut self` in every handler replaces the `Rc<RefCell<_>>`-with-closures pattern.
- `AppCtx` is the single effect handle (bus + windows + lifecycle).

Motivation is **code and architectural clarity**. No new use case is forced; the existing apps (shell, terminal) become simpler.

## Design

### Trait

```rust
pub trait SolaApp: 'static {
    const APP_ID: &'static str;

    fn new(ctx: &mut AppCtx) -> Self where Self: Sized;

    fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) {}

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {}

    fn on_shutdown(&mut self, ctx: &mut AppCtx) {}
}

pub fn run<A: SolaApp>() { /* bootstrap + GTK main loop */ }
```

All methods except `APP_ID` and `new` have default no-op impls — apps opt in to what they need.

### AppCtx

The effect handle passed to every trait method.

```rust
impl AppCtx {
    pub fn add_window(&mut self, cfg: WindowConfig) -> WindowHandle;
    pub fn remove_window(&mut self, handle: &WindowHandle);
    pub fn emit(&mut self, topic: Topic);
    pub fn emit_sticky(&mut self, topic: Topic);
}
```

No `window(title)` lookup — hold the handle returned by `add_window` as a field on your app struct.

### WindowHandle

```rust
#[derive(Clone)]
pub struct WindowHandle { /* Rc<WindowInner> */ }

impl WindowHandle {
    pub fn title(&self) -> &str;
    pub fn eval_js(&self, script: &str);
    pub fn send_to_js(&self, value: &Value);  // wraps eval_js("window.__solaRecv({...})")
}

impl PartialEq for WindowHandle { /* by Rc pointer identity */ }
```

### WindowConfig

```rust
pub struct WindowConfig {
    pub title: String,
    pub size: (i32, i32),
    pub position: Option<(i32, i32)>,
    pub decorated: bool,
    pub transparent: bool,
    pub assets: &'static AssetBundle,
    pub initial_state: Option<String>,
    // WindowPolicy fields (auto-emitted to bus by sola-app):
    pub zoned: bool,
    pub keyboard_target: bool,
}
```

### Runtime flow

`sola_app::run::<A>()`:

1. Tracing (file + stderr), Wayland socket wait, env setup — preserved from today's `lib.rs`.
2. `gtk4::Application::connect_activate`.
3. Inside activate:
   - Connect `BusClient`.
   - Build empty `AppCtx { bus, gtk_app, windows: Vec::new() }`.
   - `let app = A::new(&mut ctx)` — `ctx.add_window` populates the list.
   - Auto-emit `Topic::SetWindowPolicy` sticky from the collected windows.
   - Wrap in `Rc<RefCell<AppRuntime<A> { app, ctx }>>`.
   - Register bus FD poller → destructure + `app.on_bus_event(&topic, ctx)`.
   - Each window's `UserContentManager` handler → `app.on_js_command(cmd, args, &source, ctx)`.
4. Run GTK main loop.

### Ownership

`AppRuntime<A>` in `Rc<RefCell<_>>`. Each callback destructures to get disjoint `&mut` borrows:

```rust
let mut runtime = runtime.borrow_mut();
let AppRuntime { app, ctx } = &mut *runtime;
app.on_bus_event(&topic, ctx);
```

### Per-window JS dispatch

When `add_window` creates a window, the `UserContentManager` handler closure captures a clone of the resulting `WindowHandle` as `source`. JS `sola.send(cmd, args)` routes into that closure with the source already bound — no lookup.

```rust
fn on_js_command(&mut self, cmd: &str, args: &Value, source: &WindowHandle, ctx: &mut AppCtx) {
    match (source.title(), cmd) {
        ("menubar", "open_menu")  => self.open_menu(args, ctx),
        ("menubar", "close_menu") => self.close_menu(ctx),
        ("menu",    "action")     => self.handle_menu_action(args, ctx),
        _ => {}
    }
}
```

### Shutdown

sola-app intercepts `Topic::Shutdown` before user code: calls `app.on_shutdown(ctx)`, then `gtk_app.quit()`. Binary-watcher restart preserved — `watcher::watch_own_binary()` continues to run; process manager respawns on binary change.

### Async commands

Replaces the current `AppHandler` trait + `.handler()` builder method.

```rust
#[async_trait::async_trait]
pub trait AppHandler: Send + Sync + 'static {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value;
}

pub struct AsyncDispatcher { /* tokio runtime thread + sender */ }

impl AsyncDispatcher {
    pub fn spawn<H: AppHandler>(handler: H) -> Self;
    pub fn dispatch(
        &self,
        cmd: String,
        args: Value,
        reply: impl FnOnce(Value) + Send + 'static,
    );
}
```

Apps needing async work construct an `AsyncDispatcher` in `new`, forward from sync `on_js_command`:

```rust
fn on_js_command(&mut self, cmd: &str, args: &Value, source: &WindowHandle, ctx: &mut AppCtx) {
    let source = source.clone();
    let id = args.get("id").and_then(|v| v.as_u64());
    self.dispatcher.dispatch(cmd.into(), args.clone(), move |result| {
        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    });
}
```

The dispatcher's reply-to-js callback runs on the tokio runtime; `source.send_to_js` uses `glib::MainContext` to hop back to the GTK thread (same pattern as today's `bridge::setup_event_poller`).

## Migration

Single PR, three commits.

1. **sola-app refactor.**
   - Add `SolaApp` trait, `AppCtx`, `WindowHandle`, `WindowConfig`, `run::<A>()`.
   - Add `AsyncDispatcher`; replace old `AppHandler` plumbing.
   - Retire: builder, `on_activate`, `on_bus_event`, `on_js_command`, `handler` builder method.
   - Preserve: `assets.rs`, `bridge.rs`, `config.rs`, `watcher.rs`, `strip.rs`, the `asset_bundle!` macro.
   - `webview.rs` becomes per-window (UCM creation parameterized by window).

2. **Migrate terminal.**
   - `struct TerminalApp` with `impl SolaApp`.
   - One `ctx.add_window` in `new`.
   - `AsyncDispatcher` holds the existing `TerminalHandler`.
   - `on_bus_event` handles menu actions as today.
   - Validates single-window + async case.

3. **Migrate shell.**
   - `struct ShellApp` replaces `ShellState` + free-function scaffolding.
   - Three `ctx.add_window` calls in `new` (menubar, switcher, menu panel).
   - Remove `setup_switcher_panel`, `setup_menu_panel` entirely (sola-app handles GTK window + UCM creation).
   - `setup_key_controller` stays — key controller is attached to the menubar's GTK window after creation; it takes `&mut ShellApp` and uses ctx.
   - `title`-property JS→Rust hack in `menu/panel.rs` goes away: the menu webview's JS calls `sola.send("action", {app_id, action_id})` and the app dispatches via `source.title() == "menu"`.
   - Menu panel and switcher panel HTML become proper `AssetBundle`s (not inline HTML with `__OVERLAY_JS__` replace).
   - Auto-emit replaces the manual `SetWindowPolicy` in `on_activate`.

Browser is untouched — it already bypasses sola-app.

## Parity check after migration

- Shell emits the same `SetWindowPolicy` payload.
- Terminal still serves async tmux commands with request-id replies.
- All three shell windows render and respond to input.
- `Topic::Shutdown` exits every app.
- Binary-watcher restart still works.
- `@sola/ipc` JS library needs no changes (still `sola.send(cmd, args)`, still `window.__solaRecv` for receives).
- sola-bus protocol needs no changes.

## Out of scope

- Migrating sola-browser. It already bypasses sola-app; a separate project will catch it up.
- Dynamic window creation in response to runtime bus events or JS commands. API supports it (`ctx.add_window` is callable from any handler), but no current app exercises this.
- Zero-window apps. API permits (empty `new` body), but would need `gtk_application_hold()` to keep the loop alive — defer until a real use case.
- Changing the JS platform library (`@sola/ipc`, `@sola/store`, `@sola/theme`).
- Changing the sola-bus wire protocol.
