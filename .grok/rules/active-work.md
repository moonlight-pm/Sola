# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**float-shadows** (worktree `.worktrees/float-shadows`, branch
`feature/float-shadows`) — compositor/WM drop shadows for floating
windows via River `get_decoration_below`.

### Status

Implemented in `sola-river`:
- `client/shadow.rs` — SHM soft rounded-rect silhouette, empty input region
- attach/rebuild on float + size change; tear-down on unfloat/fullscreen/close
- `render_start` path: `set_offset` + `sync_next_commit` when buffer is new
- binds `wl_compositor`; reuses existing `wl_shm`

### Next

- User smoke: float Monitor / foreign app; confirm soft shadow, no click steal
- Tune MARGIN / BLUR / PEAK_ALPHA / OFFSET_Y if needed
- `cargo make install sola-river` when ready (ask first)

### Last completed (prior)

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
- Opt other kit apps into titlebar / floating_frame + resize
- Remaining worktrees: `libei-portal`

### Resume

```text
cd .worktrees/float-shadows
# after approval + install permission:
# cargo make install sola-river
```
