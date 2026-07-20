# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**P1 visual-state convention + baseline** — branch `docs/visual-baseline`
(worktree `.worktrees/visual-baseline`).

Delivered:

- `docs/visual/README.md` — capture convention, chords, pass layout
- `docs/visual/baseline/01`–`04` PNGs (full-output chrome states; **colors
  wrong** — Bgr888 R↔B swap, fixed in `sola-river` commit on this branch)
- Fix: `pixels_to_rgba8` Bgr888 LE memory is R,G,B (not B,G,R)
- `05-storybook-theme` deferred (kit blank / bad region during capture)

**Blocked on user:** `cargo make install sola-river` from this worktree
(or equivalent), then agent re-captures `docs/visual/baseline/01`–`04`
with correct slate/cyan colors.

**Stop after recapture:** user agrees “this is current Sola” baseline,
then merge branch and cleanup worktree. Next: **P2** token & type baseline.

### Last completed

**P0 screenshot capture** — merged to `master` (2026-07-20). Branch
`feat/screenshot-capture` (`216340b` + encode fix `fdd5c58`). Plan:
`docs/specs/2026-07-20-screenshot-capture-plan.md`.

Smoke-tested: `solactl screenshot` full-output PNG; Super+Shift freeze
fixed by off-thread encode.

**sola-kit hardening** (A + B + C1) — merged to `master` at `cd3d2f1`
(2026-07-20). Smoke-tested. Plan:
`docs/specs/2026-07-19-sola-kit-hardening-plan.md`.

Deferred (not auto-start): C2 notify-fd, Phase D/E.
