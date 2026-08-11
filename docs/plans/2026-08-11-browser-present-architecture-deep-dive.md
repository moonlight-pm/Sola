# Browser present architecture — deep dive (2026-08-11)

**Trigger:** Residual YouTube black swaths + flicker after frame-pace and
FrameDone-after-present fixes. Blur improved; black/flicker did not.
**GPU (dogfood):** NVIDIA GeForce RTX 3090 Ti; non-linear DMA-BUF modifier
`0x300000000400014` class; `fence_none` always in telem.

## 1. What “solved many times” actually is

Canonical reference (upstream WebKit):

| Port | Present path |
|------|----------------|
| **WPE Wayland** | `WPEViewWayland::render_buffer` → `wl_surface` on **WPE’s** display |
| **WPE DRM** | Direct KMS |
| **WebKitGTK** | Import once → fence wait → `GdkTexture` paint → FrameDone → Release previous |

Authoritative docs:
[WebKit Graphics](https://docs.webkit.org/Ports/WebKitGTK%20and%20WPE%20WebKit/Graphics.html),
stock source
[`WPEViewWayland.cpp`](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/WPEPlatform/wpe/wayland/WPEViewWayland.cpp).

### Stock `WPEViewWayland` algorithm (the gold standard)

```text
DidCreateBuffer (once per pool slot)
  → create wl_buffer (linux-dmabuf create_immed OR eglCreateWaylandBufferFromImageWL)
  → CACHE on WPEBuffer user_data  ← critical

Frame (id + fence fd)
  → if explicit sync: set acquire fence on surface
    else: UI already waited fence before render_buffer
  → wl_surface.attach(CACHED wl_buffer)   ← not a new params every frame
  → damage (rects or full)
  → wl_surface.frame → FrameDone ONLY here
  → commit
  → ONE outstanding frame callback (assert no second)

wl_buffer.release
  → wpe_view_buffer_released
  → KEEP wl_buffer alive for reuse   ← do not destroy
```

Igalia/Carlos: explicit fence export improved MotionMark; UI must not paint
incomplete GPU work. NVIDIA + WebKit DMABuf has a long history of
flicker/blank (driver + modifier + sync).

## 2. Sola as-built (why we diverge)

```text
WPEDisplayHeadless (no Wayland knowledge)
  → render_buffer hijack → claim (dup FD)
  → ContentPlane on iced's FOREIGN wl_display
  → create_immed EVERY present
  → subsurface desync under iced xdg_toplevel
  → destroy wl_buffer on compositor Release
  → drop rendering fence (fence_none in practice)
```

| Dimension | Stock WPE Wayland | Sola content plane |
|-----------|-------------------|--------------------|
| Who owns Wayland | WPE platform | iced + foreign plane |
| Buffer import | **Once** per pool slot | **Every frame** `create_immed` |
| wl_buffer lifetime | Until WPEBuffer dies | Destroy on every Release |
| Fence | Acquire fence or pre-wait | Dropped; telem `fence_none` |
| Surface | Real toplevel/surface | Subsurface of iced, desync |
| Display connection | WPE-owned | Shared with iced (foreign) |
| FrameDone | frame callback only | (now) frame cb — good |
| NVIDIA modifiers | Same path, battle-tested | Same mods, untested path |

**This is not “one more race in our timer.”** We reimplemented a half of
`WPEViewWayland` incorrectly on a foreign display + subsurface topology.

## 3. Why symptoms match the divergence

| Symptom | Likely mechanism |
|---------|------------------|
| Black swaths under hard scroll | Incomplete GPU frame (no fence) **or** destroy/recreate wl_buffer while NVIDIA still maps **or** pool rewrite while compositor samples |
| Nav / masthead flicker | Same buffer path; sticky chrome is **in** the page (content plane), not iced |
| Soft text (improved) | 2× supersample helps; not 1:1 with output scale honesty |
| Healthy telem, bad pixels | Counters track loans, not scanout correctness |

NVIDIA 3090 Ti + proprietary modifiers make create_immed-every-frame +
destroy-on-Release especially risky (known WebKitGTK DMABuf flicker class).

## 4. Alignment options (ordered)

### A. Mirror `WPEViewWayland` on our subsurface (near-term) — **do now**

Stay on ContentPlane + headless, but present like stock:

1. **Cache `wl_buffer` by WPEBuffer pointer** — create_immed once per slot  
2. **Never destroy on compositor Release** — only WPE return + keep cache  
3. **Pass all plane FDs** (dup each), not plane0-only when multi-fd  
4. **Wait rendering fence before attach** when FD present  
5. **Strict single frame callback** (stock assert) — no second attach until Done  
6. Full damage fallback like stock when no rects  

Success metric: YT homepage hard scroll — black/flicker absent to eye.

### B. Prefer stock `WPEDisplayWayland` (medium)

Use real WPE Wayland platform for content:

- Same algorithm as A, but code is upstream’s  
- Must parent content under iced: same `wl_display` + subsurface or
  river-positioned sibling (freeze F2 — ask human)  
- Keep sealing `WAYLAND_DISPLAY` for WebProcess (phantom toplevel guard)

### C. GTK-style owned copy (fallback)

Import → blit to linear/owned → present owned → release WPE early.
Import path already blits for iced; plane can present a **stable** buffer.
Higher bandwidth; known-good against rewrite races.

### D. Not product paths

- Nested libwpe-fdo long-term  
- CEF OSR  
- `WEBKIT_DISABLE_DMABUF` as permanent bar (debug only)

## 5. Decision

**Implement A immediately** (cache + fence wait + no destroy-on-Release).
If dogfood still fails on NVIDIA → spike B (stock WPE Wayland surface) or
C (owned linear present).

## 6. Code map for A

| File | Change |
|------|--------|
| `content_plane/plane.rs` | `HashMap` cache; Release ≠ destroy; fence wait |
| `wpe/engine.rs` | Dup all planes; pass fence + buffer key to plane |
| telem | cache_hit / create / fence_wait |

## 7. One-line summary

**Stop inventing a second Wayland present stack: do what `WPEViewWayland`
does — import once, fence, attach cached buffer, FrameDone on frame cb,
release without destroying the protocol object.**
