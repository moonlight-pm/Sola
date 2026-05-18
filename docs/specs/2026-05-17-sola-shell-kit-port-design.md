# sola-shell → sola-kit Port

**Status:** approved, ready for implementation plan
**Date:** 2026-05-17
**Owner:** Joshua

## Goal

Port `sola-shell` from the legacy `sola-app` framework (GTK4 + WebKit6) to
`sola-kit` (CEF + Remix v3). The port aims for faithful visual parity with
today's shell while allowing small judgement-call cleanups (paddings, naming,
the eval-string IPC pattern, `app.rs` decomposition). No UX redesign.

## Scope

Single milestone, all four shell surfaces ported in one PR (one direct-to-
master commit series in this case — no PR, per Joshua's direction). After
the milestone, sola-shell no longer depends on sola-app at all.

In scope:

- All four shell surfaces (menubar, launcher, menu, switcher) re-rendered
  through CEF/Remix v3.
- A small additive extension to `sola-kit` to support per-window root
  components and per-window initial state (needed because sola-shell is
  the first multi-surface kit app).
- Per-surface theme bindings added to the default `sola-core::theme::Theme`
  so a fresh install looks right.
- Cleanup carried in the same milestone:
  - Replace `eval_js(format!("…"))` string IPC with typed JSON envelopes.
  - Collapse the four HTML files into the kit's auto-injected `index.html`.
  - Decompose `app.rs` (1192 LOC) so per-surface logic lives in its
    surface module; target `app.rs` < ~500 LOC focused on `SolaApp` impl
    + cross-surface coordination.
  - Address any real bugs that surface during the port — each one called
    out as its own task in the plan, not folded silently.

Out of scope:

- Any UX/visual redesign beyond parity + minor tightening.
- Promoting shell-specific components into `sola-kit`. Components live in
  the shell crate; a later promotion is a single-file move if anything
  becomes reusable.
- An automated UI test harness. None exists today and that gap is not
  part of this port.

## Architecture

### Process model

sola-shell stays one process. The `SolaApp` impl creates four windows,
each with its own asset bundle and its own root `Main` component. Shared
state (focused window, MRU, application list, menu cache, zoning state)
lives in one `ShellApp` Rust struct. The four windows are facets onto
that state.

### Crate changes

`crates/sola-shell/Cargo.toml`:

- Remove: `gtk4`, `gdk4`, `glib`, `gio`, `webkit6`, `sola-app`.
- Add: `sola-kit`.
- Keep: `sola-bus`, `sola-core`, `tracing`, `serde`, `serde_json`.

### Source tree (after)

```
crates/sola-shell/
  src/
    app.rs                     # SolaApp impl + ShellApp state, retargeted to kit
    keys.rs                    # unchanged — pure key/chord/keysym logic
    zoning.rs                  # unchanged — pure window-zone math
    menubar/
      mod.rs                   # setup_menubar(ctx) — kit window w/ menubar.tsx
      assets.rs                # MENUBAR_ASSETS bundle
    launcher/
      mod.rs                   # setup_launcher(ctx)
      assets.rs                # LAUNCHER_ASSETS bundle (existing)
      state.rs                 # LauncherState (unchanged)
    menu/
      mod.rs                   # setup_menu(ctx)
      assets.rs                # MENU_ASSETS bundle (existing)
      state.rs                 # MenuCache (unchanged)
    switcher/
      mod.rs                   # setup_switcher(ctx)
      assets.rs                # SWITCHER_ASSETS bundle (existing)
      state.rs                 # SwitcherState (unchanged)
    main.rs                    # unchanged module list
  web/
    assets/                    # kept as-is (flower.svg, pillars.svg)
    menubar.tsx                # Main → <Menubar/>
    launcher.tsx               # Main → <Launcher/>
    menu.tsx                   # Main → <Menu/>
    switcher.tsx               # Main → <Switcher/>
    components/
      menubar/{menubar,app-title,tray}.tsx + menubar.css
      launcher/{launcher,app-row}.tsx + launcher.css
      menu/{menu,menu-item}.tsx + menu.css
      switcher/{switcher,switcher-card}.tsx + switcher.css
```

Deleted: `web/index.html`, `web/launcher.html`, `web/menu.html`,
`web/overlay.html`, `web/src/*.ts`. The kit auto-injects `index.html`
per window from its built-in platform assets.

### Surface inventory

| Surface  | Geometry                                       | Keyboard target | Zoned | Root component  |
|----------|------------------------------------------------|-----------------|-------|------------------|
| menubar  | 1920 × `MENUBAR_HEIGHT`, top-anchored, transparent | yes         | no    | `menubar.tsx`   |
| launcher | popup under menubar, transparent               | yes             | no    | `launcher.tsx`  |
| menu     | popup anchored under a menubar item            | yes             | no    | `menu.tsx`      |
| switcher | full-screen overlay, transparent               | yes             | no    | `switcher.tsx`  |

Exact sizes and positions come straight from today's `setup_menubar` and
`ctx.add_window` calls in `app.rs` — no changes to window geometry as
part of this port.

## Kit extension (additive, scoped to `sola-kit`)

Two new optional `WindowConfig` fields:

```rust
pub struct WindowConfig {
    // existing fields ...
    pub root_component: Option<&'static str>,
    pub initial_state: Option<serde_json::Value>,
}
```

Semantics:

- `root_component = None` (default) → window uses `SolaApp::ROOT_COMPONENT`,
  preserving every existing single-window kit app's behavior. `Some(path)`
  → that window's importmap `@sola/app-root` entry resolves to `path`.
- `initial_state = None` → kit pushes `{ event: "init", state: null }` to
  the window before `Main` is invoked. `Some(json)` → kit pushes
  `{ event: "init", state: json }`. Always one envelope shape — `Main`
  always knows whether seed data is present.

Implementation:

- `crates/sola-kit/src/lib.rs::build_importmap` becomes a per-window
  function taking the resolved root path.
- `crates/sola-kit/src/ctx.rs::add_window` pushes the init envelope via
  the same `__solaRecv` mechanism the theme bus-pump already uses,
  ordered before the dynamic import of `Main`.
- The kit's `index.tsx` exposes `state` from the init envelope as
  `handle.props.initial` so `Main(handle)` reads seed data synchronously
  on its first call.

Regression coverage: existing kit apps (`monitor`, `settings`) keep
building and behave identically. If `build_importmap` lacks a unit test
today, this change adds one.

CLAUDE.md updates: a paragraph in the kit section documents the
multi-window pattern (`root_component` + `initial_state`).

## Data flow

Same three flows as `sola-monitor` / `sola-settings`, scaled up across
four windows.

### Flow 1 — bus → state → window

`ShellApp` subscribes to bus topics through `register_bus`. Topic
handlers mutate state, then push fresh state to the windows that need
it via `window.recv(envelope)` (a thin wrapper around
`window.eval_js("window.__solaRecv(" + json + ")")`).

```rust
fn on_focus(&mut self, delivery: &Delivery, ctx: &mut AppCtx) {
    self.focused_app_id = …;
    self.windows.menubar.recv(&envelope("focus", json!({ "app_id": … })));
    if self.switcher.is_visible() {
        self.windows.switcher.recv(&envelope("focus", json!({ … })));
    }
}
```

JS side, per surface:

```tsx
// menubar.tsx
import { recv } from "@sola/ipc";
recv("focus", (msg) => {
  state.focused = msg.app_id;
  handle.update();
});
```

### Flow 2 — JS user input → action → bus

JS calls `invoke("name", args)` (existing kit pattern). Rust handles in
`on_js_command`:

```rust
"open_menu" => {
    let idx = args["index"].as_u64().ok_or(…)? as usize;
    let x   = args["anchor_x"].as_f64().unwrap_or(0.0);
    self.open_menu(source.title(), idx, x, ctx);
}
```

All current `eval_js(&format!("…"))` call sites become typed envelope
pushes via `window.recv`. Estimated ~10–15 inbound `invoke` commands
across all four surfaces (open_menu, close_menu, launch, focus_window,
raise_window, dismiss_launcher, switcher_pick, …); the exact set is
enumerated during the per-surface tasks by reading today's JS bridge
calls.

### Flow 3 — initial state per window

Each `WindowConfig::initial_state` carries the slice of `ShellApp`
state relevant to that surface (e.g., menubar gets the focused app's
title and menus; launcher gets the application list). Kit delivers the
init envelope before `Main` renders, so the first render never sees an
undefined shape. Subsequent updates flow via `window.recv`.

### State ownership

| Lives in Rust                                                              | Lives in JS                                          |
|----------------------------------------------------------------------------|------------------------------------------------------|
| focused app/window, MRU, application list, menu cache, zoning state,        | hover, focus ring, scroll, transient animation phase, |
| canonical launcher query                                                   | current `<input>` text before submit                 |

Same boundary as today.

## Component decomposition (JS)

Per-surface Remix v3 components — small files, slot-based composition
where natural, all class-based selectors referencing
`--sola-<component>-<slot>` theme vars. Pattern matches the kit's
existing `sidebar.tsx` / `sidebar.css` arrangement.

Components live inside `crates/sola-shell/web/components/<surface>/`.
They are not promoted to `sola-kit`. If something becomes reusable
later, promotion is a one-file move.

## Theme

Each new shell component gets entries in `sola-core::theme::Theme`
declaring its slots and the selection group each slot belongs to (per
the `2026-05-07-sidebar-and-theme-protocol-design.md` protocol).
Default-theme bindings are added so a fresh install looks right. CSS
references only the scoped vars (`var(--sola-<component>-<slot>)`),
never palette atoms directly.

## Migration sequencing (within the single milestone)

1. **Kit extension.** Land `WindowConfig::{root_component, initial_state}`,
   per-window importmap, init envelope. Verify existing kit apps
   (`monitor`, `settings`) still build and behave identically.
2. **Shell Cargo.toml swap.** Drop sola-app + GTK/WebKit; add sola-kit.
   Expected to break `cargo make build` until step 3.
3. **Scaffold `ShellApp` against kit.** Re-impl `SolaApp`; window-creation
   calls move to kit's `WindowConfig`; all four windows initially mount a
   placeholder `Main` returning `<div>{surface name}</div>`. `register_bus`
   retargets to `sola_kit::BusRegistry`. State struct keeps its shape
   (field types change from `sola_app::WindowHandle` to
   `sola_kit::WindowHandle`). After this step the shell launches and shows
   four labeled rectangles in the correct geometries.
4. **Port surfaces, in order:**
   1. menubar (always-visible, lowest interaction)
   2. launcher (popup + text input, exercises focus handling)
   3. menu (anchored popup, exercises positioning math)
   4. switcher (full-screen overlay + MRU rendering, most complex layout)

   Per surface: TSX root + child components + CSS, wire inbound
   `on_js_command` cases, wire outbound `recv` envelopes. Build, install
   only with explicit per-call user permission, eyeball-test from a real
   shell session.
5. **Sweep.** Delete `web/index.html`, the three other `.html` files, and
   `web/src/*.ts`. Confirm no dead asset references. `cargo make build`.
6. **Theme defaults.** Add the shell's component bindings to the default
   theme so a fresh install looks right.

`app.rs` decomposition happens incrementally during steps 3–4: each
surface port pulls its per-surface methods out of `app.rs` into the
appropriate `<surface>/mod.rs`. Target: `app.rs` ≤ ~500 LOC after step 4.

## Bugs / cleanup catch list (filled in during plan writing + implementation)

During plan writing and implementation, any real bug that surfaces gets
called out as its own task with the fix. Known cleanup items at spec
time (no bugs flagged yet):

- `eval_js(format!(…))` IPC pattern → typed JSON envelopes (port-wide).
- Four HTML files → one kit-auto-injected (port-wide).
- `app.rs` decomposition (≤ ~500 LOC after, surface logic in surface modules).

## Testing

- **Unit:** `zoning.rs`, `keys.rs`, `launcher::state::tests` keep their
  existing tests. Grow them when port-time changes touch covered logic.
- **Kit-side:** add a `build_importmap` unit test if one doesn't exist
  (the per-window change makes this load-bearing).
- **Manual per-surface smoke** (also lives in the plan as the
  per-surface acceptance criteria):
  - **menubar**: shows focused app's title + menus, clock updates,
    hover-for-`FOCUS_HOVER_DELAY` raises window.
  - **launcher**: super-tap opens, typing filters, enter launches,
    escape dismisses.
  - **menu**: click on menubar menu opens it under the right x, click
    outside closes, keyboard navigation works.
  - **switcher**: super-tab cycles MRU, release confirms, escape cancels.

No automated UI test harness — out of scope.

## Build / install / branch policy

- `cargo make build` verifies compile. Per CLAUDE.md, install requires
  explicit per-call user permission — the plan never auto-runs
  `cargo make install`.
- Per Joshua's direction for this specific milestone: work on `master`
  directly, no worktree, no PR. Normal CLAUDE.md worktree/master-merge
  rules are explicitly waived for this port only.

## Open questions

None at spec time. Implementation will surface concrete inbound-`invoke`
names and any state-shape decisions; those are decided per surface
during plan writing.
