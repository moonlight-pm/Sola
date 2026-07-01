# Floating Windows — Phase D2: Self-drawn Titlebar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a floating sola-kit app draw its own in-window titlebar (title +
close + drag-to-move); sola-river services the resulting Wayland CSD move/resize
requests by reusing D1's op loop.

**Architecture:** In-window CSD, not a shell overlay. A kit `titlebar` component
+ a `FloatState` tracker let an app opt into a bar when floating. Dragging the
bar calls `iced::window::drag(id)` → `xdg_toplevel.move` → river
`pointer_move_requested` → sola-river `op_start_pointer` (the exact D1 op loop).
No sola-shell changes, no new bus topics, no new bus subscriptions.

**Tech Stack:** Rust; iced 0.14 (via `sola_kit::iced` re-export);
`wayland-client` + macro-generated river-window-management-v1 bindings;
`sola-bus`.

Design: `docs/specs/2026-06-30-floating-windows-phase-d2-titlebar-design.md`.
Builds on D1: `docs/specs/2026-06-29-floating-windows-phase-d1-move-resize-design.md`.

## Global Constraints

- **Build only with `cargo make build [<target>]`** — never raw `cargo build`
  or `cp` (keeps the build system tested). Building needs no permission.
- **NEVER run `cargo make install` (or any variant) without explicit,
  per-install user permission.** The end-to-end smoke in Task 6 requires an
  install + a live TTY session — that step is **user-run**, not agent-run.
- **`sola-kit` is workspace-excluded.** From the repo root `-p sola-kit` will not
  resolve. Build it with `cargo make build sola-kit`; test it with
  `cargo test --manifest-path crates/sola-kit/Cargo.toml`. `sola-river` is a
  normal workspace member (`cargo test -p sola-river`).
- **Prefer Serena symbol-aware tools** (`find_symbol`, `replace_symbol_body`,
  `insert_after_symbol`) for all code edits; built-in Edit only where a symbolic
  edit doesn't fit.
- **Reuse D1.** Do not touch the op loop (`op_delta`/`op_release`/`op_end`,
  `op::drive`, `op::on_delta`, `op::on_released`) or `manage.rs`. D2 adds only a
  new *entry* into `op.rs` and two new event arms.
- **Chrome styling:** borders/fills only — no drop shadows (they render as hard
  rectangles in this renderer; `project_shell_no_soft_shadows`).
- Commit after each task. End every commit message with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

---

## Task 1: sola-river — `op::begin_for` + edges→corner mapping

Add a second entry point into the D1 op state machine that takes the target
window explicitly (from a Wayland event) and, for resize, an explicit corner
(from requested edges). Refactor `on_pressed` to delegate to it so the
floating-gate / geometry / corner logic lives in one place.

**Files:**
- Modify: `crates/sola-river/src/client/op.rs`
- Test: `crates/sola-river/src/client/op.rs` (existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `OpKind`, `Corner`, `Rect`, `OpState`, `pick_corner`, `AppData`
  (`op`, `floating`, `pointer_window`, `pointer_pos`, `registry.geometry`) — all
  existing from D1.
- Produces:
  - `pub fn begin_for(state: &mut AppData, kind: OpKind, window_id: u32, corner: Option<Corner>)`
  - `pub fn edges_to_corner(edges: Edges) -> Corner`

- [ ] **Step 1: Add the `Edges` import at the top of `op.rs`**

Insert with the other protocol imports (mirror `manage.rs`, which already uses
this exact path):

```rust
use crate::protocol::river_window_management_v1::river_window_v1::Edges;
```

- [ ] **Step 2: Add `edges_to_corner` and `begin_for` (insert after `on_pressed`)**

```rust
/// Map an xdg-shell resize `edges` bitfield to the corner our resize op grabs.
/// The protocol guarantees `edges` never sets both top+bottom or both
/// left+right; a single-edge request collapses to the corner on that edge
/// (free axis defaults to right/bottom). `none` → BottomRight.
pub fn edges_to_corner(edges: Edges) -> Corner {
    match (edges.contains(Edges::Top), edges.contains(Edges::Left)) {
        (true, true) => Corner::TopLeft,
        (true, false) => Corner::TopRight,
        (false, true) => Corner::BottomLeft,
        (false, false) => Corner::BottomRight,
    }
}

/// Begin an interactive op on an explicit window. Shared by the Meta-drag
/// pointer-binding path (`on_pressed`) and the CSD-request path
/// (`pointer_move_requested` / `pointer_resize_requested`). Floating-gated.
///
/// `corner`: `Some(c)` uses that corner (resize from requested edges);
/// `None` on a resize falls back to `pick_corner` from the pointer position;
/// ignored for a move.
pub fn begin_for(state: &mut AppData, kind: OpKind, window_id: u32, corner: Option<Corner>) {
    if state.op.is_some() {
        return;
    }
    if !state.floating.contains(&window_id) {
        tracing::debug!(window_id, ?kind, "interactive op ignored: window not floating");
        return; // move/resize is floating-only
    }
    let Some(g) = state.registry.geometry(window_id) else {
        tracing::debug!(window_id, "interactive op ignored: geometry unknown");
        return;
    };
    let start = Rect { x: g.x, y: g.y, w: g.width, h: g.height };
    let corner = match kind {
        OpKind::Resize => corner.or_else(|| Some(pick_corner(start, state.pointer_pos))),
        OpKind::Move => None,
    };
    tracing::info!(window_id, ?kind, ?corner, ?start, "begin interactive op");
    state.op = Some(OpState {
        kind,
        window_id,
        start,
        corner,
        started: false,
        released: false,
    });
}
```

- [ ] **Step 3: Refactor `on_pressed` to delegate**

Replace `on_pressed`'s body (keep its entry-debug log) so it resolves the target
from the pointer and forwards to `begin_for`:

```rust
pub fn on_pressed(state: &mut AppData, kind: OpKind) {
    tracing::debug!(
        ?kind,
        pointer_window = ?state.pointer_window,
        floating = ?state.floating,
        op_active = state.op.is_some(),
        "Meta-drag pointer binding pressed"
    );
    let Some(wid) = state.pointer_window else {
        tracing::debug!("Meta-drag ignored: no window under pointer");
        return;
    };
    // corner=None → begin_for picks it from pointer_pos for a resize.
    begin_for(state, kind, wid, None);
}
```

- [ ] **Step 4: Add `edges_to_corner` unit tests (in the existing `tests` module)**

```rust
#[test]
fn edges_map_to_corners() {
    assert_eq!(edges_to_corner(Edges::Top | Edges::Left), Corner::TopLeft);
    assert_eq!(edges_to_corner(Edges::Top | Edges::Right), Corner::TopRight);
    assert_eq!(edges_to_corner(Edges::Bottom | Edges::Left), Corner::BottomLeft);
    assert_eq!(edges_to_corner(Edges::Bottom | Edges::Right), Corner::BottomRight);
    // single-edge requests collapse to a corner on that edge
    assert_eq!(edges_to_corner(Edges::Top), Corner::TopRight);
    assert_eq!(edges_to_corner(Edges::Left), Corner::BottomLeft);
    assert_eq!(edges_to_corner(Edges::empty()), Corner::BottomRight);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p sola-river op::`
Expected: PASS (new `edges_map_to_corners` plus the existing D1 op tests).

> Note: `begin_for`'s floating-gate/geometry branches need a live `AppData`
> (registry geometry) and are exercised by the Task 6 smoke, not a unit test —
> consistent with how D1 covers its Wayland-touching paths.

- [ ] **Step 6: Build**

Run: `cargo make build sola-river`
Expected: builds clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-river/src/client/op.rs
git commit -m "feat(sola-river): op::begin_for + edges→corner for CSD-driven move/resize

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: sola-river — handle `pointer_move_requested` / `pointer_resize_requested`

Wire the two river window events (currently unhandled) into `begin_for`. They are
each "followed by a manage_start," so the existing `op::drive` call in
`handle_manage_start` issues `op_start_pointer` on the next cycle — no `manage.rs`
change.

**Files:**
- Modify: `crates/sola-river/src/client/window.rs` (`impl Dispatch<RiverWindowV1, ()>`, the `match event` block)

**Interfaces:**
- Consumes: `op::begin_for`, `op::edges_to_corner`, `op::OpKind` (Task 1);
  `window_id` (already resolved at the top of the dispatch via
  `state.windows_by_object.get(&window.id())`).

- [ ] **Step 1: Add the two event arms**

Insert into the `match event { … }` block, just before the final `_ => {}`
(mirror the style of the neighbouring `FullscreenRequested` arms):

```rust
Event::PointerMoveRequested { .. } => {
    // Client-side-decoration move (e.g. a kit titlebar drag → xdg_toplevel.move).
    // Reuse D1's move op; begin_for gates on `floating` and is a no-op for
    // tiled windows (Meta+drag still moves those). op_start_pointer is issued
    // on the manage_start that follows this event.
    op::begin_for(state, op::OpKind::Move, window_id, None);
}
Event::PointerResizeRequested { edges, .. } => {
    // CSD resize (edge/corner drag). `edges` is a bitfield enum arg
    // (`WEnum<Edges>`); resolve it, defaulting an unknown value to a
    // pointer-position-derived corner (None).
    let corner = edges.into_result().ok().map(op::edges_to_corner);
    op::begin_for(state, op::OpKind::Resize, window_id, corner);
}
```

> **Build-driven type check:** wayland-scanner emits a bitfield enum arg as
> `WEnum<Edges>`, so `.into_result().ok().map(op::edges_to_corner)` is correct.
> If the compiler reports `edges` is already `Edges` (not `WEnum`), drop the
> `.into_result().ok()` and write `Some(op::edges_to_corner(edges))`. No extra
> imports are needed in `window.rs` — `into_result` is inherent on `WEnum` and
> `op::edges_to_corner` owns the `Edges` type.

- [ ] **Step 2: Build**

Run: `cargo make build sola-river`
Expected: builds clean. (If it fails on the `edges` type, apply the fallback in
the note above and rebuild.)

- [ ] **Step 3: Commit**

```bash
git add crates/sola-river/src/client/window.rs
git commit -m "feat(sola-river): service CSD pointer_move/resize_requested via D1 op

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: sola-kit — `FloatState` tracker

A kit app doesn't know its own sola-river `window_id`; correlate it by
`(app_id, title)` from `Topic::Windows`, then read the float bit from
`Topic::WindowFloating` (both keyed by `window_id`).

**Files:**
- Create: `crates/sola-kit/src/float.rs`
- Modify: `crates/sola-kit/src/lib.rs`
- Test: `crates/sola-kit/src/float.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `sola_bus::Message`, `sola_bus::topics::{Topic, Window, WindowFloating}`,
  `Topic::parse(&Message) -> Option<Topic>`, `Topic::to_message()`.
- Produces:
  - `pub struct FloatState`
  - `FloatState::new(app_id: impl Into<String>) -> Self`
  - `FloatState::update(&mut self, msg: &Message)`
  - `FloatState::is_floating(&self, title: &str) -> bool`
  - `FloatState::is_floating_any(&self) -> bool`

- [ ] **Step 1: Write the failing test file `crates/sola-kit/src/float.rs`**

```rust
//! Per-app float-state tracking for kit apps that draw their own titlebar.
//!
//! An app doesn't know its own sola-river `window_id`, so we learn it by
//! matching `(app_id, title)` from `Topic::Windows`, then track the float bit
//! from the sticky `Topic::WindowFloating`. Feed [`update`] every bus message
//! (from the app's `bus_subscription` fold); read [`is_floating`] /
//! [`is_floating_any`] in `view`.
//!
//! [`update`]: FloatState::update
//! [`is_floating`]: FloatState::is_floating
//! [`is_floating_any`]: FloatState::is_floating_any

use std::collections::{HashMap, HashSet};

use sola_bus::Message;
use sola_bus::topics::Topic;

#[derive(Debug, Default)]
pub struct FloatState {
    app_id: String,
    /// This app's surfaces: sola-river `window_id` keyed by window title.
    ids_by_title: HashMap<String, u32>,
    /// Currently-floating `window_id`s (all apps; filtered on read).
    floating: HashSet<u32>,
}

impl FloatState {
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            ..Default::default()
        }
    }

    /// Fold one bus message. Call from the app's bus-message update arm.
    pub fn update(&mut self, msg: &Message) {
        match Topic::parse(msg) {
            // Windows is the full list each time — rebuild so closed windows drop.
            Some(Topic::Windows(windows)) => {
                self.ids_by_title.clear();
                for w in windows {
                    if w.app_id == self.app_id {
                        self.ids_by_title.insert(w.title, w.window_id);
                    }
                }
            }
            Some(Topic::WindowFloating(wf)) => {
                if wf.floating {
                    self.floating.insert(wf.window_id);
                } else {
                    self.floating.remove(&wf.window_id);
                }
            }
            _ => {}
        }
    }

    /// Is this app's surface with `title` currently floating?
    pub fn is_floating(&self, title: &str) -> bool {
        self.ids_by_title
            .get(title)
            .is_some_and(|id| self.floating.contains(id))
    }

    /// Is any of this app's surfaces floating? Convenient for single-window apps.
    pub fn is_floating_any(&self) -> bool {
        self.ids_by_title.values().any(|id| self.floating.contains(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::{Window, WindowFloating};

    fn win(window_id: u32, app_id: &str, title: &str) -> Window {
        Window {
            window_id,
            app_id: app_id.into(),
            title: title.into(),
            pid: None,
        }
    }

    #[test]
    fn tracks_own_float_by_app_id_and_title() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(
            &Topic::Windows(vec![
                win(7, "sola-monitor", "Monitor"),
                win(9, "other-app", "Other"),
            ])
            .to_message(),
        );
        assert!(!fs.is_floating_any());

        // our window floats
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 7, floating: true }).to_message());
        assert!(fs.is_floating_any());
        assert!(fs.is_floating("Monitor"));

        // another app's float does not count as ours
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 9, floating: true }).to_message());
        assert!(fs.is_floating("Monitor"));
        assert!(!fs.is_floating("Other")); // "Other" isn't ours

        // unfloat clears it
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 7, floating: false }).to_message());
        assert!(!fs.is_floating_any());
        assert!(!fs.is_floating("Monitor"));
    }

    #[test]
    fn closed_window_drops_from_tracking() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(&Topic::Windows(vec![win(7, "sola-monitor", "Monitor")]).to_message());
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 7, floating: true }).to_message());
        assert!(fs.is_floating_any());
        // window closes → Windows no longer lists it
        fs.update(&Topic::Windows(vec![]).to_message());
        assert!(!fs.is_floating_any());
    }
}
```

- [ ] **Step 2: Register the module in `crates/sola-kit/src/lib.rs`**

Add `pub mod float;` beside the other `pub mod` lines, and add the re-export
beside `pub use app::{…}`:

```rust
pub mod float;
```
```rust
pub use float::FloatState;
```

- [ ] **Step 3: Run the tests (expect FAIL first, then PASS)**

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml float::`
Expected: both tests PASS. (If `Topic::parse`/`to_message` mismatch, they live in
`sola-bus/src/topic.rs` — `parse(msg: &Message)` / `to_message(&self)`.)

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/float.rs crates/sola-kit/src/lib.rs
git commit -m "feat(sola-kit): FloatState — per-app float tracking by (app_id,title)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: sola-kit — `titlebar` component

An in-window bar: title + close button, whole bar drags. Styled from the iced
palette (borders/fills, no shadow).

> Design note: the design doc said "ShellStyle", but `ShellStyle` is a
> shell-held, bus-derived struct not available inside an iced component style fn.
> Like every other kit component, the titlebar styles from
> `theme.extended_palette()`. Same visual intent (raised bg + hairline border),
> realizable API.

**Files:**
- Create: `crates/sola-kit/src/components/titlebar.rs`
- Modify: `crates/sola-kit/src/components/mod.rs`

**Interfaces:**
- Consumes: `iced` widgets (`button`, `container`, `mouse_area`, `row`, `text`,
  `Space`); `crate::components::button as kit_btn` (`ghost`);
  `crate::components::text as kit_text` (`body`).
- Produces:
  - `pub const HEIGHT: f32`
  - `pub fn titlebar<'a, Message: Clone + 'a>(title: impl Into<String>, on_drag: Message, on_close: Message) -> Element<'a, Message, Theme>`

- [ ] **Step 1: Write `crates/sola-kit/src/components/titlebar.rs`**

```rust
//! In-window titlebar for a floating kit app that opts into drawing chrome.
//!
//! Title text + a close button; the whole bar is a drag handle. `on_drag`
//! fires on press anywhere on the bar; the close button consumes its own press
//! and fires `on_close` instead. The app maps `on_drag` to
//! `iced::window::drag(id)` and `on_close` to its close action.
//!
//! Borders/fills only — no drop shadow (they render hard here).

use iced::widget::{Space, button, container, mouse_area, row, text};
use iced::{Alignment, Border, Element, Length, Theme};

use crate::components::button as kit_btn;
use crate::components::text as kit_text;

/// Titlebar strip height, logical px.
pub const HEIGHT: f32 = 28.0;

pub fn titlebar<'a, Message>(
    title: impl Into<String>,
    on_drag: Message,
    on_close: Message,
) -> Element<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let label = text(title.into()).size(13).style(kit_text::body);
    let close = button(text("✕").size(12))
        .padding([2, 8])
        .style(kit_btn::ghost)
        .on_press(on_close);

    let bar = container(
        row![label, Space::with_width(Length::Fill), close]
            .align_y(Alignment::Center)
            .spacing(8)
            .padding([0, 8]),
    )
    .width(Length::Fill)
    .height(Length::Fixed(HEIGHT))
    .style(bar_style);

    mouse_area(bar).on_press(on_drag).into()
}

/// Raised background + hairline bottom-ish border. No shadow.
fn bar_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(p.background.weak.color.into()),
        border: Border {
            color: p.background.strong.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
```

- [ ] **Step 2: Register in `crates/sola-kit/src/components/mod.rs`**

Add the module beside the other `pub mod` lines and the factory re-export beside
`pub use badge::…`:

```rust
pub mod titlebar;
```
```rust
pub use titlebar::titlebar;
```

- [ ] **Step 3: Build the kit**

Run: `cargo make build sola-kit`
Expected: builds clean.

> If `.align_y` / `Space::with_width` / `kit_text::body` / `kit_btn::ghost` names
> differ, confirm against a sibling component (e.g. `components/toolbar.rs`) and
> adjust — these are the established kit spellings at time of writing.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/components/titlebar.rs crates/sola-kit/src/components/mod.rs
git commit -m "feat(sola-kit): titlebar component (title + close + drag handle)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: sola-kit — storybook page for the titlebar

Every kit component dogfoods in the storybook. Add a `Titlebar` page.

**Files:**
- Create: `crates/sola-kit/src/storybook/pages/titlebar.rs`
- Modify: `crates/sola-kit/src/storybook/pages/mod.rs`
- Modify: `crates/sola-kit/src/storybook/mod.rs` (the `Page` enum + its `impl`
  + the per-page view dispatch)

**Interfaces:**
- Consumes: `sola_kit::components::titlebar::titlebar`; `crate::storybook::Msg`
  (has a `Noop` variant — used by the stateless `button` page).

- [ ] **Step 1: Write `crates/sola-kit/src/storybook/pages/titlebar.rs`**

```rust
//! Titlebar showcase — the floating-window titlebar in isolation. Drag/close
//! are inert here (map to `Noop`); the real behaviour lives in a consumer app.

use iced::widget::{column, container, text};
use iced::{Element, Length};

use sola_kit::components::text::caption;
use sola_kit::components::titlebar::titlebar;

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let demo = container(titlebar("Floating Window", Msg::Noop, Msg::Noop))
        .width(Length::Fixed(420.0));

    column![
        text("Titlebar").size(20),
        text("Drawn in-window by a floating kit app. Bar drags to move; ✕ closes.")
            .style(caption),
        demo,
    ]
    .spacing(12)
    .into()
}
```

- [ ] **Step 2: Register the page module in `pages/mod.rs`**

Add beside the other `pub mod` lines:

```rust
pub mod titlebar;
```

- [ ] **Step 3: Add the `Page::Titlebar` variant + all its exhaustive-match arms**

In `crates/sola-kit/src/storybook/mod.rs`:

1. `enum Page { … }` — add `Titlebar,`.
2. `Page::ALL` — add `Page::Titlebar,` in the Components group (e.g. after
   `Page::Button`).
3. `fn label(self)` — add `Page::Titlebar => "Titlebar",`.
4. `fn section(self)` — add `Page::Titlebar` to the `Some("Components")` arm's
   `|` list.
5. `fn atoms(self)` — add `Page::Titlebar => &[Bg, BgRaised, Border, Fg],`
   (all four are already imported in that `use AtomField::{…}`).

- [ ] **Step 4: Add the view dispatch arm**

Find the per-page match (the method that returns the page body — it maps each
`Page` to `pages::<name>::view()`, e.g. `Page::Button => pages::button::view()`).
Add, mirroring the stateless `button` arm (no `.map`, the page uses `Msg`
directly):

```rust
Page::Titlebar => pages::titlebar::view(),
```

- [ ] **Step 5: Build the storybook**

Run: `cargo make build sola-kit`
Expected: builds clean (all exhaustive `match self` arms on `Page` satisfied).

- [ ] **Step 6: (Optional, user or dev) visual check**

Run: `cargo run --manifest-path crates/sola-kit/Cargo.toml` in a Wayland
session; select **Titlebar** in the sidebar and confirm the bar renders with a
title and ✕. (Dev-run only — not an install.)

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/src/storybook/
git commit -m "feat(sola-kit): storybook page for the titlebar component

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: sola-monitor — opt into the titlebar (end-to-end dogfood)

The consumer that ties the whole feature together and is the live smoke vehicle.
sola-monitor is a single-window `iced::application` whose `update` already
returns `Task<Msg>` and which already subscribes to `TopicKind::ALL`.

**Files:**
- Modify: `crates/sola-monitor/src/main.rs`

**Interfaces:**
- Consumes: `sola_kit::FloatState` (Task 3), `sola_kit::components::titlebar::titlebar`
  (Task 4), `iced::window::{drag, latest, Id}`, `Topic::CloseApp`.
- Produces: nothing (leaf consumer).

- [ ] **Step 1: Add fields to `App`**

In the `struct App { … }` add:

```rust
    /// Float-state tracker so a floating Monitor draws its own titlebar.
    float: sola_kit::FloatState,
    /// This app's window id, learned on boot (for `iced::window::drag`).
    window_id: Option<iced::window::Id>,
```

In `impl Default for App { fn default() -> Self { … } }` initialise them in the
struct literal:

```rust
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
```

- [ ] **Step 2: Add a boot task so `App` learns its window id**

Add a boot fn on `App` (pattern matches sola-shell's `boot`):

```rust
    fn boot() -> (Self, iced::Task<Msg>) {
        (Self::default(), iced::window::latest().map(Msg::WindowReady))
    }
```

In `main`, change the application constructor's first arg from `App::default` to
`App::boot`:

```rust
    let app = iced::application(App::boot, App::update, App::view)
```

- [ ] **Step 3: Add the message variants**

In `enum Msg { … }`:

```rust
    /// Window id resolved on boot (for interactive move).
    WindowReady(Option<iced::window::Id>),
    /// Titlebar drag started — begin an interactive move.
    TitleDrag,
    /// Titlebar close button.
    TitleClose,
```

- [ ] **Step 4: Handle the new messages + feed `FloatState`**

In `fn update(&mut self, msg: Msg) -> Task<Msg>`:

- At the top of the existing `Msg::BusMessage(msg) => { … }` arm, add:
  ```rust
  self.float.update(&msg);
  ```
- Add three new arms (return `Task::none()` where noted):
  ```rust
  Msg::WindowReady(id) => {
      self.window_id = id;
      return Task::none();
  }
  Msg::TitleDrag => {
      if let Some(id) = self.window_id {
          return iced::window::drag(id);
      }
      return Task::none();
  }
  Msg::TitleClose => {
      if let Ok(mut bus) = sola_kit::app::bus().lock() {
          let _ = bus.emit(sola_kit::sola_bus::topics::Topic::CloseApp(APP_ID.into()));
      }
      return Task::none();
  }
  ```

> `Task` is already in scope in this file (update returns `Task<Msg>`). If the
> `Topic`/`bus` paths differ from other emit sites in this file, reuse whatever
> that file already imports for bus emits.

- [ ] **Step 5: Wrap the view in a titlebar when floating**

In `fn view(&self) -> Element<'_, Msg>`, bind the current returned top-level
element to `let content = …;` and return:

```rust
    if self.float.is_floating_any() {
        iced::widget::column![
            sola_kit::components::titlebar::titlebar("Monitor", Msg::TitleDrag, Msg::TitleClose),
            content,
        ]
        .into()
    } else {
        content
    }
```

- [ ] **Step 6: Build**

Run: `cargo make build sola-monitor`
Expected: builds clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sola-monitor/src/main.rs
git commit -m "feat(sola-monitor): draw a titlebar when floating (D2 dogfood)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 8: End-to-end smoke — USER-RUN (needs install + a TTY)**

> **Do not run `cargo make install` as the agent.** Ask the user to run the
> smoke, or wait for explicit install permission. The manual check:
> 1. Install the three changed binaries (`sola-river`, `sola-monitor`; the kit
>    is linked in): user runs `cargo make install sola-river sola-monitor`.
> 2. Launch sola from a TTY; open sola-monitor.
> 3. `Meta`+KP-`*` to float it → a titlebar appears at the top of the window.
> 4. Drag the titlebar → the window moves (compositor-driven, no lag).
> 5. Drag a window edge/corner → the window resizes (CSD resize path).
> 6. Click ✕ → the window closes.
> 7. Zone it back (any `Meta`+numpad zone) → titlebar disappears.

---

## Self-Review

- **Spec coverage:** §2 CSD flow → Tasks 1–2 (river) + Task 6 (drag call). §3
  river handlers → Task 2 (arms) + Task 1 (`begin_for`/edges). §4.1 `FloatState`
  → Task 3. §4.2 titlebar component → Task 4. §4.3 opt-in composition → Task 6
  view wrap. Kit storybook convention → Task 5. §5 gap / §6–7 tests / §8 risks →
  reflected in Task 6 smoke + Task 1/2 notes. No shell changes, no bus topics,
  no new subscriptions — none appear in any task. ✓
- **Placeholder scan:** none — every code step carries real code; the two
  deferrals (edges `WEnum` type; view wrap point) are build-driven with explicit
  fallbacks. ✓
- **Type consistency:** `begin_for(state, kind, window_id, corner)` and
  `edges_to_corner(edges) -> Corner` are used with matching signatures in Task 2;
  `FloatState::{new,update,is_floating,is_floating_any}` match between Tasks 3
  and 6; `titlebar(title, on_drag, on_close)` matches between Tasks 4, 5, 6. ✓
