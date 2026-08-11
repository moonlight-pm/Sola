# sola-browser · stock WPE Wayland present + river lockstep (Option A)

**Date:** 2026-08-11  
**Status:** **Frozen — product path** (human lock: “Option A … lock it in”)  
**Supersedes (for product paint quality):** preferred hybrid in
[`2026-08-11-sola-browser-content-plane-design.md`](2026-08-11-sola-browser-content-plane-design.md)
§4.1 as **daily-driver target**. Content plane remains implemented and may
stay interim default until A0–A3 cut over.

**Related:** [content plane freeze](2026-08-11-sola-browser-content-plane-design.md)
(§4.2 alternate elevated); [checkerboard deep fix](../plans/2026-08-11-browser-checkerboard-deep-fix.md);
[present deep-dive](../plans/2026-08-11-browser-present-architecture-deep-dive.md);
[implementation plan](../plans/2026-08-11-sola-browser-stock-wayland-lockstep-plan.md);
CURRENT; architecture.

---

## 1. Intent

Match **open-source WebKit/WPE browsers** (cog / Epiphany-class present):

**WPE owns Wayland present** (`WPEDisplayWayland` + `WPEViewWayland`).  
Sola owns **chrome** (iced) and **shell geometry** (river lockstep).

Stop treating headless → custom content-plane re-present as the quality
finish line. That hybrid fixed loan races; it does **not** equal stock
display-link / present quality under YouTube-class scroll.

---

## 2. Locked product architecture (Option A)

```text
┌─ iced  xdg_toplevel  app_id=sola-browser ─────────────────────┐
│  Chrome only: CSD, tabs, omnibox, menus, vault               │
│  Content scissor = transparent hole (no page sampling)       │
└──────────────────────────────┬────────────────────────────────┘
                               │ z-order: chrome above content
┌─ WPE   xdg_toplevel  app_id=sola-browser-content ────────────┐
│  Stock WPEDisplayWayland / WPEViewWayland present            │
│  Position+size = content scissor (river lockstep)            │
│  Decoration none; not a second “app” in product UX           │
└──────────────────────────────────────────────────────────────┘
         River composites both; one visual browser unit
```

| Layer | Owner | Role |
|-------|--------|------|
| Chrome | iced | Tabs, omnibox, menus, vault, float CSD |
| Content | **Stock WPE Wayland** | Page pixels, FrameDone, Release, fences |
| Glue | sola-browser + sola-river | Geometry, z-order, joint lifecycle, focus |
| Import / plane | Interim / fallback | Not daily-driver target after cut over |

### Explicit non-goals (this freeze)

- HTML chrome rewrite (Option B)  
- More headless present race patches as the primary quality program  
- CEF  
- Second free-floating window users must manage by hand (lockstep is required for ship)

---

## 3. Why (decision record)

1. Content-plane preferred path is **implemented**; dogfood still shows
   **full-width black / checkerboard** under hard scroll (fixed nav goes
   black with the band → incomplete full frames).  
2. Open-source browsers use **stock platform present**, not headless
   re-import.  
3. Subsurface-of-iced requires same `wl_display`; stock WPE opens its own
   connection → **sibling + river lockstep** is the viable single-unit UX.  
4. Human **2026-08-11:** “Option A … lock it in.”

---

## 4. Phases (mandatory order)

| Phase | Name | Exit criteria |
|-------|------|----------------|
| **A0** | Quality proof | Dual-window `SOLA_BROWSER_CONTENT=wayland` on `sola:scroll-stress` + YT: **clearly better** full-width black than plane (human eye). If **not** better → stop; reassess paint budget / NVIDIA (present not sole cause). |
| **A1** | Harden stock engine mode | Stable connect, resize, scale, multi-tab visibility, seal WebProcess, no phantom toplevels, input path defined. |
| **A2** | Geometry + river lockstep | Content window tracks chrome content scissor (move/resize/zone/float drag); z-order correct; hole aligned. |
| **A3** | Lifecycle + shell hygiene | Joint minimize/close/hide; focus rules; switcher/MRU grouping or hide companion; install/re-exec. |
| **A4** | Cut over | Default product path = stock Wayland + lockstep; plane/import demoted; docs + capabilities; optional delete later. |

**Do not** skip A0. **Do not** default to Wayland in A1 without lockstep (A2) unless dogfood explicitly accepts dual window temporarily.

---

## 5. App ids and process model

| Surface | `app_id` (target) | Process |
|---------|-------------------|---------|
| Chrome | `sola-browser` | `sola-browser` (iced) |
| Content | `sola-browser-content` | same process UI+WPE worker (preferred) or document if split |

One bus app identity for session/launch where possible; river may key the
companion by app_id or bus-published window id.

---

## 6. Input (locked preference)

**v1:** Keep **empty input region / iced hit-test → WPE inject** for the
content rect if that already works with dual surface; **or** seat focus on
content surface when pointer is over content — pick one in A1 and do not
mix. Prefer **not** regressing page scroll/click.

---

## 7. Fallback matrix

| Mode | Env | Role after A4 |
|------|-----|----------------|
| Stock Wayland + lockstep | default | **Product** |
| Content plane | `SOLA_BROWSER_CONTENT=plane` | Debug / emergency |
| iced import | `SOLA_BROWSER_CONTENT=import` | Debug only |

---

## 8. Acceptance (daily-driver bar)

Same as content-plane freeze §9, measured on **product path after A4**:

- `sola:scroll-stress` and `https://www.youtube.com/` hard scroll  
- No habitual full-width black bands (incl. fixed nav solid)  
- No constant tile flicker  
- One visual window unit under normal use  
- No SEGV / WPE_IS_BUFFER storms  

---

## 9. Decisions locked

1. **Product present** = stock **WPEDisplayWayland** / **WPEViewWayland**.  
2. **Chrome** remains **iced**.  
3. **Single visual unit** via **river lockstep sibling** (not free dual window).  
4. Content plane = **interim**, not quality endgame.  
5. **A0 gate** required before investing A2–A4 as default.  
6. Engine remains **WPE only**.

---

## 10. One-line summary

**Stock WPE presents content; iced draws chrome; River glues them into one browser.**
