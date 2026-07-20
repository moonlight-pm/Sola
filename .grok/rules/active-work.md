# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**none** (P0 screenshot capture merged to `master`)

Next when user says go: **P1** visual-state convention + baseline shots —
see `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` §4 P1.

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
