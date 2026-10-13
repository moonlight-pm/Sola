# sola-design — implementation plan

**Date:** 2026-10-12  
**Worktree:** `.worktrees/sola-design` · branch `sola-design`  
**Design:** [`../specs/2026-10-12-sola-design-design.md`](../specs/2026-10-12-sola-design-design.md)  
**Status:** slice 1 in progress

---

## Phase 0 — Align (no code)

- [x] Human answers: own project list; root `design/` only; items are
      canvas directories; Select default + Interact via floating button
      and shortcut; files-are-the-document.
- [x] Controls: right-drag pans; the wheel always zooms (no scrolling).
- [x] Freeze + this plan; capabilities row `design`; `CURRENT.md`.

---

## Phase 1 — sola-browser CEF board mode

**Files:** `crates/sola-browser/src/engine.rs`, `cef/engine.rs`,
`cef/ipc.rs`, `cef/host.rs`, `cef/router.rs`, `agent.rs`, `cef/agent.rs`.

- [x] `Cmd::SetBoard { id, width, height, scale }` (+ `ToEngine` twin):
      the tab gets its own CSS view size and device scale; it is shown
      (`was_hidden(0)`), resized, and invalidated.
- [x] `view_rect` / `root_screen_rect` / `screen_info` read the board
      size and scale.
- [x] `on_paint` publishes board tabs whether or not they are active.
- [x] Board tabs are never hidden by tab switch or agent conceal.
- [x] `Cmd::BoardInput { id, input }` and `Cmd::BoardFocus { id, focused }`
      dispatch to that tab, not the active one.
- [x] `AgentOp::Eval { expression }` → `Runtime.evaluate`
      (`returnByValue`) → reply `data`. Not wired into `solactl browser`.
- [x] Browser and wrapper behaviour unchanged (no board tabs there).

## Phase 2 — sola-design app

**Files:** `crates/sola-design/` (new).

- [x] Argv: chrome vs `--engine --profile=sola-design`; `build.rs` CEF
      link path (copy of wrapper).
- [x] `projects.rs` — `~/.config/sola/design/projects.json` (add, drop,
      collapse, selected canvas).
- [x] `canvas.rs` — scan `design/*/canvas.toml`; parse; validate
      (unique ids, sizes > 0); resolve `src` to `file://` or URL.
- [x] `boards.rs` — diff layout → open / close / `SetBoard` / reload tabs.
- [x] `camera.rs` — pan, zoom about a point, fit, 100%; screen ↔ canvas.
- [x] `plane.rs` — wgpu shader: one texture per artboard, dirty-rect
      upload (`cpu_import`), mip chain + trilinear sampling, clipped to
      the widget.
- [x] `overlay.rs` — canvas::Program: background, titles, frames,
      selection outline; all pointer input (right-drag pan, wheel
      zoom, left click → select or forward).
- [x] Interact: pointer + keys → `BoardInput` on the board under the
      pointer; focus follows the board last clicked.
- [x] Select: click → `AgentOp::Eval` (`elementFromPoint` script) →
      outline + held selection; Esc / empty click clears.
- [x] Mode button (floating, bottom centre) + View menu ⌘I; ⌘0 / ⌘1 /
      ⌘= / ⌘-.
- [x] Watch `design/` (`notify`), debounce, re-layout / reload; shared
      files outside every canvas reload too; sibling canvases ignored.
- [x] Rail: kit `SidebarPanel`; Add project modal (folder path, `~`);
      `+` New canvas (name → `design/<slug>/canvas.toml`); Drop project.
- [x] Unit tests: canvas parse/validate, camera math, layout diff,
      selection script result parse, projects round-trip.
- [x] Smoke fixture `crates/sola-design/fixture/` (Add project → that
      folder → canvas **demo**: desktop + phone, shared `tokens.css`).
- [x] `cargo make build design`, `browser`, `wrapper` (release) pass;
      26 design tests + 297/298 browser tests (the one failure,
      `util::tab_title_chars_fills_default_column`, is pre-existing on
      untouched code). **No install.**
- [x] Single instance (`$XDG_RUNTIME_DIR/sola/design.sock`; second launch
      raises the window).

- [x] Launcher builtin **Design** (`lucide/frame`) in `sola-shell/src/builtins.rs`
      (needs a `shell` install to appear).
- [x] **Installed** `design` release 2026-10-12. Smoke: human.

## Phase 3 — docs at slice end

- [x] `docs/capabilities.md` row `design` (status + gaps).
- [x] `CURRENT.md` (worktree state, next: human install + smoke).
- [x] `docs/architecture.md` crate map; `AGENTS.md` workspace tree.
- [x] No `docs/manual/` page until it is installed and smoked.

---

## Slice 1 smoke fixes

- [x] `AgentReply.data` crosses the helper IPC as JSON text (bincode cannot
      decode `serde_json::Value`; a pick killed the router's reader).
      Also unblocks `solactl browser get` / `links` / `fill` replies.
- [x] Router kills a helper that is still running before respawning
      (a second CEF on the profile failed `cef::initialize` and Chromium
      opened a stray window in the old one).
- [x] Design reopens its boards when the helper comes back empty.

## Slice 2 — `solactl design`

- [x] `calls.rs` specs: `project.list`, `project.add`, `open`, `status`,
      `selection`, `view`, `wait`, `reload`, `screenshot`, `inspect`.
- [x] `app/cli.rs` handlers; page calls over `AgentOp::Screenshot` /
      `AgentOp::Eval`; `wait` = painted + idle + (load seen or 1 s grace).
- [x] `inspect.rs` page script (viewport, document, element styles,
      overflow / clipping issues); shared `cssPath` in `select.rs`.
- [x] Board tabs skip the agent reveal (it took page focus).
- [x] solactl: first-class `design`; `Path` args resolve against the
      caller's cwd (`~` + relative).
- [x] `docs/manual/solactl.md` Design section; `docs/manual/sola-design.md`.
- [x] Human: install `design` + `solactl` and smoke the verbs (picks
      work after the IPC fix — 2026-10-12).

## Slice 4 — `solactl mcp`

- [x] `solactl mcp [--owner …]`: stdio JSON-RPC (2025-06-18 / 03-26 /
      2024-11-05), `initialize` (+ instructions on the Design workflow),
      `ping`, `tools/list`, `tools/call` (own thread), list_changed.
- [x] Tools from the live catalog (`mcp/tools.rs`): names, JSON schema from
      `ArgSpec`, same preparation as `solactl` (paths, Workspaces context,
      bot policy, timeouts); PNG / JPEG `path` → image content (≤ ~3.7 MB).
- [x] Probe: design tools listed; `design_status` text; `design_screenshot`
      text + image.
- [x] Docs: solactl manual § MCP, capabilities `call`, architecture.
- [x] `design canvas.new NAME [--path] [--raise]`: agents make their own
      canvas (`<project>/design/<slug>/`, project = caller cwd via
      solactl / MCP injection), registered + shown; existing → opened.
- [x] Registered in Claude Code user config: `sola` → `solactl mcp`.
- [ ] Human: an agent session designs through MCP end to end.

## Slice 3 — crisp zoom, auto height, PDF

- [x] `boards::raster_scale(w, h, want)`: base 2×, up to 4× within the
      texture / frame budget; sync keeps a surviving board's scale.
- [x] Raster settle 250 ms after the camera stops (one wake per window, not
      per move); on-screen boards → zoom × output scale, off-screen → 2×.
- [x] `height = "auto"`: starts at 900, measured after each load
      (`inspect::MEASURE_HEIGHT`), resized via `SetBoard`; sync keeps the
      measured height; re-fit while the camera is untouched.
- [x] `loads.rs`: load / measure bookkeeping (seen loading → loaded, 1 s
      grace, 15 s give-up); `wait` also waits for the measure.
- [x] Pending tick (200 ms) only while a wait or a measure is pending.
- [x] `AgentOp::Pdf` (screen media → `Page.printToPDF` → restore) +
      `solactl design pdf`.
- [x] `app.rs` split: board lifecycle in `app/live.rs`.
- [x] Fixture: `landing` (auto height).
- [x] Docs: freeze, manual, solactl manual, capabilities, architecture.

### Slice 3 smoke fixes (2026-10-12)

- [x] Zoom-adaptive re-raster removed: the 250 ms settle felt laggy, the
      re-raster popped, and fractional zoom (2.25×) moved text lines by a
      pixel. Fixed integer 2× (`boards::canvas_scale`, 1× only if a board
      cannot fit); plane snaps to pixels only at 1:1.
- [x] Select / Interact toggle removed: pages always live; Super+click
      picks (Super via River bus chords, `sola_browser::input`); Super+click
      empty canvas clears. Menu ⌘I gone; bar = zoom % + pick.
- [x] Slack wrapper: helper argv rejected `--password-store=basic` (router
      adds it since 2026-09-04) → helper exited, every command "no helper".
      `--engine` now ignores unknown Chromium switches; chrome argv stays
      strict.
- [x] Frame IPC cap 64 → 192 MiB; frame budget 160 MiB (tall auto pages
      keep 2× up to the 8192 px texture side).

### Slice 3 findings (live check, 2026-10-12)

- [x] Boards were painting at **1×** all along: this CEF's OSR ignores
      `GetScreenInfo().device_scale_factor` (probed: handler returns 2.0,
      `on_paint` 390×844) and even `--force-device-scale-factor`. Boards now
      raster through **page zoom**: OSR view = CSS × scale px,
      `set_zoom_level(ln s / ln 1.2)` on create / `SetBoard` / load state;
      board input scaled to view px in the engine. Zoom is per host → one
      scale per canvas (`canvas_scale`). Verified: DPR 2 at rest, 2.25 at
      300% (capped by `landing`), CSS viewport unchanged, PNG 780×1688.
- [x] `Cmd::OpenBoard`: boards are born at their size / scale / shown
      (no 1280×800 throwaway layout).
- [x] Helper tab snapshot only rebuilt after a command drain → an idle
      Design never saw loads finish (`reload` timed out, auto height never
      measured). CEF callbacks now `mark_snapshot_dirty` + post a drain.
