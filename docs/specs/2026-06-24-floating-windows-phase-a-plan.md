# Floating Windows — Phase A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Float` window class the shell positions but never sizes, plus a universal "don't size a window before its surface is initialized" race fix — together resolving the deterministic UnrealEditor (Vulkan/SDL3) crash on launch.

**Architecture:** A new `Zone::Float` variant threads through the existing zone pipeline (`state.yaml` `Zones:` → `app_zone_config` → keybind table → `Topic::Zones`). The shell records the assignment but emits no sizing `Topic::Frame` for it, so `sola-river`'s existing `apply_default_placement` centers the self-sized window. Independently, `sola-river` gains a per-window gate keyed on the first `river_window_v1.dimensions` event: any real size requested before that event is deferred (the window self-sizes first, then takes the size as a normal runtime resize one cycle later). A `Meta+KP_Multiply` chord floats the focused window at runtime.

**Tech Stack:** Rust, Smithay-adjacent river-window-management-v1 protocol (wayland-client), iced shell, `cargo make build`, `cargo test`. Bus IPC via `sola-bus`.

## Global Constraints

- **NEVER run `cargo make install` (or any variant) without express per-install user permission.** This plan's deliverable is *built and tested*, never installed. (CLAUDE.md)
- **Build with `cargo make build`** — never raw `cargo build` or `cp`. Building needs no permission; installing does.
- **Run tests with `cargo test -p <crate>`** for the crate you changed.
- Keep modules small and focused; no speculative abstractions (YAGNI). (CLAUDE.md)
- This is a single active Sola session: work directly on `master` per the user's standing preference (skip the `.worktrees/` ceremony). Confirm commit cadence with the user at execution start; do not push.
- Persistent bus topics (`Zones`) load once at bus startup — no hot reload. A `state.yaml` edit takes effect on the next `sola` launch.
- `Zone` is a `Copy` unit enum deriving `Serialize`/`Deserialize`; unit variants serialize as their bare name (`Float`). Do not add serde rename attributes.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/sola-bus/src/topics.rs` | Bus topic + `Zone` enum definitions | Add `Zone::Float` + `rect()` arm |
| `crates/sola-core/src/keys.rs` | XKB keycode constants | Add `KP_MULTIPLY` + display arm |
| `crates/sola-shell/src/zoning.rs` | Shell zoning state machine | `Float` in keymap; short-circuit `handle_key`/`apply_config_zone` |
| `crates/sola-river/src/client/mod.rs` | `AppData` state | Add `first_dimensions` / `deferred_size` fields |
| `crates/sola-river/src/client/manage.rs` | Manage/render sequence handlers | Gate helpers + wire into `handle_manage_start` |
| `crates/sola-river/src/client/window.rs` | `river_window_v1` event dispatch | Handle `dimensions` event; cleanup on close |

No new files. No new dependencies.

---

## Task 1: `Zone::Float` variant + `rect()` arm (sola-bus)

**Files:**
- Modify: `crates/sola-bus/src/topics.rs` (enum `Zone` ~line 165; `impl Zone/rect` ~line 183)
- Test: `crates/sola-bus/src/topics.rs` (`#[cfg(test)] mod tests`, alongside `zones_roundtrips_via_yaml` ~line 741)

**Interfaces:**
- Produces: `Zone::Float` — a new unit variant of the existing `pub enum Zone`. `Zone::Float.rect()` returns `(0.0, 0.0, 0.0, 0.0)` (value unused; floats never go through the sizing path). Consumed by Task 3.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/sola-bus/src/topics.rs` (next to `zones_roundtrips_via_yaml`):

```rust
    #[test]
    fn zone_float_rect_is_zero() {
        // Float never goes through the sizing path; rect() must stay
        // exhaustive but its value is unused.
        assert_eq!(Zone::Float.rect(), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn zone_float_roundtrips_via_yaml() {
        let mut zones: HashMap<String, Zone> = HashMap::new();
        zones.insert("UnrealEditor".into(), Zone::Float);

        let value = Topic::Zones(zones.clone())
            .to_yaml_value()
            .expect("Zones is persistent; must serialize to YAML");

        match Topic::from_yaml_section(TopicKind::Zones, value) {
            Some(Topic::Zones(back)) => assert_eq!(back, zones),
            other => panic!("expected Zones, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sola-bus zone_float`
Expected: FAIL — `no variant named Float found for enum Zone` (compile error).

- [ ] **Step 3: Add the `Float` variant**

In `crates/sola-bus/src/topics.rs`, add `Float` as the last variant of `pub enum Zone` (after `Cinema`):

```rust
    /// True fullscreen including the menubar — the cinema / no-chrome
    /// view. The shell skips its menubar offset for this zone so the
    /// window covers the whole output.
    Cinema,
    /// App-sized / floating: positioned by the shell (centered, or a
    /// remembered location in later phases) but never force-resized. The
    /// window keeps the size it chooses for itself; the shell emits no
    /// sizing frame for it. Reuses the whole zone pipeline for
    /// designation + persistence.
    Float,
```

- [ ] **Step 4: Add the `rect()` arm**

In `impl Zone`, add a `Float` arm to the `rect()` match (after `Zone::Cinema`):

```rust
            Zone::Cinema => (0.0, 0.0, 1.0, 1.0),
            // Float never goes through the sizing path; the value is unused
            // but the match must stay exhaustive.
            Zone::Float => (0.0, 0.0, 0.0, 0.0),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p sola-bus`
Expected: PASS — `zone_float_rect_is_zero`, `zone_float_roundtrips_via_yaml`, and all existing `sola-bus` tests green.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-bus/src/topics.rs
git commit -m "feat(sola-bus): add Zone::Float variant for app-sized windows"
```

---

## Task 2: `KP_MULTIPLY` keycode (sola-core)

**Files:**
- Modify: `crates/sola-core/src/keys.rs` (numpad constants ~line 126; `display()` numpad arms ~line 198)
- Test: `crates/sola-core/src/keys.rs` (`#[cfg(test)] mod tests` ~line 300)

**Interfaces:**
- Produces: `KeyCode::KP_MULTIPLY` = `KeyCode(63)` (XKB code: evdev `KEY_KPASTERISK` 55 + 8). `KeyCode::KP_MULTIPLY.display()` returns `"KP*"`. Consumed by Task 3.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `crates/sola-core/src/keys.rs`:

```rust
    #[test]
    fn kp_multiply_has_xkb_code_and_label() {
        assert_eq!(KeyCode::KP_MULTIPLY.raw(), 63);
        assert_eq!(KeyCode::KP_MULTIPLY.display(), "KP*");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sola-core kp_multiply`
Expected: FAIL — `no associated item named KP_MULTIPLY found` (compile error).

- [ ] **Step 3: Add the constant**

In `crates/sola-core/src/keys.rs`, under the `// --- Numpad used by zoning ---` block, add after `KP_ENTER`:

```rust
    /// Numpad Enter — used by zoning for the Cinema zone (true
    /// fullscreen including the menubar).
    pub const KP_ENTER: Self = Self(104);
    /// Numpad `*` (KP_Multiply) — floats the focused window.
    pub const KP_MULTIPLY: Self = Self(63);
```

- [ ] **Step 4: Add the display arm**

In `display()`, add to the `// Numpad` arms (after `125 => "KP="`):

```rust
            91 => "KP.",
            125 => "KP=",
            63 => "KP*",
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p sola-core`
Expected: PASS — `kp_multiply_has_xkb_code_and_label` plus existing key tests green.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-core/src/keys.rs
git commit -m "feat(sola-core): add KP_MULTIPLY keycode for float toggle"
```

---

## Task 3: Float in zoning — keymap + no-frame short-circuit (sola-shell)

**Files:**
- Modify: `crates/sola-shell/src/zoning.rs`
  - `ZONING_KEYCODES` (~line 207)
  - `zone_for_keycode` (~line 219)
  - `impl ZoningState/apply_config_zone` (~line 102)
  - `impl ZoningState/handle_key` (~line 119)
- Test: `crates/sola-shell/src/zoning.rs` (`#[cfg(test)] mod tests` ~line 263)

**Interfaces:**
- Consumes: `Zone::Float` (Task 1), `KeyCode::KP_MULTIPLY` (Task 2).
- Produces: behavior change only — `handle_key` and `apply_config_zone` return `Option<FrameUpdate>` exactly as before, but return `None` for `Float` while still recording the assignment in `window_zones` / `app_zone_config` and (for `handle_key`) setting `zones_dirty`. `ZONING_KEYCODES` now includes `KP_MULTIPLY` (so `Shell::shell_key_chords` auto-registers `Meta+KP_Multiply` — no change needed there, it iterates `ZONING_KEYCODES`).

- [ ] **Step 1: Write the failing tests**

Add to `#[cfg(test)] mod tests` in `crates/sola-shell/src/zoning.rs`:

```rust
    #[test]
    fn kp_multiply_maps_to_float() {
        assert_eq!(zone_for_keycode(KeyCode::KP_MULTIPLY.raw()), Some(Zone::Float));
    }

    #[test]
    fn zoning_keycodes_include_float_key() {
        assert!(ZONING_KEYCODES.contains(&KeyCode::KP_MULTIPLY.raw()));
    }

    #[test]
    fn handle_key_float_records_zone_emits_no_frame() {
        let mut s = state_with_output(1920, 1080);
        s.set_focused("UnrealEditor".to_string());
        let frame = s.handle_key(KeyCode::KP_MULTIPLY.raw(), Some(42));
        // No sizing frame for a floating window.
        assert!(frame.is_none(), "Float must not emit a frame");
        // But the assignment is recorded + persisted.
        assert_eq!(s.window_zones.get(&42).copied(), Some(Zone::Float));
        let update = s.take_zones_update().expect("Float must dirty zones");
        assert_eq!(update.get("UnrealEditor").copied(), Some(Zone::Float));
    }

    #[test]
    fn apply_config_zone_float_records_zone_emits_no_frame() {
        let mut s = state_with_output(1920, 1080);
        let mut zones = std::collections::HashMap::new();
        zones.insert("UnrealEditor".to_string(), Zone::Float);
        s.set_zones(zones);

        let frame = s.apply_config_zone("UnrealEditor", 7);
        assert!(frame.is_none(), "Float config must not emit a frame");
        assert_eq!(s.window_zones.get(&7).copied(), Some(Zone::Float));
        // Marked applied so it isn't retried every Windows event.
        assert!(s.apply_config_zone("UnrealEditor", 7).is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sola-shell zone`
Expected: FAIL — `zone_for_keycode` has no `Float` arm so `kp_multiply_maps_to_float` returns `None`; `handle_key`/`apply_config_zone` currently call `compute_frame` and return `Some` for any saved zone (or `None` only when geometry/app missing), so the Float assertions fail.

- [ ] **Step 3: Add `KP_MULTIPLY` to `ZONING_KEYCODES`**

In `crates/sola-shell/src/zoning.rs`, append to the `ZONING_KEYCODES` slice (after `KP_ENTER`):

```rust
pub const ZONING_KEYCODES: &[u32] = &[
    KeyCode::KP_8.raw(),
    KeyCode::KP_4.raw(),
    KeyCode::KP_5.raw(),
    KeyCode::KP_6.raw(),
    KeyCode::KP_2.raw(),
    KeyCode::KP_0.raw(),
    KeyCode::KP_EQUAL.raw(),
    KeyCode::KP_DECIMAL.raw(),
    KeyCode::KP_ENTER.raw(),
    KeyCode::KP_MULTIPLY.raw(),
];
```

- [ ] **Step 4: Add the `zone_for_keycode` arm**

Add a `Float` arm to `zone_for_keycode` (before `_ => None`):

```rust
        c if c == KeyCode::KP_ENTER.raw() => Some(Zone::Cinema),
        c if c == KeyCode::KP_MULTIPLY.raw() => Some(Zone::Float),
        _ => None,
```

- [ ] **Step 5: Short-circuit `Float` in `apply_config_zone`**

Replace the body of `apply_config_zone` so `Float` is recorded but never sized. The `Float` check goes *before* the `output_size?` guard (floats need no geometry):

```rust
    pub fn apply_config_zone(&mut self, app_id: &str, window_id: u32) -> Option<FrameUpdate> {
        if self.config_applied.contains(&window_id) {
            return None;
        }
        let zone = self.app_zone_config.get(app_id).copied()?;
        // Floating windows are positioned by sola-river (centered) but never
        // sized by the shell. Record the assignment so it isn't retried each
        // Windows event and so phases B/D can see it, then emit no frame.
        // No output geometry is required.
        if matches!(zone, Zone::Float) {
            self.config_applied.insert(window_id);
            self.window_zones.insert(window_id, zone);
            return None;
        }
        // If geometry hasn't arrived yet we can't compute the frame. Bail
        // without mutating state so a later Apps event retries once
        // OutputGeometry has been cached.
        let (w, h) = self.output_size?;
        self.config_applied.insert(window_id);
        self.window_zones.insert(window_id, zone);
        Some(compute_frame(zone, window_id, w, h))
    }
```

- [ ] **Step 6: Short-circuit `Float` in `handle_key`**

Replace the body of `handle_key`. Resolve `zone`, `app_id`, and `window_id` first; `Float` records + persists + returns `None` before the geometry guard; every other zone keeps the existing path:

```rust
    pub fn handle_key(&mut self, code: u32, focused_window_id: Option<u32>) -> Option<FrameUpdate> {
        let zone = zone_for_keycode(code)?;

        let app_id = match self.focused_app_id.clone() {
            Some(id) => id,
            None => {
                warn!("zone key pressed but no focused app");
                return None;
            }
        };

        let window_id = match focused_window_id {
            Some(wid) => wid,
            None => {
                warn!("zone key pressed but no focused window_id");
                return None;
            }
        };

        // Floating: record + persist the assignment, emit no frame. The
        // client keeps its own size; sola-river centers it. Unfloat is just
        // pressing any other Meta+Numpad zone key, which overwrites the zone
        // here and emits a sizing frame as usual.
        if matches!(zone, Zone::Float) {
            info!(app_id = %app_id, window_id, "floating window (no sizing frame)");
            self.window_zones.insert(window_id, zone);
            self.app_zone_config.insert(app_id, zone);
            self.config_applied.insert(window_id);
            self.zones_dirty = true;
            return None;
        }

        let (w, h) = match self.output_size {
            Some(s) => s,
            None => {
                warn!("zone key pressed but no output geometry cached");
                return None;
            }
        };

        let frame = compute_frame(zone, window_id, w, h);
        info!(
            app_id = %app_id,
            window_id,
            ?zone,
            x = frame.x,
            y = frame.y,
            width = frame.width,
            height = frame.height,
            "snapping to zone"
        );

        self.window_zones.insert(window_id, zone);

        // Persist the zone for every app — sola or external — so layouts
        // survive a restart and re-apply when the app's window reappears.
        self.app_zone_config.insert(app_id, zone);
        self.config_applied.insert(window_id);
        self.zones_dirty = true;

        Some(frame)
    }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p sola-shell`
Expected: PASS — the four new Float tests plus all existing zoning tests (`handle_key_returns_none_without_geometry`, `..._without_focused_app`, `..._snaps_window_and_marks_sola_zone_dirty`, `..._persists_external_app_zone`, the geometry-parity tests) stay green. The reordered guards in `handle_key` still return `None` for the missing-app and missing-geometry cases.

- [ ] **Step 8: Commit**

```bash
git add crates/sola-shell/src/zoning.rs
git commit -m "feat(sola-shell): Float zone records assignment but emits no sizing frame"
```

---

## Task 4: Gate state + pure helpers (sola-river)

**Files:**
- Modify: `crates/sola-river/src/client/mod.rs` (`struct AppData` ~line 40; `impl AppData/new` ~line 100)
- Modify: `crates/sola-river/src/client/manage.rs` (add helpers + `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `AppData.first_dimensions: std::collections::HashSet<u32>` — window ids that have received their first `dimensions` event.
  - `AppData.deferred_size: HashMap<u32, (i32, i32)>` — sizes held back until a window initializes.
  - `crate::client::manage::SizeDecision` (enum: `Propose(i32, i32)`, `Defer(i32, i32)`) and `pub(crate) fn size_decision(requested: (i32, i32), initialized: bool) -> SizeDecision`.
  - `pub(crate) fn note_dimensions(first_dimensions: &mut HashSet<u32>, deferred_size: &mut HashMap<u32,(i32,i32)>, window_id: u32) -> Option<(i32,i32)>`.
- Consumed by Task 5.

- [ ] **Step 1: Write the failing tests**

Append a test module to the end of `crates/sola-river/src/client/manage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn uninitialized_real_size_is_deferred() {
        assert_eq!(size_decision((800, 600), false), SizeDecision::Defer(800, 600));
    }

    #[test]
    fn initialized_real_size_is_proposed() {
        assert_eq!(size_decision((800, 600), true), SizeDecision::Propose(800, 600));
    }

    #[test]
    fn self_size_is_always_proposed() {
        // (0,0) means "client decides" — safe pre-init and post-init.
        assert_eq!(size_decision((0, 0), false), SizeDecision::Propose(0, 0));
        assert_eq!(size_decision((0, 0), true), SizeDecision::Propose(0, 0));
    }

    #[test]
    fn note_dimensions_hands_back_deferred_size_once() {
        let mut first = HashSet::new();
        let mut deferred = HashMap::new();
        deferred.insert(7u32, (1280, 720));

        // First dimensions event: marks initialized, returns the held size.
        assert_eq!(note_dimensions(&mut first, &mut deferred, 7), Some((1280, 720)));
        assert!(first.contains(&7));
        assert!(deferred.is_empty());

        // Second event: already initialized, nothing left to apply.
        assert_eq!(note_dimensions(&mut first, &mut deferred, 7), None);
        assert!(first.contains(&7));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sola-river size_decision note_dimensions`
Expected: FAIL — `cannot find function size_decision` / `cannot find type SizeDecision` (compile error).

- [ ] **Step 3: Add the gate helpers to `manage.rs`**

Insert near the top of `crates/sola-river/src/client/manage.rs` (after the `use` lines, before `handle_manage_start`):

```rust
use std::collections::{HashMap, HashSet};

/// Outcome of the first-`dimensions` gate for one window in a manage cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeDecision {
    /// Propose this size now.
    Propose(i32, i32),
    /// Self-size now (propose `(0, 0)`) and hold this size until the
    /// window's first `dimensions` event proves the surface is initialized.
    Defer(i32, i32),
}

/// Decide what to propose for a window this manage cycle.
///
/// `river-window-management-v1` guarantees a window is not displayed until
/// its first `dimensions` event. Sending a *sizing* configure before that
/// event can invalidate a client's swapchain mid-init — UnrealEditor
/// (Vulkan/SDL3) dies exactly this way. So any real size requested before
/// initialization is deferred; the window self-sizes first and takes the
/// real size as a normal runtime resize one cycle later. A `(0, 0)` request
/// is "client decides its own size" and is always safe.
pub(crate) fn size_decision(requested: (i32, i32), initialized: bool) -> SizeDecision {
    if !initialized && requested != (0, 0) {
        SizeDecision::Defer(requested.0, requested.1)
    } else {
        SizeDecision::Propose(requested.0, requested.1)
    }
}

/// Record that `window_id` received its first `dimensions` event and return
/// any size that was deferred waiting for it, so the caller can re-queue it
/// for the next manage cycle. Returns `None` if nothing was deferred.
pub(crate) fn note_dimensions(
    first_dimensions: &mut HashSet<u32>,
    deferred_size: &mut HashMap<u32, (i32, i32)>,
    window_id: u32,
) -> Option<(i32, i32)> {
    first_dimensions.insert(window_id);
    deferred_size.remove(&window_id)
}
```

> If the `use std::collections::{HashMap, HashSet};` line collides with an existing import in `manage.rs`, merge them into one line rather than duplicating.

- [ ] **Step 4: Add the `AppData` fields**

In `crates/sola-river/src/client/mod.rs`, add two fields to `struct AppData` immediately after the `placed` field:

```rust
    pub placed: std::collections::HashSet<u32>,
    /// Windows that have received their first `river_window_v1.dimensions`
    /// event. Until a window is in this set, we never send it a sizing
    /// configure — only `propose_dimensions(0, 0)` (self-size) — so a
    /// GPU/Vulkan client can build its swapchain against its own size
    /// before any resize arrives. See `manage::size_decision`.
    pub first_dimensions: std::collections::HashSet<u32>,
    /// Sizes (zone or restore) requested for a window before it was
    /// initialized, held back until its first `dimensions` event. Applied
    /// as a normal runtime resize on the next manage cycle. See
    /// `manage::note_dimensions`.
    pub deferred_size: HashMap<u32, (i32, i32)>,
```

- [ ] **Step 5: Initialize the fields in `AppData::new`**

In `impl AppData/new`, add the two initializers after `placed: std::collections::HashSet::new(),`:

```rust
            placed: std::collections::HashSet::new(),
            first_dimensions: std::collections::HashSet::new(),
            deferred_size: HashMap::new(),
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p sola-river size_decision note_dimensions`
Expected: PASS — all four helper tests green. (`cargo test -p sola-river` should also still build the whole crate clean.)

- [ ] **Step 7: Commit**

```bash
git add crates/sola-river/src/client/mod.rs crates/sola-river/src/client/manage.rs
git commit -m "feat(sola-river): add first-dimensions gate state + decision helpers"
```

---

## Task 5: Wire the gate into manage/render + dimensions event (sola-river)

**Files:**
- Modify: `crates/sola-river/src/client/manage.rs` (`handle_manage_start` ~line 18)
- Modify: `crates/sola-river/src/client/window.rs` (`Dispatch<RiverWindowV1>` match: add `Dimensions` arm ~line 176; extend `Closed` cleanup ~line 138)

**Interfaces:**
- Consumes: `size_decision`, `SizeDecision`, `note_dimensions`, `AppData.first_dimensions`, `AppData.deferred_size` (Task 4).
- Produces: integrated behavior. `handle_manage_start` proposes `(0,0)` and stashes the real size in `deferred_size` for any window not yet in `first_dimensions`; the `dimensions` event marks the window initialized and re-queues the deferred size (`pending.manage` + `manage_dirty`), which the next 20ms `bus_tick` turns into a manage cycle. The `Closed` handler purges both maps.

> No unit test is added here: `handle_manage_start` and the dispatch handler call live wayland proxies (`propose_dimensions`) that require a running compositor. The gate *logic* is fully covered by Task 4's pure-helper tests; this task's deliverable is verified by a clean `cargo make build` plus the Task 4 tests, and by the manual smoke in Task 6.

- [ ] **Step 1: Rewrite `handle_manage_start` to apply the gate**

Replace the whole body of `handle_manage_start` in `crates/sola-river/src/client/manage.rs`. The only change from the current version is the per-window gate in the proposal loop (drain into an owned vec so we can mutate `deferred_size` inside the loop); everything below the loop is unchanged:

```rust
pub fn handle_manage_start(state: &mut AppData) {
    let Some(wm) = state.wm.clone() else { return };

    // Drain into an owned vec so we can mutate `state.deferred_size` inside
    // the loop without aliasing `state.pending.manage`.
    let manage: Vec<(u32, (i32, i32))> = state.pending.manage.drain().collect();
    let pending_count = manage.len();
    for (window_id, (w, h)) in manage {
        let Some(proxy) = state.windows_by_id.get(&window_id).cloned() else {
            continue;
        };
        let app_id = state
            .registry
            .app_id_for(window_id)
            .unwrap_or("?")
            .to_string();
        let initialized = state.first_dimensions.contains(&window_id);
        match size_decision((w, h), initialized) {
            SizeDecision::Propose(pw, ph) => {
                tracing::info!(window_id, %app_id, w = pw, h = ph, "propose_dimensions");
                proxy.propose_dimensions(pw, ph);
            }
            SizeDecision::Defer(dw, dh) => {
                tracing::info!(
                    window_id,
                    %app_id,
                    w = dw,
                    h = dh,
                    "deferring size until first dimensions; self-sizing"
                );
                state.deferred_size.insert(window_id, (dw, dh));
                proxy.propose_dimensions(0, 0);
            }
        }
    }
    state.pending.manage_dirty = false;
    // `pending.manage` was drained above; no separate clear needed.

    if let Some(focus) = state.pending.focus.take() {
        if let Some(seat) = state.seat.as_ref() {
            match focus {
                FocusAction::Window(id) => {
                    if let Some(proxy) = state.windows_by_id.get(&id) {
                        seat.focus_window(proxy);
                        state.focused_window = Some(id);
                    }
                }
                FocusAction::None => {
                    seat.clear_focus();
                    state.focused_window = None;
                }
            }
        }
    }

    if let Some(pairs) = state.pending.chords.take() {
        crate::translator::apply_pending_chords(state, pairs);
    }

    let close_ids: Vec<u32> = std::mem::take(&mut state.pending.close_windows);
    let mut close_count = 0;
    for window_id in close_ids {
        if let Some(proxy) = state.windows_by_id.get(&window_id) {
            proxy.close();
            close_count += 1;
        }
    }
    if close_count > 0 {
        tracing::info!(close_count, "CloseApp: sent river_window_v1.close");
    }

    apply_fullscreen_requests(state);

    wm.manage_finish();
    debug!(pending_count, "manage_finish sent");
}
```

- [ ] **Step 2: Handle the `dimensions` event in `window.rs`**

In `crates/sola-river/src/client/window.rs`, in the `Dispatch<RiverWindowV1>` `match event` block, add a `Dimensions` arm just before the final `_ => {}`:

```rust
            Event::Dimensions { width, height } => {
                let newly_initialized = !state.first_dimensions.contains(&window_id);
                if let Some((w, h)) = crate::client::manage::note_dimensions(
                    &mut state.first_dimensions,
                    &mut state.deferred_size,
                    window_id,
                ) {
                    // Surface is initialized now — apply the size we held back
                    // as a normal runtime resize. The next bus_tick (≤20ms)
                    // turns manage_dirty into a manage cycle that proposes it.
                    state.pending.manage.insert(window_id, (w, h));
                    state.pending.manage_dirty = true;
                }
                tracing::debug!(window_id, width, height, newly_initialized, "window dimensions");
            }
            _ => {}
```

> The `river_window_v1` `dimensions` event has exactly two `int` args (`width`, `height`). If the generated binding carries extra fields, change the pattern to `Event::Dimensions { width, height, .. }`.

- [ ] **Step 3: Purge gate state on window close**

In the same file, extend the `Event::Closed` arm's cleanup (after `state.currently_fullscreen.remove(&window_id);`):

```rust
                state.placed.remove(&window_id);
                state.currently_fullscreen.remove(&window_id);
                state.first_dimensions.remove(&window_id);
                state.deferred_size.remove(&window_id);
```

- [ ] **Step 4: Build and run the crate tests**

Run: `cargo make build`
Expected: clean build (no errors, no warnings from these files).

Run: `cargo test -p sola-river`
Expected: PASS — Task 4 helper tests plus all existing `sola-river` tests green.

- [ ] **Step 5: Commit**

```bash
git add crates/sola-river/src/client/manage.rs crates/sola-river/src/client/window.rs
git commit -m "fix(sola-river): never size a window before its first dimensions event"
```

---

## Task 6: Make UnrealEditor float + manual verification (user-run)

This task delivers the actual UnrealEditor fix and verifies the feature end to end. It contains **no committed code** — `state.yaml` is user config, not in the repo — and the run/smoke steps happen on the physical TTY, so the **user runs them**. Present these steps to the user; do not install or launch anything yourself.

**Files:**
- Modify (user machine, not repo): `~/.config/sola/state.yaml`

- [ ] **Step 1: Build everything**

Run: `cargo make build`
Expected: clean build of all crates.

- [ ] **Step 2: Install (USER PERMISSION REQUIRED)**

Do **not** run this without the user's explicit go-ahead for this specific install. When approved:

Run: `cargo make install sola-river sola-shell` (and `sola-bus`/`sola-core` consumers rebuild transitively)
Expected: binaries copied to `/opt/sola/bin/`.

- [ ] **Step 3: Designate UnrealEditor as floating**

Edit `~/.config/sola/state.yaml` so the `Zones:` section contains:

```yaml
Zones:
  UnrealEditor: Float
```

(Leave any other app→zone entries intact.) Persistent topics load once at bus startup, so this takes effect on the next `sola` launch.

- [ ] **Step 4: Launch Sola and Unreal, with logs**

From a TTY:

```bash
RUST_LOG=info /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
```

Then launch the editor via `/home/joshua/.local/bin/unreal-editor`.

Expected:
- UnrealEditor maps at **its own size**, centered, and **survives past ~13s** (no "outdated swapchain" / "Wayland display connection closed by server (fatal)" in the UE log; no clean exit code 0).
- `sola-river` log shows `propose_dimensions … w=0 h=0` for the `UnrealEditor` window on first manage, then a `window dimensions` line, and **no** zone-sized `propose_dimensions` before it.

- [ ] **Step 5: Verify the runtime float toggle**

With a normally-zoned app focused (e.g. a terminal), press **Meta+KP_Multiply** (numpad `*`).
Expected: the window stops being shell-sized (takes its own size, centered). Pressing any other Meta+Numpad zone key (e.g. Meta+KP_5) re-snaps it to a zone. The float state persists across relaunch (written to `state.yaml` `Zones:`).

- [ ] **Step 6: Confirm the universal gate on a zoned GPU app (regression guard)**

Launch any zoned Vulkan/GPU client (not floating). Confirm in the `sola-river` log that its **first** `propose_dimensions` is `0,0` (self-size), a `window dimensions` line follows, and the zone size is proposed on the **next** cycle — i.e. the zoned path now also self-sizes first, then resizes. The app must not crash on map.

---

## Self-Review

**Spec coverage (against `2026-06-24-floating-windows-design.md` §4):**
- A1 `Zone::Float` variant + `rect()` arm → Task 1. ✓
- A2 shell stops sizing floating windows (`apply_config_zone`/`handle_key` return `None`, still record) → Task 3. ✓
- A3 race-fix gate (`first_dimensions`, `deferred_size`, defer in `handle_manage_start`) → Tasks 4 (logic) + 5 (wiring). ✓
- A4 handle the `dimensions` event (mark initialized; phase A needs only the flag) → Task 5 Step 2. ✓
- A5 float toggle keybind (`KP_MULTIPLY`, `ZONING_KEYCODES`, `zone_for_keycode`) → Tasks 2 + 3. ✓
- A6 tests (zone_for_keycode==Float; handle_key Float→None+dirty; apply_config_zone Float→None+marks; defer-then-apply) → Tasks 1/3/4 tests. The defer→apply round-trip is covered by `size_decision` + `note_dimensions` unit tests (Task 4) rather than an integration test, because the wiring (Task 5) calls live wayland proxies; documented inline. ✓
- Config `UnrealEditor: Float` → Task 6 Step 3. ✓

**Placeholder scan:** No `TBD`/`TODO`/"add error handling"-style vagueness. Every code step shows complete code; every test step shows the assertions; every run step shows the command and expected result. ✓

**Type consistency:** `Zone::Float` (Task 1) is consumed identically in Task 3. `KeyCode::KP_MULTIPLY` (Task 2) is used in Task 3's `ZONING_KEYCODES`/`zone_for_keycode`. `size_decision`/`SizeDecision`/`note_dimensions` signatures defined in Task 4 match their call sites in Task 5 (`size_decision((w,h), initialized)`, `note_dimensions(&mut first_dimensions, &mut deferred_size, window_id)`). `AppData.first_dimensions: HashSet<u32>` and `AppData.deferred_size: HashMap<u32,(i32,i32)>` match the types passed to `note_dimensions`. ✓

**Scope:** Phase A only — independently shippable, resolves the UnrealEditor crash. Phases B (live geometry + float memory) and D (move/resize + titlebar + window menu) get their own plans.

---

## Execution Handoff

After this plan is approved, two execution options:

1. **Subagent-Driven (recommended)** — a fresh subagent implements each task, with review between tasks.
2. **Inline Execution** — execute tasks in this session with checkpoints for review.

Tasks 1–5 are code + automated tests; Task 6 is user-run (build is fine unattended, but **install and the TTY smoke require explicit user action/permission**).
