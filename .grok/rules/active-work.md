# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**none**

### Last completed (prior)

**app-icon-raster → master**: full-color app icons (path/PNG refs) in launcher + switcher; case-insensitive catalog lookup for Wayland app_id mismatches (e.g. orca).

**session-id-routing → master**: live ACP stream keyed by session UUID
(not title); OD session cards (graphite select, slim context bar, hover
× + time rail); kit `SidebarItem` card chrome + custom content; toolbar
RESET deletes open session and starts fresh.

**sidebar-hover-trash → master**: stable hover trash (stack overlay +
enter-only hover); no hard-crop GFM table cells; live thinking stream
→ "Thought for N sec"; directory-first session tabs with context KB
from disk `usage_update`.

**float-shadows → master**: WM drop shadows via `get_decoration_below`;
filled soft silhouette (no bottom bleed); half-size cast; PEAK_ALPHA 0.28.

**agent-session-perf → master**: optimistic session switch + transcript
cache; full-row sidebar hit targets / `item_spacing`; no
`session/cancel` on tab switch (shared leader + TUI); cursor alias fix
in sola-make.

**focus-follows-mouse → master**: focus without raise (raise on click);
200ms dwell; pointer resync after map; float drag clamps under menubar;
full-height menubar hits (macOS idle chrome); edge/corner float resize
(geometry rim, col/row cursors, square corner pads).

**agent-ui-fixes → master**: markdown tables; Enter submit / Shift+Enter
newline + growing composer; always-approve auto-answer + effort/mode
order fix; approval strip redesign; Edit menu cut/copy/paste/select-all.

**agent-leader → master**: leader-only attach; console group removed.

### Future / follow-ups

- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Further Grok TUI presentation parity
- Storybook page parity for non-Overview tabs (on demand)
- Remaining worktrees: `libei-portal`, `app-icon-raster`

### Resume

```text
# no active feature worktree
```
