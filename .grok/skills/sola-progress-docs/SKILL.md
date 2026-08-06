---
name: sola-progress-docs
description: >
  Mandatory Sola documentation and progress maintenance. Update CURRENT.md,
  docs/capabilities.md, specs headers, architecture.md, roadmap, open-questions,
  and docs/manual whenever product behavior or capability status changes.
  Meta work is first-class: incomplete docs mean incomplete product work.
  Use when finishing a slice, shipping code, changing status, dogfood state,
  priority, architecture, freezes, roadmap phases, or operator-visible UX;
  before claiming work complete; when committing real product progress;
  on /sola-progress-docs; or whenever AGENTS.md / progress-model requires
  handoff updates. Auto-invoke after any non-trivial Sola product change.
---

# Sola progress docs (mandatory)

Describing the system and its progress is **paramount**. Do not treat this as
optional cleanup after code.

Full model: [`docs/progress-model.md`](../../../docs/progress-model.md).

## When this skill applies

**Always** after any change that affects:

- Product behavior (shell, apps, bus, river, install paths)  
- Capability maturity (shipped / partial / gaps)  
- Dogfood / runtime facts  
- Priority or “what next”  
- System map (crates, processes, state paths)  
- Spec decisions or roadmap phase status  
- Operator-visible copy in `docs/manual/`  

If you only reformatted code with no behavior change, skip. If you are unsure,
**update the docs**.

## Status vocabulary (capabilities)

| Status | Meaning | Manual? |
|--------|---------|---------|
| shipped | In code, dogfoodable | yes |
| partial | Subset; **list gaps** | limited only |
| spec’d | Freeze, little code | no |
| planned | Roadmap, no freeze | no |
| idea | `docs/ideas/` only | no |

Partial **without** gaps is invalid.

## End-of-slice checklist (same commit as code)

Run through every item. Mark N/A only with a reason.

1. **[`docs/capabilities.md`](../../../docs/capabilities.md)**  
   - Update row(s): Status, Gaps, Dogfood, Manual column.  
   - Add a row if a new capability appeared.

2. **[`CURRENT.md`](../../../CURRENT.md)**  
   - **Now** if priority or next moves changed.  
   - **Known dogfood state** if runtime facts changed.  
   - **Locked models** only if a product lock was decided.  
   - Keep CURRENT a **dashboard** — do not dump long history here.

3. **Product docs** [`docs/manual/`](../../../docs/manual/)  
   - Only if operator-visible **shipped** (or honestly limited partial)
     behavior changed.  
   - Never document aspirational roadmap as fact.

4. **[`docs/architecture.md`](../../../docs/architecture.md)**  
   - If as-built system map changed (processes, crates, paths, IPC).

5. **Active freeze under `docs/specs/`**  
   - Refresh Implementation / Dogfood / Gaps header if the freeze’s
     implementation status changed.  
   - Do **not** rewrite freezes into changelogs.

6. **[`docs/roadmap.md`](../../../docs/roadmap.md)**  
   - Only if a **phase-level** status flipped.

7. **[`docs/open-questions.md`](../../../docs/open-questions.md)**  
   - If a design fork opened or closed, or a D* was answered.

8. **Commit**  
   - Prefer one commit that includes code + meta, or an immediate follow-up
     commit in the same session. Never leave meta “for later.”

## Hard rules

- **No second living handoff** (`STATUS.md`, `HANDOFF.md`, session diaries,
  living review trees). Absorb into capabilities + plan + open-questions.  
- **Decision points** in open-questions: **ask the human**; do not invent
  product policy. Record answers in the decision log.  
- **Code wins** over stale docs — then fix the docs immediately.  
- **`docs/manual/` = as shipped** (progress model).  
- **Meta is not deferred** for shipped work. Deferred meta *improvements*
  live only in
  [`docs/progress-model.md` deferred section](../../../docs/progress-model.md#deferred-meta-work).

## Before claiming “done”

You may not claim a slice complete unless:

- Capability row(s) reflect new truth  
- CURRENT reflects new priority/dogfood if either changed  
- Manual updated if operator-facing shipped behavior changed  
- Tests/format appropriate to the code change  

If the user asks to skip docs, still list what would have been updated and
update unless they explicitly forbid it for a throwaway experiment.
