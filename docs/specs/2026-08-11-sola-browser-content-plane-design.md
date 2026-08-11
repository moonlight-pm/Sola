# sola-browser · content plane (Wayland present)

**Date:** 2026-08-11  
**Status:** **Frozen + implemented (partial quality)** — default path on
`naturalethic/browser`; daily-driver scroll bar **not** met yet

### Implementation progress (2026-08-11)

| Gate | Status |
|------|--------|
| G1 parent handles from iced `window::run` | **Pass** |
| G2 subsurface | **Pass** (main-thread Wayland only) |
| G3 attach SHM + dma-buf | **Pass** — homepage grid dogfood |
| G4 hold until compositor `wl_buffer.release` | **Pass** (not immediate prev drop) |
| Input (scroll/click) | **Pass** — empty input region → iced → WPE |
| Content scale 2× + `set_buffer_scale` | **Pass** (override `SOLA_BROWSER_DPR`) |
| Frame-callback paced attach | **Pass** (code) — no force-release of attached buffers; latest-wins queue while awaiting frame |
| Daily-driver YT hard-scroll quality | **Open** — re-dogfood after frame-pace; soft text still open |

**Default:** `SOLA_BROWSER_CONTENT=plane`. Fallback: `import`.  
**Probe:** `SOLA_BROWSER_PLANE_PROBE=1` for magenta SHM visibility test.

**Dogfood:** `solactl emit OpenUrl '…'` (not `solactl open` → Helium).
**Branch context:** `naturalethic/browser`  
**Related:**
[paint investigation](../plans/2026-08-10-browser-paint-investigation.md);
[hardening](../plans/2026-08-09-sola-browser-hardening.md);
[profiles D8](2026-08-10-sola-browser-profiles-design.md);
[Bitwarden D7](2026-08-10-sola-browser-bitwarden-design.md);
[architecture](../architecture.md); [CURRENT](../../CURRENT.md).

---

## 1. Intent

Ship **daily-driver paint quality** on heavy sites (YouTube homepage scroll:
no black swaths, no chrome flicker, smooth scroll, crisp text) by fixing
**who owns the content plane**, not by further headless dma-buf→iced patches.

**Locked product direction (this freeze):**

| Do | Don’t |
|----|--------|
| Web content presented as a **real Wayland buffer** (River composites) | Sample web content through iced/wgpu every frame (product path) |
| **Iced = chrome only** (tabs, omnibox, menus, vault, CSD) | Treat the browser as “another full-window kit shader app” for page pixels |
| Engine stays **WPE WebKit** | Restore CEF for paint (unless a later explicit product reopen) |
| One app id `sola-browser`, one shell window unit | Separate shell windows for chrome vs content (unless spike fails) |

Headless dma-buf import remains a **debug / emergency fallback**, not the
quality bar.

---

## 2. Problem (as-built)

```text
WebKit GPU → dma-buf → headless WPE
  → sola claim → Vulkan import → blit owned → iced sample
  → one xdg_toplevel (chrome + content) → River
```

The UI process is a **mini compositor** for every frame. That fights WebKit’s
protocol (Frame / fence / FrameDone / ReleaseBuffer) and iced’s frame clock.
Patches (owned blit, headless `render_buffer` hijack, mailbox) reduce races
but cannot match native present quality on YouTube-class sites.

Production model (GTK / WPE Wayland / DRM): **platform presents; compositor
holds the buffer; FrameDone after real present.**

---

## 3. Target architecture

### 3.1 Surface topology

```text
┌─ sola-browser  (single Wayland client, one xdg_toplevel) ─────────┐
│  Parent surface — iced (float CSD, tabs, omnibox, sidebar, menus) │
│                                                                   │
│  ┌─ content plane — wl_subsurface (or equivalent child surface) ─┐│
│  │  WebKit frame attached as dma-buf (linux-dmabuf / WPE present) ││
│  │  River composites; iced does NOT sample page pixels            ││
│  └────────────────────────────────────────────────────────────────┘│
└───────────────────────────────────────────────────────────────────┘
```

| Layer | Owner | Draws |
|-------|--------|--------|
| **Chrome plane** | iced / wgpu | Tab strip, omnibox, sidebar, find, vault, CSD chrome |
| **Content plane** | Content presenter (below) | Active tab’s web pixels only |
| **Compositor** | River | Final stack; damage; presentation |

**Hole rule:** In the content scissor, iced **must not** paint opaque web
fallback every frame. Either:

- leave that rect **fully transparent** so the subsurface shows through, or  
- omit fragment writes in that rect (implementation choice; transparent hole
  is the default).

Sidebar / chrome remain iced and **occlude** the content plane where they
overlap (subsurface position + z-order under chrome siblings as needed).

### 3.2 Content presenter (product path)

A small module in `sola-browser` (name freeze: **`ContentPlane`**) that:

1. Owns the **content `wl_surface`** (subsurface of the iced toplevel).
2. Receives completed frames from WPE (buffer + optional fence).
3. **Attaches** the buffer to the content surface (`zwp_linux_dmabuf` or
   WPE platform present path — see §4).
4. **Commits** with damage; waits for **frame callback** / presentation.
5. On present complete: signals **FrameDone** semantics to WebKit
   (`wpe_view_buffer_rendered` / equivalent).
6. On compositor release (or next attach policy): **`wpe_view_buffer_released`**
   (loan/return). **Only** this path releases presented buffers.

Iced’s `WpeProgram` / `blit_to_owned` / `wgpu_import` path is **not** used for
the active tab on the product path.

### 3.3 Process model (unchanged shape)

| Thread / process | Role |
|------------------|------|
| iced main | Chrome UI, bus, geometry of content rect |
| `wpe-engine` GLib | WebKit views, navigation, JS, vault inject |
| WebKit Web/Network/GPU | Unchanged multi-process |
| ContentPlane | Same process as sola-browser; may share GLib or iced wakeups |

**No** separate content OS process in v1 (optional later for crash isolation).

### 3.4 Multi-tab

| Rule | Detail |
|------|--------|
| **One** content plane surface | Not one subsurface per tab |
| Active tab only | Maps / attaches for active WPE view |
| Inactive tabs | `wpe_view_set_visible(false)` (keep); no present |
| Tab switch | Park last buffer optional; prefer immediate first frame of new tab; no full-window sample.clear flash |
| Buffer pool | Still release promptly; do not hold N tabs of dma-bufs |

### 3.5 Input

| Event | Path |
|-------|------|
| Pointer / scroll / keyboard over **content rect** | → WPE (`wpe_view_event`), CSS/layout coords, device scale applied |
| Pointer over **chrome** | → iced only |
| Focus | Content focused when click-in content; chrome controls take focus as today |
| Clipboard | Keep chrome bridges (page copy → iced; paste → `PasteText`) until content plane has a real seat path that works; freeze does **not** require regressing paste |

Coordinates: content plane origin = content scissor top-left in the toplevel;
input transformed into view CSS pixels (same contract as today).

### 3.6 Scale / DPR

| Source | Consumer |
|--------|----------|
| River **output scale** (and iced `scale_factor` once honest) | Content plane buffer size + `wpe_toplevel_scale_changed` |
| CSS size | Content rect in logical px (chrome layout) |
| Physical buffer | `round(css × scale)` — must match attached buffer dimensions |

**Bar:** physical buffer width/height equals CSS × scale (within 1 px). Soft
text from 1× buffer on dense output is a **bug**, not “WebKit fonts.”

If River reports 1.0 on a dense panel, fix is **compositor/output config**
and/or iced scale plumbing — not supersampling hacks in the shader path.

### 3.7 Resize / move / CSD

| Event | Behavior |
|-------|----------|
| Window resize | Content rect from chrome layout → resize content surface + WPE view CSS size + scale |
| Sidebar drag | Same |
| Float CSD move | Subsurface moves with parent (Wayland); no separate shell window |
| Minimize / unmap | Unmap or hide content plane with parent |

---

## 4. WPE integration choice (locked preference + fallback)

### 4.1 Preferred: **custom platform present on our content surface**

Keep producing frames via WPE (headless display **or** thin platform), but
**present** is ours:

```text
WebKit Frame (fence waited by FenceMonitor or explicit sync)
  → ContentPlane attaches dma-buf to content wl_surface
  → commit
  → wl_surface.frame / presentation feedback
  → wpe_view_buffer_rendered  (FrameDone)
  → later buffer_released when safe
```

This matches DRM/Wayland platform semantics without requiring stock
`WPEDisplayWayland` to parent under iced.

Stock headless auto-release **must not** run on the product path (already
hijacked; ContentPlane owns release of presented frames).

### 4.2 Alternate (if preferred blocked): **WPE Wayland display**

Only if ContentPlane can share or parent correctly. Do **not** open a second
unrelated `xdg_toplevel` as the product UX (shell would show two windows).

### 4.3 Explicit non-choice for v1

- Nested libwpe-fdo compositor as long-term design (historical WPE; extra
  complexity). Allowed only as a spike if §4.1 fails.
- CEF OSR into iced.
- River-specific proprietary buffer protocol (prefer standard linux-dmabuf +
  subsurface).

---

## 5. Wayland client / iced coupling (critical constraint)

**Fact:** `wl_subsurface` parent and child must be the **same Wayland client**
(same `wl_display` connection).

Therefore ContentPlane **must** create the content surface on **iced’s**
Wayland connection (or iced must expose parent `wl_surface` + display for the
same connection).

### 5.1 Spike gate (must pass before full implement)

| # | Gate | Pass criteria |
|---|------|----------------|
| G1 | Obtain parent `wl_surface` (or equivalent) for the iced window | Stable handle for browser window lifetime |
| G2 | Create subsurface + position to content rect | Visible hole test (solid color buffer) under chrome |
| G3 | Attach one dma-buf (or SHM) and commit | Pixels visible through chrome hole; no iced sample |
| G4 | Frame callback → release previous buffer | No pool growth; no SEGV |

**If G1 fails** (iced cannot expose parent surface):

| Fallback | Notes |
|----------|--------|
| **F1** | iced/winit contribution or local iced fork to export surface handles |
| **F2** | Content as **sibling** decorationless surface; sola-river or browser
  positions it in lockstep with chrome (worse; only if F1 refused) |
| **F3** | Temporary keep headless→iced with paint-backed FrameDone (not freeze
  success) |

Freeze **success** requires G1–G4 on product path, not F3.

### 5.2 River / sola-river

| Change | v1 requirement |
|--------|----------------|
| Subsurface support | Already in wlroots/River — verify no Sola policy blocks it |
| Output scale honesty | **Required** for crisp text (may be config + iced, not river code) |
| Presentation / display link bus topic | **Optional v1**; content plane may use `wl_surface.frame` only |
| Grouped chrome+content windows | **Not** required if subsurface works |

---

## 6. Lifecycle vs headless import path

| Mode | When | Behavior |
|------|------|----------|
| **Product** | Default after ship | ContentPlane present; no iced web sampling |
| **Fallback** | Env flag e.g. `SOLA_BROWSER_CONTENT=import` | Legacy dma-buf→wgpu→iced (debug) |
| **CI / headless CI** | No Wayland | Import path or skip GPU tests |

Removing import code is **not** required in the first ship slice; gating it
off the default path is.

---

## 7. What this freeze does *not* change

| Locked elsewhere | Still true |
|------------------|------------|
| Engine | WPE only (`pre-cef-removal` stays archive) |
| D7 Bitwarden | First-party vault; fill via JS inject |
| D8 profiles | Paths and single active profile |
| D3 OpenUrl / Helium | Unchanged until browser ship-ready |
| D5 / D6 | Still ask-human |
| Bus menus, float CSD chrome | Stay iced |
| Kit stack for other apps | Unchanged |

---

## 8. Phased delivery

### Phase 0 — Freeze lock + spike (this doc)

- Human locks this freeze (status → **Frozen**).
- Spike G1–G4 in a worktree; write pass/fail into this doc or plan.
- **Stop** if G1 fails without an agreed fallback (F1/F2).

### Phase 1 — ContentPlane MVP (one tab worth of behavior)

- Content surface + hole + attach active-tab frames.
- Input to content rect.
- Scale from compositor.
- Dogfood YouTube **homepage** scroll (see §9).
- Fallback env for import path.

### Phase 2 — Multi-tab + polish

- Tab switch, hide inactive, session restore interaction.
- Resize/sidebar stress; float CSD stress.
- Retire default use of iced sample path; document fallback only.
- Telem: present path counters (attach, frame_cb, released, gap).

### Phase 3 — Optional hardening

- Presentation-time feedback if frame callback is insufficient.
- Separate content process (only if crash isolation demanded).
- Delete dead import path if fallback unused.

---

## 9. Acceptance (dogfood bar)

**Must pass on `https://www.youtube.com/` (root homepage), hard scroll:**

| Check | Pass |
|-------|------|
| Black swaths / full-content black flash | Absent to human eye |
| Top nav / masthead flicker | Absent |
| Scroll | Continuous; no multi-hundred-ms freezes as normal feel |
| Text | Crisp at session output scale (not soft 1× upscale) |
| Telem (product path) | No `sample_clear` for web; release ≈ present; no live pool pin |
| Crash | No SEGV / WPE_IS_BUFFER storms under 30s scroll |

**Compare:** side-by-side optional with import fallback env — product path must
win clearly.

---

## 10. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| iced has no public parent surface API | Spike G1 first; F1 fork/patch iced; F2 sibling only with human OK |
| Subsurface input / focus bugs | Explicit hit-test; seat focus to content surface when needed |
| Z-order: content covers chrome | Subsurface below chrome widgets; clip to content rect only |
| Double scale (CSS×scale wrong) | Single scale pipeline; log css/phys/scale on resize |
| Pool thrash if FrameDone early | FrameDone only after frame callback (or later) |
| NVIDIA modifiers | Prefer formats we can attach; LINEAR acceptable if crisp+smooth |

---

## 11. Implementation map (when unlocked)

| Area | Likely touch |
|------|----------------|
| New | `crates/sola-browser/src/content_plane/` (Wayland surface, attach, frame cb) |
| Chrome | `app.rs` / layout: content rect → plane; transparent hole |
| Engine | `wpe/engine.rs`: hand frames to ContentPlane; no iced mailbox for product |
| Retire default | `wpe/frame.rs` shader sample path behind fallback flag |
| C hijacks | Keep preferred-formats / cursor / no auto-release; align release with plane |
| Docs | architecture.md browser section; capabilities; CURRENT; this freeze status |
| River | Only if spike proves need (scale policy / subsurface quirks) |

Work in **`.worktrees/`** per AGENTS. Install only with standing browser OK.

---

## 12. Documentation updates on ship

Same change as code (progress model):

1. This freeze status → **Frozen** then **Shipped** when Phase 1 dogfood passes.  
2. `docs/architecture.md` — browser surface split.  
3. `docs/capabilities.md` — paint path gap → content plane.  
4. `CURRENT.md` — Now / dogfood.  
5. Optional short plan checklist under `docs/plans/` after freeze lock.

---

## 13. Decisions locked by this freeze

1. Product paint path = **Wayland content plane**, not iced sampling.  
2. Chrome remains **iced**; content is **not** a kit shader.  
3. Engine remains **WPE**; CEF not revived for this.  
4. **One** content surface; active tab only.  
5. FrameDone/release owned by **ContentPlane** present cycle.  
6. Spike **G1–G4** required before full implementation.  
7. Import path demoted to **fallback**, not deleted day one.

---

## 14. Open items (non-blocking vs blocking)

### Non-blocking (resolve during implement)

- Exact crate module names; SHM solid-color spike before dma-buf.
- Whether to use `wp_viewport` / fractional scale later.
- Telem field names.

### Blocking — human before implement (if disagree)

None assumed if this freeze is accepted as written.  
**Only re-open if:** you reject subsurface-in-same-client, want CEF, or want
sibling dual-window as the primary design.

### Locked confirms (2026-08-11)

1. **Spike-first:** yes — G1–G4 / plane path, then cut over.  
2. **Iced surface export:** yes — patch iced/winit in-tree if required.  
3. **Fallback:** `SOLA_BROWSER_CONTENT=plane|import`; default `import` until
   dogfood passes, then `plane`.

---

## 15. One-line summary

**Iced draws chrome; a Wayland content plane presents WPE frames; River
composites; we stop being a mini compositor inside wgpu.**
