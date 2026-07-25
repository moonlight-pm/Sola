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

**titlebar-macos → master**: macOS-style floating titlebar (38px, left traffic-light
close, centered title, rounded `floating_frame`). Monitor dogfoods via transparent
window + overlay theme. Fixed float region screenshots (prefer live geometry;
reject 0×0 Frame / FloatGeometry poison).

### Future / follow-ups

- Opt other kit apps into titlebar / floating_frame (agent, settings, terminal, …)
- Leader daemon (`ConnectionMode::Leader`)
- Polish from further agent UI feedback
- Storybook page parity for non-Overview tabs (on demand when touching components;
  see `.grok/rules/kit-storybook-pages.md`)
- Remaining worktrees: `libei-portal`, `agent-console-sessions` (unrelated)

### Resume

```text
# master is current; already installed river/shell/monitor from this work.
# cargo make install sola-kit sola-monitor sola-river sola-shell
```
