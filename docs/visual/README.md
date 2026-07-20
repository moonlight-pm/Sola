# Visual baselines

Process convention for macOS look-and-feel work (roadmap P1+).

Agents and humans capture PNGs of known shell/kit surfaces, then critique
before/after against `docs/manual/design-language.md`. **Never claim a visual
pass is done without paths an agent can open.**

Roadmap: `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md`.

---

## Layout

```
docs/visual/
  README.md                 # this file
  baseline/                 # “current Sola” reference set
    01-menubar-idle.png
    02-menu-open.png
    03-launcher.png
    04-switcher.png
    05-storybook-theme.png  # optional — add when kit is tile-visible
  passes/<pass-id>/         # before/after for a single signature move
    before-*.png
    after-*.png
    notes.md                # optional: intent + critique
```

### Current baseline set (2026-07-20)

Recaptured after the `Bgr888` R↔B fix in `sola-river` (cyan `#00d4ff`,
slate `#0d1117` / `#161b22` — not yellow/brown).

| File | Status | Notes |
|------|--------|--------|
| `01-menubar-idle.png` | present | Full output; menubar, no overlays |
| `02-menu-open.png` | present | System menu open (`solactl click 280 16` on this layout) |
| `03-launcher.png` | present | `Meta+Space` launcher; cyan selection row |
| `04-switcher.png` | present | `Meta+Tab` app icons; cyan selected tile |
| `05-storybook-theme.png` | present | Region capture `-a sola-kit` Theme page |

Agree “this is current Sola” before treating these as the compare root for P2+.

### Where PNGs live

- **In-repo** (`docs/visual/…`) when you want agents and git history to share
  the same set. Prefer this for agreed baselines and pass artifacts that
  document a decision.
- **Ephemeral** (`/tmp/sola/visual/…`) for throwaway probes. Same filenames are
  fine; copy into `docs/visual/` only when worth keeping.

Large PNGs (~0.5–5 MiB each at 5120×2160) are intentional. Do not compress
away chrome detail.

---

## Prerequisites

1. Sola session running from a TTY (`sola` process manager up).
2. Installed `solactl` with working screenshot (P0):  
   `solactl screenshot -o /tmp/sola/screenshots/smoke.png`
3. Shell chrome available (menubar). Launcher/switcher/menu shots need shell
   key chords and virtual pointer as listed below.
4. Storybook shot needs `sola-kit` running (session may already launch it).

---

## Chord / input cheat sheet

| Chord / action | Surface |
|----------------|---------|
| *(idle)* | Menubar only — dismiss overlays with `Escape` first |
| Click menubar label / flower | App or system menu open |
| Click menubar clock | Calendar panel (not a baseline slot; useful probe) |
| `Meta+Space` | Launcher toggle |
| `Meta+Tab` | Switcher (hold-style; capture while open) |
| `Meta+\`` | Cycle windows of focused app |
| `Super+Shift+3` | Full-output screenshot (shell hotkey) |
| `Super+Shift+4` | Focused-window region screenshot |
| `Escape` | Dismiss launcher / switcher / menu / panels |

`solactl` helpers:

| Command | Use |
|---------|-----|
| `solactl screenshot -o PATH` | Full first-output PNG |
| `solactl screenshot -o PATH -a APP_ID` | Window-region crop (screen content at that rect) |
| `solactl key "Meta+Space"` | Synthesize a key chord |
| `solactl click X Y` | Absolute output coordinates (physical pixels) |
| `solactl apps` | Running apps / window titles |

Menubar height is **28 logical px** at the **top** of the output
(`crates/sola-shell/src/menubar/mod.rs`). On HiDPI outputs, click `Y` in the
top strip (~0–40 physical px); flower is the leftmost control, then focused
app title, then menu labels.

---

## Capture: baseline set

From a quiet desktop (user not mid-typing). Prefer a worktree checkout path
or `/tmp/sola/visual/baseline/`.

```bash
BASE=docs/visual/baseline   # or /tmp/sola/visual/baseline
mkdir -p "$BASE" docs/visual/passes

# dismiss overlays
for _ in 1 2 3; do solactl key Escape; sleep 0.1; done
sleep 0.3

# 01 — menubar idle
solactl screenshot -o "$BASE/01-menubar-idle.png"

# 02 — menu open (click flower or an app menu label)
# Absolute output pixels. On a 5120×2160 first-output, menubar Y ≈ 12–24;
# app labels start after the flower (~x 80–300 depending on focused app).
# Verified once: `solactl click 200 20` opened Shell → Table on that layout.
solactl click 200 20
sleep 0.5
solactl screenshot -o "$BASE/02-menu-open.png"
solactl key Escape
sleep 0.3

# 03 — launcher
solactl key "Meta+Space"
sleep 0.4
solactl screenshot -o "$BASE/03-launcher.png"
solactl key Escape
sleep 0.3

# 04 — switcher
solactl key "Meta+Tab"
sleep 0.4
solactl screenshot -o "$BASE/04-switcher.png"
solactl key Escape
sleep 0.3

# 05 — storybook Theme page (optional)
# Prefer focus first so the window is raised (region capture is screen content
# at the registered rect — overlaps show whatever is on top):
#   solactl emit Focus '{"window_id":<id>}'   # id from `solactl apps`
#   solactl screenshot -o "$BASE/05-storybook-theme.png" -a sola-kit
# If the kit window is blank, off-screen, or geometry is stale, skip 05 or
# capture full-output after focusing and crop by hand.
```

### Verify each PNG

Open the file (agent: image read tool; human: image viewer). Confirm:

| File | Must show |
|------|-----------|
| `01-menubar-idle` | Full desktop + menubar; no launcher/switcher/menu overlay |
| `02-menu-open` | Dropdown menu under menubar (system or app) |
| `03-launcher` | Centered launcher list / search |
| `04-switcher` | App icons strip (Mission Control–style switcher) |
| `05-storybook-theme` | sola-kit storybook Theme page chrome |

If a chord fires but the shot looks idle, re-run with a longer `sleep` (0.5–0.8s)
before `screenshot`. Switcher is especially timing-sensitive.

---

## Capture: a visual pass

One signature move per pass (design language §6.4). Example pass id:
`p2-token-greys`.

```bash
PASS=docs/visual/passes/p2-token-greys
mkdir -p "$PASS"

# before (from current baseline or fresh capture)
cp docs/visual/baseline/01-menubar-idle.png "$PASS/before-01-menubar-idle.png"
# … or re-capture the surfaces you will change …

# implement + user installs + restarts as needed …

# after
solactl screenshot -o "$PASS/after-01-menubar-idle.png"
# … matching surfaces …
```

Optional `notes.md` in the pass dir: what changed, keep/adjust/revert, links to
commits.

**Stop criteria:** before/after paths exist; critique against design language +
(optional) macOS reference; decision recorded. Then next pass.

---

## Naming

- Baseline files: `NN-surface.png` (two-digit order, kebab surface name).
- Pass dirs: `pN-short-slug` or `YYYY-MM-DD-short-slug`.
- Pass files: `before-<same-as-baseline>` / `after-<same-as-baseline>`.

---

## Out of scope (for now)

| Item | Notes |
|------|--------|
| Automated pixel-diff CI | Human/agent visual critique only |
| Multi-output picker | V1 captures first output |
| Region crop of menubar strip only | Full-output + manual crop later |
| Committing every probe PNG | Only agreed baselines and meaningful pass shots |

---

## Related

- Design language: `docs/manual/design-language.md`
- Screenshot plan (P0): `docs/specs/2026-07-20-screenshot-capture-plan.md`
- Look-and-feel roadmap: `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md`
