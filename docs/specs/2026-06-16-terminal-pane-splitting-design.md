# Terminal Pane Splitting — Design

**Date:** 2026-06-16
**Status:** Approved, ready to plan

## Goal

Let a `sola-terminal` tab be split into multiple **panes**, each pane
independently splittable side-by-side or stacked, recursively. Each pane is its
own tmux session with its own shell. The split layout and its divider sizing are
remembered across a terminal restart. Sola draws the dividers (reusing the kit
`split` component); tmux is **not** asked to manage pane geometry.

Bundled in: the shell menubar/menu must finally **render the keyboard shortcut**
next to each menu item (the accelerator data is already on the bus; only the
view omits it). This is a generic shell fix that every app benefits from.

## Background

### Today: one tab = one pane

A tab is a single shell. The relevant state (`crates/sola-terminal/src/`):

- `state.rs` — `TabRuntime { emulator: Emulator, backend: PtyBackend }` is the
  live runtime; `TabMeta { id, tmux_session, cwd, ordinal }` is the metadata;
  `Tabs { meta, runtime }` holds `meta: Vec<TabMeta>` and
  `runtime: HashMap<id, TabRuntime>`. Tab id and the (single) shell are 1:1.
- `tmux.rs` — each tab has its own tmux session `sola-{id}` (`session_name`);
  `PtyBackend` runs `tmux attach`. A plain `Drop` deliberately **preserves** the
  session (crash-safe); only an explicit close calls `kill_session`.
- `term_view.rs` — `TermView` is a `canvas::Program` rendering **one** emulator
  grid to the full content area. `CellMetrics` + `cols_rows_for` derive
  cols/rows from a pixel size.
- `main.rs` — `App { tabs, active, … }`; `Msg`; `dispatch_action` (≈L1213)
  handles menu-action ids (`new_tab`, `close_tab`, `copy`, `paste`,
  `select_tab_{N}`); `new_tab`/`close_tab`/`select_tab`. A dead app-local key
  block (≈L715–756) intercepts Ctrl+Shift+{C,V,T,W} and ⌘/Ctrl+digit — see
  below.
- Persistence: `crates/sola-bus/src/topics.rs`
  `#[persistent(keys=[id])] TerminalSession { id, tmux_session, cwd, ordinal }`
  — one slot per tab, replays on boot.

### Shell-driven keyboard shortcuts (load-bearing — do not reinvent in the app)

There is **no correct app-local shortcut path**. Every shortcut flows through
sola-shell via the app menu:

1. **App → shell:** the app publishes `Topic::SetAppMenu` (sticky per `app_id`).
   Each `MenuItem::Action` carries `shortcut: Option<KeyChord>`. The terminal
   already ships `Edit ▸ Copy ⌘C`, `Paste ⌘V`; `Shell ▸ New Tab ⌘T`,
   `Close Tab ⌘W`; `Quit ⌘Q`; `Tabs ▸ ⌘1‑9` (`crates/sola-terminal/src/menu.rs`).
2. **Shell caches** them in `MenuCache` as a `KeyChord → (app_id, action_id)`
   table (`crates/sola-shell/src/menu/state.rs`).
3. **Shell → River:** `shell_key_chords()` collects the **focused app's** menu
   shortcuts **filtered to `.meta` only** (`.filter(|b| b.meta)`), plus fixed
   shell chords, and `emit_registered_chords()` emits `Topic::RegisteredChords`;
   sola-river binds them via `river-xkb-bindings-v1`. Re-emitted on focus change,
   so an app's chords are grabbed **only while it is focused**.
4. **Keypress → River → shell:** River emits `Topic::Chord`; `on_chord`
   (`crates/sola-shell/src/app/bus.rs`) decodes it, runs the fixed shell
   handlers, then `menus.lookup_shortcut(chord, focused_app)` →
   `Topic::MenuAction { app_id, action_id }` (and flashes the menubar label).
5. **Shell → app:** the app receives `Msg::Bus(MenuAction)` → `dispatch_action`.
   **Menu clicks call the same `dispatch_action`** — click and shortcut are one
   path.
6. **Non-meta keys pass through:** Ctrl‑P, Ctrl‑A, Ctrl‑C (SIGINT), Ctrl‑W
   (werase) are never registered, so River never grabs them; they flow through
   Wayland to the focused terminal → encoded → PTY. This is the emacs passthrough.

**Consequences that pin this design:**

- Only **`.meta` (⌘) menu items** become live shortcuts. The
  `modifiers.control() && modifiers.shift()` block in `input.rs` is **dead** —
  and it still fires on a *physical* Ctrl+Shift+C (which River passes through),
  duplicating copy. It is deleted, not ported.
- **⌘W = no-op** is achieved by **un-binding ⌘W** (the close item stops
  carrying `KeyCode::W.meta()`). Nothing registers it; nothing fires. A guard in
  `input.rs` ensures a Super-modified key is never forwarded to the PTY, so a
  stray ⌘W is truly inert.
- New actions are **menu items with `.meta` shortcuts**, never app key handlers.

## Design overview

Two id domains, no longer 1:1:

- **`PaneId`** — one live shell: one `Emulator` + `PtyBackend` + tmux session +
  geometry cache. This is exactly today's `TabRuntime`, renamed `PaneRuntime`.
- **`TabId`** — a tab strip entry. A tab owns a **tree** of panes plus the
  focused-pane pointer.

Per-PTY, id-keyed messages (`PtyOutput`, `PtyExit`, `Title`, `CwdResult`) move
from tab-id to **pane-id**. Tab-level concepts (`new_tab`, `select_tab`,
`ordinal`, tab strip) stay **tab-id**. Where tab id == pane id today, splitting
is what breaks the identity.

## 1. Pane data model

```rust
// crates/sola-terminal/src/state.rs (sketch)

pub struct PaneId(String);     // uuid; replaces today's per-tab id for the PTY
pub struct SplitId(String);    // uuid; stable identity for a divider across rebuilds

// SplitDir is defined ONCE in sola-bus (the PaneLayout wire type needs it, and
// sola-bus cannot depend on sola-terminal). sola-terminal imports it.
//   Vertical   = side-by-side → kit row!,    new pane on the RIGHT (⌘⇧→)
//   Horizontal = stacked      → kit column!, new pane BELOW        (⌘⇧↓)
use sola_bus::topics::SplitDir;

pub enum PaneNode {
    Leaf(PaneId),
    Split { id: SplitId, dir: SplitDir, ratio: f32, a: Box<PaneNode>, b: Box<PaneNode> },
}

// `ratio` is pane `a`'s fraction of the split's main axis, in (0,1).

pub struct PaneRuntime {   // was TabRuntime
    pub emulator: Emulator,
    pub backend: PtyBackend,
    // per-pane geometry cache (moved off App.term_cache, now keyed by PaneId)
}

pub struct Tab {
    pub id: String,                 // TabId
    pub layout: PaneNode,           // tree of panes
    pub active_pane: PaneId,        // focus-follows-mouse target
    pub ordinal: u32,
}

pub struct Tabs {
    pub tabs: Vec<Tab>,                       // was meta: Vec<TabMeta>
    pub panes: HashMap<PaneId, PaneRuntime>,  // was runtime keyed by tab id
    pub pane_meta: HashMap<PaneId, PaneMeta>, // tmux_session, cwd per pane
}
```

Pure tree helpers (unit-tested, no I/O):

- `split_leaf(tree, target: PaneId, dir, new: PaneId) -> PaneNode` — replace the
  `Leaf(target)` with `Split{ a: Leaf(target), b: Leaf(new), ratio: 0.5, … }`.
- `close_leaf(tree, target: PaneId) -> Option<PaneNode>` — drop the leaf and
  **promote its sibling** into the parent's place; `None` when the last leaf
  goes (caller then closes the tab).
- `leaves(tree) -> Vec<PaneId>` and `next_active_after_close(tree, closed)` —
  pick the new `active_pane` (sibling-first, like `next_active_after_close` for
  tabs today).
- `pane_rects(tree, content_box, metrics) -> HashMap<PaneId, Rect>` — the layout
  pass (see §3), with divider thickness and a **minimum pane size** subtracted.

## 2. Invocation & shortcuts (menu-driven)

All in `crates/sola-terminal/src/menu.rs` + `dispatch_action`; nothing new in the
key path.

Menu changes:

- `Shell ▸ Close Tab (⌘W)` → **`Close Pane (⌘⇧W)`**, id `close_pane`,
  `shortcut: Some(KeyCode::W.meta_shift())`. ⌘W is now bound to nothing.
- New **`Pane`** menu (or section): `Split Vertical (⌘⇧→)` id `split_vertical`
  `KeyCode::RIGHT.meta_shift()`; `Split Horizontal (⌘⇧↓)` id `split_horizontal`
  `KeyCode::DOWN.meta_shift()`.

`dispatch_action` (`main.rs`) gains:

- `"split_vertical"` / `"split_horizontal"` → split `active_pane` in the given
  dir (see §5), relayout + resize, set the new pane active.
- `"close_pane"` → close `active_pane` (see §5); collapse the tab when it was the
  last pane.

`sola-core` (`crates/sola-core/src/keys.rs`): add `KeyCode::meta_shift()`
(meta + shift). The shell's `.filter(|b| b.meta)` keeps meta+shift chords, and
`river_modifiers`/`to_registered` already fold `shift` into the modifier mask, so
⌘⇧→/↓/W register and fire with no shell change.

Deletions in `input.rs`: the `control() && shift()` copy/paste/new-tab/close
block and the `control() || logo()` digit block (≈L715–756). Add a guard so a
**Super-modified** key never encodes to the PTY (keeps ⌘W et al. inert).

Defaults: only `⌘⇧→`/`⌘⇧↓` (new pane right/below); no `⌘⇧←`/`⌘⇧↑`.

## 3. Rendering & the kit `split` component

Render folds `PaneNode` into kit splits recursively; each `Leaf` is a `TermView`
canvas over that pane's emulator:

- `Split{ dir: Vertical, .. }` → `split(Vertical, a, ratio, on_resize, b)` (row).
- `Split{ dir: Horizontal, .. }` → same call with `Horizontal` (column).

**Kit `split` changes** (`crates/sola-kit/src/components/split.rs`). Today it is
horizontal-only, fixed `left_width`, single `divider_msg`. Generalize to one
orientation-parameterized function:

```rust
pub fn split<'a, Message>(
    dir: SplitDir,
    a: impl Into<Element<'a, Message, Theme>>,
    ratio: f32,                              // a's fraction of the main axis (0,1)
    on_resize: impl Fn(f32) -> Message + 'a, // continuous: new ratio from divider drag
    b: impl Into<Element<'a, Message, Theme>>,
) -> Element<'a, Message, Theme>
```

- Ratio-based (not fixed px) so the layout reflows on window resize.
- `Vertical` builds `row![a, vertical_divider, b]`; `Horizontal` builds
  `column![a, horizontal_divider, b]` (new). Children get
  `FillPortion`-style weights derived from `ratio`.
- The divider is a **draggable** handle (mouse_area / small custom widget): a
  pointer drag along the main axis maps to a new ratio and calls `on_resize`.
  The existing single-message divider used by the sidebar keeps a thin
  press-only wrapper, or the sidebar migrates to the drag form.
- The storybook `Split` page gains both orientations + a draggable demo.

The terminal supplies `on_resize = move |r| Msg::DividerResize(split_id, r)`;
the handler clamps `r` to the minimum-pane bound and updates that `Split` node's
`ratio`, then relayouts + resizes.

**Resize fan-out (critical).** Each pane now owns a sub-rectangle, not the whole
window. On window resize, split, close, or divider drag: run `pane_rects` →
for each pane convert its `Rect` to cols/rows via `CellMetrics`/`cols_rows_for`
→ `emulator.resize(...)` + tmux `resize_window`. A minimum pane size (e.g. a
small floor in cols/rows) clamps ratios so no pane collapses below it.

New `Msg`: `DividerResize(SplitId, f32)`, `PaneFocused(PaneId)` (§4). Existing
`PtyOutput`/`PtyExit`/`Title`/`CwdResult`/`SelectionChanged`/`Scrolled` become
`PaneId`-keyed.

## 4. Focus-follows-mouse

The active pane is the one **under the pointer** (sloppy focus), per the chosen
model — no click-to-focus, no focus chords:

- Each `Leaf`'s `TermView` reports pointer-enter → `Msg::PaneFocused(pane_id)`
  sets `tab.active_pane`. (The `canvas::Program` already sees cursor position;
  enter detection is a bounds check, akin to the existing I-beam
  `mouse_interaction`.)
- Keyboard routing, cursor blink, and the active-pane border all key off
  `active_pane`. Keystrokes (passthrough keys) write to `active_pane`'s PTY.
- Pointer over a divider/gap → keep the last `active_pane` (don't drop focus).
- The shell already sends `MenuAction` to the focused **app**; the app applies it
  to its own `active_pane`. No shell change.

## 5. tmux: a session per pane

Per-pane sessions reuse today's per-tab logic (`tmux.rs`), now keyed by `PaneId`
(`session_name(pane_id)` → `sola-{pane_id}`):

- **Split:** mint a `PaneId`, create+attach a new `sola-{pane_id}` session
  inheriting the **source pane's cwd** (`pane_current_path` / existing
  `inherit_cwd`), insert it as the split sibling, set it active.
- **Close pane:** explicit `kill_session(pane_id)`, drop its `PaneRuntime`,
  `close_leaf` promotes the sibling; closing the last pane closes the tab
  (existing `close_tab`). A plain `Drop` still preserves sessions (crash-safe).
- **Restart:** re-attach each leaf's session (existing attach path, per pane).

## 6. Persistence across restarts

Extend the existing per-tab record rather than adding a topic (keeps a tab's
full state atomic in one slot). `crates/sola-bus/src/topics.rs`:

```rust
pub enum PaneLayout {                 // serializable mirror of PaneNode
    Leaf { tmux_session: String, #[serde(default)] cwd: Option<String> },
    Split { dir: SplitDir, ratio: f32, a: Box<PaneLayout>, b: Box<PaneLayout> },
}

pub struct TerminalSession {
    pub id: String,
    pub tmux_session: String,         // kept: root/first leaf, for back-compat
    #[serde(default)]
    pub cwd: Option<String>,
    pub ordinal: u32,
    #[serde(default)]
    pub layout: Option<PaneLayout>,   // None ⇒ single pane using `tmux_session`
}
```

- `None` layout deserializes old tabs as a single `Leaf(tmux_session)` — no
  migration needed.
- On change (split/close/divider-drag-settled) the terminal emits an updated
  `TerminalSession` with the serialized tree (leaf order, `dir`, `ratio`, each
  leaf's `tmux_session`/`cwd`).
- On boot, rebuild `PaneNode` from `layout`, re-attach each leaf's session,
  restore ratios. The boot-reconcile (`live_tmux_at_startup`, ≈main.rs L966)
  generalizes from "one session per tab" to "one per leaf".

`SplitId`/`PaneId` are regenerated on load (only structure + tmux session names
+ ratios persist; ids are runtime identity).

## 7. Menu shortcut display (shell)

The accelerator is already on the bus (`MenuItem::Action.shortcut`); only the
view drops it. In `crates/sola-shell/src/menu*` (dropdown rows; the menubar flash
path already resolves items):

- Render each item's `shortcut` right-aligned in the row.
- Add a `KeyChord → String` formatter (`"⌘⇧→"`, `"⌘C"`, …): map
  `meta/alt/ctrl/shift` → `⌘⌥⌃⇧` and the keycode → a glyph/name. No `Display`
  impl exists yet — add one in `sola-core` (`KeyChord`) or a shell-side helper;
  prefer `sola-core` so other surfaces can reuse it.

## Defaults & decided behaviors

- ⌘-only keymap; the dead Ctrl+Shift app-key path is removed.
- ⌘W → no-op (unbound); close pane is ⌘⇧W.
- New pane lands right (⌘⇧→) or below (⌘⇧↓); 50/50 initial ratio; no ←/↑.
- Focus-follows-mouse; sloppy focus over dividers.
- New pane inherits the split source's cwd.
- Tab title/cwd in the strip follow the tab's `active_pane`.
- Minimum pane size clamps ratios on drag and on window resize.
- Layout persists across restarts (extended `TerminalSession`).

## Out of scope / limitations

- No detach/move pane between tabs, no pane zoom/maximize, no swap.
- No `⌘⇧←`/`⌘⇧↑` (left/up) splits.
- No keyboard pane navigation (focus-follows-mouse only).
- tmux is never asked to lay out panes; Sola owns geometry. (tmux's own
  `split-window` is unused.)
- Multi-pane restore relies on each leaf's tmux session surviving; a killed
  server loses panes (same failure surface as tabs today).

## Testing

Pure helpers (no I/O), mirroring the repo's existing unit-test style:

- **Tree:** `split_leaf` inserts a balanced 50/50 split at the target;
  `close_leaf` promotes the sibling and returns `None` on the last leaf;
  `next_active_after_close` picks the sibling; `leaves` ordering is stable.
- **Layout:** `pane_rects` partitions a content box by `dir`/`ratio` minus
  divider thickness; ratios clamp to the minimum pane size; a single `Leaf`
  fills the box.
- **Persistence:** `TerminalSession` with `layout: Some(...)` round-trips through
  `to_yaml_value`/`from_yaml_section`; `layout: None` (old record) restores as a
  single pane; `behavior() == Persistent`.
- **Shortcuts:** `KeyCode::meta_shift()` sets meta+shift; the `KeyChord`
  formatter renders `⌘⇧→`, `⌘C`, `⌘⇧W` correctly.
- **Menu:** the new menu carries `split_vertical`/`split_horizontal`/`close_pane`
  with the expected meta+shift shortcuts; `close_tab`/⌘W is gone.

## File-touch summary

- `crates/sola-core/src/keys.rs` — `KeyCode::meta_shift()`; `KeyChord` accel
  formatter (`Display` or helper).
- `crates/sola-bus/src/topics.rs` — `PaneLayout`, `SplitDir`; extend
  `TerminalSession` with `layout`.
- `crates/sola-kit/src/components/split.rs` — orientation + ratio + draggable
  divider; storybook `Split` page.
- `crates/sola-terminal/src/state.rs` — `PaneId`/`SplitId`/`SplitDir`/`PaneNode`,
  `PaneRuntime`, `Tab`, reworked `Tabs`; tree helpers.
- `crates/sola-terminal/src/main.rs` — `App`/`Msg` (pane-id-keyed events,
  `DividerResize`, `PaneFocused`); `dispatch_action` split/close-pane; resize
  fan-out; boot reconcile per leaf; persistence emit.
- `crates/sola-terminal/src/term_view.rs` — per-pane rect/metrics; pointer-enter
  focus; active-pane border.
- `crates/sola-terminal/src/tmux.rs` — session helpers per `PaneId`.
- `crates/sola-terminal/src/menu.rs` — Pane menu; Close Pane ⌘⇧W; drop ⌘W.
- `crates/sola-terminal/src/input.rs` — delete dead Ctrl+Shift/digit blocks;
  Super-modified guard.
- `crates/sola-shell/src/menu*` — render the accelerator in dropdown rows.
