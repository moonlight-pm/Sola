# Plan — stock WPE Wayland + river lockstep (Option A)

**Date:** 2026-08-11  
**Freeze:**
[`docs/specs/2026-08-11-sola-browser-stock-wayland-present-design.md`](../specs/2026-08-11-sola-browser-stock-wayland-present-design.md)  
**Status:** **Active** — product path locked

Checklist only. Status: `[ ]` / `[~]` / `[x]`.

---

## A0 — Quality proof (gate)

- [ ] Run browser with `SOLA_BROWSER_CONTENT=wayland` (dual window OK)
- [ ] Open `sola:scroll-stress`; hard fling; note fixed nav vs full-width black vs plane
- [ ] Open YouTube homepage; same
- [ ] Human verdict: **better / same / worse** than plane
- [ ] If **not better** → stop A2–A4 default work; record in CURRENT + freeze header

**Owner:** human dogfood + agent support logs

---

## A1 — Harden stock engine mode

- [x] `WPEDisplayWayland` connect via captured socket (already partially wired)
- [x] Content app_id `sola.browser-content` (GLib-valid wire form of freeze name)
- [x] Resize/scale from chrome content CSS size (shader prepare → Cmd::Resize)
- [x] Multi-tab: only active view mapped/visible (`wpe_view_set_visible`)
- [x] WebProcess: no phantom `org.webkit.*` (seal after connect)
- [x] Input path chosen and documented (seat on companion; inject only on chrome hole)
- [x] No content-plane Present on this mode (no double present)
- [x] Telem: present path = `wayland-stock`

---

## A2 — Geometry + river lockstep

- [x] Browser publishes content scissor in global/output coords (`Topic::BrowserContentScissor`)
- [x] sola-river places/sizes companion window under hole (scissor + app_id match)
- [x] Z-order: content below chrome; hole transparent (composition insert + Load hole)
- [~] Chrome move (CSD drag), resize, zone, output change → content follows (via WindowGeometry + prepare; dogfood)
- [ ] Alignment dogfood: no permanent gutter / overlap

---

## A3 — Lifecycle + shell hygiene

- [ ] Close/minimize/hide chrome ↔ content
- [ ] Focus rules (click hole vs chrome)
- [ ] Switcher/MRU: one unit or hide companion
- [ ] Re-exec / install restart both surfaces cleanly

---

## A4 — Cut over

- [x] Default env/path = stock Wayland + lockstep (`ContentMode::from_env`)
- [x] `plane` / `import` demoted in CURRENT + capabilities + freeze headers
- [x] architecture.md as-built paint section updated
- [ ] Optional: delete or quarantine dead present code later (not day-one)

---

## Explicitly out of this plan

- HTML chrome rewrite  
- Headless present micro-fixes as primary track  
- CEF  

---

## First coding slice after lock (suggested)

1. A0 dogfood script + log notes  
2. A1: app_id + resize from content rect + ensure plane disabled in wayland mode  
3. A2 spike: bus geometry + river `set_position` for matching app_id  
