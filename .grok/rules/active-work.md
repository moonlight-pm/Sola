# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**agent-console-sessions** (worktree `.worktrees/agent-console-sessions`)

Console / external Grok TUI sessions in sola-agent:

1. Activity **dot** = working (recent transcript / streaming), not “open in console”
2. Console sessions in sidebar section **In console** (rest under Sessions/Recent)
3. Auto-sync transcript while viewing a console session (poll `updates.jsonl`)
4. Read-only viewer — no composer; no ACP `session/load` (avoids fighting TUI)

### Status

Implementation complete; `cargo make build sola-agent` passes. Awaiting install /
smoke + user approval to merge.

### Resume

```text
cd .worktrees/agent-console-sessions
# cargo make install sola-agent   # only with user permission
```

### Last completed

**agent-bulk-delete → master**: sola-agent **Bulk Delete…** panel (Agent menu).

### Future / follow-ups

- Leader daemon (`ConnectionMode::Leader`)
- Polish from further agent UI feedback
- Storybook page parity for non-Overview tabs (on demand when touching components;
  see `.grok/rules/kit-storybook-pages.md`) — **Sidebar** indicator is new kit API
- Remaining worktree: `libei-portal` (unrelated)
