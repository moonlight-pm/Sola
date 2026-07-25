# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**agent-leader** (worktree `.worktrees/agent-leader`)

sola-agent requires shared Grok leader; remove console group; host
`grok-leader.service` + `[cli] use_leader = true`.

### Next

- User smoke: install agent, open sessions with TUI + sola-agent multi-client
- Merge when approved

### Last completed (prior)

**agent-console-sessions → master**: console group (now superseded by leader multi-client).

### Future / follow-ups

- Permission fan-out UX when TUI + sola-agent both attached
- Further Grok TUI presentation parity
- Storybook page parity for non-Overview tabs (on demand)
- Remaining worktree: `libei-portal` (unrelated)

### Resume

```text
# worktree:
cd .worktrees/agent-leader
# cargo make build agent
# (install only with user permission)
# host:
# systemctl --user status grok-leader.service
```
