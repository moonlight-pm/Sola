# Floating Windows — Phase D2: Self-drawn Titlebar — Design

**Date:** 2026-06-30
**Status:** Frozen — implemented in `sola-kit` (`titlebar` / `floating_frame` /
`FloatState`); first-party kit apps opt in via `wrap_if_floating`. Part of
[`2026-06-24-floating-windows-design.md`](2026-06-24-floating-windows-design.md)
§6 / Phase D, following D1. The window menu (old D3 / phase C) is
**out of scope — explicitly not wanted.**
**Implementation:** in-window CSD (this freeze). Face clip is a rounded dest-out
punch (`components/float_clip.rs`) because iced 0.14 `clip(true)` is AABB-only.
**Dogfood:** unit tests (resize AABB hits + rounded-rect SDF); storybook Titlebar
page uses a full-bleed body so the bottom-corner clip is visible. Not installed
from the float-corners worktree.
**Gaps:** desk smoke after `install kit` (and apps); CEF/terminal subsurfaces
are covered only when they paint in the iced target before the punch layer.

> **Supersedes the parent design's D2 sketch.** The parent proposed a
> *shell-drawn overlay window per float* (`WindowKind::Titlebar`). That is
> dropped. The titlebar is drawn **in-window by the app itself** via a sola-kit
> component. See §2 for why.

---

## 1. Goal & scope

A floating **sola-kit** app may draw its own titlebar — **title text + close
button + drag-to-move** — as part of its own window. It is **opt-in**: the kit
ships the pieces, the app decides whether and how to draw a bar. Nothing else
(no window menu, no maximize/minimize).

- **sola-shell changes: none.** The shell already emits the one signal needed
  (`Topic::WindowFloating`, from D1).
- **Non-kit / foreign apps are unaffected.** River already steers them to CSD
  (their own titlebars/close buttons). `Meta`+drag (D1) still moves/resizes
  *every* window as a universal fallback, decorations or not.

Out of scope: window menu, maximize/minimize, server-side decorations, drawing
chrome for non-kit clients.

## 2. The architectural decision — in-window CSD via the standard Wayland path

The parent design's overlay approach (a separate shell window floating above
each content window) requires: two-window position tracking over the bus,
reworking D1's "suppress geometry mid-op" to emit live geometry, inserting the
overlay into the composition z-stack above its content, and a question mark over
whether `op_start_pointer` can grab a button pressed on a shell surface. Every
one of those problems exists *because the bar is a different surface than the
window*.

Drawing the bar **inside the app's own window** dissolves all of them: the bar
is part of the surface, so it moves and resizes in lockstep with the window for
free. This is exactly what client-side decorations (CSD) are, and Wayland
already has the machinery:

- A CSD client draws its own decorations and asks the compositor to start an
  interactive move/resize via xdg-shell (`xdg_toplevel.move` / `.resize`).
- River forwards these to the window manager as
  `river_window_v1.pointer_move_requested(seat)` /
  `pointer_resize_requested(seat, edges)` (protocol text literally cites "when a
  client-side rendered titlebar is dragged").
- **iced 0.14 exposes the client side:** `iced_runtime` has
  `Action::Drag(Id)` / `Action::DragResize(Id, Direction)`, surfaced as
  `iced::window::drag(id)` / `iced::window::drag_resize(id, dir)` and
  implemented in `iced_winit` via winit's `drag_window()` → `xdg_toplevel.move`.

Move flow:

```
kit titlebar press → iced::window::drag(window::Id) → xdg_toplevel.move(seat, serial)
  → river → pointer_move_requested(seat) → sola-river op_start_pointer   [D1 move op, floating-gated]
  → op_delta → node.set_position → op_release → op_end
```

Resize flow (free bonus, not required by the goal):

```
kit edge press → iced::window::drag_resize(id, dir) → xdg_toplevel.resize(seat, serial, edges)
  → river → pointer_resize_requested(seat, edges) → sola-river op_start_pointer   [D1 resize op]
```

This reuses D1's op state machine (`op.rs`): the op *loop* (`op_delta` /
`op_release` / `op_end`) is unchanged — only the *entry trigger* is new (a
Wayland CSD event instead of a river pointer binding; see §3 for the new entry
point). No new bus topic, no shell involvement, no geometry tracking.

## 3. sola-river — service the CSD requests (reuse `op.rs`)

`pointer_move_requested` / `pointer_resize_requested` are currently **unhandled**
(`river_window_v1` events dispatch in `crates/sola-river/src/client/window.rs`,
`impl Dispatch<RiverWindowV1, ()>`, alongside `AppId` / `Dimensions`). Add arms:

- **`pointer_move_requested { seat }`** for window `W`: if `W ∈ state.floating`
  (the set D1 already maintains), set a pending move-op-begin for `W` — the same
  pending state `op::on_pressed(state, OpKind::Move)` sets, but with the target
  window taken from the event rather than `state.pointer_window`. If `W` is not
  floating, **ignore** (tiled windows are not draggable this way; `Meta`+drag
  still works on them).
- **`pointer_resize_requested { seat, edges }`** for floating `W`: set a pending
  resize-op-begin, mapping the protocol `edges` bitfield → `op::Corner`
  (`top|left` → `TL`, etc.; the protocol guarantees never-both-horizontal and
  never-both-vertical, and never `none`).

Both events are documented as "followed by a manage_start event," matching D1's
manage-sequence-gated op start: the begin is recorded as pending and
`op_start_pointer` is issued in the next manage cycle by the existing
`op::drive` path. The op loop (`op_delta` → `on_delta`, `op_release` →
`on_released`, then `op_end`) is **unchanged from D1**.

**Implementation note (entry point):** `op::on_pressed(state, kind)` today reads
the target from `state.pointer_window` and (for resize) the corner from
`pick_corner(start, state.pointer_pos)`. Add a sibling entry — e.g.
`op::begin_for(state, kind, window_id, corner_override: Option<Corner>)` — that
takes the window from the event and an explicit corner for the resize case
(from `edges`), then funnels into the same `OpState`/`drive` machinery. Keep
`on_pressed` as the pointer-binding path.

**No bus-subscription change.** These are Wayland events, not bus topics, so the
two-file bus-consumer rule (`project_sola_river_bus_subscription`) does **not**
apply here — flagged so the implementer doesn't go hunting for a `subscribe()`
edit that isn't needed. The `floating` gate reuses D1's set.

## 4. sola-kit — titlebar component + float plumbing (opt-in)

The app decides whether and how to draw the bar. The kit provides the parts; it
does **not** force a wrapper around every app's view.

### 4.1 Float-state tracking (`FloatState`)

An app does not directly know its own sola-river `window_id`, but
`Topic::WindowFloating` is keyed by it. Correlate exactly as sola-shell does for
its overlays (`lookup_window_id` by `(app_id, title)`):

- The app's bus subscription adds `TopicKind::Windows` + `TopicKind::WindowFloating`.
- A kit-provided `FloatState` the app holds in its own state:
  - `update(&Message)` — folds `Topic::Windows` (learn this app's `window_id`s by
    matching `(app_id, title)`) and `Topic::WindowFloating` (the float bit per
    `window_id`).
  - `is_floating(&self, title: &str) -> bool` — per-window (keyed by the iced
    window's `title()` tag), so multi-window kit apps work; single-window apps
    pass their one title (or a `is_floating_any()` convenience).

Designing the tracker **per-window from the start** avoids a later multi-window
rewrite; the common single-window case is just one entry.

### 4.2 Titlebar component (`components/titlebar.rs`)

A kit widget styled with `ShellStyle` — **borders/fills only, no drop shadows**
(they render as hard rectangles in this renderer,
`project_shell_no_soft_shadows`). It carries:

- title text (display-role font),
- a close button (kit `button`),
- the bar surface itself as a drag handle (a `mouse_area`/press region).

It emits two messages the app maps into its own enum:

- **drag-start** → the app returns `iced::window::drag(self.window_id)` from
  `update`,
- **close** → the app emits `Topic::CloseApp(self.app_id)` (session-aware; the
  kit's existing `is_self_quit` turns the echoed `CloseApp` into a clean exit).

Title text is supplied by the app — the app owns its title. Close-button side and
exact metrics are a component-styling detail (decide in the plan; default:
close on the right).

A storybook page is added per kit convention (every component dogfoods there).

### 4.3 How an app opts in

The app composes it itself — full control over placement and whether a bar
appears at all:

```rust
let body = /* the app's normal content */;
let view = if self.float.is_floating(&self.title) {
    column![
        kit::titlebar(&self.title)
            .on_drag(Msg::TitleDrag)
            .on_close(Msg::TitleClose),
        body,
    ]
} else {
    body          // tiled / zoned: no bar, full content
};
```

```rust
// in update:
Msg::TitleDrag  => return iced::window::drag(self.window_id),
Msg::TitleClose => { /* emit Topic::CloseApp(self.app_id) */ }
```

An app that wants a different bar can ignore the component, read
`FloatState` directly, and call `window::drag` from its own chrome.

## 5. The accepted gap

Only kit apps that opt in get a sola titlebar. Non-kit (sola-browser WebKit/CEF)
and foreign apps (Unreal, GTK/Qt/Electron) keep their own CSD, which river
already enables via xdg-decoration. `Meta`+drag (D1) is the universal
move/resize fallback for everything. The currently-floating kit set is thin
today (sola-monitor, sola-settings, the kit storybook; sola-terminal is
mid-port), so near-term reach is small **by design** — this is first-class
chrome for kit apps that the ecosystem grows into, not a universal titlebar.

## 6. Testing

- **sola-river:** `pointer_move_requested` for a floating window records a
  move-op-begin for that window; for a non-floating window it is ignored.
  `pointer_resize_requested` maps `edges` → `Corner` correctly for each edge
  combination. (Unit-test the op-begin state + the edges→corner mapping; the
  Wayland wiring is build-verified + manual smoke, consistent with D1.)
- **sola-kit:** `FloatState` correlation — `Topic::Windows` +
  `Topic::WindowFloating` by `(app_id, title)` → the right `is_floating` bit,
  including a multi-window case. Titlebar component renders title + close and
  emits drag/close messages. Storybook page renders.
- **Smoke:** opt a kit app in (sola-monitor), float it (`Meta`+KP-`*`), drag the
  bar to move, drag an edge to resize, click close.

## 7. Touched files

- `crates/sola-river/src/client/window.rs` — handle `pointer_move_requested` /
  `pointer_resize_requested` (floating-gated) → pending op-begin.
- `crates/sola-river/src/client/op.rs` — `begin_for(state, kind, window_id,
  corner_override)` entry; `edges → Corner` mapping helper.
- `crates/sola-kit/src/float.rs` *(new)* — `FloatState` tracker.
- `crates/sola-kit/src/components/titlebar.rs` *(new)* — titlebar component.
- `crates/sola-kit/src/lib.rs` — export `FloatState` + `titlebar`.
- `crates/sola-kit/src/storybook/pages/` — titlebar page.
- *(dogfood, optional this phase)* a consumer app (e.g. `sola-monitor`) opts in.
- **No `sola-shell` changes. No new bus topics. No new bus subscriptions.**

## 8. Risks

- **`iced::window::drag` needs a valid input serial.** winit uses the latest
  pointer serial; the titlebar press is on the app's own surface, so a serial
  exists. Smoke-verify the move actually starts on press.
- **`op_start_pointer` from a CSD request vs a pointer binding.** The protocol
  documents this path explicitly ("when a client-side rendered titlebar is
  dragged"), so it is lower-risk than the dropped overlay approach's grab trick —
  but still smoke-verify the op begins and ends cleanly.
- **Multi-window kit apps.** Float state must be keyed by `window_id`, not
  `app_id`. The tracker is per-window from the start (§4.1) to avoid a rewrite.
