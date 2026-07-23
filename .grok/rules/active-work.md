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

Next when user says go: **P8 — Kit apps inherit** —
settings / monitor / terminal / agent / browser chrome consume kit tokens
and helpers (`button::labeled`, `field` / `form_row`, type roles, quiet
selection) with no per-app themes.

Parent roadmap: `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md` §4 P8.
P7 closed: `docs/specs/2026-07-21-p7-kit-controls-plan.md` (passes A–F).

### Last completed

**P7 Pass F** — docs + handoff to P8 — closed P7 kit controls (roadmap
checklist, design-language already aligned, text_input fork strategy
documented, AGENTS fonts path corrected).

**P7 Pass E** — surfaces + storybook completeness — merged to `master` at
`f064642` (2026-07-23). Quiet Neutral badges; `card::plain` / `style_plain`;
sidebar section headers title case + muted chrome; storybook matrix.

**P7 Pass D** — form primitives — merged to `master` at `a911207`
(2026-07-22). `form_row`, field error caption, `checkbox_style` /
`toggle_style`; storybook Field + Form pages; settings call sites updated.

**P7 Pass C** — quiet interactions — merged to `master` at `6cceda8`
(2026-07-21). Ghost hover grey-only; text selection uses selection atom;
storybook Button covers danger_outline + confirm + flower menubar.

**P7 Pass B** — type + control density — merged to `master` at `6dae05a`
(2026-07-21). Body 13, named PAD_CONTROL pads, field labels body+muted,
button::labeled / labeled_sm.

**P7 Pass A** — theme binding — merged to `master` at `f8fb072`
(2026-07-21). `overlay` / `menubar` preserve sola Extended atom map
instead of iced `Extended::generate`.

**P6 switcher** — merged to `master` at `4b49dec`
(2026-07-21). Cmd+Tab HUD: horizontal large-icon strip, selected-only
caption, soft light selection plate, frosted pill backplate. Also multi-app
`cargo make install`.

**P5 launcher** — merged to `master` at `a2b2e7f`
(2026-07-20). Spotlight restraint: quiet selection, calmer modal, denser
query/list hierarchy, default width 560 / pad 8.

**P4 menus & popovers** — merged to `master` at `cddffae`.

**Stat menubar width pin** — merged earlier same day.

**P3 menubar density + type** — merged to `master` at `a2cc87a`.

**P2 token & type baseline** — merged to `master` at `4ed660c`.

**P1 visual baselines + Bgr888 fix** — merged earlier.

**P0 screenshot capture** — merged earlier.
