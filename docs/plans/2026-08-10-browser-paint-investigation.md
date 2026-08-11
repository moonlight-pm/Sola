# Browser paint pipeline investigation — YouTube crash

**Date:** 2026-08-10  
**Branch:** `naturalethic/browser`  
**Trigger:** Load YouTube → `sola-browser` + `WPEWebProcess` **SIGSEGV** (~19:35); earlier sessions **EMFILE** / GStreamer abort.  
**Related:** hardening plan B2/B3; D8 profiles (orthogonal).

## Crash evidence (dogfood)

| Time | Event |
|------|--------|
| 19:34:50 | `wpe_view_buffer_released: assertion 'WPE_IS_BUFFER(buffer)' failed` (×2) |
| 19:34:55 | `Nav::LoadUrl https://youtube.com` |
| 19:35:01 | `sola-browser` **SIGSEGV** (coredump present) |
| 19:35:11 | `WPEWebProcess` **SIGSEGV** |

Earlier (17:27 YouTube media): `Too many open files` → GStreamer-GL `Bail out!` → freeze/black.

## Pipeline (as-built)

```text
WebKit GPU → WPEBufferDMABuf → buffer-rendered hook
  → dup plane0 FD + live_buffers claim + epoch
  → sync_channel(1) → iced prepare
  → Vulkan import (ARGB/XRGB only) → sample
  → HeldToken Drop → Cmd::Release → wpe_view_buffer_released
```

NV12 multi-plane: **release without import** (pool safety; video may be black).  
Multi-plane RGB (NVIDIA modifiers): import shared-FD layouts when ARGB/XRGB.

## External practice (summary)

Canonical WebKit DMA-BUF UI protocol ([Graphics docs](https://docs.webkit.org/Ports/WebKitGTK%20and%20WPE%20WebKit/Graphics.html)):

1. Negotiate formats the UI can import  
2. Import buffer once (id + planes)  
3. Frame = id + **fence**  
4. **Always ReleaseBuffer** (loan/return)  
5. FrameDone  

Industry: single-plane RGB zero-copy; **NV12 convert or skip+release** until multi-plane wgpu works ([wgpu#9801](https://github.com/gfx-rs/wgpu/issues/9801)).  
Blogs: Carlos GC DMABUF compositing; GStreamer modifier negotiation; Mad Devs CEF vs WPE; Slint+Servo external memory → wgpu.

## Hypotheses (ranked)

| # | Hypothesis | Evidence |
|---|------------|----------|
| H1 | **Double-release / UAF** on WPE buffer | `WPE_IS_BUFFER` criticals → SEGV; historical sign-in crash |
| H2 | Release while GPU still samples (retire too shallow / no fence) | Retire depth 1; no Vulkan fence wait |
| H3 | **EMFILE** → cascade | Logs; many holds under media |
| H4 | Multi-plane import fail under load | Black band; pool churn |
| H5 | Token lost without Release | Panic window after `take_token` |

## P0 fix sprint

1. **Release audit:** never call `wpe_view_buffer_released` on a buffer still in `live_buffers`  
2. **Lifecycle trace ring** (last N events) + log dump on refuse / high pressure  
3. **Hard in-flight cap:** refuse new claims when `live_buffers` ≥ max (release untracked)  
4. Prefer dropping under pressure over claiming more  
5. **GObject ref on claim** (`sola_wpe_buffer_ref`) until release — root cause of SEGV:
   `process_cmd` → `wpe_view_buffer_released` → `g_type_check_instance_is_a` on freed GObject  
6. **`sola_wpe_view_buffer_released_safe`** + OpenUrl bus for dogfood  

**Dogfood 2026-08-10 19:58:** launch → OpenUrl YouTube → **alive 15s+**, scroll stress OK,
screenshot shows YouTube chrome (signed-out empty feed on clean D8 profile). No new
`WPE_IS_BUFFER` criticals on that PID.

**Not in this sprint:** NV12 convert, GPU fence wait, preferred-format negotiation rewrite.

## Follow-ups (P1+)

- Fence-before-sample (WebKit Frame fence)  
- Async NV12→RGBA with release-before-convert  
- Prefer newer frame on channel Full (needs rx access)  
- Install gdb for symbolized coredumps  
- Cookie `"cookie"` EROFS path (separate from paint)

## Paint quality telemetry + fix (2026-08-10)

After SEGV + scroll freeze fixed: **brief blackout on fast scroll**,
**menus / top-left nav flicker**.

### Telem

`src/wpe/paint_stats.rs` — 2s `paint telem` lines; warn on gap ≥ 250 ms and
`sample.clear`. Browser menu **Paint Stats** (⇧⌘I).

```bash
rg "paint telem" /opt/sola/log/app-sola-browser.log | tail -40
```

| Field | Read as |
|-------|---------|
| `drop_ch` | mailbox replaced older frame (latest-wins; healthy under load) |
| `drop_bg` | inactive tab present released without claim |
| `drop_cap` | live buffer cap; untracked release |
| `ignore` | same buffer re-presented while held (pool pin if claim=0) |
| `prep_idle` ≫ `prep_new` | redraws without new frames |
| `gap_present_ms` / `gap_import_ms` | freeze / black gap size |
| `sample_clear` | bind group cleared (true black flash) |
| `yuv_skip` | NV12/video not painted |

### Fix from telem (same day)

| Root cause | Change |
|------------|--------|
| `sync_channel(1)` dropped **newer** frame on Full | **FrameMailbox** latest-wins |
| Background tabs claimed then `drop_bg` (pool burn) | Worker **releases inactive** without claim |
| `RETIRE_DEPTH=1` + active pin both pool slots → ignore forever | **RETIRE_DEPTH=0** |
| Stuck `redraw_queued` | Clear on prepare when taking pending |

Healthy under load: `ignore≈0`, `claim≈import_ok≈released`, `live=1`.
## Code touch

- `crates/sola-browser/src/wpe/engine.rs` — claim/release/cap/trace  
- `crates/sola-browser/src/wpe/paint_stats.rs` — quality telem  
- Docs: this plan, CURRENT, capabilities gap notes  
