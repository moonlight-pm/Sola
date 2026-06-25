# Floating / App-Sized Windows — Design

**Date:** 2026-06-24
**Status:** Phase A implemented & committed. **The §1 root-cause claim below is
SUPERSEDED** — Phase A did *not* fix the UnrealEditor crash (floating the window
removed the forced resize, yet UE still crashed identically). The live
investigation moved to
[`2026-06-24-unreal-editor-crash-investigation.md`](./2026-06-24-unreal-editor-crash-investigation.md).
Phase A still stands as a standalone feature + robustness gate. **Phase B
(live geometry + per-app float position/size memory) is now planned and
implemented** — see `2026-06-24-floating-windows-phase-b-plan.md`. Phase D
(move/resize + titlebar + window menu) remains unplanned.
**Scope:** New window class whose size is chosen by the application, positioned
but never force-resized by the shell. Generalizes a robustness fix for all
GPU/Vulkan clients. Adds runtime float toggle, live geometry reporting,
per-app geometry memory, river-native move/resize, and shell-drawn titlebar
chrome.

---

## 1. Motivation

### The symptom (external)

Unreal Engine 5.8 editor (`app_id = UnrealEditor`, Vulkan + SDL3) dies ~13s into
bringup, deterministically, under Sola. The UE log ends with:

```
LogVulkanRHI: AcquireNextImage() failed due to the outdated swapchain, not even attempting to present.
LogSDL3: Wayland display connection closed by server (fatal)
LogLinuxWindow: Warning: Received SDL_EVENT_QUIT, requesting engine exit.
FUnixPlatformMisc::RequestExit(bForce=true, ReturnCode=0)
```

Clean exit (code 0, no crash dump). The compositor dropped the client because
SDL3 hit a fatal Wayland protocol error during a resize. UE did not crash.

### Root cause (confirmed in-repo)

> **⚠️ SUPERSEDED (2026-06-24).** The mechanism below was disproven by testing:
> with `Zones: UnrealEditor: Float` sola sends no resize (only `propose(0,0)`),
> yet UE crashes identically at the same spot. The forced resize is not the (sole)
> cause. See `2026-06-24-unreal-editor-crash-investigation.md`. The text is kept
> for history; the Phase A design built on it is still sound as a feature.

Sola force-resizes the window to a zone on map, and that early resize
invalidates UE's Vulkan swapchain before SDL3 has stabilized the surface:

1. **New toplevel.** `crates/sola-river/src/client/window.rs:41` seeds
   `state.pending.manage.entry(wid).or_insert((0, 0))` — "client self-sizes."
2. **AppId arrives.** `window.rs:112` → `translator::emit_windows` → `Topic::Windows`.
3. **Shell applies the config zone.** `crates/sola-shell/src/app/bus.rs:153`
   (`on_windows`) calls `zoning::apply_config_zone(app_id, window_id)`
   (`crates/sola-shell/src/zoning.rs:103`). For `UnrealEditor` it finds the
   persisted zone `FullMiddle` (from `~/.config/sola/state.yaml` `Zones:`),
   computes geometry via `compute_frame` (`zoning.rs:234`), and emits
   `Topic::Frame` with the zone size.
4. **River overwrites the self-size.** The `Topic::Frame` handler
   (`crates/sola-river/src/client/mod.rs:177`) calls `pending.frame(...)`
   (`crates/sola-river/src/pending.rs:40`), which does
   `self.manage.insert(id, (w, h))` — **replacing the `(0,0)` self-size** with
   the zone size.
5. **First configure is a forced resize.** `handle_manage_start`
   (`crates/sola-river/src/client/manage.rs:18`) calls
   `proxy.propose_dimensions(w, h)` with the zone size as the window's *first*
   configure → swapchain outdated → SDL3 fatal.

The one-time first-launch success happened because no zone had been learned yet,
so no `Topic::Frame` arrived and the window self-sized. Once Sola persisted
`UnrealEditor: FullMiddle`, every launch forced the resize.

### Two problems, one feature

- **User-facing feature:** floating / app-sized windows — a window class the
  shell positions but never sizes.
- **Robustness fix underneath:** even *zoned* windows should not receive a
  sizing configure before their surface is initialized. A per-window gate keyed
  on the first `dimensions` event makes zoning safe for any GPU client.

---

## 2. Relevant existing architecture

### Window lifecycle (`sola-river`)

- `Event::Window` → mint `window_id`, create `river_node_v1`, seed
  `pending.manage[wid] = (0,0)`, set `manage_dirty`. (`window.rs:29`)
- `Event::AppId` / `Title` / `UnreliablePid` → update `WindowRegistry`,
  `emit_windows`. (`window.rs:111`)
- `Event::DimensionsHint { max_width, max_height }` → `registry.set_max_size`.
  Used only to center unzoned windows. (`window.rs:143`)
- `Event::dimensions { width, height }` — **currently unhandled** (`_ => {}`).
  Reports the window's *actual* content size. Protocol guarantees: "The window
  will not be displayed until the first dimensions event is received and the
  render sequence is finished."
- `Event::ManageStart` → `handle_manage_start`: for each `pending.manage` entry,
  `proxy.propose_dimensions(w, h)`; then focus, chords, close, fullscreen,
  `manage_finish`. (`manage.rs:18`)
- `Event::RenderStart` → `handle_render_start`: `set_borders(empty)`,
  composition show/hide + `place_top`, apply `pending.render_positions`
  (`node.set_position`), `apply_default_placement` (centers any unplaced window
  using `default_size_for`, **without** proposing dimensions), `render_finish`.
  (`manage.rs:115`)

### `PendingUpdate` (`sola-river/src/pending.rs`)

- `manage: HashMap<u32,(w,h)>` → `propose_dimensions`.
- `render_positions: HashMap<u32,(x,y)>` → `node.set_position`.
- `frame(id,x,y,w,h)` sets **both** `manage[id]=(w,h)` and
  `render_positions[id]=(x,y)`, flags `manage_dirty` + `render_dirty`.
- `placed: HashSet<u32>` (on `AppData`) tracks positioned windows so the default
  center fires once.

### Shell zoning (`sola-shell/src/zoning.rs`)

- `ZoningState { output_size, focused_app_id, app_zone_config: HashMap<String,Zone>,
  window_zones: HashMap<u32,Zone>, config_applied: HashSet<u32>, zones_dirty }`.
- `set_zones(map)` ← `Topic::Zones` (`on_zones`), clears `config_applied`.
- `apply_config_zone(app_id, wid) -> Option<FrameUpdate>` — looks up the zone,
  computes a frame, returns it. Called from `on_windows` (per new window) and
  `on_zones` (re-apply after the map changes).
- `handle_key(code, wid) -> Option<FrameUpdate>` ← `on_chord` zoning branch;
  persists the zone into `app_zone_config`, sets `zones_dirty`.
- `compute_frame(zone, wid, w, h)` — uses `zone.rect()` + menubar offset →
  `FrameUpdate { window_id, x, y, width, height, fullscreen }`.
- `on_chord` zoning branch (`bus.rs:499`) emits `Topic::Frame(frame)` then
  `Topic::Zones(zones)` from `take_zones_update`.

### Bus types (`sola-bus/src/topics.rs`)

- `Zone` (enum, line 165): `Left, Right, Top, Bottom, TopMiddle, BottomMiddle,
  FullMiddle, Fullscreen, Cinema`. `Zone::rect() -> (f64,f64,f64,f64)`.
- `FrameUpdate { window_id, x, y, width, height, fullscreen }` (line 72).
- `Window { window_id, app_id, title, pid }` (line 11) — **no geometry**.
- `Topic::Windows(Vec<Window>)`, `Topic::Frame(FrameUpdate)`,
  `Topic::Zones(HashMap<String,Zone>)` (persistent → `state.yaml`).
- Persistent topics load once at bus startup (`sola-bus/src/state.rs`,
  `main.rs`); no hot reload.

### Keys (`sola-core/src/keys.rs`)

- XKB keycodes (evdev + 8). `KP_5 = 84`, `KP_ENTER = 104`, etc.
- **No keypad-star constant** today. `KP_MULTIPLY` = xkb `Self(63)`
  (evdev `KEY_KPASTERISK` 55 + 8).
- `ZONING_KEYCODES` (`zoning.rs:207`) → registered as Meta chords in
  `Shell::shell_key_chords` (`app.rs:445`).

### River capabilities relevant to phase D (`river-window-management-v1.xml`)

- `river_seat_v1.op_start_pointer` + `op_delta`/`op_release`/`op_end`:
  compositor-driven interactive move/resize. WM sets position / proposes
  dimensions from `op_delta`. Compositor owns pointer focus + cursor during op.
- `river_seat_v1.get_pointer_binding(button, modifiers)` →
  `river_pointer_binding_v1` with `pressed`/`released`. Bound buttons are *not*
  delivered to the focused surface.
- `river_window_v1.set_borders(edges, width, r,g,b,a)` — solid compositor-drawn
  borders only.
- `get_decoration_above/below` — decoration *surfaces* (WM supplies buffers).
  **Not used** in this design (chrome is shell-drawn; see §6).
- `pointer_move_requested` / `pointer_resize_requested` — emitted when a CSD
  client requests a move/resize; WM may honor via `op_start_pointer`.

---

## 3. Phasing

| Phase | Delivers | Depends on |
|---|---|---|
| **A** | `Zone::Float` + ordering/race fix + Meta+KP-Star toggle. **Fixes UnrealEditor.** | — |
| **B** | Live window-geometry reporting + per-app_id float position/size memory. | A |
| **D** | River-native Meta+drag move/resize + shell-drawn iced titlebar + window menu (phase C). | A, B |

Phase A is independently shippable and resolves the motivating bug. C
(surfacing Float in the UI) merges into D's window menu — there is no
standalone zone-picker UI today (zones are keybind-only), so the floating
window's own titlebar/right-click menu is the natural and only useful home.

---

## 4. Phase A — Floating core + race fix

### A1. `Zone::Float` variant

`sola-bus/src/topics.rs`: add `Float` to the `Zone` enum. It reuses the entire
existing zone pipeline — `state.yaml` `Zones:`, `app_zone_config`,
`window_zones`, the keybind table, `Topic::Zones` persistence. `Zone::rect()`
gains a `Float` arm for exhaustiveness but its value is never used for sizing
(returns e.g. `(0.0, 0.0, 0.0, 0.0)`; floating windows never go through the
sizing path).

Config to make UE floatable (no code, just `state.yaml`):

```yaml
Zones:
  UnrealEditor: Float
```

### A2. Shell stops sizing floating windows

`compute_frame` must not produce a sizing frame for `Float`. Cleanest shape:
have `apply_config_zone` and `handle_key` short-circuit on `Float` and return
`None` (no `Topic::Frame` emitted), while still recording the zone in
`window_zones` / `app_zone_config` and persisting via `take_zones_update`.

Result: the window keeps its seeded `(0,0)` self-size in `pending.manage`, and
`sola-river`'s existing `apply_default_placement` centers it using the client's
`dimensions_hint`. No sizing configure is ever sent → no swapchain churn.

> Note: `apply_config_zone` currently returns `Option<FrameUpdate>` and both
> callers push/emit the frame only when `Some`. Returning `None` for `Float` is
> already handled by both call sites. The persistence + `window_zones` bookkeeping
> must still run, so `Float` is recorded and re-applied on relaunch (as a no-op
> sizing-wise, but it marks the window floating for phases B/D).

### A3. The race-fix gate (universal robustness fix)

Add per-window first-`dimensions` tracking in `sola-river`:

- New field on `AppData` (or `Entry`): `first_dimensions: HashSet<u32>` (or a
  bool per `Entry`). Set when the window's first `river_window_v1.dimensions`
  event arrives (phase A starts handling this event; see A4).
- **Rule:** no proposed *size* (zone or float-restore) is applied before a
  window's first `dimensions` event. Only position may be set early.
- Implementation in `handle_manage_start`: for any window not yet in
  `first_dimensions`, force `propose_dimensions(0, 0)` regardless of what is in
  `pending.manage`. Stash the intended size in a `deferred_size:
  HashMap<u32,(w,h)>` and apply it on the *next* manage cycle once
  `first_dimensions` contains the window. Positions in `pending.render_positions`
  are applied normally (positioning is safe pre-init).

This makes zoning safe for any Vulkan/SDL/GPU client, not just UE: a zoned GPU
app self-sizes for its first frame (swapchain built against its own size), then
receives the zone resize as a normal runtime resize one cycle later. The brief
self-size→zone transition is a single frame and acceptable; floating windows
never get a size so they never transition.

> The shell change in A2 already prevents the UE crash by itself. A3 is the
> generalized fix so a *zoned* Vulkan app can't reproduce the same race. Both
> ship in phase A.

### A4. Handle the `dimensions` event

`window.rs` `Dispatch<RiverWindowV1>`: add an `Event::dimensions { width, height }`
arm that (1) marks `first_dimensions` for the window and (2) records actual
`(w,h)` for the geometry path (phase B reads this; phase A only needs the gate
flag). This is the protocol-sanctioned "surface is now displayable" signal.

### A5. Float toggle keybind

- `sola-core/src/keys.rs`: add `pub const KP_MULTIPLY: Self = Self(63);`.
- `sola-shell/src/zoning.rs`: add `KP_MULTIPLY` to `ZONING_KEYCODES`;
  `zone_for_keycode(KP_MULTIPLY) => Some(Zone::Float)`.
- Meta+KP-Star floats the focused window. Unfloat = any other Meta+Numpad zone
  key (already works: `handle_key` overwrites the zone, and a non-`Float` zone
  emits a sizing frame). Float state persists via `Topic::Zones`.

### A6. Tests (phase A)

- `zone_for_keycode(KP_MULTIPLY) == Float`.
- `handle_key` with `Float` records the zone, sets `zones_dirty`, returns `None`
  (no frame).
- `apply_config_zone` with `Float` returns `None` but marks `config_applied` /
  `window_zones`.
- `sola-river` pending/manage: a `Topic::Frame` arriving for a window with no
  prior `dimensions` event defers the size (proposes `(0,0)` first), then applies
  the size on the next cycle after a `dimensions` event.

---

## 5. Phase B — Live geometry + float memory

### B1. Live geometry path (new)

`sola-river` tracks and publishes each window's current rectangle:

- Width/height from the `dimensions` event (A4).
- Position from what the WM sets (`node.set_position` in `handle_render_start` /
  `apply_default_placement`) and from `op_delta` during a move (phase D). For a
  centered float, the WM computes the center in `apply_default_placement` and
  therefore knows `(x,y)`.
- New non-persistent bus topic:
  `Topic::WindowGeometry { window_id, x, y, width, height }`, emitted when a
  window's rectangle changes (debounced to render cycles). Reuses the existing
  `Entry.frame` storage shape where possible.

This is the backbone phase D's titlebar consumes; it is also independently
useful (e.g. per-window screenshot regions already want real geometry).

### B2. Float geometry memory (new persistent topic)

- `Topic::FloatGeometry` — persistent, namespaced, keyed by `app_id` →
  `{ x, y, width, height }`. Kept separate from `Zones:` so `Zone` stays a clean
  unit enum and float geometry can change without rewriting the zone map.
- The shell records a floating window's geometry (from `Topic::WindowGeometry`)
  against its `app_id`, and persists it.
- On relaunch of a floating app, the shell restores:
  - **Position immediately** (safe pre-init — only `render_positions`).
  - **Size only after the first-`dimensions` gate** (A3), via the deferred path,
    so restore cannot reproduce the UE resize-before-init crash. Restoring size
    is allowed (not forbidden) precisely because the gate makes it safe.

### B3. Tests (phase B)

- `WindowGeometry` emitted on `dimensions` change and on position change.
- Float geometry persists round-trips through `state.yaml` (namespaced file).
- Restore applies position pre-gate and size post-gate.

---

## 6. Phase D — Move/resize + shell titlebar (+ phase C)

### D1. River-native move/resize

`sola-river` creates two pointer bindings via `river_seat_v1.get_pointer_binding`:

- **Meta + BTN_LEFT = move.** On `pressed`, `op_start_pointer`; each `op_delta`
  → `node.set_position(origin + delta)`; `op_release` → `op_end`.
- **Meta + BTN_RIGHT = resize.** Floating windows only. On `pressed`,
  `op_start_pointer`; each `op_delta` → `propose_dimensions(origin_size + delta)`;
  `op_release` → `op_end`. (Resize on a zoned window is ignored or unfloats it —
  decide during implementation; default: ignore.)

Bindings, `op_start_pointer`, and `op_end` must be issued inside a manage
sequence (per protocol). The op state machine lives in a new
`sola-river/src/client/op.rs` module. Pointer-binding enable/disable follows the
existing chord-registration pattern (during manage).

This requires **no drawn chrome** and is low-latency (compositor-driven). It is
the primary move/resize mechanism; the titlebar (D2) is an additional affordance
that reuses it.

### D2. Shell-drawn titlebar (iced)

- New `WindowKind::Titlebar` overlay in `sola-shell`, one per floating window.
- Positioned just above the window's content rect by subscribing to
  `Topic::WindowGeometry`; themed via `sola-kit`.
- Title text from `Topic::Windows`. Close / minimize / maximize buttons emit bus
  actions (`CloseApp` exists; minimize/maximize map to river
  `set_capabilities` + the corresponding requests, or to zone changes).
- Dragging the titlebar starts a move: the shell emits a small new bus request
  (e.g. `Topic::BeginMove { window_id }`) that `sola-river` services by starting
  an `op_start_pointer` move — reusing D1's machinery. (Alternative: rely on the
  client `pointer_move_requested` path; rejected because arbitrary apps without
  CSD won't emit it.)
- Lifecycle: titlebar opens when a window becomes floating, closes when it
  unfloats or the window closes. Z-order: placed directly above its window via
  the node `place_above` family.

### D3. Window menu (phase C — "surface Float in the UI")

Right-clicking the titlebar opens an iced menu listing the zone choices
**including Float / unfloat**. This is the only place zones are chosen in the UI
and the natural home for the Float requirement. Selecting an entry routes through
the existing `handle_key`/zone-apply path (emits `Topic::Frame` + `Topic::Zones`).

### D4. Tests (phase D)

- Pointer-binding enable/disable issued only within a manage sequence.
- Move op: `op_delta` accumulation updates position; `op_release` ends cleanly.
- Resize op: floating only; `propose_dimensions` tracks `op_delta`.
- Titlebar overlay opens/closes on float/unfloat/close; tracks geometry.

---

## 7. Decisions (rationale)

- **Config = `Zone::Float`** (not a parallel `Floating:` list). Threads through
  every existing zone mechanism with zero new plumbing for designation/persistence.
- **Float geometry = separate persistent topic** keyed by `app_id`. Keeps `Zone`
  a clean unit enum; geometry churn doesn't rewrite the zone map.
- **Race gate = "first `dimensions` event received."** Protocol-grounded
  (a window isn't displayed until its first `dimensions` event), universal
  (protects zoned GPU apps too).
- **Size restore is deferred-but-allowed**, not forbidden — the gate makes it
  safe.
- **Move/resize = river-native Meta+Left/Right drag** via pointer bindings +
  `op_start_pointer`. Works on any window, low-latency, no chrome required.
- **Chrome = shell-drawn iced overlay** (not river decoration surfaces). Keeps
  `sola-river` renderer-free (per `CLAUDE.md`); reuses iced + the kit theme.
- **Phase C merges into D's window menu.** No standalone zone picker exists today.

---

## 8. Risks & open questions

- **Titlebar tracking latency.** During a Meta+drag move, the window position is
  compositor-driven (`op_delta`) but the shell titlebar position comes over the
  bus (`Topic::WindowGeometry`). The titlebar may lag the window by a frame or
  two during fast drags. Mitigation: emit geometry every render cycle during an
  active op; accept minor lag, or (later) let river draw a lightweight border via
  `set_borders` during the drag for immediate feedback. Decide in phase D.
- **Resize on a zoned window.** Default: ignore Meta+RightDrag unless floating.
  Revisit if a "resize unfloats" UX is wanted.
- **Minimize/maximize semantics.** Sola has no minimize concept today. Phase D
  may map maximize → `Zone::Fullscreen` and omit minimize, or introduce a real
  minimized state. To be settled when phase D is planned.
- **Multi-output.** Centering and geometry use the single primary output (v1
  assumption, consistent with existing zoning). Floating across outputs is out of
  scope.
- **`pointer_move_requested` from CSD clients.** Optional: honoring it would let
  CSD apps' own titlebars drive moves. Not required; can be added later.

---

## 9. Touched files (anticipated)

**Phase A**
- `crates/sola-bus/src/topics.rs` — `Zone::Float`, `Zone::rect()` arm.
- `crates/sola-shell/src/zoning.rs` — `Float` in `ZONING_KEYCODES` /
  `zone_for_keycode`; `Float` short-circuit in `apply_config_zone` / `handle_key`
  / `compute_frame`.
- `crates/sola-core/src/keys.rs` — `KP_MULTIPLY`.
- `crates/sola-river/src/client/window.rs` — handle `dimensions` event; set
  first-`dimensions` flag.
- `crates/sola-river/src/client/manage.rs` — defer sizes before first
  `dimensions`.
- `crates/sola-river/src/pending.rs` / `client/mod.rs` — `deferred_size`,
  `first_dimensions` plumbing.

**Phase B**
- `crates/sola-bus/src/topics.rs` — `Topic::WindowGeometry`,
  `Topic::FloatGeometry` (persistent).
- `crates/sola-river/src/{registry.rs,translator.rs,client/*}` — track + emit
  geometry.
- `crates/sola-shell/src/app/bus.rs` — consume geometry; persist/restore float
  geometry.

**Phase D**
- `crates/sola-river/src/client/op.rs` (new) — pointer bindings + op state
  machine.
- `crates/sola-river/src/client/mod.rs` — `BeginMove` bus request handling.
- `crates/sola-shell/src/**` — `WindowKind::Titlebar`, overlay view, window menu.
- `crates/sola-bus/src/topics.rs` — `Topic::BeginMove` (+ any minimize/maximize).
- `crates/sola-kit` — titlebar component if not expressible with existing widgets.
