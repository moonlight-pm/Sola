# Progress model — how Sola documents itself

**Describing the system and its progress is first-class work.** Incomplete
meta work is incomplete product work. Any session that changes behavior,
capability maturity, dogfood/runtime state, or priority must update the
matching docs **in the same change** as the code.

**Portable practice (project-agnostic):**  
[`progress-documentation-practice.md`](progress-documentation-practice.md)

**Session skills:**  
[`.grok/skills/sola-session-start/SKILL.md`](../.grok/skills/sola-session-start/SKILL.md) ·  
[`.grok/skills/sola-progress-docs/SKILL.md`](../.grok/skills/sola-progress-docs/SKILL.md)

---

## Four document kinds

Never mix “is” and “will be” in the same paragraph without an explicit label.

```text
┌─────────────────────────────────────────────────────────┐
│  PRODUCT DOCS (operator)     docs/manual/               │
│  Audience: people running Sola.                         │
│  Voice: “what you can do.” AS-SHIPPED only.             │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  AS-BUILT (engineering)      architecture + capabilities│
│  “What exists in code and on the dogfood machine.”      │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  TARGET (engineering)        specs + roadmap remaining  │
│  “What we decided; what’s still left.”                  │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  FOCUS (session)             CURRENT.md only            │
│  Priority, next slice, runtime facts, locks             │
└─────────────────────────────────────────────────────────┘
```

| Kind | Canonical home | Update when |
|------|----------------|-------------|
| **Focus** | Root [`CURRENT.md`](../CURRENT.md) | Priority, next moves, or dogfood/runtime facts change |
| **GPU / idle track** | Root [`PERFORMANCE.md`](../PERFORMANCE.md) | A present/scanout mitigation lands, smoke result changes, or the next perf slice is chosen. **Not** a second CURRENT. |
| **As-built progress** | [`capabilities.md`](capabilities.md) | Capability status or gaps change |
| **As-built system map** | [`architecture.md`](architecture.md) | Processes, crates, paths, IPC, install layout change |
| **Target design** | [`specs/`](specs/) freezes | Decisions lock or amend (**not** a changelog) |
| **Horizon** | [`roadmap.md`](roadmap.md) | Phase-level status flips only |
| **Open forks** | [`open-questions.md`](open-questions.md) | A fork opens or closes |
| **Parked** | [`ideas/`](ideas/) | Thought captured; promote later |
| **Implementation checklists** | [`plans/`](plans/) (+ historical `*-plan.md` under `specs/`) | Active slice only; at most one open plan |
| **Product / operator docs** | [`manual/`](manual/) | **Shipped** (or honestly limited) operator-visible behavior |
| **History** | [`../apocrypha/`](../apocrypha/), vault notes, old freezes | Never authoritative |

### Authority order

1. **Code that ships** (and tests)  
2. **Root `CURRENT.md`** for active priority and live dogfood/runtime facts  
3. **`docs/capabilities.md`** for capability maturity  
4. **This `docs/` suite** for intent and map  
5. **`apocrypha/`**, `docs/vault/`, `docs/notes/` — history / reference only  

**Code wins over stale docs** — then fix the docs immediately.

---

## Capability status vocabulary

| Status | Meaning | Product docs (`docs/manual/`) may say? |
|--------|---------|----------------------------------------|
| **shipped** | In code; dogfoodable on the local TTY install | **Yes** — as fact |
| **partial** | Scaffold or subset; **gaps must be listed** | Only with honest limits |
| **spec’d** | Freeze exists; little or no code | **No** |
| **planned** | On roadmap; no freeze yet | **No** |
| **idea** | [`ideas/`](ideas/) only | **No** |
| **retired** | Removed from the tree; row kept so the removal is not a silent gap | **No** |

**Partial without listed gaps is invalid.**

Roadmap phases may use `done` / `partial` / `active` / `next` / `planned` /
`unplanned`. Capabilities are the day-to-day truth for agents.

---

## Spec freeze headers

Active freezes under `docs/specs/` should open with:

```markdown
**Date:** YYYY-MM-DD
**Status:** target (freeze)
**Implementation:** shipped | partial | not started
**Dogfood:** none | local TTY | …
**Gaps:** (bullets or “none for v0”)
**As-built:** [../capabilities.md](../capabilities.md) · [../architecture.md](../architecture.md)
```

- Freezes stay **normative desired shape** — not progress trackers.  
- Prefer date-prefixed filenames.  
- New implementation checklists go under `docs/plans/`. Older `*-plan.md`
  files under `specs/` are historical; do not invent a second living tracker.  
- At most **one** open plan for the active slice.

---

## Product docs rule (hard)

[`docs/manual/`](manual/) documents **working, operator-visible behavior only**.

| Capability status | Product docs |
|-------------------|--------------|
| shipped | Document as fact |
| partial | Omit, or document only what works, labeled limited |
| spec’d / planned / idea | Do not present as product fact |

Engineering desire lives in specs + roadmap + capabilities — not the manual.

---

## `CURRENT.md` is a dashboard

**Keep:** Now (3–7 next moves), known dogfood/runtime state, locked models,
pointers, short ops notes.

**Do not grow into:** full phase inventory, long ship history, design essays.

**No second living handoff.** Forbidden as living truth:

- Parallel `STATUS.md` / `HANDOFF.md` / session diaries  
- Duplicating priority into `.grok/rules/active-work.md` (that file is a
  **pointer** only)  
- Living “review findings” trees  

Absorb findings into capabilities + active plan + open-questions, then delete
the duplicate.

---

## Open questions

[`open-questions.md`](open-questions.md) holds **unresolved design forks**, not
the implementation backlog.

1. **Decision points (ask human)** — `D1`, `D2`, … agents must not invent  
2. **Open technical questions** — still design, not policy ownership  
3. **Decision log** — when answered: date, decision, pointer  

---

## Session loop

### Start

1. [`AGENTS.md`](../AGENTS.md)  
2. [`CURRENT.md`](../CURRENT.md)  
3. [`capabilities.md`](capabilities.md) — rows for the slice  
4. [`open-questions.md`](open-questions.md) — any D* for this slice?  
5. One freeze or plan if building that domain  
6. Architecture only if needed  

### End (same change as code)

1. Capability row(s)  
2. CURRENT if priority/dogfood changed  
3. Product manual if operator-visible shipped behavior changed  
4. architecture if system map changed  
5. Freeze header if implementation status moved  
6. roadmap only if phase-level flip  
7. open-questions if a fork opened/closed or D* answered  

Skipping capability / CURRENT / product-doc updates while shipping behavior is
a **process failure**.

### Before claiming “done”

- Capability rows reflect new truth  
- CURRENT reflects new priority/dogfood if either changed  
- Product docs updated if operator-facing shipped behavior changed  
- Tests/format appropriate to the code change  

---

## Project defaults

| Default | Rule |
|---------|------|
| Worktrees | Feature code in `.worktrees/`; never commit product code directly to master without explicit ask |
| Install | Never `cargo make install` without express permission for that install |
| Build verify | `cargo make build` is enough; user installs and smokes on TTY |
| UI stack | Iced + sola-kit only; `apocrypha/` WebView is reference, not product |
| Dogfood | Single local machine; user launches `sola` from a physical TTY |

---

## Deferred meta work

| Item | Why deferred | When to revisit |
|------|--------------|-----------------|
| CI that fails if shipped capability has no manual page | Manual discipline first | After matrix stabilizes |
| Auto-generate architecture diagram from crates | Hand map is enough | After topology settles |
| Backfill Implementation headers on every historical freeze | Large one-time pass | When touching that freeze |
| Move all historical `specs/*-plan.md` into `plans/` | Noise; paths linked widely | Optional hygiene pass |

Do not use this table as a second product tracker.
