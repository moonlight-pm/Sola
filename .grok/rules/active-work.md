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

<<<<<<< HEAD
**titlebar-macos → master**: macOS-style floating titlebar (38px, left traffic-light
close, centered title, rounded `floating_frame`). Monitor dogfoods via transparent
window + overlay theme. Fixed float region screenshots (prefer live geometry;
reject 0×0 Frame / FloatGeometry poison).
||||||| e9ba755
**agent-bulk-delete → master**: sola-agent **Bulk Delete…** panel (Agent menu).
Age filters, safety toggles, preview with size, two-step confirm, worker-thread
`grok sessions delete` + overlay scrub. List padding + title ellipsis so
trailing sizes stay visible.
=======
**agent-bulk-delete → master**: sola-agent **Bulk Delete…** panel (Agent menu).
>>>>>>> agent-console-sessions

### Future / follow-ups

- Opt other kit apps into titlebar / floating_frame (agent, settings, terminal, …)
- Leader daemon (`ConnectionMode::Leader`)
- Polish from further agent UI feedback
- Storybook page parity for non-Overview tabs (on demand when touching components;
<<<<<<< HEAD
  see `.grok/rules/kit-storybook-pages.md`)
- Remaining worktrees: `libei-portal`, `agent-console-sessions` (unrelated)

### Resume

```text
# master is current; already installed river/shell/monitor from this work.
# cargo make install sola-kit sola-monitor sola-river sola-shell
```
||||||| e9ba755
  see `.grok/rules/kit-storybook-pages.md`)
- Remaining worktree: `libei-portal` (unrelated)

### Resume

```text
# master is current; install when needed:
# cargo make install sola-agent
```
=======
  see `.grok/rules/kit-storybook-pages.md`) — **Sidebar** indicator is new kit API
- Remaining worktree: `libei-portal` (unrelated)
>>>>>>> agent-console-sessions
