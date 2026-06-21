# Browser chrome density, cmd-click new tab, and edit menu — Design

**Goal:** Three browser improvements landing together: (1) less-compressed chrome
via a new `large` tab variant in sola-kit plus more nav-bar padding, (2) cmd /
middle-click on a link opens it in a background tab, and (3) an Edit menu
(copy/paste/cut/select-all/undo/redo) routed to whichever surface is focused.

**Architecture:** All shared logic stays in `sola-browser-core`; engine-specific
work is confined to `sola-browser-wpe` (WebKit C shim) and `sola-browser-cef`
(CEF handlers), behind the existing `Cmd<E>` / `Engine` abstraction. The tab
size variant lives in `sola-kit`.

**In scope:** chrome density, cmd-click→tab, edit menu.
**Out of scope (deferred):** DevTools (off-screen rendering makes an in-app
inspector a large separate effort; remote-debugging path parked), Bitwarden
(its own brainstorm).

---

## Feature 1 — Chrome density

### sola-kit: a tab size variant

`crates/sola-kit/src/components/sidebar.rs` currently hard-codes one size:
row padding `[6,10]`, label font 13, close button 15, column padding `[8,6]`,
`SPACE_XS` between rows. The kit has no size-variant pattern yet, so introduce a
small, explicit one — the canonical example future components can copy:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabSize {
    #[default]
    Normal,
    Large,
}

struct TabMetrics { row_pad_v: u16, row_pad_h: u16, font: u16, close: u16, gap: f32 }

impl TabSize {
    fn metrics(self) -> TabMetrics {
        match self {
            TabSize::Normal => TabMetrics { row_pad_v: 6,  row_pad_h: 10, font: 13, close: 15, gap: SPACE_XS },
            TabSize::Large  => TabMetrics { row_pad_v: 10, row_pad_h: 12, font: 14, close: 17, gap: SPACE_SM },
        }
    }
}
```

- Keep `vertical_tabs(tabs, hovered, on_hover)` as-is (it calls the sized form
  with `TabSize::Normal`), so existing consumers are untouched.
- Add `vertical_tabs_sized(tabs, hovered, on_hover, size: TabSize)` carrying the
  metrics into the row/column builders.
- Export `TabSize` from the components module.
- Storybook: the Sidebar page (`crates/sola-kit/src/storybook/pages/`) shows both
  `Normal` and `Large` side by side so the variant is dogfooded.

### browser: more padding

`crates/sola-browser-core/src/app.rs`:

- `CHROME_HEIGHT` 38.0 → 46.0
- `view_nav_bar`: row `padding 6 → 10`, `spacing 6 → 8`, URL `text_input`
  `padding 6 → 9`.
- `view_tab_sidebar`: call `vertical_tabs_sized(..., TabSize::Large)`.

Values are deliberate, not derived; tune once visually if needed. No new
abstraction in the browser — just constants and the variant call.

---

## Feature 2 — Cmd / middle-click → background tab

A link opened with **middle-click, Ctrl-left-click, or ⌘/Super-left-click**
opens in a new **background** tab (the current tab stays active). The engine
handles this entirely on its worker side: it already shares the `next_id`
`AtomicU64` with the chrome, so it mints a tab id, opens the tab, and does *not*
change the active tab. The chrome observes the new tab on its next `Tick`
(snapshot poll) — no new chrome message or `Cmd` variant is required.

### WPE

The WebKit view exposes the navigation decision via the `decide-policy` signal.
Add a handler in the WPE C shim (`crates/sola-browser-wpe/src/` C source +
header, compiled by `build.rs`):

- Connect `decide-policy` per webview in `open_tab`
  (`crates/sola-browser-wpe/src/engine.rs`).
- For a `WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION`, read
  `webkit_navigation_policy_decision_get_navigation_action()`, then
  `webkit_navigation_action_get_mouse_button()` and
  `webkit_navigation_action_get_modifiers()`.
- If button == 2 (middle) **or** (button == 1 and modifiers include Ctrl or
  Super/Meta): call `webkit_policy_decision_ignore(decision)` (suppress in-place
  nav) and invoke a Rust callback with the target URI from
  `webkit_navigation_action_get_request()`.
- Otherwise return `FALSE` (default handling).

Rust side: register a `sola_wpe_set_open_in_new_tab_callback`-style callback
(mirroring the existing buffer/cursor callbacks) whose user-data is the
`WorkerCtx`. On fire, the worker mints `TabId(next_id.fetch_add(1))` and calls
the existing `open_tab(ctx, id, uri)` without a following `SetActiveTab`.

### CEF

CEF routes ctrl/cmd/middle-click and `target=_blank` through the life-span
handler's popup callback. In `crates/sola-browser-cef/src/engine.rs`:

- Add `on_before_popup` to the life-span handler (built via `wrap_*` macro near
  the existing `on_before_close`).
- Read the `target_url`; return `true` to **cancel** the native popup, and
  instead open it as a background tab: mint `TabId(next_id.fetch_add(1))`, call
  the existing `open_tab(state, id, target_url)`, no `SetActiveTab`.

### Modifier note

⌘/Meta vs Ctrl mapping differs per engine and per compositor. We accept the
union (middle, Ctrl, Super/Meta) so the user's ⌘-click works regardless of which
mask the key lands on; middle-click is the always-works fallback. Exact masks
are confirmed during implementation against the live engines.

---

## Feature 3 — Edit menu + copy/paste

### Engine command

Add to `crates/sola-browser-core/src/engine.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCmd { Copy, Cut, Paste, SelectAll, Undo, Redo }
```

and a `Cmd::Edit(EditCmd)` variant. Each engine implements it in its
`process_cmd`:

- **WPE**: `webkit_web_view_execute_editing_command(view, name)` with names
  `"Copy" | "Cut" | "Paste" | "SelectAll" | "Undo" | "Redo"` — exposed through
  the C shim. Acts on the active tab's webview.
- **CEF**: on the active browser's `main_frame()`:
  `copy() | cut() | paste() | select_all() | undo() | redo()`.

### The "Edit" menu (second top-level menu)

The bus payload already supports multiple menus per app
(`AppMenuPayload { app_id, menus: Vec<MenuDefinition> }` in
`crates/sola-bus/src/topics.rs`). Today `sola_kit::app::BusSetup::app_menu`
stores a single `MenuDefinition`. Extend `BusSetup` so an app can declare more
than one menu — e.g. an additional `.app_menu_more(label, items)` (or change the
internal store to `Vec<MenuDefinition>` and publish all). The browser then
publishes both:

- **Browser**: existing items (New Tab, Close Tab, Reload, Focus URL, Back,
  Forward, Quit) — unchanged.
- **Edit**: Undo `⌘Z`, Redo `⌘⇧Z`, Cut `⌘X`, Copy `⌘C`, Paste `⌘V`,
  Select All `⌘A`.

These are meta chords, consistent with the rest of Sola's shortcut scheme, so
they register globally via the existing `SetAppMenu → RegisteredChords → Chord →
MenuAction` flow. (Redo is `⌘⇧Z`; confirm the `KeyChord` builder supports a
shifted-meta chord — if not, fall back to `⌘Y`.)

Action ids: `edit-undo`, `edit-redo`, `edit-cut`, `edit-copy`, `edit-paste`,
`edit-select-all`. `intent_for_menu_action` maps each to a new
`BrowserIntent::Edit(EditCmd)`.

### Focus-routed dispatch

`⌘` clipboard keys aren't native clipboard modifiers on Linux (iced text_input
and WebKit-GTK both key off Ctrl), so grabbing them globally breaks nothing —
there is no native ⌘ behavior to preempt. On a `BrowserIntent::Edit(cmd)` the
browser routes to whichever surface is focused:

- **Web content focused** → `Cmd::Edit(cmd)` to the engine (full fidelity:
  honors the real text selection in the page).
- **URL bar focused** → handle in iced (best-effort, since iced exposes no
  text-selection state):
  - `Copy` → `iced::clipboard::write(url_field)` (whole field)
  - `Cut` → `write(url_field)` then clear the field
  - `Paste` → `iced::clipboard::read(Msg::UrlPasted)` → set/append into `url_field`
  - `SelectAll` → `text_input::select_all(url_input_id())` if the operation
    exists in this iced version; otherwise re-focus (no-op)
  - `Undo`/`Redo` → no-op for the URL bar

**Tracking the focused surface** (`App` gains a `url_bar_focused: bool`):

- set **true** on: `BrowserIntent::FocusUrl` (⌘L), `NewBlankTab` (⌘T already
  focuses the bar), and `Msg::UrlInput` (user typing in the bar).
- set **false** on a web-view pointer-press: the engine `Program`
  (`frame.rs::update`) publishes a new `Msg::WebViewFocused` alongside the input
  forward when a `ButtonPressed` lands inside the web-view bounds.

Documented edge case: clicking directly into the URL bar *without* ⌘L or typing
leaves the flag stale (still pointing at web content) until the next typed
character; a subsequent ⌘C would copy from the page. Acceptable for a
best-effort field; ⌘L is the reliable "focus the bar" gesture.

---

## Where it plugs in (touchpoints)

| Concern | File |
| --- | --- |
| `TabSize`, `vertical_tabs_sized` | `crates/sola-kit/src/components/sidebar.rs` |
| Sidebar storybook page | `crates/sola-kit/src/storybook/pages/…` |
| Chrome constants + views | `crates/sola-browser-core/src/app.rs` |
| `EditCmd`, `Cmd::Edit` | `crates/sola-browser-core/src/engine.rs` |
| `BrowserIntent::Edit`, Edit menu items, action ids, focus routing | `crates/sola-browser-core/src/integration.rs` |
| `url_bar_focused`, `Msg::WebViewFocused`, `Msg::UrlPasted`, URL-bar clipboard | `crates/sola-browser-core/src/app.rs` |
| Multi-menu publish | `crates/sola-kit/src/app.rs` (`BusSetup`) |
| WPE: decide-policy shim, edit command, new-tab callback | `crates/sola-browser-wpe/src/engine.rs` + C shim + `build.rs` |
| WPE: web-view-focus publish | `crates/sola-browser-wpe/src/frame.rs` |
| CEF: `on_before_popup`, edit command | `crates/sola-browser-cef/src/engine.rs` |
| CEF: web-view-focus publish | `crates/sola-browser-cef/src/frame.rs` |

---

## Testing

Pure logic is unit-tested; engine/GUI behavior is smoke-tested by the user.

- `sola-browser-core`:
  - `intent_for_menu_action` maps each `edit-*` id to `BrowserIntent::Edit(_)`.
  - focus-routing decision: a small pure helper `edit_target(url_bar_focused) ->
    {Engine | UrlBar}` (or equivalent match) is unit-tested both ways.
  - `url_bar_focused` transitions: a table test over the triggering messages.
- `sola-kit`: `TabSize::metrics()` returns the documented values for
  `Normal`/`Large` (guards against silent drift).
- Engine bits (cmd-click suppression, editing commands, popup cancel) and the
  density look are verified by building + manual smoke (`cargo make install` is
  the user's call).

## Risks / open items

- **Modifier masks** for ⌘/Super differ per engine; middle-click is the
  guaranteed fallback. Confirmed live during implementation.
- **URL-bar fidelity** is intentionally coarse (whole-field copy, no partial
  selection) — iced doesn't expose the selection. ⌘L + retype covers the gap.
- **Redo chord** `⌘⇧Z` depends on `KeyChord` supporting shifted-meta; `⌘Y`
  fallback noted.
- **`text_input::select_all`** availability in this iced version is confirmed
  during implementation; no-op fallback if absent.
