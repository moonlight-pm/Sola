# Sola Browser Upgrade — Design

## Context

`sola-browser` was written before the `SolaApp` trait API landed and bypasses
`sola-app` almost entirely: ~460 LOC in `main.rs` re-implements bootstrap (logging,
Wayland wait, GTK app, bus connect, shutdown), a dedicated `ipc.rs` wires its own
`UserContentManager`, and it never emits `SetWindowPolicy`. Meanwhile
`sola-terminal` — the canonical trait-based app — is half the size and gets all
of that for free. This upgrade brings the browser onto the same rails.

Strictly a conformance refactor. No feature changes, no frontend changes, no
protocol changes.

## Goal

Port `sola-browser` to `impl SolaApp for BrowserApp` while preserving the
current UX exactly:

- Single OS window with a vertical-sidebar chrome.
- N tab WebViews, one per open page, positioned inside the chrome window's
  content area and shown/hidden on switch (no reload).
- Session persistence, history, menu shortcuts, `OpenUrl` bus topic — all
  unchanged.

## The structural problem

`sola-app`'s window model is: one `WindowHandle` → one `ApplicationWindow` whose
direct child is the chrome `WebView`. The browser has one OS window with
**many** WebViews (chrome + N tabs) living inside a `gtk4::Fixed`. That's why
the browser bypassed `ctx.add_window` — there was no hook to attach sibling
WebViews after construction.

### Resolution: A1 — minimal accessor

Add one public accessor to `WindowHandle`:

```rust
impl WindowHandle {
    pub fn webview(&self) -> &webkit6::WebView { &self.inner.webview }
}
```

In `BrowserApp::new`, after `ctx.add_window`, the browser reparents the chrome
WebView into a `gtk4::Fixed`, sets the Fixed as the window's child, and manages
tab WebViews as Fixed siblings. The JS dispatcher / `UserContentManager` stays
attached to the WebView across reparenting, so `on_js_command` routing is
unaffected.

This keeps browser-specific widget-tree gymnastics inside the browser and
doesn't grow `sola-app`'s surface area for a single consumer.

### Why not alternatives

- **Build hook on `WindowConfig` (`body: Option<FnOnce(&WebView) -> Widget>`)**
  — cleaner on paper but adds `sola-app` API that only the browser uses today.
  Premature.
- **Tabs-as-windows (each tab a separate `WindowHandle`)** — would require
  compositor/shell protocol work to nest tab windows inside the chrome's
  content area. Out of scope for a conformance refactor.
- **Skip `ctx.add_window` entirely** — defeats the conformance goal: no
  auto-`SetWindowPolicy`, no unified JS dispatch, duplicated bootstrap.

## Design

### `BrowserApp`

```rust
struct BrowserApp {
    chrome: WindowHandle,
    container: gtk4::Fixed,
    web_context: webkit6::WebContext,
    network_session: webkit6::NetworkSession,
    tabs: Vec<Tab>,
    active_tab_id: Option<String>,
    tab_store: TabStore,
    history: BrowsingHistory,
}

struct Tab {
    id: String,
    webview: webkit6::WebView,
}

impl SolaApp for BrowserApp {
    const APP_ID: &'static str = "sola-browser";
    fn new(ctx: &mut AppCtx) -> Self { /* ... */ }
    fn on_js_command(&mut self, cmd: &str, args: &Value, id: Option<u64>, source: &WindowHandle, ctx: &mut AppCtx) { /* ... */ }
    fn on_bus_event(&mut self, topic: &Topic, ctx: &mut AppCtx) { /* ... */ }
    fn after_runtime_ready(&mut self, runtime: Weak<RefCell<AppRuntime<Self>>>, ctx: &mut AppCtx) { /* ... */ }
    fn on_shutdown(&mut self, ctx: &mut AppCtx) { /* ... */ }
}

fn main() { sola_app::run::<BrowserApp>(); }
```

### `new` flow

1. `ctx.add_window(WindowConfig { title: "main", size: (1920, 1080),
   decorated: false, transparent: true, assets: APP_ASSETS,
   initial_state: None, zoned: true, keyboard_target: true })` → `chrome`.
2. Grab `chrome.webview()`. `chrome.gtk_window().set_child(None)` (detach).
   Build `gtk4::Fixed`, `put(chrome_webview, 0, 0)`, set Fixed as window child.
3. Reuse `chrome.webview().web_context()` as the shared `WebContext` for tab
   WebViews (already registered against `APP_ASSETS` via `app:///`).
4. Build `webkit6::NetworkSession` pointing at
   `$XDG_DATA_HOME/sola/browser` + `$XDG_CACHE_HOME/sola/browser`, persistent
   SQLite cookie storage.
5. Load `TabStore` + `BrowsingHistory` from `$XDG_CONFIG_HOME/sola/`.
6. `ctx.emit_sticky(Topic::SetAppMenu(browser_menu()))`.
7. Construct `BrowserApp { ... }`.

Tabs are **not** restored in `new()` — they're materialized in the `ready`
command handler so the chrome is mounted first.

### `after_runtime_ready`

Hook resize signals on the chrome `ApplicationWindow`. Callbacks capture a
`Weak<RefCell<AppRuntime<BrowserApp>>>` (same pattern shell uses for its key
controller), upgrade on fire, call `app.resize_tabs(w, h, ctx)`.

### Tab management

Tab operations live as `&mut self` methods on `BrowserApp`:

- `create_tab(&mut self, id: &str, url: Option<&str>, session_state: Option<&str>)`
- `close_tab(&mut self, id: &str)`
- `switch_tab(&mut self, id: &str)`
- `navigate_active(&mut self, url: &str)`, `go_back`, `go_forward`, `reload`
- `resize_tabs(&mut self, w: i32, h: i32)`
- `capture_session_state(&mut self)` — captures `WebViewSessionState` bytes for
  every tab into `self.tab_store`.
- `persist_tabs(&self)` — atomic write-through to `browser-tabs.json`.
- `persist_history(&self)` — atomic write-through to `browser-history.json`.

`build_web_page_view(&WebContext, &NetworkSession, &TabConfig) -> WebView` is a
free helper in `tabs.rs` that centralizes per-tab WebView construction:
Safari-compatible user agent, emacs `UserScript` injection, session-state
restore, URL load. (Future lift target: see Out of scope.)

### Signal wiring (per tab)

`create_tab` attaches four handlers to each tab WebView. Each captures a
`WindowHandle` for `send_to_js` (the chrome) and a `Weak<RefCell<AppRuntime>>`
where it needs mutable state (history record, session-state snapshot):

- `notify::title` → `self.chrome.send_to_js({"event":"tab_title_changed","tabId":...,"title":...})`.
- `notify::uri` → emit `tab_url_changed`; record visit in history; snapshot
  the tab's session state into `self.tab_store`; `persist_tabs` + `persist_history`.
- `notify::is-loading` → emit `tab_load_changed`.
- `connect_decide_policy` for `NewWindowAction` → `create_tab` with the new
  URL, activate, emit `bus_new_tab`.

### IPC — `on_js_command`

All handlers sync. No `AsyncDispatcher` (in-memory ops are microseconds; add
async later when/if commands grow expensive).

| cmd | args | reply via `source.send_to_js({id, result})` |
|---|---|---|
| `ready` | — | `{ tabs: [...], activeTabId }`. Side-effect: materialize restored tab WebViews. |
| `create_tab` | `{ tabId, url?, activate? }` | `"ok"` |
| `close_tab` | `{ tabId }` | `"ok"` |
| `switch_tab` | `{ tabId }` | `"ok"` |
| `navigate` | `{ url }` | `"ok"` |
| `go_back` / `go_forward` / `reload` | — | `"ok"` |
| `history_search` | `{ query }` | `[{ url, title, visits }]` |

### Bus — `on_bus_event`

- `Topic::MenuAction(a)` with `a.app_id == APP_ID`:
  - `"new_tab"` → `create_tab(new_uuid, None, None)`; `switch_tab`; push
    `bus_new_tab` to chrome.
  - `"close_tab"` → close active; switch to the last remaining tab if any;
    push `tab_closed` to chrome.
  - `"focus_address"` → push `bus_focus_address` to chrome;
    `chrome.webview().grab_focus()`.
  - `"quit"` → `ctx.emit(Topic::Shutdown)`.
- `Topic::OpenUrl(req)` → `create_tab(new_uuid, Some(&req.url), None)`;
  switch if `req.activate`; push `bus_new_tab` to chrome.

Focus tracking via `FocusChanged` is **dropped**. The compositor routes
menu-shortcut key combinations through `MenuAction` only to the focused app, so
the browser no longer needs to gate shortcuts itself.

### Menu

Emitted sticky once in `new()`:

```rust
fn browser_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: BrowserApp::APP_ID.into(),
        menus: vec![
            MenuDefinition { label: "Browser", items: [New Tab ⌘T, Close Tab ⌘W, ---, Quit ⌘Q] },
            MenuDefinition { label: "Edit",    items: [Focus Address Bar ⌘L] },
        ],
    }
}
```

### Persistence

Write-through on every mutation. Two files under
`$XDG_CONFIG_HOME/sola/`:

- `browser-tabs.json` — `{ tabs: [{url, title, session_state: base64?}], active_tab_id }`
- `browser-history.json` — `{ entries: [{url, title, visits}] }` (cap 1000)

Both use atomic tmp+rename via `TabStore::save` / `BrowsingHistory::save`
(unchanged).

**Session state capture:** `notify::uri` handler snapshots the tab's current
`WebViewSessionState`, base64-encodes, updates `tab_store`, writes. For ~10
tabs and normal browsing this is a handful of writes per minute.

`on_shutdown` is a final belt-and-suspenders flush — not the primary write
path.

### Logging, Wayland wait, bus connect, shutdown

All delegated to `sola_app::run::<BrowserApp>()`. The browser no longer owns
any bootstrap. `Topic::Shutdown` is intercepted by `sola-app` before
`on_bus_event`, calls `on_shutdown`, then quits GTK. The existing
`connect_close_request` hook goes away.

## File deltas

### `crates/sola-app`

- `src/window.rs`: add `pub fn webview(&self) -> &webkit6::WebView`.

### `apps/browser`

| File | Change |
|---|---|
| `src/main.rs` | Shrinks to `mod` decls + `fn main() { sola_app::run::<BrowserApp>(); }` + `fn browser_menu()`. |
| `src/app.rs` | **New.** `struct BrowserApp`, `impl SolaApp`, `new`, `on_js_command`, `on_bus_event`, `after_runtime_ready`, `on_shutdown`, plus the `&mut self` tab operations. |
| `src/tabs.rs` | Refactored: `build_web_page_view` free helper; signal handlers use `Weak<RefCell<AppRuntime<BrowserApp>>>`. Tab ops move onto `BrowserApp`. |
| `src/ipc.rs` | **Deleted.** Command dispatch lives in `BrowserApp::on_js_command`. |
| `src/state.rs` | Unchanged. |
| `src/chrome.rs` | Unchanged. |
| `web/` | Unchanged. |
| `Cargo.toml` | Remove `tracing-subscriber` and `tracing-appender` (sola-app owns logging). |

## Parity criteria

- Chrome + tabs render identically to current deploy.
- Tab switching is instant, no reload.
- Session state restores across restarts.
- `browser-tabs.json` and `browser-history.json` formats preserved.
- Menu shortcuts ⌘T / ⌘W / ⌘L / ⌘Q fire.
- `Topic::OpenUrl` opens a new tab from other apps.
- `Topic::Shutdown` triggers final flush and clean exit.
- `Topic::SetWindowPolicy` sticky is now auto-emitted (new — was missing).
- `cargo make build` succeeds; `cargo make install browser` installs.

## Out of scope (noted for future work)

- **`WindowContent::Url` / app wrapper support.** When the SSB/app-wrapper tool
  is built, `sola-app` should grow a `WindowConfig::content` enum
  (`Assets(&AssetBundle)` | `Url { url, network_session, user_agent, scripts }`)
  so the wrapper can create a URL-backed window with one call. At that point,
  lift `build_web_page_view` out of the browser into `sola-app`.
- **Tabs-as-windows.** Each tab becoming a real `WindowHandle` is a much
  larger architectural move — needs shell/compositor protocol for nesting a
  window inside another window's content area. Revisit only if the tab model
  outgrows GTK `Fixed`.
- **Tab hibernation.** Kill the web process after N minutes idle; reload on
  focus. Saves memory for users with many tabs open.
- **SQLite-backed history.** Current JSON is capped at 1000 entries and
  searches with a linear scan. Move to SQLite (FTS) if history grows large.
- **Cross-fade / animated tab transitions.** Today's hard cut is intentional
  and matches mainstream browsers. Could be done via a chrome-side overlay if
  ever desired.
- **Async `history_search` / persistence via `AsyncDispatcher`.** Not needed
  at current sizes; add when a command crosses the "blocks the GTK thread"
  threshold.
- **Frontend refactor (Arrow.js code in `web/`).** This upgrade touches Rust
  only; the JS IPC shape is unchanged.
