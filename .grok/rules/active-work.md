# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**agent-session-perf** (branch `fix/agent-session-perf`)

### Goal

Instant sidebar session switch: selection + content on click, not after
ACP `session/load`.

### Done

- Optimistic `session_id` + `history_tail` paint on `SelectSession`
- Older history / auto-fill on UI thread (not blocked by worker attach)
- Worker coalesces rapid `LoadSession` / `NewSession`
- Transcript/HistoryOlder carry `session_id`; stale events dropped
- `acp_attached` gate so prior-session stream events cannot paint into
  the optimistic transcript

### Next

- Smoke in UI (`cargo make install sola-agent` from worktree when ready)
- Merge when approved

### Resume

```text
cd /home/joshua/Workspace/Sola/.worktrees/agent-session-perf
# install only with user permission:
# cargo make install sola-agent
```

### Last completed (prior)

**focus-follows-mouse → master**: focus without raise (raise on click);
200ms dwell; pointer resync after map; float drag clamps under menubar;
full-height menubar hits (macOS idle chrome); edge/corner float resize
(geometry rim, col/row cursors, square corner pads).

**agent-ui-fixes → master**: markdown tables; Enter submit / Shift+Enter
newline + growing composer; always-approve auto-answer + effort/mode
order fix; approval strip redesign; Edit menu cut/copy/paste/select-all.

### Future / follow-ups

- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Further Grok TUI presentation parity
- Storybook page parity for non-Overview tabs (on demand)
- Opt other kit apps into titlebar / floating_frame + resize
- Remaining worktrees: `libei-portal`
