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

### Last completed

**agent-console-sessions → master**: console **In console** group, read-only
file viewer + transcript watch for TUI sessions; activity dots (green/grey,
always present); stable sidebar merge (no thrash-sort); mono transcript at
terminal density.

### Future / follow-ups

- Leader daemon (`ConnectionMode::Leader`)
- Further Grok TUI presentation parity
- Storybook page parity for non-Overview tabs (on demand when touching components;
  see `.grok/rules/kit-storybook-pages.md`)
- Remaining worktree: `libei-portal` (unrelated)

### Resume

```text
# master is current; install when needed:
# cargo make install sola-agent
```
