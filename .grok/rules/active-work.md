# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**none**

Next when user says go: **P4** menus & popovers —
see `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` §4 P4.

### Last completed

**Stat menubar width pin** — merged to `master` (2026-07-20). Fixed-width
value slots on CPU/GPU/MEM/RX/TX so proportional chrome type doesn't reflow
the right cluster as numbers update.

**P3 menubar density + type** — merged to `master` at `a2cc87a`
(2026-07-20). Compact menubar chrome type, bold app title, flower optical
lift, Restart Shell, theme-derived fg, tighter pad/cluster spacing.

**P2 token & type baseline** — merged to `master` at `4ed660c`
(2026-07-20). macOS greys seed, quiet selection, neutral switcher,
keep cyan; mono **Iosevka Term Slab**; UI prefers **SF Pro Text**
(fallback Inter); `.local/fonts/` stash (gitignored binaries + README).

**P1 visual baselines + Bgr888 fix** — merged earlier same day.

**P0 screenshot capture** — merged earlier.
