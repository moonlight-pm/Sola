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

Phase **A** done. Next: UI backlog phases **B → I**.
Do **not** merge to master until user approves.

### Star (pin)

★ **pins** a session in Sola overlay (`~/.config/sola/agent/overlay.json`) so it
sorts first. Not a Grok-native flag.

### Backlog phases (on **go**, start at B unless named)

| Phase | Item |
|---|---|
| ~~A~~ | ~~Composer: roomier, full width, no Send (Enter sends); Stop while streaming~~ |
| **B** | Sidebar fills full vertical height (header / scroll list / cwd footer) |
| **C** | Each session row shows **project cwd** (short path) |
| **D** | New session: **pick project dir** (not silent process cwd) |
| **E** | Larger transcript fonts; content uses more width |
| **F** | **Markdown** rendering for assistant content |
| **G** | Token usage: **`pct% · nnnK/500K`** |
| **H** | **Rename** session titles |
| **I** | **Lazy** transcript (tail first, load older on scroll up) + **scroll to bottom** on select |

### Future

- Leader daemon (`ConnectionMode::Leader`)
- Merge `agent-acp-runner` → master + delete stale `.worktrees/sola-agent`

### Last completed

**Phase A — Composer** on `agent-acp-runner`: full-width roomier field, Enter
sends (no Send button), Stop while streaming.

### Resume

```text
cd .worktrees/agent-acp-runner   # branch agent-acp-runner
# read docs/specs/2026-07-23-sola-agent-ui-backlog.md
# on "go" → Phase B
```
