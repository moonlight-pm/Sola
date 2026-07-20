# macOS look-and-feel — roadmap

**Date:** 2026-07-20  
**Status:** agreed direction; execute phase-by-phase with visual stops  
**North star:** `docs/manual/design-language.md`  
**Immediate plan:** `docs/specs/2026-07-20-screenshot-capture-plan.md` (P0)

---

## 1. Goal

Make Sola read as **macOS Dark Mode chrome** — materials hierarchy, density,
control calm, sparse accent — while keeping Sola’s intentional departures:

| Keep | Do not invent |
|------|----------------|
| Tiling / zoning as primary WM | A “unique Sola brand look” beyond documented departures |
| No title bars on zoned windows | Purple gradients, oversized radii, hero chrome |
| Tunable tokens / presets | Hard-coded hex/spacing snowflakes in shell views |

If a visual choice is not listed as a departure in the design language §1,
**prefer the macOS answer**.

---

## 2. Where we are

### Infrastructure (good)

- Bus theme protocol (`Topic::Theme`, palette tokens, presets)
- Kit atoms → iced `Extended` palette (`crates/sola-kit/src/theme.rs`)
- Shell tokens (`shell-*`) + storybook Shell page
- Font roles (`fonts::ui` / `chrome` / `mono`)
- Design language + suggested redesign order (design language §9)

### Visual defaults (early / wrong target)

Current seed is still **GitHub Primer dark + cyan**, not Apple dark greys:

| Role | Today (approx) | Direction |
|------|----------------|-----------|
| Canvas / raised | `#0d1117` / `#161b22` | Cooler system greys, elevation by step |
| Accent | `#00d4ff` | Keep tunable; use **sparsely** |
| Selection | `#1f6feb` | Quieter accent-tinted selection |
| Menubar | solid `#000` | Translucent material values (opaque greys first if blur not ready) |
| Switcher glass | cyan tint `#00d4ff2e` | Neutral material, not neon |

### Tooling gap (blocker for iteration)

- `solactl screenshot` CLI + bus topics exist
- `sola-river` capture body is a **stub** → always errors
- No shell Super+Shift+3/4 chords
- No baseline image set for compare loops

**Agents cannot usefully polish chrome without PNGs they can open.**  
Screenshot is therefore P0 — before any token/menubar pass.

---

## 3. Working model

Casual progress with **stops for visual inspection**:

1. Capture baseline (or previous pass) for the surface under change  
2. Implement **one signature move** in a worktree  
3. `cargo make build` only — user installs and runs from TTY  
4. Capture after shots  
5. Critique against design language + (optional) macOS reference  
6. Keep, adjust, or revert; then next pass  

Never claim a visual pass is done without before/after paths an agent can open.

### Pass sizing (from design language §6.4)

One signature move per pass. Examples:

| This pass | Out of scope for same pass |
|-----------|----------------------------|
| Screenshot capture | Token palette retune |
| Token baseline greys | Menubar layout rewrite |
| Menubar density + type | Launcher redesign |
| Launcher card hierarchy | New font families |
| Kit button/field density | Shell layout rewrites |

---

## 4. Phased plan

### P0 — Screenshot capture (execute first)

**Plan:** `docs/specs/2026-07-20-screenshot-capture-plan.md`

| Deliverable | Owner |
|-------------|--------|
| `wlr-screencopy` full-output PNG in sola-river | river |
| Window-region capture via `CaptureTarget::Window` | river |
| `solactl screenshot` works end-to-end | already CLI-ready |
| Shell `Super+Shift+3` full / `Super+Shift+4` focused window | shell |
| Optional toast with path / error | shell |

**Stop:** User installs river (+ shell if hotkeys). Agent runs or user runs:

```bash
solactl screenshot -o /tmp/sola/screenshots/smoke-full.png
```

Agent opens the PNG and confirms desktop chrome is visible.

### P1 — Visual-state convention + baseline set

Lightweight, mostly process + paths (not a large product feature).

**Status (2026-07-20):** convention landed under `docs/visual/`; baseline
`01`–`04` captured on first output (5120×2160). `05-storybook-theme`
deferred (sola-kit was blank / region geometry unusable during capture).

```
docs/visual/
  README.md                 # how to capture states; chord cheat sheet
  baseline/
    01-menubar-idle.png
    02-menu-open.png
    03-launcher.png
    04-switcher.png
    05-storybook-theme.png  # optional if sola-kit running
  passes/<pass-id>/
    ...
```

**Known chords today** (shell):

| Chord | Surface |
|-------|---------|
| `Meta+Space` | Launcher |
| `Meta+Tab` | Switcher |
| `Meta+\`` | Cycle windows of focused app |
| App menus | Click menubar labels / open via UI |

Automation sketch (after P0):

```bash
# idle
solactl screenshot -o docs/visual/baseline/01-menubar-idle.png

# launcher
solactl key "Meta+Space"
sleep 0.3
solactl screenshot -o docs/visual/baseline/03-launcher.png
solactl key Escape

# switcher (hold-style UX may need Meta+Tab + delay; adjust if needed)
solactl key "Meta+Tab"
sleep 0.3
solactl screenshot -o docs/visual/baseline/04-switcher.png
```

**Stop:** Agree “this is current Sola” baseline. Commit baselines only if the user wants them in-repo (PNGs can stay under `/tmp/sola/visual/` if preferred).

### P2 — Token & type baseline

Align kit / seed greys and hierarchy with macOS dark mode **without** a layout rewrite.

- Retune `hex::*` / `Palette::seed` surfaces, borders, text hierarchy  
- Decide accent identity: keep cyan as sparse signal **or** move default accent toward system blue (product call at start of P2)  
- Quiet selection atom; stop painting switcher with neon cyan glass in **defaults**  
- Font roles: ensure SF Pro path documented; seed family names match installed reality  
- Storybook Theme page is the regression surface  

**Stop:** Full-output + storybook screenshots before/after. Keep accent change only if sparse usage still holds.

### P3 — Menubar

Density, type, status items, quiet scanability. Tokens only; no new hex.

### P4 — Menus & popovers

Spacing, separators, materials values, kit popover calm.

### P5 — Launcher

Spotlight-like restraint (single focus, dim backdrop, list hierarchy).

### P6 — Switcher

Mission Control / app-switcher restraint; neutral backplate tokens.

### P7 — Kit controls

Buttons, fields, sidebar, cards via storybook; small radii, quiet hover.

### P8 — Kit apps inherit

Settings / monitor / terminal / agent / browser chrome — inherit tokens; no per-app themes.

### Deferred (not auto-start)

| Item | Why deferred |
|------|----------------|
| Real compositor blur / vibrancy | Needs materials research; opaque greys first |
| Multi-output screenshot picker | V1 first output is enough |
| `solactl theme` dump CLI | Nice-to-have; sticky Theme + presets already readable on disk |
| Storybook deep-link by page | Later for kit-only passes |
| Region capture of menubar strip only | Crop manually or add later |
| Automated pixel-diff CI | Human/agent visual critique is enough for now |

---

## 5. What helps agents (LLM) iterate

| Need | Status after P0 | Notes |
|------|-----------------|-------|
| Full-output PNG path on stdout | Required | Primary feedback channel |
| Deterministic `-o path` | Already on solactl | Always use named paths in passes |
| Window-region capture | P0 | Isolate shell / storybook when possible |
| Shell hotkeys | P0b | Human captures without CLI |
| State automation via `solactl key` | Exists | Document chords in `docs/visual/README.md` |
| Image open in agent (`read_file` on PNG) | Exists | Multimodal critique |
| Theme hex dump | Optional later | `~/.config/sola/theme/presets/*.yaml` works short-term |
| Design language + this roadmap | Exists | Constraints for every visual PR |

Agents must not “improve taste” without screenshots. When stuck, ask for captures rather than guessing hex.

---

## 6. Architecture rules for all visual work

1. **Tokens first** — kit atoms / bus palette / shell tokens before view snowflakes  
2. **Kit components** over one-off shell widgets  
3. **Storybook** for any kit style change  
4. **Worktrees only** for code; no direct master commits without permission  
5. **Build only** — never `cargo make install` without explicit user permission each time  
6. **One surface per pass** unless the user expands scope  
7. New intentional departures from macOS → write into design language §1  

---

## 7. Decision log (this initiative)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| North star | macOS Dark Mode via design language | Already product intent |
| Start with | Screenshot (P0), not greys | Cannot iterate blindly |
| Capture owner | `sola-river` via `wlr-screencopy` | Already Wayland client; self-contained; no grim dep |
| CLI surface | Existing `solactl screenshot` | Already wired; LLM-first JSON/path stdout |
| Human surface | Shell Super+Shift+3/4 → same bus topic | macOS muscle memory; no second capture path |
| grim | Not required | Prefer hand-rolled screencopy (documented in river stub) |
| Visual process | Casual stops + baselines | User preference |

---

## 8. Session handoff

**New session should:**

1. Read this roadmap + design language  
2. Execute **Current** in `.grok/rules/active-work.md` (starts at P0)  
3. Use `docs/specs/2026-07-20-screenshot-capture-plan.md`  
4. Work in `.worktrees/` only  
5. Build, not install  
6. After P0 merges / is smoke-ready, stop for user install + first PNGs  
7. Only then open P1/P2  

Do **not** re-audit theme system or redesign tokens in the same pass as screenshot capture.
