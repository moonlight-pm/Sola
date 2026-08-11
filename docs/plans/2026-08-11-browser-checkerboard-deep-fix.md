# Browser checkerboard / full-width black — deep fix (2026-08-11)

## Diagnosis (user dogfood)

Residual “black swaths” after present-path races were fixed are **not** primarily
buffer loan bugs. They are **checkerboarding**:

- Scroll outruns WebKit tile raster → unpainted tiles clear to empty/black.
- **Full-width** black that also hits the **fixed left nav** at the same Y means
  the **whole framebuffer row** is unpainted (incomplete composition), not only
  the feed scroller.

Chrome shows a soft checkerboard; we showed pure black (harsh). Forced **2×
supersample** (paint 4× pixels vs 1×) made tile outrun far more likely on
NVIDIA + large windows (~2.5k×4k buffers in logs).

## What other browsers do

| Technique | Role |
|-----------|------|
| Speculative / async tiles | Paint ahead of viewport |
| Soft placeholder (checkerboard) | Unpainted ≠ “broken black” |
| Fixed-layer promotion | Sticky nav survives feed tile misses |
| Honest paint budget | Don’t supersample during fling |
| Real display link | FrameDone / composition schedule from present |

## Deep fix package (this change)

### 1. Adaptive paint budget (product default)

`crates/sola-browser/src/wpe/paint_budget.rs`

- Default: **honest compositor scale** (no forced 2×).
- While scrolling (wheel within 220 ms): clamp ≤ **1.25×** so tiles keep up.
- Opt-in old supersample: `SOLA_BROWSER_SUPER_SAMPLE=1`.
- Force: `SOLA_BROWSER_DPR=N`.

### 2. Stock WPE Wayland present (architecture A/B)

`SOLA_BROWSER_CONTENT=wayland`

- `WPEDisplayWayland` + upstream `WPEViewWayland::render_buffer`.
- Real compositor FrameDone / release / buffer cache — not headless hijack.
- **Dogfood trade-off:** separate content `xdg_toplevel` (dual window with iced
  chrome). Quality comparison path; product lockstep (river sibling under hole)
  is the follow-on if this wins on YouTube/scroll-stress.

### 3. Built-in stress page

Navigate to **`sola:scroll-stress`** (or omnibox `scroll-stress`).

- Fixed left nav + sticky section headers + large color-card grid.
- Isolates full-width black vs feed-only holes better than YouTube.

### 4. Softer unpainted clear

WebView background `#18181c` (raised dark) instead of pure black void.

## Dogfood order

```bash
# A — product plane + adaptive budget (default install)
solactl emit OpenUrl '{"url":"sola:scroll-stress","activate":true}'
# hard fling — nav should stay solid more often; fewer full-width voids

# B — stock Wayland quality comparison (restart browser with env)
SOLA_BROWSER_CONTENT=wayland /opt/sola/bin/sola-browser
# open scroll-stress / YouTube; note dual window
```

## Follow-ons (if A insufficient)

1. River lockstep sibling: position WPE Wayland surface under iced content hole.  
2. Owned linear present (option C) + near-black hold heuristic.  
3. WebKit tile / speculative settings when exposed.

## One-line

**Stop starving the tile rasterizer (honest + scroll budget); offer stock WPE
Wayland present for display-link quality; dogfood on `sola:scroll-stress`.**
