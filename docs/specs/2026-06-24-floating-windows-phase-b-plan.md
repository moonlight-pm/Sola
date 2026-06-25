# Floating Windows — Phase B Implementation Plan

> **For agentic workers:** implement task-by-task. Steps use checkbox (`- [ ]`)
> syntax. Build with `cargo make build`; never install without explicit user
> permission.

**Goal:** Report each window's live rectangle on the bus, and remember a floating
app's position+size across relaunch.

**Architecture:** `sola-river` already knows every window's size (the `dimensions`
event) and position (what it sets via `node.set_position`). Phase B records both
in the `WindowRegistry`, emits a new sticky `Topic::WindowGeometry` keyed by
`window_id` whenever the rectangle changes, and retracts it on close. `sola-shell`
listens for geometry of *floating* windows, persists it per `app_id` as a new
persistent `Topic::FloatGeometry`, and on relaunch restores a float's saved
rectangle through the existing Phase A first-`dimensions` gate (position applied
immediately, size deferred until the surface initializes — so restore cannot
reproduce the resize-before-init crash).

**Tech stack:** Rust, `sola-bus` `define_topics!` macro, `serde_yaml_ng`.

## Global Constraints

- Build with `cargo make build` (never raw `cargo build`). Do **not** install.
- Code edits go on `master`, committed per task (single active session — no
  worktree).
- Use Serena symbol tools for code reads/edits.
- `WindowGeometry` is **sticky, non-persistent** (live state, retracted on close).
  `FloatGeometry` is **persistent, keyed by `app_id`** (survives restart).
- Floating windows must keep going through the Phase A gate — never propose a size
  before a window's first `dimensions` event.

## Design recap (shapes)

```rust
// sola-bus/src/topics.rs
pub struct WindowGeometry { pub window_id: u32, pub x: i32, pub y: i32, pub width: i32, pub height: i32 }
pub struct FloatGeometry  { pub app_id: String, pub x: i32, pub y: i32, pub width: i32, pub height: i32 }

// in define_topics! { ... }
#[sticky(keys = [window_id])]      WindowGeometry(WindowGeometry)
#[persistent(keys = [app_id])]     FloatGeometry(FloatGeometry)
```

`window_id` (u32) and `app_id` (String) are the macro key fields (extracted via
`Display`), matching the existing `#[persistent(keys = [app_id])] Application(..)`
and `#[sticky(keys = [app_id])] SetAppMenu(..)` precedents.

---

## Task 1: Bus types — `WindowGeometry` + `FloatGeometry`

**Files:**
- Modify: `crates/sola-bus/src/topics.rs` (structs near `OutputGeometry:96`;
  variants inside `define_topics!` near `OutputGeometry:495` / `Zones:519`)
- Test: same file's `#[cfg(test)]` module (mirror `session_apps_is_persistent`,
  `from_yaml_section` roundtrip tests)

- [ ] **Step 1 — failing test.** Add to the tests module:

```rust
#[test]
fn float_geometry_is_persistent_and_roundtrips() {
    use crate::topic::Behavior;
    assert_eq!(TopicKind::FloatGeometry.behavior(), Behavior::Persistent);
    let fg = FloatGeometry { app_id: "UnrealEditor".into(), x: 10, y: 20, width: 1280, height: 800 };
    let value = Topic::FloatGeometry(fg.clone()).to_yaml_value().expect("persistent → YAML");
    match Topic::from_yaml_section(TopicKind::FloatGeometry, value) {
        Some(Topic::FloatGeometry(back)) => {
            assert_eq!(back.app_id, fg.app_id);
            assert_eq!((back.x, back.y, back.width, back.height), (10, 20, 1280, 800));
        }
        other => panic!("expected FloatGeometry, got {other:?}"),
    }
}

#[test]
fn window_geometry_is_sticky_not_persistent() {
    use crate::topic::Behavior;
    assert_eq!(TopicKind::WindowGeometry.behavior(), Behavior::Sticky);
}
```

- [ ] **Step 2 — run, expect fail** (`FloatGeometry`/`WindowGeometry` undefined):
  `cargo test -p sola-bus float_geometry_is_persistent_and_roundtrips`

- [ ] **Step 3 — implement.** Add the two structs (derive
  `Debug, Clone, Serialize, Deserialize`; `FloatGeometry` also `PartialEq` for the
  shell dedup in Task 4) beside `OutputGeometry`, and the two variants in
  `define_topics!` (put `WindowGeometry` near the other sticky live-state topics,
  `FloatGeometry` near `Zones`). Comment each variant in the existing house style.

- [ ] **Step 4 — run tests:** `cargo test -p sola-bus` → all pass.

- [ ] **Step 5 — commit:**
  `feat(sola-bus): add WindowGeometry (sticky) + FloatGeometry (persistent) topics`

---

## Task 2: `sola-river` registry — record size + position

**Files:**
- Modify: `crates/sola-river/src/registry.rs` (`Entry:28`, `impl WindowRegistry`)
- Test: the `tests` module in the same file

`Entry` keeps its existing `frame` (shell-requested frame, used by solactl). Add
the *actual* tracked rect as two independent Options (size and position arrive in
either order, from different events):

- [ ] **Step 1 — failing test:**

```rust
#[test]
fn geometry_is_some_only_when_size_and_position_known() {
    let mut r = WindowRegistry::default();
    let id = r.mint();
    assert!(r.geometry(id).is_none());
    assert!(r.set_size(id, 800, 600));      // changed
    assert!(r.geometry(id).is_none());       // position still unknown
    assert!(r.set_position(id, 10, 20));    // changed
    let g = r.geometry(id).expect("both known now");
    assert_eq!((g.window_id, g.x, g.y, g.width, g.height), (id, 10, 20, 800, 600));
    assert!(!r.set_size(id, 800, 600));      // unchanged → false
}
```

- [ ] **Step 2 — run, expect fail.** `cargo test -p sola-river geometry_is_some_only`

- [ ] **Step 3 — implement.** Add to `Entry`:

```rust
    /// Actual content size from `river_window_v1.dimensions`. `None` until the
    /// first dimensions event. Distinct from `frame` (the shell's requested rect).
    pub size: Option<(i32, i32)>,
    /// Actual on-screen position from the last `node.set_position`. `None` until
    /// the window is first placed.
    pub position: Option<(i32, i32)>,
```

Initialize both `None` wherever `Entry` is constructed. Add to
`impl WindowRegistry` (use `sola_bus::topics::WindowGeometry`):

```rust
    /// Record the window's actual size. Returns true if it changed.
    pub fn set_size(&mut self, window_id: u32, width: i32, height: i32) -> bool {
        let Some(e) = self.by_id.get_mut(&window_id) else { return false };
        if e.size == Some((width, height)) { return false; }
        e.size = Some((width, height));
        true
    }

    /// Record the window's actual position. Returns true if it changed.
    pub fn set_position(&mut self, window_id: u32, x: i32, y: i32) -> bool {
        let Some(e) = self.by_id.get_mut(&window_id) else { return false };
        if e.position == Some((x, y)) { return false; }
        e.position = Some((x, y));
        true
    }

    /// The window's full rectangle, once both size and position are known.
    pub fn geometry(&self, window_id: u32) -> Option<WindowGeometry> {
        let e = self.by_id.get(&window_id)?;
        let (width, height) = e.size?;
        let (x, y) = e.position?;
        Some(WindowGeometry { window_id, x, y, width, height })
    }
```

(Check the exact field/method names — `mint`, `by_id` — against the file; adapt if
`Entry` construction is centralized in a helper.)

- [ ] **Step 4 — run tests:** `cargo test -p sola-river` → pass.

- [ ] **Step 5 — commit:**
  `feat(sola-river): track actual window size+position in the registry`

---

## Task 3: `sola-river` — emit `WindowGeometry` on change; retract on close

**Files:**
- Modify: `crates/sola-river/src/translator.rs` (new `emit_geometry`, beside
  `emit_windows:11`)
- Modify: `crates/sola-river/src/client/window.rs` (`Event::Dimensions:180`,
  `Event::Closed:124`)
- Modify: `crates/sola-river/src/client/manage.rs` (`handle_render_start:202`
  position loop; `apply_default_placement:260`)

- [ ] **Step 1 — `emit_geometry` helper** in `translator.rs`:

```rust
/// Emit the window's current rectangle as a sticky `Topic::WindowGeometry`.
/// No-op until both size and position are known. Callers gate on the
/// registry setter returning `true` (changed) so this only fires on real moves.
pub fn emit_geometry(state: &mut AppData, window_id: u32) {
    let Some(g) = state.registry.geometry(window_id) else { return };
    debug!(window_id, g.x, g.y, g.width, g.height, "emitting WindowGeometry");
    state.bus.emit(Topic::WindowGeometry(g));
}
```

- [ ] **Step 2 — wire size** in `window.rs` `Event::Dimensions`, after the existing
  `note_dimensions` block:

```rust
                if state.registry.set_size(window_id, width, height) {
                    crate::translator::emit_geometry(state, window_id);
                }
```

- [ ] **Step 3 — wire position** in `manage.rs`. In `handle_render_start`'s
  `render_positions` loop, inside the `should_send` block right after
  `node.set_position(x, y)`:

```rust
                if state.registry.set_position(window_id, x, y) {
                    crate::translator::emit_geometry(state, window_id);
                }
```

  And in `apply_default_placement`, after `node.set_position(x, y)`:

```rust
            if state.registry.set_position(window_id, x, y) {
                crate::translator::emit_geometry(state, window_id);
            }
```

  (Mind the borrow: `set_position` takes `&mut state.registry`, then `emit_geometry`
  re-borrows `state`; both are sequential statements so this is fine. If a `for …
  in &state.pending.render_positions` loop holds an immutable borrow of `state`,
  collect the `(window_id, x, y)` tuples into a `Vec` first, then mutate — match
  whatever pattern the existing churn-fix code already uses there.)

- [ ] **Step 4 — retract on close** in `window.rs` `Event::Closed`, alongside the
  existing `first_dimensions`/`last_position` cleanup:

```rust
                let _ = state.bus.retract(Topic::WindowGeometry(WindowGeometry {
                    window_id, x: 0, y: 0, width: 0, height: 0,
                }));
```

  (Retract keys on `window_id`; the other fields are ignored. Import
  `sola_bus::topics::WindowGeometry`.)

- [ ] **Step 5 — build:** `cargo make build` → clean. Run `cargo test -p sola-river`
  → existing 17 still pass (no new unit test here; this is wiring verified by
  build + the Task 2 registry tests).

- [ ] **Step 6 — commit:**
  `feat(sola-river): emit WindowGeometry on size/position change, retract on close`

---

## Task 4: `sola-shell` — record floating windows' geometry

**Files:**
- Modify: `crates/sola-shell/src/zoning.rs` (`ZoningState:14` — add
  `float_geometry` map + a setter)
- Modify: `crates/sola-shell/src/app/bus.rs` (dispatch match `:30-42`; new
  handlers `on_window_geometry`, `on_float_geometry`)
- Test: `zoning.rs` tests module

Store the per-app float rectangle in `ZoningState` (it already owns
`app_zone_config`), so Task 5's `apply_config_zone` can read it:

- [ ] **Step 1 — add field** to `ZoningState`:

```rust
    /// Last known rectangle of each floating app, keyed by app_id. Fed by
    /// Topic::WindowGeometry for floating windows and by Topic::FloatGeometry
    /// replay at startup; consumed by apply_config_zone to restore on relaunch.
    pub float_geometry: std::collections::HashMap<String, sola_bus::topics::FloatGeometry>,
```

  Initialize it in the struct's `Default`/constructor.

- [ ] **Step 2 — failing test** (records only when the window is floating):

```rust
#[test]
fn floating_window_geometry_is_recorded_by_app() {
    let mut z = ZoningState::default();
    z.window_zones.insert(7, Zone::Float);
    let changed = z.note_window_geometry("UnrealEditor", 7, 10, 20, 1280, 800);
    assert!(changed);
    let g = z.float_geometry.get("UnrealEditor").expect("recorded");
    assert_eq!((g.x, g.y, g.width, g.height), (10, 20, 1280, 800));
    // A non-floating window's geometry is ignored.
    z.window_zones.insert(8, Zone::Left);
    assert!(!z.note_window_geometry("Helium", 8, 0, 0, 100, 100));
    assert!(z.float_geometry.get("Helium").is_none());
}
```

- [ ] **Step 3 — implement `note_window_geometry`** on `ZoningState`:

```rust
    /// Record a floating window's geometry against its app_id. Returns true if a
    /// new/changed FloatGeometry should be persisted. Ignores non-floating windows.
    pub fn note_window_geometry(
        &mut self, app_id: &str, window_id: u32, x: i32, y: i32, width: i32, height: i32,
    ) -> bool {
        if self.window_zones.get(&window_id) != Some(&Zone::Float) {
            return false;
        }
        let next = sola_bus::topics::FloatGeometry {
            app_id: app_id.to_string(), x, y, width, height,
        };
        if self.float_geometry.get(app_id) == Some(&next) {
            return false;
        }
        self.float_geometry.insert(app_id.to_string(), next);
        true
    }
```

  (`FloatGeometry` derives `PartialEq` from Task 1.)

- [ ] **Step 4 — dispatch + handlers** in `app/bus.rs`. Add arms to the match:

```rust
            Topic::WindowGeometry(g) => { self.on_window_geometry(g); Task::none() }
            Topic::FloatGeometry(f) => { self.on_float_geometry(f); Task::none() }
```

  Handlers on `impl Shell`:

```rust
    /// Cache a floating app's restored geometry (Topic::FloatGeometry replay at
    /// startup, or our own echo).
    fn on_float_geometry(&mut self, f: FloatGeometry) {
        self.zoning.float_geometry.insert(f.app_id.clone(), f);
    }

    /// A window moved/resized. If it's floating, persist its rectangle per app_id.
    fn on_window_geometry(&mut self, g: WindowGeometry) {
        let Some(app_id) = self
            .known_windows
            .iter()
            .find(|w| w.window_id == g.window_id)
            .map(|w| w.app_id.clone())
        else { return };
        if self.zoning.note_window_geometry(&app_id, g.window_id, g.x, g.y, g.width, g.height) {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                let _ = bus.emit(Topic::FloatGeometry(
                    self.zoning.float_geometry[&app_id].clone(),
                ));
            }
        }
    }
```

  (Import `WindowGeometry`, `FloatGeometry` from `sola_bus::topics` at the use site.)

- [ ] **Step 5 — run:** `cargo test -p sola-shell` (72 + new) → pass;
  `cargo make build` → clean.

- [ ] **Step 6 — commit:**
  `feat(sola-shell): persist floating windows' geometry per app_id`

---

## Task 5: `sola-shell` — restore float geometry on relaunch

**Files:**
- Modify: `crates/sola-shell/src/zoning.rs` (`apply_config_zone` — the `Float` arm)
- Test: `zoning.rs` tests module

Phase A's `apply_config_zone` returns `None` for `Float` (no sizing frame). Phase B
makes it return the *saved* rectangle when one exists, so the float restores where
it was. The Phase A first-`dimensions` gate in `sola-river` defers the size until
the surface initializes; position applies immediately. A float with no saved
geometry still returns `None` (centered by `apply_default_placement` as before).

- [ ] **Step 1 — failing test:**

```rust
#[test]
fn float_with_saved_geometry_restores_a_frame() {
    let mut z = ZoningState::default();
    z.output_size = Some((5120, 2160));
    z.app_zone_config.insert("UnrealEditor".into(), Zone::Float);
    z.float_geometry.insert("UnrealEditor".into(), sola_bus::topics::FloatGeometry {
        app_id: "UnrealEditor".into(), x: 100, y: 50, width: 1280, height: 800,
    });
    let frame = z.apply_config_zone("UnrealEditor", 3).expect("restore frame");
    assert_eq!((frame.x, frame.y, frame.width, frame.height), (100, 50, 1280, 800));
    assert!(!frame.fullscreen);
    // Float without saved geometry → still None (centered by sola-river).
    z.app_zone_config.insert("Blender".into(), Zone::Float);
    assert!(z.apply_config_zone("Blender", 4).is_none());
}
```

- [ ] **Step 2 — run, expect fail.** `cargo test -p sola-shell float_with_saved_geometry`

- [ ] **Step 3 — implement.** In `apply_config_zone`'s `Float` branch (which today
  records `config_applied`/`window_zones` and returns `None`), before returning
  `None`, consult `self.float_geometry`:

```rust
        if matches!(zone, Zone::Float) {
            self.window_zones.insert(window_id, zone);
            self.config_applied.insert(window_id);
            if let Some(g) = self.float_geometry.get(app_id) {
                return Some(FrameUpdate {
                    window_id, x: g.x, y: g.y, width: g.width, height: g.height,
                    fullscreen: false,
                });
            }
            return None;
        }
```

  (Match the existing `Float` arm's exact bookkeeping — keep whatever
  `config_applied`/`window_zones` writes are already there; only add the
  `float_geometry` lookup + frame return.)

- [ ] **Step 4 — run:** `cargo test -p sola-shell` → pass; `cargo make build` → clean.

- [ ] **Step 5 — commit:**
  `feat(sola-shell): restore a floating app's saved rectangle on relaunch`

---

## Out of scope (deferred to Phase D)

- Debouncing `FloatGeometry` persistence during a live drag (Phase B has no drag,
  so geometry changes are rare — initial place + dimensions only).
- Retracting `FloatGeometry` on unfloat (leaving it means a re-float restores the
  last spot — desirable; revisit if it becomes surprising).
- Any titlebar/move/resize UI — that's Phase D.

## Self-review checklist (run after implementing)

- [ ] `WindowGeometry` sticky+keyed, `FloatGeometry` persistent+keyed (Task 1 tests).
- [ ] No size is ever proposed before first `dimensions` — restore rides the Phase A
  gate (size in the restore `FrameUpdate` flows through `pending.frame` →
  `manage`, which the gate defers). Confirm by re-reading `handle_manage_start`.
- [ ] `set_size`/`set_position` names match what Task 3 calls.
- [ ] Shell dispatch arms added for both new topics; `known_windows` lookup maps
  `window_id → app_id`.
