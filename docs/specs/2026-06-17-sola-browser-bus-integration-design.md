# sola-browser bus integration — design

**Date:** 2026-06-17
**Status:** approved, ready for implementation plan
**Crates:** `sola-browser-wpe` (primary), `sola-browser-cef` (parity)

## Goal

Make both browser engines first-class Sola **bus** clients — handle internal
`Topic::OpenUrl`, publish an app-menu (which is also how they receive keyboard
shortcuts), and theme their chrome from `Topic::Theme` — by adopting the same
kit glue `sola-monitor` already uses. This is the "desktop integration parity"
track agreed after retiring the legacy GTK `sola-browser` crate.

Scope is **internal bus integration only**. External (system) `xdg-open`
http/https routing now goes to Helium and is out of scope here.

## Background

`sola-browser-wpe` and `sola-browser-cef` are independent `iced` apps with the
same shape (`App` struct, `Msg` enum, `impl App { update, subscription, view }`,
`iced::application(...)` in `main`); cef mirrors wpe deliberately. Today they:

- depend on `sola-bus` + `sola-core` + `iced` only — **not** `sola-kit`;
- make **zero** use of the bus (no `Topic::` subscriptions, no app-menu, no
  theme) — they are seen by the shell's switcher solely via their Wayland
  `application_id`;
- set that `application_id` from `APP_ID` (`"sola-browser-wpe"` /
  `"sola-browser-cef"`), so the bus app_id and the Wayland app_id already match.

The retired GTK `sola-browser` was the only prior `Topic::OpenUrl` subscriber,
so nothing handles internal open-URL requests right now.

The kit (`sola-kit`) already provides everything needed, proven by
`sola-monitor`:

- `BusSetup::new(id).subscribe(kinds).app_menu(label, items).install()` —
  connect + subscribe + publish `SetAppMenu`, stash the client in the kit's
  process-wide `BUS` slot. (`crates/sola-kit/src/app.rs`)
- `bus_subscription() -> Subscription<Arc<Message>>` — an 8 ms poller forwarding
  the bus client's `try_recv` into an iced subscription.
- `apply_theme_update(&Message, &mut iced::Theme) -> bool` — on `Topic::Theme`,
  rebuild the iced theme + install fonts + selection color; returns whether it
  applied.
- `is_self_quit(&Message, app_id) -> bool` — true for `MenuAction("quit")` or
  `CloseApp` addressed to us.

Relevant bus types (`crates/sola-bus/src/topics.rs`):

```rust
pub struct OpenUrlRequest { pub url: String, pub activate: bool }   // ephemeral
pub struct AppMenuPayload  { pub app_id: String, pub menus: Vec<MenuDefinition> } // sticky, keyed by app_id
pub struct MenuDefinition  { pub label: String, pub items: Vec<MenuItem> }
pub enum   MenuItem        { Action { id, label, shortcut: Option<KeyChord>, disabled, checked }, Divider }
pub struct MenuActionPayload { pub app_id: String, pub action_id: String }
```

`KeyChord` / `KeyCode` come from `sola_core::keys`; `KeyCode::T.meta()` builds a
⌘+T chord. Available key constants include `T, W, R, L, Q, N, LEFT, RIGHT`
(no bracket keys, so Back/Forward use ⌘←/⌘→).

## Approach

**Both browsers become kit consumers.** Each adds a `sola-kit` path dependency
and wires the bus exactly like `sola-monitor`, but **keeps its bespoke boot** —
WPE/CEF env juggling and `sola_core::log::init(APP_ID)` stay; we do **not** call
kit's `startup()` (it would double-init logging/wayland). We adopt only:

1. `BusSetup` in `main()` (after the existing wayland activation, before
   `iced::application(...).run()`).
2. `bus_subscription().map(Msg::Bus)` added to the existing subscription batch.
3. A `Msg::Bus(Arc<Message>)` update arm and an `App::theme()` method wired via
   `.theme(App::theme)` on the iced builder.

**Feasibility (verified):** kit's iced features
(`wgpu, tokio, wayland, svg, advanced`) are a superset of the browsers'
(`wgpu, tokio, wayland`); Cargo feature unification is additive, so adding the
dep does not conflict. The browsers already depend on `sola-core`, so `KeyCode`
is in hand.

**wpe is implemented first, then mirrored to cef** (the standing parity rule).

### Code placement

Per-crate, the integration lives in a **new small module** `src/integration.rs`
so `main.rs` stays focused. It owns:

- `browser_menu() -> MenuDefinition` — the published menu (see below).
- `enum BrowserIntent { NewTab { url: String, activate: bool }, CloseActiveTab,
  Reload, Back, Forward, FocusUrl, Quit, None }`.
- pure mappers (unit-testable, no bus):
  - `intent_for_open_url(&OpenUrlRequest) -> BrowserIntent`
  - `intent_for_menu_action(action_id: &str) -> BrowserIntent`
- `handle_bus(app: &mut App, msg: Arc<Message>) -> Task<Msg>` — the `Msg::Bus`
  handler: apply theme, check self-quit, parse the topic, dispatch the intent.

This module is duplicated in each crate (mirroring the existing wpe/cef
duplication). A shared browser crate is explicitly **not** introduced here — it
would be a larger refactor beyond integration parity.

## Data flow

**OpenUrl** (internal emitter, e.g. `solactl open <url>`):

```
Topic::OpenUrl { url, activate }
  → bus → bus_subscription → Msg::Bus
  → intent_for_open_url → BrowserIntent::NewTab { url, activate }
  → engine.cmd(Cmd::OpenTab(url)) + set-active (when activate)
  → new frame
```

**Menu / shortcuts** (the existing app-shortcut-via-menu flow):

```
startup: SetAppMenu { app_id: "sola-browser-wpe", menus: [Browser] }
  → shell caches menu + binds chords via sola-river
user presses ⌘T (only while the browser is focused)
  → Topic::Chord → shell lookup_shortcut
  → Topic::MenuAction { app_id, action_id: "new-tab" }
  → bus → Msg::Bus → intent_for_menu_action → BrowserIntent::NewTab
  → engine
```

Because the shell routes `MenuAction` to the **focused** app only, browser
shortcuts fire exactly when the browser is focused, and non-meta keys still
reach the page (only ⌘/meta items are grabbed by the shell).

**Theme:**

```
Topic::Theme(BusTheme)
  → apply_theme_update(&msg, &mut self.theme)   // iced theme + fonts + selection
  → App::theme() returns self.theme.clone()
  → chrome (tab strip, URL bar, buttons) restyles live
```

## Menu set

One top-level **"Browser"** menu (this is `menus[0]`, the bold app-title slot in
the menubar), all items meta-bound so non-meta keys still reach the page:

| Item         | Chord | Intent            | Existing wpe `Msg` / action      |
| ------------ | ----- | ----------------- | -------------------------------- |
| New Tab      | ⌘T    | `NewTab(blank)`   | `OpenTab`                        |
| Close Tab    | ⌘W    | `CloseActiveTab`  | `CloseTab(cached_active)`        |
| Reopen Closed Tab | ⌘⇧T | `ReopenClosedTab` | pop `session.json` closed stack |
| Reload       | ⌘R    | `Reload`          | `NavReload`                      |
| Focus URL    | ⌘L    | `FocusUrl`        | `text_input::focus(url_field)`   |
| Back         | ⌘←    | `Back`            | `NavBack`                        |
| Forward      | ⌘→    | `Forward`         | `NavForward`                     |
| Quit Browser | ⌘Q    | `Quit`            | `iced::exit()` (via self-quit)   |

Built with `BusSetup::app_menu("Browser", [ (id, label, chord), … ])`. A divider
before Quit would require the `app_menu_definition(MenuDefinition)` form with an
explicit `MenuItem::Divider`; it is optional cosmetic polish, not required.

"New Tab" opens the browser's default new-tab page (the existing `OpenTab`
behavior); "Focus URL" selects/focuses the chrome URL `text_input`.

## Components / files (wpe; cef mirrors)

- **`crates/sola-browser-wpe/Cargo.toml`** — add
  `sola-kit = { path = "../sola-kit" }`.
- **`crates/sola-browser-wpe/src/integration.rs` (new)** — `browser_menu()`,
  `BrowserIntent`, `intent_for_open_url`, `intent_for_menu_action`,
  `handle_bus`, and the unit tests.
- **`crates/sola-browser-wpe/src/main.rs`** —
  - `main()`: insert the `BusSetup::new(APP_ID).subscribe(&[…]).app_menu("Browser",
    […]).install()` block; load kit fonts and set `.default_font(fonts::ui())`;
    add `.theme(App::theme)` to the iced builder.
  - `App`: add `theme: iced::Theme` (init from `sola_kit::default_theme()`).
  - `Msg`: add `Bus(Arc<sola_bus::Message>)`.
  - `impl App`: `subscription()` += `bus_subscription().map(Msg::Bus)` in the
    existing batch; `update()` += `Msg::Bus(m) => integration::handle_bus(self, m)`;
    add `fn theme(&self) -> iced::Theme { self.theme.clone() }`.
  - `mod integration;` declared.

cef receives the identical set of changes against its mirrored `main.rs`
(`CloseTab`, `NavBack`, etc. have the same names; the CEF engine `Cmd` set is
the parity equivalent).

### Subscription set

Subscribe to the kinds the browser actually consumes:
`&[TopicKind::Theme, TopicKind::OpenUrl, TopicKind::MenuAction, TopicKind::CloseApp]`.
(`TopicKind::ALL` would also work, as in `sola-monitor`, but the targeted set
avoids unrelated traffic through the 8 ms poller.)

## Error handling / edge cases

- **Bus unavailable.** If `BusSetup::install()` cannot reach the bus host, the
  browser must still run standalone (chrome works; no theme/menu/OpenUrl). The
  plan verifies `install()` degrades rather than panics on connect-timeout; if
  it panics today, wrap the connect so a missing bus is non-fatal (log a warn,
  continue without the global client).
- **Close last tab.** Never drop below one tab — closing the last tab opens a
  fresh blank tab, so there is no empty render state.
- **URL hygiene.** OpenUrl URLs arrive pre-formed (http/https); still pass them
  through the existing `normalize_url` defensively before `Cmd::OpenTab`.
- **Two browsers running at once.** wpe and cef publish menus under distinct
  app_ids, so menus do not collide; each acts only on `MenuAction` /`CloseApp`
  addressed to its own `APP_ID` (enforced by `is_self_quit` and an explicit
  `payload.app_id == APP_ID` guard in `handle_bus`).
- **MenuAction while unfocused.** Cannot happen — the shell only routes
  `MenuAction` to the focused app — but the app_id guard makes it safe
  regardless.

## Testing

- **Unit (no bus).** The pure mappers in `integration.rs`:
  - `intent_for_menu_action("new-tab")` → `NewTab { activate: true, .. }`;
    `"reload"` → `Reload`; `"quit"` → `Quit`; unknown id → `None`.
  - `intent_for_open_url(&OpenUrlRequest { url, activate })` preserves the URL
    and honors `activate`.
  - Existing `normalize_url` tests stay.
  Same suite in both crates (parity).
- **Manual smoke (running sola).** `solactl open <url>` opens an active tab;
  ⌘T/⌘W/⌘R/⌘L/⌘←/⌘→ work when the browser is focused; editing the theme in the
  `sola-kit` storybook restyles the browser chrome live.

## Out of scope (future work)

- Window-raise / focus-on-OpenUrl, and launch-if-not-running (OpenUrl is
  ephemeral/non-sticky, so autostart needs shell or session-manager
  involvement).
- Roadmap product features unchanged: downloads, bookmarks, history UI,
  devtools, cookie/profile persistence (see `docs/vault/sola-browser.md`).
- No keyboard shortcuts beyond the published menu chords — the app-menu **is**
  the shortcut mechanism.

## References

- `docs/vault/sola-browser.md` — current browser architecture + roadmap.
- `crates/sola-monitor/src/main.rs` — canonical kit-consumer wiring template.
- `crates/sola-kit/src/app.rs` — `BusSetup`, `bus_subscription`,
  `apply_theme_update`, `is_self_quit`, `window_settings`.
- `crates/sola-bus/src/topics.rs` — `OpenUrl`, `SetAppMenu`, `MenuAction` types.
- Memory: `project_browser_engine`, `project_app_shortcut_menu_flow`,
  `project_theme_persistence_flow`.
