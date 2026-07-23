# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan.

If Current is `none`, ask what they want instead of inventing work.

## Current

**sola-agent ACP runner** — branch `agent-acp-runner` in
`.worktrees/agent-acp-runner`

Design: `docs/specs/2026-07-23-sola-agent-acp-runner-design.md`

### Status

v1 greenfield rewrite implemented (resume-only `grok agent stdio`, hybrid
sessions, kit UI). **Awaiting visual smoke + user approval to merge.**

### Next when approved

1. User: `cargo make install agent` and smoke (new/resume/permission)
2. Merge worktree → master + clean up worktree/branch
3. Optional follow-ups: leader daemon (`ConnectionMode::Leader`), project
   cwd picker, richer tool cards

### Last completed

**Agent ACP runner v1** — replaced Fugu harness with ACP client + Grok
backend; design documents future leader daemon.
