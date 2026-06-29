# Floating Windows — Phase D1: Interactive Move/Resize — Design

**Date:** 2026-06-29
**Status:** Implemented (commits `e8b0819` → `88a805a`). Part of the floating-windows feature
(`2026-06-24-floating-windows-design.md` §6 / Phase D). This is the **D1** slice
only — river-native interactive move/resize. The shell-drawn titlebar (D2) and
window menu (D3) remain future work.

---

## 1. Goal & scope

`Meta`+drag interactively moves/resizes **floating windows only**, river-native
(compositor-driven via `river_seat_v1.op_start_pointer`, low-latency, no drawn
chrome). The whole op loop lives in `sola-river`.

```
Meta + BTN_LEFT  → move   (floating windows only)
Meta + BTN_RIGHT → resize (floating windows only; nearest-corner)
```

Out of scope for D1: titlebar chrome, window menu, drag-to-tear-off (a tiled
window must be floated with `Meta`+numpad-`*` first), screen-bounds clamping on
move, live geometry tracking *during* a drag (see §6).

## 2. The one architectural decision — telling sola-river what is "floating"

`sola-river` has **no concept of zones or floating** — zones live entirely in the
shell, and `sola-river` only ever receives `Topic::Frame` (raw geometry). To gate
the bindings at press-time (which must be instant — a bus round-trip on press
would start the op late, and because `op_delta` is *cumulative from op start*, a
late start makes the window jump), `sola-river` needs the float bit **locally**.

**Decision:** add a minimal sticky bus topic, mirroring the existing
`WindowGeometry` pattern:

```rust
// sola-bus/src/topics.rs
pub struct WindowFloating { pub window_id: u32, pub floating: bool }

// in define_topics! { ... } near WindowGeometry
#[sticky(keys = [window_id])] WindowFloating(WindowFloating)
```

The shell emits it whenever a window's float state changes; `sola-river` folds it
into a `floating: HashSet<u32>` (insert on `true`, remove on `false` or on window
`Closed`). Zone *semantics* stay in the shell; `sola-river` gets exactly the one
bit it needs. The `Zone` enum never crosses the process boundary.

*Rejected:* gating in the shell by round-tripping the binding press over the bus —
responsiveness demands the gate be local to `sola-river`.

## 3. Op state machine (`sola-river/src/client/op.rs`, new)

Every river op request (`op_start_pointer`, `op_end`) and `pointer_binding.enable`
is **manage-sequence-gated** — identical to the existing chord wiring
(`translator::apply_pending_chords`). So binding/seat events set pending state and
the manage cycle issues the protocol requests.

`AppData` gains:
- `op: Option<OpState>` — the active op.
- `pointer_window: Option<u32>` — hovered window, tracked from
  `river_seat_v1` `pointer_enter`/`pointer_leave`.
- `pointer_pos: Option<(i32, i32)>` — latest pointer position, from the
  `pointer_position` seat event (used to pick the resize corner at press).
- `move_binding`, `resize_binding`: `Option<RiverPointerBindingV1>`.
- pending op flags (begin/release) — on `AppData` or `PendingUpdate`.

`OpState { kind: OpKind, window_id: u32, start: Rect, corner: Option<Corner>, released: bool }`
where `OpKind ∈ {Move, Resize}` and `Rect = {x,y,w,h}`.

Lifecycle:

```
pointer_binding `pressed` (BTN_LEFT/RIGHT + mod4)
   target = pointer_window
   if target is None or target ∉ floating → ignore (no op).
   else record a "begin op" (kind, target).

next manage_start            (pressed is followed by a manage_start)
   if begin pending && op.is_none():
     capture start rect (pos from last_position, size from registry.geometry),
     resize → corner = pick_corner(start_rect, pointer_pos),
     seat.op_start_pointer(),
     set cursor shape,
     op = Some(OpState{…}).

op_delta(dx, dy)             (cumulative motion since start; followed by manage_start)
   recompute target rect from op.start + (dx,dy):
     move   → render_positions[id] = new_pos
     resize → pending.frame(id, new_rect)  (position + propose_dimensions)
   mark dirty; applied in the render/manage cycle.

op_release                   (all buttons up; followed by manage_start)
   op.released = true.

next manage_start
   if op.released:
     seat.op_end(),
     reset cursor to default,
     emit ONE WindowGeometry for the final rect (see §6),
     op = None.
```

Guards: `op_start_pointer` only when `op.is_none()` (the protocol also ignores a
double-start). Window `Closed` mid-op clears `op`. A `pressed` over a non-floating
window or empty space is swallowed by the binding (Meta+click is a reserved WM
gesture) but starts no op.

## 4. Move & resize math (pure, unit-tested)

Pure functions in `op.rs`, no Wayland needed:

- **Move:** `new_pos = (start.x + dx, start.y + dy)`; size unchanged; no
  screen-bounds clamp in v1.
- **`pick_corner(rect, pointer) → Corner`** — pointer left/right of `rect`
  horizontal center × top/bottom of vertical center ⇒ one of `{TL, TR, BL, BR}`.
- **Resize (nearest corner):** the grabbed corner moves by the delta; the
  **opposite corner stays pinned**. Axes independent:
  - right edge: `w = start.w + dx` (x fixed); left edge: `x = start.x + dx,
    w = start.w − dx`.
  - bottom edge: `h = start.h + dy` (y fixed); top edge: `y = start.y + dy,
    h = start.h − dy`.
  - clamp `w,h ≥ MIN_DIM (100)`; when clamping a left/top-anchored edge, freeze
    that edge's position so the pinned (opposite) corner does not drift.

Resize issues `propose_dimensions`. The window is already initialized (it is
floating and on-screen), so Phase A's first-`dimensions` gate is a no-op here.

## 5. Cursor feedback (`wp-cursor-shape-v1`)

During an op, river guarantees no client holds pointer focus, so the WM's
`wl_pointer` cursor wins (the compositor ignores the `set_shape` serial since seat
v4). Wiring:

- Add the `wp-cursor-shape-v1` protocol; bind `wp_cursor_shape_manager_v1` from
  the registry; `get_pointer` on the existing `wl_seat`; create one
  `wp_cursor_shape_device_v1`.
- Op start: `set_shape` → `move`/`grabbing` for move; for resize the directional
  shape for the grabbed corner — `nwse-resize` (TL/BR) or `nesw-resize` (TR/BL).
- Op end: restore the default shape.

This is the heaviest new piece (a new protocol + `wl_pointer`), so it is the
**last, separable task**: move/resize is fully functional without it; the cursor
lands on top.

## 6. Geometry persistence & drag debounce

Persistence is **already free** via Phase B: `set_position`/`set_size` →
`translator::emit_geometry` → `Topic::WindowGeometry` → shell `on_window_geometry`
→ `note_window_geometry` → `Topic::FloatGeometry` (persisted to `state.yaml`). But
emitting per `op_delta` would rewrite `state.yaml` on every drag frame — the
"debounce during a live drag" concern deferred in the Phase B plan.

**Fix:** while `op.is_some()`, `sola-river` still drives river live
(`set_position`/`propose_dimensions` — the window moves smoothly) but
**suppresses the `emit_geometry` bus-emit**, then emits a **single**
`WindowGeometry` on `op_end`. The shell therefore persists `FloatGeometry` exactly
once per drag. (Live bus tracking during a drag — wanted by the future D2 titlebar
— can be revisited then; D1 has no observers.)

## 7. Testing

- **`op.rs` pure math:** move offset; `pick_corner` for all four quadrants; resize
  per corner including the `MIN_DIM` clamp + anchor-freeze.
- **`sola-bus`:** `WindowFloating` is sticky + keyed + round-trips (mirror
  `window_geometry_is_sticky_not_persistent`).
- **shell `zoning`:** a float-state change emits `WindowFloating{floating:true}` on
  float and `{false}` on unfloat.
- Wayland wiring (bindings, op requests, cursor) is build-verified + manual smoke,
  consistent with how the existing chord wiring is covered.

## 8. Touched files

- `crates/sola-bus/src/topics.rs` — `WindowFloating` struct + sticky variant.
- `crates/sola-river/src/client/op.rs` *(new)* — `OpState`, op lifecycle helpers,
  pure move/resize/corner math.
- `crates/sola-river/src/client/seat.rs` — handle `op_delta`/`op_release`/
  `pointer_position`; track `pointer_window` from `pointer_enter`/`leave`.
- `crates/sola-river/src/client/mod.rs` — `AppData` fields; `RiverPointerBindingV1`
  `pressed`/`released` → op; `bus_tick` arm for `WindowFloating`; bind the
  cursor-shape global; create the two pointer bindings.
- `crates/sola-river/src/client/manage.rs` — drive op start/end + binding enable in
  the manage sequence; suppress `emit_geometry` mid-op; apply op rects.
- `crates/sola-river/src/translator.rs` — pointer-binding create+enable helper;
  geometry-emit gate.
- `crates/sola-river/src/pending.rs` — pending op flags.
- `crates/sola-shell/src/{zoning.rs,app/bus.rs}` — emit `WindowFloating` on
  float-state change (float key, config-zone apply, unfloat, close).
- `crates/sola-river/protocols/` — add `wp-cursor-shape-v1.xml` (+ `wl_pointer`).

## 9. Build sequence

1. `WindowFloating` topic (`sola-bus`).
2. Shell emits `WindowFloating` on float-state change.
3. `sola-river` consumes `WindowFloating`; track hovered window + pointer position.
4. Pointer bindings + op state machine + **move**.
5. **Resize** with nearest-corner math.
6. Cursor feedback (`wp-cursor-shape-v1`).

Move/resize (steps 1–5) is independently shippable; cursor feedback (6) lands on
top.
