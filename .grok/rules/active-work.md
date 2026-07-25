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

**agent-acp-runner → master** (fast-forward to `11abd5c`): sola-agent ACP runner
UI, Grok-style transcript, kit SidebarPanel sessions, section-scoped app-owned
scroll with overflow chips. Worktrees `agent-acp-runner` and `sola-agent`
removed; branches deleted.

Design: `docs/specs/2026-07-23-sola-agent-acp-runner-design.md`  
Backlog: `docs/specs/2026-07-23-sola-agent-ui-backlog.md`

### Future / follow-ups

- Leader daemon (`ConnectionMode::Leader`)
- Polish from further agent UI feedback
- Storybook page parity for non-Overview tabs (on demand when touching components;
  see `.grok/rules/kit-storybook-pages.md`)
- Remaining worktree: `libei-portal` (unrelated)

### Resume

```text
# master is current; install when needed:
# cargo make install sola-agent
```
