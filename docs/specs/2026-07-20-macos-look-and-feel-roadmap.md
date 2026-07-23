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

### Visual defaults

**P2 (2026-07-20):** seed retuned toward macOS Dark Mode greys. Accent stays
cyan (sparse). Persistent user presets on disk may still carry older Primer
values until re-saved / reset.

| Role | Seed (P2) | Notes |
|------|-----------|--------|
| Canvas / raised | `#1c1c1e` / `#2c2c2e` | system grey ladder |
| Hover / tertiary | `#3a3a3c` | elevation step |
| Text | `#f5f5f7` / `#98989d` | primary / muted |
| Accent | `#00d4ff` | keep; use sparsely |
| Selection | `#1a3a45` | quiet accent-tinted |
| Menubar | solid `#000` | opaque greys first; blur deferred |
| Switcher glass | `#1c1c1ee6` + soft icon plate | Cmd+Tab HUD pill, not neon cyan |

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
`01`–`05` recaptured after `Bgr888` channel-order fix (slate/cyan correct).

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

- [x] Retune `hex::*` / `Palette::seed` surfaces, borders, text hierarchy  
- [x] Accent identity: **keep cyan** as sparse signal (design language §2.1)  
- [x] Quiet selection atom; neutral switcher glass defaults  
- [x] Font seed stays Inter + JetBrains Mono (installed); SF Pro optional when user-placed  
- Storybook Theme page is the regression surface  

**Stop:** Full-output + storybook screenshots before/after.

### P3 — Menubar

Density, type, status items, quiet scanability. Tokens only; no new hex.

**Status (merged):**

- [x] One chrome face for labels, stats, clock (size **13**); app name **bold**
- [x] Stats values **not** mono — same type as menu titles (user correction)
- [x] Pad `[2, 9]`; status cluster spacing **4**; label↔value gap **5**
- [x] Theme `palette().text` (no view hex); flower **14**; bar height **28**
- [x] Fixed-width stat value slots (no reflow as digits change)

### P4 — Menus & popovers

Spacing, separators, materials values, kit popover calm.

**Status (merged):**

- [x] Kit `popover`: `RADIUS_MD`, pad `SPACE_SM`, tighter shadow (blur 10 / y 2)
- [x] Kit `button::menu_item` compact hover for shell menus
- [x] Shell menu: chrome type 13, denser row pad, separator vertical pad

### P5 — Launcher

Spotlight-like restraint (single focus, dim backdrop, list hierarchy).

**Status (merged):**

- [x] Quiet list selection (`selection` atom, not accent pill)
- [x] Calmer modal chrome (RADIUS_LG, softer shadow)
- [x] Denser query/rows; default width 560 / pad 8
- [ ] User install + after shot (`docs/visual/passes/p5-launcher/`)

### P6 — Switcher

Cmd+Tab HUD (not Mission Control grid): horizontal icon strip, selected-only
caption, soft plate under icon, frosted pill backplate.

**Status (merged at `4b49dec`):**

- [x] Layout: single horizontal icon strip (no wrapping label grid)
- [x] Large icons (72px); selected app name as one caption under the strip
- [x] Soft light selection plate (`#ffffff2e`), not cyan / teal fill
- [x] HUD pill material (`#1c1c1ee6`, `RADIUS_XL`); pad 14 / tile-pad 8
- [x] Caption inside the pill (legible on light wallpapers; follow-up after
  outside-white-on-white regression)

### P7 — Kit controls — **done**

Buttons, fields, sidebar, cards via storybook; small radii, quiet hover.

**Plan:** `docs/specs/2026-07-21-p7-kit-controls-plan.md`

| Pass | Signature move | Status |
|------|----------------|--------|
| A | Fix `overlay` / `menubar` Extended binding | done (`f8fb072`) |
| B | Type + control density (body 13, named pads) | done (`6dae05a`) |
| C | Quiet ghost / unify text selection atom | done (`6cceda8`) |
| D | Form row + field error + checkbox/toggle styles | done (`a911207`) |
| E | Badge / sidebar headers / storybook matrix | done (`f064642`) |
| F | Docs + handoff to P8 | done |

**Outcome:** Shell overlay/menubar keep sola Extended atoms; kit body 13 +
named control pads; ghost hover grey-only; text selection uses selection
atom; `form_row` / field error / checkbox+toggle styles; quiet Neutral
badges, `card::plain`, calmer sidebar headers; storybook matrix complete.
Form primitives unlock P8 settings without per-app snowflakes.

### P8 — Kit apps inherit — **done**

Settings / monitor / terminal / agent / browser chrome — inherit tokens; no per-app themes.

Use kit helpers (`button::labeled`, `field` / `form_row`, `card::plain`,
shared type roles) rather than local hex, pads, or one-off styles.

**Plan:** `docs/specs/2026-07-23-p8-kit-apps-inherit-plan.md`

| Pass | Signature move | Status |
|------|----------------|--------|
| A | Settings inherits labeled + type roles + SPACE | done |
| B | Monitor selection atom + type roles | done |
| C | Agent chrome density | done |
| D | Terminal chrome only (selection atom wash) | done |
| E | Browser chrome | done |
| F | Docs + closeout | done |

**Outcome:** Kit apps consume `labeled` / type roles / `SPACE_*` / selection
atom for chrome. JSON syntax colors in monitor and ANSI grid hues remain
domain-owned.

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

**Status:** P0–P8 complete (macOS L&F chrome/control inheritance closed).

**New session should:**

1. Read this roadmap + design language  
2. Execute **Current** in `.grok/rules/active-work.md` (or ask if `none`)  
3. Prefer storybook + shell screenshots for visual stops  
4. Work in `.worktrees/` only  
5. Build (`cargo make build`), not install unless user permits  
6. Do not re-open P7 kit primitives unless a real binding/density bug surfaces  
7. Do not re-open P8 app inheritance unless a real chrome snowflake surfaces  

P0 screenshot tooling, P7 kit plan, and P8 inherit plan remain reference docs.
