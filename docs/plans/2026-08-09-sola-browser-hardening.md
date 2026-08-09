# sola-browser hardening — review + work queue

**Date:** 2026-08-09  
**Branch:** `naturalethic/browser`  
**Code:** `crates/sola-browser` only (WPE; single crate; CEF gone at tag `pre-cef-removal`)  
**Living maturity:** [`docs/capabilities.md`](../capabilities.md) row `browser`  
**As-built map:** [`docs/architecture.md`](../architecture.md) § Browser  

This is the **engineering backlog** from a full code review of chrome + WPE.
It supersedes the dual-engine parts of
[`docs/specs/2026-07-25-sola-browser-cleanup.md`](../specs/2026-07-25-sola-browser-cleanup.md)
for *what to do next*; that freeze remains useful as history.

---

## As-built snapshot (truth)

```text
sola-browser (bin + lib)
├── chrome (src/)
│   app.rs          iced App: tabs, omnibox, divider, float CSD, edit routing
│   engine.rs       Engine trait + Cmd/Nav/Edit + handles (generic, one impl)
│   run.rs          iced::application + frame subscription (inactive drop)
│   integration.rs  bus menus/theme/float; OpenUrl intentionally NOT subscribed
│   input.rs        CursorKind + projection helpers
│   shader.rs       shared WGSL sample pipeline + FPS
│   util.rs         URL/search (Kagi), new-tab click rules, edit cmd names
│   main.rs         run::<WpeEngine>("sola-browser")
└── wpe/ (src/wpe/)
    engine.rs       GMainLoop worker, tabs, policy, buffer tokens (~1.1k LOC)
    frame.rs        iced shader Program + FrameImport + resize
    input.rs        iced → WPE events (keycode always 0)
    wgpu_import.rs  dma-buf → wgpu via Vulkan modifier path
    sola_wpe.c/.h   display subclass (LINEAR prefs), cursor/selection bridges
    wpe_sys.rs      bindgen include
```

**Process model:** iced main thread + `wpe-engine` GLib thread. Frames:
worker `on_buffer_rendered` → mpsc → frame_stream (drop non-active) →
`FrameSlot.pending` → shader `prepare` dma-buf import → sample.

**Product defaults:**

| Item | Current |
|------|---------|
| app_id | `sola-browser` |
| Shell launcher | one “Browser” → `/opt/sola/bin/sola-browser` |
| Default load | Wikipedia (or argv URL) |
| Search | Kagi for non-URL omnibox |
| http/https system default | **Helium** until browser is ship-ready (**D3 decided 2026-08-09**; `OpenUrl` not subscribed) |
| Last tab | never empty — replace with `about:blank` |
| Profiles / bookmarks / history / downloads / find / zoom / devtools | **absent** |

---

## Lifecycle audit vs 2026-07-25 cleanup

| Cleanup ID | Status in code today | Notes |
|------------|----------------------|-------|
| **C1** token leak | **Fixed** | `WpeFrame` / `HeldToken` Drop → `Cmd::Release`; inactive-tab drop recycles |
| **C2** closed-tab UAF | **Mitigated** | Release ignores missing `tab_id`; still raw ptrs until free |
| **C3** release-before-GPU | **Open** | Prior hold dropped on next import; pool depth ≥2 masks tear |
| **C4** shutdown | **Fixed** | `App` Drop → `engine.shutdown()` → Quit + join |
| **C5** CEF last-tab hang | **N/A** | CEF removed |
| **C6** paste-into-page | **Fixed path** | `Cmd::PasteText` + InsertText; needs dogfood confirm |
| Last-tab policy | **Fixed** | blank replacement before close |
| Dual engine / dispatcher | **Removed** | tag `pre-cef-removal` |

---

## Findings (severity)

### P0 — correctness / dogfood blockers

| ID | Finding | Evidence | Suggested direction |
|----|---------|----------|---------------------|
| **B1** | **Background tabs keep producing frames** that only get dropped in `frame_stream`. Wastes CPU/GPU/WebProcess for every background tab. | `run.rs` filter; no worker-side suspend | Suspend paint / throttle non-active WPE views (or stop listening) |
| **B2** | **C3 still true:** GPU may still sample previous dma-buf when token is released on next import. | `frame.rs` prepare: `pipeline.hold = Some(hold)` drops prior | Hold previous import until GPU fence / triple-buffer; or document “pool depth ≥2 required” |
| **B3** | **Multi-plane dma-buf frames dropped** (`n_planes != 1`). Some content paths may never paint. | `engine.rs` `on_buffer_rendered` | Log rate-limited; support multi-plane or convert |
| **B4** | **IME / complex text broken:** `keycode: 0`, Character keys only first codepoint, no IME bridge. CJK/emoji/composing fail. | `wpe/input.rs` | Long-term: real IM protocol; short-term: document |
| **B5** | **Middle-click never reaches WPE** (`button_to_wpe` returns `None` for Middle). `decide-policy` new-tab path for middle-click is dead for iced-driven events. | `wpe/input.rs` + `on_decide_policy` | Product: enable middle→background tab **or** delete dead policy branch |

### P1 — chrome / product gaps (rough dogfood)

| ID | Finding |
|----|---------|
| **P1.1** | No stop-loading control (NavCmd::Stop exists, unused in UI) |
| **P1.2** | No find-in-page, zoom, reader, downloads, bookmarks, history, session restore |
| **P1.3** | No cookie/profile path hardening / multi-profile UI |
| **P1.4** | Tab titles/URLs update only on 250 ms Tick — sluggish omnibox/title sync |
| **P1.5** | URL-bar paste only **appends** (no selection replace) |
| **P1.6** | Text sharpness / DPR soft vs Chromium (historical note; still open) |
| **P1.7** | System links go to Helium; sola-browser is opt-in only |
| **P1.8** | No in-page context menu; no right-click menu chrome |
| **P1.9** | Error pages / cert failures / crash recovery not productized |
| **P1.10** | `target=_blank` / popup windows — policy only opens background tabs for modified clicks; default new-window policy unreviewed |

### P2 — architecture / maintainability

| ID | Finding |
|----|---------|
| **A1** | `Engine` trait still generic for one impl — fine short-term, mild noise |
| **A2** | `wpe/engine.rs` ~1089 LOC — worker/callbacks/tabs should split |
| **A3** | Three active-tab truths: worker `ctx.active`, `ActiveHandle`, chrome `cached_active` (documented; fragile) |
| **A4** | `ResourceToken` still raw `*mut c_void` — relies on quarantine, not refcount |
| **A5** | Bindgen surface still broad; historical FDO/probe comments |
| **A6** | No structured error surface to chrome (load failed, SSL, process crash) |
| **A7** | Stale dual-engine wording remains in old freezes (vault banner OK) |

### P3 — polish / ops

| ID | Finding |
|----|---------|
| **O1** | Outstanding-token counter logs if >8; good — keep for dogfood |
| **O2** | FPS at debug only — OK |
| **O3** | No operator manual (`docs/manual/`) for browser yet |
| **O4** | Install: `cargo make install` needs sudo; agent env may `cp` to `/opt/sola/bin` as user |

---

## Suggested work order (after product decisions)

1. **Dogfood matrix** (manual): load, multi-tab, close last, switch tabs, copy/paste both ways, scroll/video, cmd-click link, resize, float.
2. **B1** background-tab thrash (biggest free perf win).
3. **B5** middle-click policy (product).
4. **B2** import/token GPU ordering if tears observed.
5. **Chrome MVP slice** from product bar (P1.* subset).
6. **A2** split `wpe/engine.rs` when touching lifecycle again.
7. **Operator manual** when behavior is stable enough to claim shipped subsets.

---

## Decision points (ask human — do not invent)

See [`docs/open-questions.md`](../open-questions.md) § Browser. Work in order:

1. ~~**Default link handler**~~ — **D3:** Helium until browser is good enough.
2. **Dogfood MVP chrome** — which of bookmarks / history / downloads / find / zoom / stop are in-scope for the next slice.
3. **Middle-click** — background tab vs ignore.
4. **Search provider** — keep Kagi-only or make configurable.
5. **Profile model** — single default WebKit data dir vs multi-profile later.

---

## Smoke checklist (operator)

```bash
cargo make build sola-browser
# install (user permission): cargo make install browser shell
# or: cp target/debug/sola-browser target/debug/sola-shell /opt/sola/bin/

# From Sola session:
# - Launcher → Browser
# - ⌘T / ⌘W / ⌘L / ⌘R / ⌘← ⌘→
# - Omnibox: wikipedia.org vs bare search words
# - Cmd-click link → background tab
# - Copy selection in page → paste in terminal
# - Copy in terminal → paste in page form
# - Close last tab → blank remains
# - Float window Meta-drag if shell float on
```

Logs: `/opt/sola/log/app-sola-browser.log`
