# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**macOS look-and-feel — P0 screenshot capture**

- **Roadmap:** `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md`
- **Execute plan:** `docs/specs/2026-07-20-screenshot-capture-plan.md`
- **Next phase:** P0 Tasks 1–6 (river wlr-screencopy + solactl docs + shell
  Super+Shift+3/4). Stop for user install + smoke PNGs before P1/P2.
- **Constraints:** worktree only; build only (no install without permission);
  do **not** retune theme tokens or shell chrome in this phase.

### After P0 smoke (do not auto-start until user says go)

1. **P1** — visual-state convention + baseline shots (`docs/visual/` or
   `/tmp/sola/visual/`) — see roadmap §4 P1  
2. **P2** — token / grey baseline toward macOS dark mode  
3. **P3+** — menubar → menus → launcher → switcher → kit controls  

### Last completed

**sola-kit hardening** (A + B + C1) — merged to `master` at `cd3d2f1`
(2026-07-20). Smoke-tested. Plan:
`docs/specs/2026-07-19-sola-kit-hardening-plan.md`.

Deferred (not auto-start): C2 notify-fd, Phase D/E.
