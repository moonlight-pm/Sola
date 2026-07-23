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

Next when user says go: ask — macOS L&F roadmap P0–P8 is complete. Deferred
items (blur/vibrancy, multi-output screenshot picker, theme dump CLI, etc.)
live in the roadmap deferred table; pick one explicitly.

Parent roadmap: `docs/specs/2026-07-20-macos-look-and-feel-roadmap.md`.
P8 closed: `docs/specs/2026-07-23-p8-kit-apps-inherit-plan.md` (passes A–F).

### Last completed

**P8 Pass F** — docs + closeout — roadmap P8 done; design-language redesign
order item 7 marked done; active-work → none.

**P8 Pass E** — browser chrome — URL bar body density + kit DEFAULT_PADDING;
nav bar `SPACE_*`.

**P8 Pass D** — terminal chrome — selection wash from selection atom; kit
type roles on empty-pane chrome (sidebar already kit `SidebarPanel`).

**P8 Pass C** — agent chrome — `button::labeled`, type roles, kit text_input
style, `SPACE_*` / `RADIUS_*` pads.

**P8 Pass B** — monitor — selected rows use `theme::selection()`; body/code
sizes; `SPACE_*` gaps; JSON syntax hex kept domain-owned.

**P8 Pass A** — settings inherits kit density — merged path on branch
`p8a-settings-inherit` (awaiting master merge after visual test).

**P7 Pass F** — docs + handoff to P8 — closed P7 kit controls.

**P7 Pass E** — surfaces + storybook completeness — merged to `master` at
`f064642` (2026-07-23).

**P7 Pass D** — form primitives — merged to `master` at `a911207`
(2026-07-22).

**P7 Pass C** — quiet interactions — merged to `master` at `6cceda8`
(2026-07-21).

**P7 Pass B** — type + control density — merged to `master` at `6dae05a`
(2026-07-21).

**P7 Pass A** — theme binding — merged to `master` at `f8fb072`
(2026-07-21).

**P6 switcher** — merged to `master` at `4b49dec`
(2026-07-21).

**P5 launcher** — merged to `master` at `a2b2e7f`
(2026-07-20).

**P4 menus & popovers** — merged to `master` at `cddffae`.

**P3 menubar density + type** — merged to `master` at `a2cc87a`.

**P2 token & type baseline** — merged to `master` at `4ed660c`.

**P1 visual baselines + Bgr888 fix** — merged earlier.

**P0 screenshot capture** — merged earlier.
