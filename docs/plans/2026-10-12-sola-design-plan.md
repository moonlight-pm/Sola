# sola-design — implementation plan

**Date:** 2026-10-12  
**Worktree:** `.worktrees/sola-design` · branch `sola-design`  
**Design:** [`../specs/2026-10-12-sola-design-design.md`](../specs/2026-10-12-sola-design-design.md)  
**Status:** slices 1–6 on master (2026-10-12), installed

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

## Field report fixes (Exit, 2026-10-12)

- [x] MCP instructions: boards are Chromium from `file://` (relative URLs,
      no local fetch / ES modules / iframe docs), what reloads, `?query`.
- [x] `src` keeps `?query#fragment` (one page, two states).
- [x] Settle after every load: `document.fonts.ready` + two frames
      (`AgentOp::Eval` now `awaitPromise`), then height + loaded files;
      `wait` / `reload` wait for it. Exit light/dark now agree (6796).
- [x] Files a board loaded from outside `design/` are watched (`status`
      lists them as `linked`); a burst can be layout **and** content.
- [x] `wait` after an edit: reloads when files are newer than the boards
      (`newest_input` vs `shown_at`); watcher reloads de-duplicated.
- [x] `inspect --all`: `{selector, tag, rect, text}` per match.
- [x] **D5 decided:** loopback http origin, project root as `/`.
      `serve.rs` (GET/HEAD, registered roots, no dotfiles / listing, Host
      check, no CORS); board URLs rewritten per layout to
      `<board>--<canvas>.<project>.localhost`; per-board raster scale
      (`host_scales`); settle resources map back via `local_path`; MCP
      instructions + docs updated; Exit workspace told.

## Exit follow-up fixes (2026-10-12)

- [x] Changing an artboard's `src` did not re-point its tab: any
      `canvas.toml` edit counted as stale input, so a `ReloadIgnoreCache` in
      the same update reloaded the committed URL and cancelled the
      `LoadUrl`. `canvas.toml` left out of `newest_input`; reload loads the
      intended URL when the tab shows something else; settle replies carry
      a load generation (late ones ignored).
- [x] A fresh tall `auto` board measured 20 px high: its first layout at a
      900 px viewport had a vertical scrollbar (narrower → more wrapping).
      `--hide-scrollbars` for Design's CEF; one bounded confirm settle after
      each auto resize. Exit light / dark measure 6796 on first open.
- [x] `status` shows each board's tab `url` and `error`; a missing `src` or
      HTTP ≥ 400 is a red board title.

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
- [x] Human: an agent session designs through MCP end to end (Exit, `design/foundation`).

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

## Slice 5 — limits, agent / human (2026-10-12)

- [x] Detail renders: board layout pinned (`Emulation.setDeviceMetricsOverride`), viewport override for the visible part at screen scale; frames tagged with the acknowledged request `seq`; plane base + detail textures; surfaces grow to the plane on demand.
- [x] Raster scale under 1× for pages past 8192 px (`MAX_SIDE` 32768); zoom limit 3200%.
- [x] Origin: single byte ranges; canvas `root` web root (looked in first; reload when it changes).
- [x] `auto` re-measures 1 / 3 / 8 s after a settle.
- [x] Agent reads independent of the view: captures hold the board whole, pointer leaves first; `view` yields to the human.
- [x] Agent glow (overlay shimmer).
- [x] Page IME through an overlay wrapper.
- [x] Helper waits for a predecessor still shutting down (self-restart race).
- [x] Verified: detail frames sharp and aligned to the base (engine frame dump); captures whole-board while zoomed; PDF under the pin.
- [ ] Human: smoke on screen — zoom past 200%, pan, glow, IME.

## Slice 6 — multi-agent canvases, live share (2026-10-12)

- [x] `app/canvases.rs`: many live canvases, one shown; parked cameras; agents' `open` / `canvas.new` off screen (`--raise` shows); idle off-screen canvases close (10 min).
- [x] `--canvas` on every page call; `solactl mcp` remembers each agent's canvas; rail `agent` mark.
- [x] `share.rs`: token per canvas (rotatable, `share.json`), viewer + `~canvas.json` + `~version`, token-scoped file access on real paths, `Referer` redirect, privacy headers, connection cap.
- [x] `share/tunnel.rs`: `cloudflared` connector supervised by Design (pid file: a re-exec stops the old one; inherited zombies reaped at start).
- [x] Bottom bar: copy-link and new-link buttons (tips above); Share menu (⌘⇧L).
- [x] Viewer handles like Design: live pages, right-drag pan (capture-phase in each frame; pages never see the right button), wheel zoom, touch two-finger pan / pinch, hint line; Interact toggle removed.
- [x] LAN DNS: hub Wicket v1 resolves record-less domains upstream (hub `6b47539`).
- [x] Viewer fix: `auto` boards stayed at the 900 px start (viewport-bound `scrollHeight`) and overlapped the next one — the layout now carries Design's measured heights (fallback: Design's formula); titles and frame scrollbars removed.
- [x] Cloudflare: tunnel `sola-design` (remote config, ingress → 127.0.0.1:47820), proxied CNAME `design.moonlight.pm`; `share.toml` written (0600).
- [x] Verified: background `open` / `wait` / `screenshot` leave the human's canvas; MCP memory; share link over HTTPS through the tunnel; viewer renders (auto height measured).
- [x] Rail drag-reorder (kit `reorder_within_sections`): canvases within their project, project headers as blocks; order in `projects.json` (`canvas_order`; unlisted canvases follow by name). `project.list` lists rail order.
- [x] `project.remove --path` (solactl + MCP `design_project_remove`): exact `root` match, works for a deleted folder, closes the project's live canvases. Smoked.
- [ ] Human: two agents at once; Copy Link from the Share menu; open on a phone; drag canvases and projects, restart, order kept.
- [ ] `cloudflared` into `configuration.nix` (drop the `cloudflared` path in `share.toml`).
