# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**sola-agent UI backlog** — branch `agent-acp-runner` in
`.worktrees/agent-acp-runner`

Design: `docs/specs/2026-07-23-sola-agent-acp-runner-design.md`  
Backlog: `docs/specs/2026-07-23-sola-agent-ui-backlog.md`  
(paths relative to the worktree until merged)

### Status

Phases **A–I** implemented on `agent-acp-runner`. Awaiting user review / install.
Do **not** merge to master until user approves.

### Star (pin)

★ **pins** a session in Sola overlay (`~/.config/sola/agent/overlay.json`) so it
sorts first. Not a Grok-native flag.

### Backlog phases

| Phase | Item |
|---|---|
| ~~A~~ | ~~Composer~~ |
| ~~B~~ | ~~Sidebar fill~~ |
| ~~C~~ | ~~Row cwd~~ |
| ~~D~~ | ~~Project picker~~ |
| ~~E~~ | ~~Type + width~~ |
| ~~F~~ | ~~Markdown~~ |
| ~~G~~ | ~~Token format~~ |
| ~~H~~ | ~~Rename~~ |
| ~~I~~ | ~~Lazy load + scroll bottom~~ |

### Future

- Leader daemon (`ConnectionMode::Leader`)
- Merge `agent-acp-runner` → master + delete stale `.worktrees/sola-agent`
- Polish from user feedback after A–I smoke

### Last completed

**UI backlog A–I** on `agent-acp-runner`.

### Resume

```text
cd .worktrees/agent-acp-runner   # branch agent-acp-runner
# install with user permission: cargo make install sola-agent
# next: user feedback / merge when approved
```
