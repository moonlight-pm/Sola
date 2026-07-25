# sola-browser cleanup — architecture cleanse

> Status: **implemented on `browser-cleanup`** · 2026-07-25  
> Source: Claude (fable / xhigh) full-source review of `sola-browser`,
> `sola-browser-core`, `sola-browser-wpe`, `sola-browser-cef`, plus Grok
> spot-check of the highest-severity claims.  
> Raw review: session scratch / `/tmp/sola-browser-claude-review.md`  
> Related: `docs/specs/2026-06-19-browser-engine-unification-{design,plan}.md`,
> `docs/vault/sola-browser.md`, `docs/notes/2026-05-21-wpe-vs-cef-bench.md`

## 1. Scope & goals

### Cleanse covers

1. **Lifecycle correctness** — WPE buffer-token release, tab-close UAF risk,
   CEF last-tab hang, engine shutdown wiring, WPE inbound paste.
2. **Unification completion** — deferred Task 7 (`FrameImport` / shared
   renderer) and shared input scaffolding that the 2026-06-19 plan never ran.
3. **Hygiene** — unused deps, phase-0 probes, dead code, module size, renames.
4. **Docs reconciliation** — vault/spec claims that contradict the code and
   each other (especially CEF’s product role).

### Explicitly out of scope

New product features: devtools, cookie/profile persistence, IME, find-in-page,
zoom, favicons/loading progress, audio indicators, DPR/text-sharpness work,
CEF accelerated OSR, multi-process chrome. Those can ride a later roadmap;
this cleanse is about making the existing dual-engine browser *safe to keep
working on*.

### Non-goals for this pass

- Folding WPE + CEF into one binary (libcef is 1.34 GB; exec dispatcher stays).
- Rewriting `sola_wpe.c` vmethod hijacks (load-bearing — see §8).
- Event-driving tab state (250 ms Tick stays until something lags observably).

---

## 2. Decision log (write these once, supersede prior claims)

| Decision | Resolution | Supersedes |
|---
## Implementation status (this branch)

Landed in worktree branch `browser-cleanup`:

- **P0:** WPE token-on-Drop + closed-tab quarantine + outstanding-token logs;
  chrome never drops below one tab; CEF no longer quits loop on empty tabs;
  `App` Drop → `Engine::shutdown`; paste-into-page via `Cmd::PasteText` +
  WebKit `InsertText` / CEF `frame.paste()`.
- **P1:** Shared `SamplePipeline` + `FrameImport` (core `shader.rs`); shared
  `CursorKind` / projection helpers (`input.rs`); single-writer `ActiveHandle`
  policy; CEF parity status restated in crate headers + vault.
- **P2:** Removed phase-0 probe bins; purged unused deps; `releaser` →
  `cmd_tx`; CEF `button_number` rename; dead `find_tab_by_webview` removed.
- **P3:** Vault + this doc + `sola_wpe.h` purpose text; FPS behind
  `SOLA_BROWSER_FPS` / debug; dispatcher fallback logs to stderr.

Deferred (still fine later): full `engine.rs` module splits, aggressive
bindgen/fdo header trim, bench-note supersession wording polish.

|---|---|
| **CEF status** | **Keep as parallel engine at feature parity** until a written mothball. Code and `Cargo.toml` already match parity (cmd-click, Edit, popups). Bench-note “archive” and old “reference implementation / workspace-excluded” language are obsolete. | `docs/notes/2026-05-21-wpe-vs-cef-bench.md` archive wording; early CEF-port “workspace excluded” notes |
| **App identity** | Per-engine app_id (`sola-browser-wpe` / `sola-browser-cef`) is final (reversed 2026-06-20). Dispatcher remains `sola-browser`. | Unification design’s original unify-to-`sola-browser` identity |
| **Frame-release ownership** | Target model: **token-on-Drop** (or equivalent unconditional release). Any path that drops a WPE frame without `wpe_view_buffer_released` is a bug. | Ad-hoc release only inside `prepare` on successful import |
| **Last-tab policy** | Chrome never drops below one tab (open blank / refuse close / quit app — pick one in Phase 1). CEF must not quit the message loop solely because the tab list emptied. | Informal “close last tab” behavior; CEF `on_before_close` empty-list quit |
| **Primary engine** | WPE remains the product default on this host; CEF stays launchable via `--engine cef` / `SOLA_BROWSER_ENGINE`. | — |

---

## 3. Architecture as-is (code, not docs)

```
sola-browser (bin, ~155 LOC)
  └── --engine / $SOLA_BROWSER_ENGINE → exec sibling sola-browser-{wpe,cef}
      (silent fallback if preferred binary missing)

sola-browser-{wpe,cef} main.rs (~6 LOC)
  └── sola_browser_core::run::<Engine>("sola-browser-<engine>")

sola-browser-core
  engine.rs   Engine + Cmd/NavCmd/EditCmd + FrameSlot + handles
  app.rs      generic App<E>: sidebar tabs, nav, omnibox, edit routing, Tick
  run.rs      spawn → BusSetup → iced; frame Recipe (filters inactive tabs)
  integration.rs  bus → BrowserIntent (tested) + menus
  util.rs     URL/search helpers (tested)

sola-browser-wpe                    sola-browser-cef
  engine.rs  GMainLoop worker        engine.rs  CEF message loop + handlers
  frame.rs   iced shader + dma-buf   frame.rs   iced shader + CPU upload
  input.rs   iced → GDK/WPE          input.rs   iced → VK/CEF flags
  wgpu_import.rs  Vulkan dma-buf     cpu_import.rs  write_texture
  sola_wpe.{c,h}  FINAL-type hijacks
  bin/*_probe.rs  phase-0 (~950 LOC)
```

**Frame path:** worker → `mpsc` `TaggedFrame` → core subscription (drops
inactive-tab frames) → `slot.pending` → engine `Program::prepare` imports →
`render`.

**Command path:** chrome / Program → `Cmd` channel → worker `process_cmd`.

**Tab sync:** three “active tab” facts — `App.cached_active`, shared
`ActiveHandle` atomic (written by chrome *and* worker), worker `ctx.active`.
Chrome polls a mutexed tab snapshot every 250 ms.

**Unification gap:** `Engine` exports a full iced `Program` (`make_program`).
Shared WGSL / pipeline / resize-mirror / FPS / large parts of input plumbing
live duplicated in both `frame.rs` files. Design’s `core/input.rs` and
`core/shader.rs` never landed.

Approx sizes (2026-07-25):

| Crate | Notable |
|---|---|
| core | ~1.3k LOC; pure layers tested |
| wpe | ~2.7k incl. C + probes; `engine.rs` ~989, `frame.rs` ~608 |
| cef | ~2.0k; `engine.rs` ~1027, `frame.rs` ~505 |

---

## 4. Findings summary (severity)

### P0 — correctness / safety / resources

| ID | Finding | Confidence | Evidence |
|---|---|---|---|
| **C1** | **WPE buffer tokens leak on dropped frames.** Three paths never `Cmd::Release` / `wpe_view_buffer_released`: (1) inactive-tab `continue` in `run.rs`, (2) overwrite of `slot.pending` when iced hasn’t drained, (3) import failure early-return that *deliberately* skips release. Finite pool → background tabs can stall; occlusion can starve the active tab. | Leak path: **high** (code). Symptom severity: **medium** until instrumented. | `core/run.rs` ~61–64; `wpe/frame.rs` ~330–336 |
| **C2** | **Stale-token release after tab close → potential UAF.** `ResourceToken` holds raw `WPEView*`/`WPEBuffer*`; `close_tab` unrefs the webview while frames may still be in the channel / pending / `pipeline.current`. | **Medium** (needs view-lifetime confirmation) | `wpe/engine.rs` ResourceToken + close_tab + `Cmd::Release` |
| **C3** | **Release-before-GPU-completion.** Previous buffer released to WPE as soon as the next imports; comment claims imported memory ≠ producer memory — false for dma-buf (same pages). Masked by pool depth ≥2. | Ordering: **high**. Visible tear: **low–medium**. | `wpe/frame.rs` ~357–366 |
| **C4** | **`Engine::shutdown` never called.** Quit → `iced::exit()` only; CEF clean `cef::shutdown` is opportunistic at best. | **High** | trait + grep of call sites |
| **C5** | **Closing last CEF tab quits the message loop** while chrome lives; later ⌘T hits a dead pump. WPE worker survives. Spec wanted ≥1 tab. | Hang: **high** (code path clear) | `cef/engine.rs` `on_before_close` ~520–522; `core/app.rs` CloseTab |
| **C6** | **WPE paste-into-page broken by construction.** Outbound copy is bridged via JS/selection; inbound `Paste` runs WebKit’s command against an internal clipboard with no Wayland backend. | **High** on architecture; confirm with 2-minute manual test | `wpe/engine.rs` Edit arm; `sola_wpe.c` copy bridge comments |

### P1 — architecture debt

| ID | Finding |
|---|---|
| **A1** | Engine trait boundary too high: full `Program` per engine → ~700–800 LOC duplicated shader/pipeline/input plumbing (unification Task 7 never ran). |
| **A3** | Dual writers to `ActiveHandle`; three copies of active-tab truth. |
| **A5** | CEF role documented three incompatible ways (archive / reference / parity). |
| **A6** | Trait gaps for later features (progress, zoom, find, stop button reachable, etc.) — track only, don’t expand scope. |

### P2 — hygiene

- Unused direct deps: core `wgpu`/`wgpu-hal`/`ash` (no `src/` imports; leftover for deferred shader module); cef `wgpu-hal`/`ash`/`libc`/`tokio`; wpe `tokio`. Core *does* use `tokio` (`spawn_blocking` in `run.rs`) — keep that.
- Stale Cargo comments (cef claims dma-buf via wgpu-hal-patched; wpe header mentions chromeless `--app` mode not implemented).
- Phase-0 probes (~950 LOC) + bindgen surface for FDO/EGL/GBM still in the build graph.
- Dead: `find_tab_by_webview`, unreachable `NavCmd::Stop` / `EditCmd::Undo|Redo` from chrome, `Engine::shutdown` as above.
- Naming: `releaser` is a general `cmd_tx`; CEF `button_to_wpe_like` is a naming leak.

### P3 — docs / polish

- Vault file map and roadmap partially describe a pre-unification layout.
- Unification plan still has unchecked boxes for shipped work.
- FPS `info!` every second (bench harness scrapes it — gate carefully).
- Dispatcher silent engine fallback should log.

### Input / known gaps (document, not necessarily fix)

IME, WPE `keycode: 0`, CEF mouse Back/Forward, CEF `PET_POPUP` paints (select dropdowns), CEF full-buffer paints (no dirty_rects), WPE DPR/text sharpness.

---

## 5. Phased plan

### Phase 0 — instrument before “fixing” (1 small commit)

Do **not** start with a speculative release redesign. Land observability first:

1. Worker-side **outstanding WPE token counter** (inc on emit, dec on
   `Cmd::Release` / deliberate discard). Log on tab close and every N seconds
   if non-zero.
2. Optional: sample `/proc/self/fd` count under tab churn / occlusion.
3. Manual matrix baseline: multi-tab open/close/switch, occlude-restore,
   last-tab close on **both** engines, copy **and** paste both directions.

Exit criteria: numbers that prove C1 (or disprove user-visible starvation).

### Phase 1 — lifecycle fixes (P0)

| Item | Approach | Verify |
|---|---|---|
| **P0.1 Token release** | Prefer `Drop` on `WpeFrame` (or frame+token pair) that sends `Cmd::Release` via a stored `Sender` clone; make release **idempotent** on the worker. Delete the “don’t release on import failure” special case once ownership is clear. | Token counter → 0 after tab churn; bg tabs keep updating when reactivated |
| **P0.2 Closed-tab quarantine** | Tag tokens with `TabId`; ignore Release for missing tabs **or** `g_object_ref` the view until outstanding tokens hit zero. | Close active tab under load; no crash; counter clean |
| **P0.3 Last tab** | Chrome: refuse close-last **or** replace with blank **or** quit app (prefer blank-tab to match bus-integration “never drop below one”). CEF: remove empty-list → `quit_message_loop` (or gate on real Quit). | Close last tab on CEF; ⌘T still works |
| **P0.4 Shutdown** | Either: Quit → `Cmd::Quit` → join worker → `Engine::shutdown` → exit; or delete trait method and document process-exit-as-contract. Prefer real shutdown if CEF profile flush ever matters. | Clean process exit both engines |
| **P0.5 WPE paste** | Confirm gap manually. Then: chrome reads iced clipboard, send text with paste (`EditCmd::Paste(String)` or companion); inject via WebKit insert-text / editing command with argument. | Paste from terminal into a form field |

**Ordering note:** P0.1 + P0.2 should land together (release-on-drop without
closed-tab guards reintroduces C2).

### Phase 2 — unification completion (P1)

| Item | Approach |
|---|---|
| **P1.1 `FrameImport`** | New `core` module owns Program / Primitive / Pipeline / WGSL / resize-mirror / FPS / three-state Clear-Load-draw. Engines implement `import(device, queue, frame) -> Option<TextureView>` (+ token side effects). Fold C3 release-vs-GPU decision into this move (e.g. hold previous import until GPU fence / double-buffer). |
| **P1.2 Shared input scaffolding** | Revive `core/input.rs` for `CursorKind`, projection, scroll shaping, held-button/modifier state machine. Engine crates keep **only** keymaps + native event constructors. (A prior core `input.rs` was dropped in `b37ddb9` — re-split carefully so translation tables stay engine-side.) |
| **P1.3 CEF decision text** | One paragraph in vault + crate header; delete archive/reference contradictions. |
| **P1.4 Single-writer active tab** | Worker is sole writer of `ActiveHandle`; chrome keeps optimistic `cached_active` for paint only. Document in `engine.rs`. |

Do **not** split the 1k-line `engine.rs` files until after P1.1 so code moves once.

### Phase 3 — hygiene (P2)

| Item | Notes |
|---|---|
| **P2.1** Split `engine.rs` → `engine` / `worker` / `callbacks|handlers` | After P1.1 |
| **P2.2** Manifest hygiene | Drop unused deps; fix false comments. Build each engine crate individually (workspace isolation). |
| **P2.3** Retire probes | Delete `wpe` probe bins; trim bindgen allowlists and pkg-config modules (fdo/egl/gbm) if only probes needed them. History preserves the probes. |
| **P2.4** Dead-code sweep | `find_tab_by_webview`, Stop/Undo/Redo policy, etc. |
| **P2.5** Renames | `releaser` → `cmd_tx`; `button_to_wpe_like` → neutral name |
| **P2.6** WPE spawn readiness | Don’t ignore `ready_rx.recv()` failure |

### Phase 4 — docs reconciliation (P3)

| Item | Target |
|---|---|
| **P3.1** Rewrite `docs/vault/sola-browser.md` | Real file map, Engine sketch, frame/input flow, CEF status, known gaps, C1-fixed release model |
| **P3.2** Close unification plan | Mark shipped; delta note (no core input/shader yet → this cleanse Phase 2) |
| **P3.3** Annotate bench note | “Archive” superseded by parity decision |
| **P3.4** C header / AGENTS CEF notes | Remove obsolete render_buffer-ack fiction; accelerated_osr note matches browser CPU OSR path |
| **P3.5** FPS logging | Keep scrapable for bench; prefer `debug!` or `SOLA_BROWSER_FPS=1` |
| **P3.6** Dispatcher fallback | stderr/log when substituting engines |

---

## 6. Suggested PR / worktree sequencing

Small, reviewable slices (each in `.worktrees/` per project rules):

1. **`browser-token-instrument`** — Phase 0 only  
2. **`browser-wpe-token-lifecycle`** — P0.1 + P0.2  
3. **`browser-last-tab-shutdown`** — P0.3 + P0.4  
4. **`browser-wpe-paste`** — P0.5 (after manual confirm)  
5. **`browser-frame-import`** — P1.1 (largest; no feature creep)  
6. **`browser-shared-input`** — P1.2  
7. **`browser-hygiene`** — P1.3/P1.4 + Phase 3  
8. **`browser-docs`** — Phase 4  

Slices 5–6 are where dual-engine tax drops; do not combine with feature work.

---

## 7. Verification matrix

### Manual (both engines unless noted)

| Case | Expect |
|---|---|
| Open 5 tabs, switch rapidly | Correct content; no stuck blank |
| Close non-active / active / last tab | Never hang; CEF after last-tab policy |
| Resize + occlude window 30s + restore | Active tab still paints (token pool not exhausted) |
| Cmd-click link → new tab | Works both engines |
| Copy selection → paste in terminal | Outbound clipboard |
| Copy in terminal → paste in page form | **WPE currently expected fail** until P0.5 |
| Quit via menu / bus | Process exits; no zombie CEF subprocesses |
| `SOLA_BROWSER_ENGINE=cef` / dispatcher | Correct binary; missing-binary fallback logs |

### Instrumented

- WPE outstanding-token counter returns to 0 after churn  
- FD count stable over multi-minute multi-tab session  
- Bench harness still scrapes FPS if needed (or updated to new log key)

### Build

```bash
cargo make build sola-browser
cargo make build sola-browser-wpe
cargo make build sola-browser-cef
# do not install unless user asks
```

---

## 8. Leave-alone register

Do **not** “clean” these without a dedicated design:

| Item | Why |
|---|---|
| `sola_wpe.c` vmethod hijacks + emission hook | FINAL types; only injection path. Optional: dead `get_preferred_buffer_formats` override. |
| `WAYLAND_DISPLAY` hide/restore in `WpeEngine::spawn` | Fixes phantom Wayland toplevel (`357df16`). Tighten comments only. |
| CEF `modifiers_to_cef_mouse` ⌘→CONTROL | Deliberate Chromium disposition mapping; unit-tested. |
| WPE pointer-UP `press_count = 0` | Upstream `WPEWaylandSeat` contract; breaks link clicks if wrong. |
| Three-state Clear / Load / skip-draw | Each state fixed a real bug (flash, black rect, stretch). Unify code, preserve semantics. |
| `set_viewport` + scissor belt-and-braces | UV-mapping fix + cheap insurance. |
| Exec dispatcher | Measured libcef map cost; keep separate process images. |
| `wgpu-hal-patched` | Required for `VK_EXT_image_drm_format_modifier` / NVIDIA dma-buf. |
| 250 ms Tick polling | Proportionate; don’t event-drive without a lag complaint. |
| Middle-click inertness, Kagi omnibox heuristics | Product decisions with tests. |

---

## 9. Open questions (need a human call)

1. **Last-tab policy:** blank replacement vs refuse-close vs quit-app?  
2. **CEF long-term:** confirm **parity** (this doc’s default) vs mothball (then P1.1 still helps WPE-only by deleting cef drift tax later).  
3. **P0.1 design:** pure `Drop` on frame vs core always `Release` on every drop path (including inactive filter) without engine Drop — same goal, different ownership. Prefer engine-local Drop so CEF tokens (if any) stay engine-defined.  
4. **Paste API:** change `EditCmd::Paste` to carry `String` (breaks pure enum Copy) vs parallel `PasteText(String)` command.

---

## 10. Appendix — review provenance

- **Claude review** (2026-07-25): full read of the four crates, C shim, probes,
  build scripts, and the main browser specs/vault/bench notes. Findings labeled
  [read] vs [inferred] in the raw report.
- **Grok spot-check:** confirmed C1 drop paths, import-failure skip-release
  comment, CEF `on_before_close` empty quit, unused core ash/wgpu-hal imports,
  WPE copy-only bridge + Paste → `webkit_web_view_execute_editing_command`,
  and that core **does** use `tokio` (do not remove).
- **Not runtime-verified in this pass:** actual pool exhaustion, UAF under
  load, paste no-op (manual), CEF hang after last tab (inferred from loop quit).

When Phase 0 instrumentation lands, update this doc’s confidence column and
check off items as PRs merge.
