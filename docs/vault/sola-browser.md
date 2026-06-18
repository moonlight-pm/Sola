# sola-browser

A standalone web browser inside Sola, implemented twice — once on
WebKit (WPE Platform API) and once on Chromium (CEF) — so we can
compare engines, hedge against single-engine bugs, and keep our
shader/iced integration honest. Both live in the cargo workspace at
`crates/sola-browser-wpe/` and `crates/sola-browser-cef/` as ordinary
members (root `members = ["crates/*"]`; only `wgpu-hal-patched` is
excluded). They were once held outside the workspace because their `iced`
dependency drags in `smithay-clipboard` → `wayland-sys` with the `dlopen`
feature, whose workspace-wide unification threatened [[sola-river]]'s
direct-link wayland — but that was resolved by baking
`/run/current-system/sw/lib` into every binary's RUNPATH (via
`.cargo/config.toml`), so sola-river's runtime dlopen finds the system
libwayland. The workspace was unified accordingly (commit 7f97004); the
stale per-crate `Cargo.lock` files under each browser crate are leftovers
from the isolated era and are ignored.

**Status (2026-06-17).** WPE is primary, CEF is at feature parity. The
legacy GTK/WebKit `sola-browser` crate has been retired (WPE is the shell's
default "Browser"). Both engines are now wired into the Sola bus
(`Topic::OpenUrl` → new tab, a "Browser" app-menu that doubles as the
keyboard-shortcut mechanism, and live `Topic::Theme` chrome restyling) — see
`docs/specs/2026-06-17-sola-browser-bus-integration-design.md`. Neither is
yet a production-grade browser: no profile/cookie persistence, minimal
chrome. The architecture is proven; what remains is polish + the deeper
browser feature set (bookmarks, downloads, history, devtools UI, etc.).

## Architecture

```
                ┌──────── iced (main thread) ─────────┐
                │  App                                │
                │  ├─ tab strip (Open / Close / Pick) │
                │  ├─ chrome row (◀ ▶ ↻  url-input)   │
                │  └─ Shader widget (fills rest)      │
                │       │                             │
                │       └─ prepare/render:            │
                │            • size mismatch? skip    │
                │            • Cmd::Resize on change  │
                │            • upload texture, draw   │
                └──────────┬──────────────────────────┘
                           │ slot.releaser (mpsc Sender<Cmd>)
                           ▼
              ┌──── engine worker thread ─────┐
              │  GMainLoop (WPE) /            │
              │  run_message_loop (CEF)       │
              │                               │
              │  process_cmd(cmd):            │
              │   Resize  → active tab        │
              │   Input   → active tab        │
              │   Nav     → active tab        │
              │   Focus   → active tab        │
              │   OpenTab → create webview    │
              │   Close   → remove + unref    │
              │   SetActive → atomic swap     │
              │                               │
              │  tabs: Vec<TabState>          │
              │   ├─ WebKitWebView / cef::Browser
              │   ├─ url: Arc<Mutex<String>>  │
              │   └─ title: Arc<Mutex<String>>│
              │                               │
              │  Frame callbacks:             │
              │   WPE buffer-rendered  ──┐    │
              │   CEF on_paint          ──┤    │
              │                            │   │
              └────────────────────────────┼──┘
                                           │ TaggedFrame { tab_id, frame }
                                           ▼
                                  mpsc → iced subscription
                                  → filter by active tab
                                  → slot.pending
                                  → next iced redraw
```

### The split, in one line each

- **`crates/sola-browser-wpe/`** — WPE WebKit Platform API,
  zero-copy DMA-BUF import via modifier-aware Vulkan
  (wgpu-hal-patched), one GLib mainloop per process, hijacked
  `WPEDisplayHeadless` for headless rendering + `WPEViewHeadless`
  cursor passthrough.
- **`crates/sola-browser-cef/`** — Chromium Embedded Framework,
  CPU OSR via `OnPaint` → `queue.write_texture` (CEF's GPU dma-buf
  transport doesn't work on NVIDIA proprietary; see [[Distribution]]).

### Why a shader widget?

iced renders into a wgpu surface; we want the webview to be
*part* of an iced UI tree (tab strip + URL bar above, future
chrome around it) rather than a separate compositor surface. So
we wrap each engine's frame output as a `wgpu::Texture` and draw
it via a fullscreen triangle in an iced
`widget::Shader::Program`. iced handles layout/clipping/input
routing for free; we just need the texture upload + draw.

The trickiest piece of the integration was correctly mapping the
triangle into the widget bounds (not the whole surface) so the
page's first scanline doesn't end up under the chrome row.
`wgpu::RenderPass::set_viewport(clip_bounds.*)` is what does it;
scissor alone clips writes but doesn't re-map UVs.

## What works

| Feature                          | Both engines |
| -------------------------------- | ------------ |
| Render a URL                     | ✓            |
| Resize follows the iced widget   | ✓            |
| Mouse: click, drag, scroll       | ✓            |
| Keyboard: typing, Enter, arrows  | ✓            |
| Dynamic cursor (CSS-driven)      | ✓            |
| URL bar (type + Enter)           | ✓            |
| Back / Forward / Reload buttons  | ✓            |
| Multiple tabs (open / close / switch) | ✓       |
| Page title in window title       | ✓            |
| sRGB-correct colour              | ✓            |
| Bus `Topic::OpenUrl` → new tab   | ✓            |
| App-menu + ⌘ shortcuts (via shell) | ✓          |
| Live theme-synced chrome (`Topic::Theme`) | ✓   |

## What doesn't

- **Cookie / login persistence.** WPE uses default `WebKitWebContext`
  (volatile cookies); CEF uses a per-app `root_cache_path` but no
  named profiles. Logging in to a site survives across navigations
  within one session; closing the app loses everything.
- **Bookmarks, history UI, downloads.** Not implemented. CEF has
  the underlying APIs (`DownloadHandler`, `RequestHandler`); WPE
  has equivalents (`webkit_download_*`, `webkit_history_*`). Plumbing
  to chrome is the work.
- **DevTools.** Both engines support it (`webkit_web_inspector_show`,
  `host.show_dev_tools(...)`), but neither is wired into our chrome.
- **Tab-cycling shortcuts (⌘Tab / next-prev tab).** The core
  shortcuts (⌘T new tab, ⌘W close, ⌘R reload, ⌘L focus URL, ⌘←/⌘→
  back/forward, ⌘Q quit) now ship via the published "Browser" app-menu —
  the shell binds the chords and routes `Topic::MenuAction` back (this is
  the anticipated "customise keybinds at a higher level" mechanism, so
  nothing is hardcoded per-app). Tab-cycling isn't wired yet; it needs a
  next/prev-tab action + menu entry.
- **Search-engine routing.** `normalize_url` prepends `https://`
  to scheme-less input. Free text doesn't go to a search engine
  yet.
- **Focus-aware URL bar.** The chrome's poll-and-update loop
  overwrites the URL field even while the user is typing if the
  page navigates. Should track input focus and skip the
  overwrite while focused.
- **Background-tab throttling.** Iced drops background-tab
  frames before wgpu sees them, but WPE/CEF keep producing them.
  Should hide non-active tabs via
  `webkit_web_view_set_is_in_window(false)` (WPE) or
  `host.was_hidden(true)` (CEF).
- **Loading state / progress bar.** No visual indicator that
  a page is loading.
- **Favicons.** Not surfaced in tabs.
- **WPE sharpness.** WebKit-on-WPE-headless text reads a bit
  softer than Chromium on the same display. Tried `FilterMode::Nearest`
  and `font-hinting-style=FULL`; neither moved the needle.
  Investigation paused; suspect device-pixel-ratio handling
  (iced reports `scale_factor = 1.0` and we pass that through;
  rendering at DPR=2 would supersample but requires a non-trivial
  resize-path change).

## Engine choice

**WPE is primary.** The 2026-05-21 benchmark
(see [docs/notes/2026-05-21-wpe-vs-cef-bench.md](../notes/2026-05-21-wpe-vs-cef-bench.md))
showed CEF needs ~70 % more CPU per frame on animated content
because NVIDIA proprietary forces CEF onto a CPU-OSR path that
memcpys a full ~12 MiB BGRA buffer per paint. WPE uses zero-copy
modifier-aware Vulkan import on the same hardware. Memory also
favours WPE by 400–700 MiB.

**CEF stays maintained at parity.** Both engines get every new
feature so the abstraction stays honest. Revisit the engine
choice if we move to Mesa (NVIDIA Open + NVK, Intel, AMD); on
those stacks CEF's GPU transport works and the bench would need
redoing.

See `docs/specs/2026-05-21-sola-browser-cef-port-and-benchmark.md`
for the original plan; see [[project_browser_engine]] (memory)
for the decision summary.

## Where to look in code

| file                                       | what it owns                                           |
| ------------------------------------------ | ------------------------------------------------------ |
| `crates/sola-browser-wpe/src/main.rs`      | iced App + chrome + tab strip + frame subscription     |
| `crates/sola-browser-wpe/src/wpe.rs`       | WpeEngine, worker thread, tab management, FFI dispatch |
| `crates/sola-browser-wpe/src/sola_wpe.c`   | C-side WPE class hijacks (LINEAR modifiers, cursor)    |
| `crates/sola-browser-wpe/src/shader.rs`    | iced shader::Program/Primitive/Pipeline for WPE frames |
| `crates/sola-browser-wpe/src/wgpu_import.rs` | DMA-BUF → wgpu::Texture import (modifier-aware)      |
| `crates/sola-browser-wpe/src/input.rs`     | iced event → WPE event translation                     |
| `crates/sola-browser-wpe/src/integration.rs` | bus wiring: OpenUrl/MenuAction/Theme → `BrowserIntent` |
| `crates/sola-browser-cef/src/main.rs`      | iced App + chrome + tab strip (mirror of WPE)          |
| `crates/sola-browser-cef/src/cef.rs`       | CefEngine, worker thread, multi-browser management     |
| `crates/sola-browser-cef/src/shader.rs`    | iced shader::Program for CEF frames (CPU upload path)  |
| `crates/sola-browser-cef/src/cpu_import.rs` | BGRA bytes → wgpu::Texture via queue.write_texture    |
| `crates/sola-browser-cef/src/input.rs`     | iced event → CEF event translation (VK_ codes, etc.)   |
| `crates/sola-browser-cef/src/integration.rs` | bus wiring: mirror of the WPE integration module     |
| `crates/wgpu-hal-patched/`                 | Vendored wgpu-hal with VK_EXT_image_drm_format_modifier|

## Pre-existing knowledge that informs this

- [[sola-kit]] already integrates CEF. Lots of patterns came
  from there (subprocess gate, `wrap_app!` / `wrap_client!`
  scaffolding, CEF resource layout). Differences:
  sola-kit renders into a Wayland surface via dma-buf (sometimes)
  or wl_shm (NVIDIA); sola-browser-cef renders into an iced
  wgpu surface via texture upload.
- [[Distribution]] documents the NVIDIA-vs-CEF dma-buf
  incompatibility in detail.
- The WPE Platform API integration was new for this project;
  there's no prior sola code that uses it. Notes on what we
  learned (the `WPEDisplayHeadless` is `G_DECLARE_FINAL_TYPE`
  so we hijack the class vtable rather than subclass; the
  headless frame-source is mis-implemented upstream so the
  60 fps rate-limit doesn't actually engage; etc.) are in
  source comments and commit messages.

## Roadmap

In rough priority order:

1. **Polish the chrome:** focus-aware URL bar, loading
   indicator, favicons. Style the tab strip with an
   active-tab visual.
2. **Background-tab throttling** so opening 10 tabs doesn't
   keep 10 WebProcess/render processes saturating cores.
3. **Search-engine routing** in `normalize_url`.
4. **Cookie/profile persistence.** Probably one profile dir
   per Sola user (`~/.config/sola/browser/<profile>/`), with
   WebKit's NetworkSession / CEF's `root_cache_path` pointing
   into it.
5. **DevTools** — wire to a button or keyboard shortcut.
6. **Tab-cycling shortcuts** (⌘Tab / next-prev tab) — the core ⌘
   shortcuts already ship via the published app-menu; this is the one
   remaining gap (needs a next/prev-tab action + menu entry).
7. **Multi-window** (independent browser windows, not just
   tabs) — separate iced::application instances? Or one app
   with multiple `iced::window::Settings` via the multi-window
   iced API?

Items 1–3 are small. 4+ are real product features.
