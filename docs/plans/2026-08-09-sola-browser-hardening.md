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
| Profiles / bookmarks / find / zoom / devtools | **absent** |
| Stop / downloads | **absent** — **D4 in-scope** |
| Session restore (open tabs) | **partial** — `~/.config/sola/browser-session.json` (tabs+active+sidebar) |
| Bitwarden / extensions | **absent** — **D4 in-scope**; **D7:** first-party UX (SDK + inject) |

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

### Dogfood fixes landed

| Date | Fix |
|------|-----|
| 2026-08-09 | **Tab switch painted wrong tab:** shader kept previous tab’s texture; same-size `apply_resize` on reactivate was a no-op so static pages never re-emitted. Fix: `paint_tab` + clear hold/pending on switch; `force_view_repaint` size nudge + focus_in on `SetActiveTab`. |
| 2026-08-09 | **Self-watch:** `run` calls `sola_kit::app::startup` so install re-execs. **Parked frames** to reduce switch flicker (still some black flash; iterate). |
| 2026-08-09 | **Session restore:** open tabs + active index + sidebar width in `browser-session.json`; restore on boot; CLI URL opens extra focused tab. |
| 2026-08-10 | **See-through content:** transparent window + `REPLACE` + WebKit α=0 punched desktop holes. Fix: fragment forces α=1; always draw content rect with dark `#0a0a0b` fallback when no frame. |
| 2026-08-10 | **Close active tab → blank content:** drop path set `active = None` + clear sample; park restore only ran when `active` was Some other tab. Fix: always sync GPU surface to `paint_tab` (restore park when `active` is None); clear pending/prime for closed tab. |
| 2026-08-10 | **Omnibox lag on tab click:** URL only synced on 250 ms Tick. Fix: `switch_active_tab` sets `url_field` / `last_seen_url` from cached tab immediately. |
| 2026-08-10 | **Stop loading (D4):** `load-changed` → per-tab `is_loading` in snapshot; nav bar ↻ ↔ ×; Escape → Stop; ⌘R still reload/stop; `NavCmd::Stop` wired. |
| 2026-08-10 | **Frame pipeline rework:** retire ring (depth 2) so dma-buf release lags GPU (MSN flicker + `WPE_IS_BUFFER` criticals); per-tab `view_size` skips no-op resize spam; `SetActiveTab` 1px nudge when same size so static pages repaint; park replace retires old park. |
| 2026-08-10 | **Zoom heal:** track `last_frame_size`; if buffer ≠ request, 1px nudge once per wrong buffer; chrome re-sends Resize while painted size mismatches. |
| 2026-08-10 | **Nav chrome:** back/forward disabled without history; fixed-width reload/stop slot. **Multi-plane buffers released** (not dropped) — YouTube/media was exhausting the WPE pool and killing the browser. |

### P0 — correctness / dogfood blockers

| ID | Finding | Evidence | Suggested direction |
|----|---------|----------|---------------------|
| **B1** | **Background tabs keep producing frames** that only get dropped in `frame_stream`. Wastes CPU/GPU/WebProcess for every background tab. | `run.rs` filter; no worker-side suspend | Suspend paint / throttle non-active WPE views (or stop listening) |
| **B2** | **C3 mitigated 2026-08-10:** retire ring depth 2 before `HeldToken` Drop / `buffer_released`. Not a GPU fence — still best-effort. | `frame.rs` `retire` | Optional: real fence if residual tear remains |
| **B3** | **Multi-plane dma-buf:** still not imported (video may stutter/blank). **Release fixed 2026-08-10** — was leaking without `buffer_released` and crashing under YouTube. | `engine.rs` `on_buffer_rendered` | Import NV12/etc. or convert to RGB for media |
| **B4** | **IME / complex text broken:** `keycode: 0`, Character keys only first codepoint, no IME bridge. CJK/emoji/composing fail. | `wpe/input.rs` | Long-term: real IM protocol; short-term: document |
| **B5** | **Middle-click never reaches WPE** (`button_to_wpe` returns `None` for Middle). `decide-policy` new-tab path for middle-click is dead for iced-driven events. | `wpe/input.rs` + `on_decide_policy` | Product: enable middle→background tab **or** delete dead policy branch |

### P1 — chrome / product gaps (rough dogfood)

| ID | Finding |
|----|---------|
| **P1.1** | ~~No stop-loading control~~ **Done 2026-08-10** (↻/× + Escape) |
| **P1.2** | No find-in-page, zoom, reader, downloads, bookmarks, **visit** history UI (session tab restore shipped) |
| **P1.3** | No cookie/profile path hardening / multi-profile UI |
| **P1.4** | Tab **title** strip still merges on 250 ms Tick; omnibox URL on switch is now optimistic (fixed 2026-08-10) |
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
2. ~~**Dogfood MVP chrome**~~ — **D4:** stop · downloads · history+restore ·
   Bitwarden (extension-class) · high polish. Not auto: find/zoom/bookmarks UI/devtools.
3. **Middle-click** — background tab vs ignore (**D5**).
4. **Search provider** — keep Kagi-only or make configurable (**D6**).
5. ~~**Bitwarden approach**~~ — **D7:** first-party Bitwarden UX (SDK/API +
   WebKitWebExtension/content inject); no store package, no system service.
6. **Profile model** — single default WebKit data dir vs multi-profile later.

### Product bar (D4) → backlog mapping

| Product ask | Plan IDs / work |
|-------------|-----------------|
| Stop loading | **Shipped 2026-08-10** — ↻/× + Escape |
| Downloads | New subsystem: WebKit download signals → chrome UI + disk paths |
| History + restore | Persist tab list / visit history; cold-start restore |
| Bitwarden | **D7:** first-party vault UI + SDK + autofill inject (design freeze later) |
| High polish | B1–B5 engine reliability first; then chrome UX polish |

**Build order:**  
engine reliability (B1, B2) → stop → history/restore → downloads → Bitwarden design → implement.

---

## Research note (2026-08-09): Extensions on WPE

**User constraint:** password manager must work **in-browser** (not a separate
system service the user has to run).

### Two different things named “web extension”

| Name | What it is | Chrome/Bitwarden store? | On WPE? |
|------|------------|-------------------------|---------|
| **WebKitWebExtension** | C/GObject **shared library** loaded into WebKit’s **WebProcess** by the *embedder* (`webkit_web_context_set_web_extensions_directory`). DOM hooks, custom UI-process messaging. | No — not a browser store package | **Yes** (GTK and WPE both document this; sample repos target both ports) |
| **WebExtensions (WECG)** | User-installable **manifest.json** add-ons (`chrome.*` / `browser.*` APIs) — Chrome, Firefox, Safari Web Extensions | Yes — Bitwarden ships this | **No** in WPE/WebKit engine itself |

### What ships where

- **WPE / sola-browser today:** no WebExtensions host. No API to load Bitwarden’s
  Chrome/Firefox package. We never wired WebKitWebExtension either.
- **Epiphany (GNOME Web):** implements a **browser-level** WebExtensions host
  (Manifest V2-oriented, Igalia; MV3 still incomplete). That is **application
  code on top of WebKitGTK**, not a free WPE feature. Partial API coverage;
  Bitwarden is commonly cited as a desired target, not a guaranteed fit.
- **Safari:** Bitwarden is a **Safari Web Extension** packaged via Mac App Store
  / Apple WebExtension APIs — Apple-only, not Linux WPE.
- **CONTENT_EXTENSIONS** in WebKit: content-blocker rule lists (Safari content
  blockers style) — **not** a password manager runtime.

### Implication for Bitwarden “as an extension”

1. **Drop-in Bitwarden Chrome/Firefox extension on WPE:** **not available**.
2. **In-browser without a user-run system service** is still possible via:
   - **A. Build a WebExtensions host in sola-browser** (Epiphany-class project:
     background scripts, content scripts, storage, messaging, browserAction,
     enough APIs for Bitwarden). Multi-month; high risk; continuous API chase.
   - **B. First-party password UX in sola-browser** using Bitwarden **SDK/API**
     + WebKitWebExtension (or script injection) for page autofill — vault lives
     inside the browser process/chrome, not a separate daemon the user launches.
   - **C. Chromium embed (CEF) only if extension store parity is non-negotiable**
     — reopens CEF cost we left for perf/dist on NVIDIA.

**Honest dealbreaker line:** if the product requirement is specifically
“install the Bitwarden package from the Chrome Web Store and have it work,”
**WPE is the wrong engine**. If the requirement is “Bitwarden-class autofill
and vault UX inside Sola Browser without running Helium or a side service,”
WPE can still work via A or B.

Sources (external): TingPing/Igalia Epiphany WebExtensions posts; WebKitGTK
`WebKitWebExtension` docs; WebKit bugs on web-process extensions; Bitwarden
Safari packaging docs; sample_webkit_extension (GTK & WPE embedder modules).

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
