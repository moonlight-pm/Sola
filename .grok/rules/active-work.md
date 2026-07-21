# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**P3** menubar density + type — **code ready**, visual stop.
Branch `p3-menubar-density` @ `160bda9`
(worktree `.worktrees/p3-menubar-density`).
See `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` §4 P3.

Signature move: compact menubar type (13 chrome / 12 mono values),
font roles (`chrome` / `ui_medium` / `mono`), theme-derived fg (no
view hex), tighter item pad + status cluster spacing. Bar height
stays 28 (zoning).

**User:** install shell, recapture menubar idle, critique.

Next when user says go after P3 merges: **P4** menus & popovers.

### Last completed

**P2 token & type baseline** — merged to `master` at `4ed660c`
(2026-07-20). macOS greys seed, quiet selection, neutral switcher,
keep cyan; mono **Iosevka Term Slab**; UI prefers **SF Pro Text**
(fallback Inter); `.local/fonts/` stash (gitignored binaries + README).

**P1 visual baselines + Bgr888 fix** — merged earlier same day.

**P0 screenshot capture** — merged earlier.
