# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**P2 token & type baseline** — branch `feat/token-type-baseline`
(worktree `.worktrees/token-baseline`). Code + build done; needs install +
after screenshots.

Delivered in branch:

- `hex::*` + `Palette::seed` → macOS system greys
- Keep cyan accent; quiet selection `#1a3a45`
- Neutral switcher glass (not cyan tint)
- Font seed stays Inter + JetBrains Mono; SF Pro docs note
- Pass dir `docs/visual/passes/p2-token-greys/` with before shots + notes

**Blocked on user:**

1. `cargo make install` from worktree (or kit + shell + core consumers)
2. Reset sticky theme if needed: storybook Default, or remove
   `~/.config/sola/theme/current.yaml` and restart
3. Agent captures after shots into the pass dir

**Stop after after-shots:** critique vs design language; merge if keep.

Next after merge: **P3** menubar density.

### Last completed

**P1 visual baselines + Bgr888 fix** — merged to `master` at `d483d66`
(2026-07-20). Branch `docs/visual-baseline`.

**P0 screenshot capture** — merged earlier same day.
