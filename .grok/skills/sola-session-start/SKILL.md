---
name: sola-session-start
description: >
  Sola session boot: read AGENTS.md, CURRENT.md, capabilities.md, and only
  the docs needed for the active slice. Use at the start of work in this repo,
  when resuming after compaction, when the user says "where are we", "status",
  "what's next", or /sola-session-start. Prefer this over inventing a new
  handoff. Ensures agents load progress model context before coding.
---

# Sola session start

## Read order (do not skip)

1. [`AGENTS.md`](../../../AGENTS.md) — worktrees, install rules, kit notes  
2. [`CURRENT.md`](../../../CURRENT.md) — **Now**, holds, dogfood, locks  
3. [`docs/capabilities.md`](../../../docs/capabilities.md) — maturity of the
   surfaces you will touch  
4. [`docs/open-questions.md`](../../../docs/open-questions.md) — **Decision
   points** if the slice needs policy (**ask human**)  
5. One freeze under `docs/specs/` or plan under `docs/plans/` if needed  
6. [`docs/architecture.md`](../../../docs/architecture.md) — only if the map
   is unclear  
7. [`docs/progress-model.md`](../../../docs/progress-model.md) — only if
   unclear how docs fit  
8. Nothing else until needed  

## Then

- State in one short paragraph: priority, holds, dogfood facts, capability
  gaps, and any D* that blocks inventing policy.  
- Start work (or ask the human first if blocked on a decision point).  
- Feature code goes in `.worktrees/` (see AGENTS).  
- When finishing, invoke **sola-progress-docs** (or follow that skill’s
  checklist) before claiming done.

## Never

- Create one-off handoff / NOTES / STATUS files  
- Treat `apocrypha/`, `docs/vault/`, or `docs/notes/` as living truth  
- Duplicate priority into `.grok/rules/active-work.md` (pointer only)  
- Assume `docs/manual/` describes unshipped roadmap work  
