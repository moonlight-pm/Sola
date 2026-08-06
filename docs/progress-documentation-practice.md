# Progress documentation practice

**A portable practice for product and engineering repos.**  
Copy this file into another project and adapt the path names; keep the rules.

Origin: refined on [Wicket](https://github.com/) (operator-facing control plane).  
The model is product-agnostic. It works for any codebase where humans and agents
ship real behavior and need a single, trustworthy picture of *what exists*,
*what we decided*, and *what to do next*.

---

## Thesis (non-negotiable)

**Describing the system and its progress is first-class work.**

Incomplete meta work is incomplete product work. Any change that alters
behavior, capability maturity, runtime dogfood state, or priority must update
the matching docs **in the same change** as the code.

If you only reformatted code with no behavior change, skip docs. If you are
unsure whether docs need an update, **update the docs**.

---

## Core principle: never mix “is” and “will be”

Most documentation rot comes from one failure mode: mixing present reality and
future intent in the same paragraph without labels. Fix that with **four
document kinds** and a hard rule that each kind has one job.

```text
┌─────────────────────────────────────────────────────────┐
│  PRODUCT DOCS (users / operators)                       │
│  Audience: people using the product.                    │
│  Voice: “what you can do.”                              │
│  Reflects AS-SHIPPED only. Never “coming soon” as fact. │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  AS-BUILT (engineering)                                 │
│  Audience: humans + agents building.                    │
│  “What exists in code and on dogfood/runtime.”          │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  TARGET (engineering)                                   │
│  Audience: design + implementation.                     │
│  “What we decided; what’s still left.”                  │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  FOCUS (session)                                        │
│  One living dashboard: priority, next slice, locks.     │
└─────────────────────────────────────────────────────────┘
```

| Kind | Role | Update when |
|------|------|-------------|
| **Focus** | What to do next; live runtime facts; do-not-re-litigate locks | Priority, next moves, or dogfood/runtime facts change |
| **As-built progress** | Capability maturity + gaps | Status or gaps change |
| **As-built system map** | Processes, packages, ports, backends, state paths | The running shape of the system changes |
| **Target design** | Freezes (normative desired shape) | Decisions lock or amend — **not** a changelog |
| **Horizon** | Coarse multi-month phases | Phase-level status flips only |
| **Open forks** | Unresolved design/policy questions | A fork opens or closes |
| **Parked** | Ideas not yet active | A thought is captured; promote later |
| **Product docs** | User/operator truth | **Shipped** (or honestly limited) behavior changes |
| **History** | Archive only | Never authoritative |

Never mix “is” and “will be” in the same paragraph without an explicit label.

---

## Recommended tree

Adapt names; keep the **roles**.

```text
repo/
├── AGENTS.md                 # (or CONTRIBUTING.md) autonomy, secrets, UX law
├── CURRENT.md                # FOCUS — only living session handoff
├── docs/
│   ├── README.md             # map of the docs suite + session boot
│   ├── progress-model.md     # project-local instance of this practice
│   ├── capabilities.md       # AS-BUILT progress matrix
│   ├── architecture.md       # AS-BUILT system map
│   ├── roadmap.md            # HORIZON (phases)
│   ├── open-questions.md     # design forks + “ask human” decision points
│   ├── specs/                # TARGET freezes (dated)
│   ├── plans/                # completable implementation checklists
│   └── ideas/                # parked thoughts
├── <product-docs>/           # wiki, user manual, help site — SHIPPED ONLY
└── archive/ or apocrypha/    # history; never truth
```

Optional engineering companions (not required by the model):

- Deployments / topology notes  
- Control-surface inventory (CLI / API / UI maturity)  
- Conventions (how you write code and operator output)

### Authority order (when documents disagree)

1. **Code that ships** (and tests)  
2. **`CURRENT.md`** for active priority and live dogfood/runtime facts  
3. **`docs/capabilities.md`** for capability maturity  
4. **The rest of `docs/`** for intent and map  
5. **Archive / history** — ignore unless hunting the past  

**Code wins over stale docs** — then fix the docs immediately.

---

## Capability status vocabulary

Use these statuses on **product surfaces** (capabilities), not only roadmap
phases. Capabilities are the day-to-day progress truth.

| Status | Meaning | Product docs may say? |
|--------|---------|------------------------|
| **shipped** | In code; dogfoodable / usable where relevant | **Yes** — as fact |
| **partial** | Scaffold or subset; **gaps must be listed** | Only with honest limits |
| **spec’d** | Freeze exists; little or no code | **No** |
| **planned** | On roadmap; no freeze yet | **No** |
| **idea** | Parked under `ideas/` only | **No** |

**Partial without listed gaps is invalid.** Always name what remains.

Roadmap phases may use a coarser legend (`done` / `partial` / `active` /
`next` / `planned` / `unplanned`). That is program-horizon language.
**Capabilities** are what agents and humans use day to day.

### Capability matrix shape

Keep one table (or a small set of banded tables). Minimum columns:

| Column | Purpose |
|--------|---------|
| **ID** | Stable short id (`install`, `backup-l1`, …) |
| **Capability** | Human name |
| **Status** | Vocabulary above |
| **Spec / plan** | Link to freeze or plan |
| **Dogfood** | Where it has been exercised (host, env, “unit tests”, “—”) |
| **Gaps** | What remains (required if partial) |
| **Wiki / product docs** | `yes` / `partial` / `no` / `n/a` |

Update the matrix **in the same change** as the code that changes maturity.

---

## Spec freezes (target design)

Freezes are **normative desired shape**. They are not progress trackers and
must not be rewritten into changelogs.

Every active freeze should open with a machine-scannable header:

```markdown
**Date:** YYYY-MM-DD
**Status:** target (freeze)
**Implementation:** shipped | partial | not started
**Dogfood:** none | <env> | …
**Gaps:** (bullets or “none for v0”)
**As-built:** [../capabilities.md](../capabilities.md) · [../architecture.md](../architecture.md)
```

Rules:

- When implementation catches the freeze, set **Implementation: shipped** and
  put remaining polish under Gaps (or “none for v0”).  
- Prefer date-prefixed filenames: `docs/specs/YYYY-MM-DD-topic.md`.  
- At most **one** open implementation plan for the active slice under
  `docs/plans/`; completed plans stay historical.  
- Do not invent a second living “review findings” tree; absorb findings into
  plan + capabilities + open-questions, then discard the artifact folder.

---

## Product docs rule (hard)

User/operator documentation documents **working, operator-visible behavior
only**.

| Capability status | Product docs |
|-------------------|--------------|
| shipped | Document as fact |
| partial | Omit, or document only what works, labeled as limited |
| spec’d / planned / idea | Do not present as product fact |

- Never write roadmap aspiration (“Phase 16 will…”) as product truth.  
- Engineering desire lives in specs + roadmap + capabilities — not the wiki.  
- When operator-visible behavior ships, update product docs **in the same
  change** as code.

If your product has no embedded wiki, the same rule applies to README user
sections, help centers, man pages, and in-app docs.

---

## `CURRENT.md` is a dashboard

**Keep:**

- **Now** — ordered next moves (about 3–7 items)  
- **Known dogfood / runtime state** — hosts, envs, critical facts  
- **Locked product models** — do-not-re-litigate decisions  
- **Pointers** into capabilities, specs, roadmap  
- Short ops notes / useful commands as needed  

**Do not grow into:**

- Full phase inventory → roadmap + capabilities  
- Long ship history → git log (optional compact note in capabilities)  
- Design essays → specs  

If it is not needed to **choose the next action** or **avoid breaking dogfood**,
it does not belong in `CURRENT.md`.

### No second living handoff

Forbidden as living sources of truth:

- `STATUS.md`, `HANDOFF.md`, session diaries  
- Root “notes” that duplicate CURRENT  
- Parallel living review trees  

Absorb findings into capabilities + active plan + open-questions, then delete
the duplicate.

---

## Open questions and decision points

`docs/open-questions.md` holds **unresolved design forks**, not the
implementation backlog.

Split into:

1. **Decision points (ask human)** — product/policy choices agents **must not
   invent**. Number them (`D1`, `D2`, …). Each row: context, the ask, “until
   decided” default, related caps/specs.  
2. **Open technical questions** — still design, but not policy ownership.  
3. **Decision log** — when answered: date, decision, pointer to code/docs.

**Agents:** if work depends on an open decision point, **stop and ask**. Record
the answer in the decision log and update capabilities / CURRENT.

Priority tags on questions (optional): **P0** blocks current work · **P1**
near-term · **P2** later.

---

## Session loop (mandatory)

### Start (read order)

1. Contributor / agent guide (`AGENTS.md` or equivalent)  
2. `CURRENT.md` — Now + dogfood  
3. `docs/capabilities.md` — rows for the slice you will touch  
4. `docs/open-questions.md` — any decision points for this slice?  
5. **One** freeze or plan if building that domain  
6. Architecture / topology only if needed  
7. Nothing else until needed  

Then: state priority, holds, host facts, capability gaps, and any blocking
decision in one short paragraph — and start work (or ask the human first).

### End (real product progress — same change as code)

1. Code + format / tests as usual  
2. Update **capability row(s)** (status and/or gaps)  
3. Update **CURRENT** Now and/or dogfood if priority or hosts changed  
4. **Product docs** only if operator-visible shipped behavior changed  
5. **architecture.md** if system map changed  
6. **Freeze header** Implementation / Dogfood / Gaps if that freeze moved  
7. **roadmap.md** only if a **phase-level** status flipped  
8. **open-questions.md** if a fork opened, closed, or a decision was answered  
9. Commit product + meta together (or immediate follow-up in the same session)  

Skipping capability / CURRENT / product-doc updates while shipping behavior is
a **process failure**, not a minor omission.

### Before claiming “done”

You may not claim a slice complete unless:

- Capability row(s) reflect new truth  
- CURRENT reflects new priority/dogfood if either changed  
- Product docs updated if operator-facing shipped behavior changed  
- Tests/format appropriate to the code change  

If someone asks to skip docs, still list what would have been updated — and
update unless they explicitly forbid it for a throwaway experiment.

---

## End-of-slice checklist

Copy into a project skill, PR template, or agent instruction. Mark N/A only
with a reason.

```text
[ ] capabilities.md — row(s): Status, Gaps, Dogfood, product-docs column
[ ] CURRENT.md — Now / dogfood / locks if any of those changed (keep thin)
[ ] Product docs — only if shipped (or limited partial) operator-visible UX changed
[ ] architecture.md — if processes, packages, ports, backends, state paths changed
[ ] Active freeze header — Implementation / Dogfood / Gaps if implementation moved
[ ] roadmap.md — only if phase-level status flipped
[ ] open-questions.md — if a design fork opened/closed or D* was answered
[ ] Commit — code + meta same change (or immediate follow-up); never “for later”
```

---

## Anti-patterns

| Anti-pattern | Why it fails |
|--------------|--------------|
| Second living handoff | Both rot; people pick the wrong one |
| Freezes as progress trackers | Desired state becomes a stale changelog |
| Product docs track the roadmap | Users get aspirational lies |
| CURRENT as full program of record | Unreadable; people stop updating |
| “Partial” with no gaps | Status becomes meaningless |
| Per-feature markdown forests without a matrix | Every session rediscovers the world |
| One-off handoff files under root or `docs/` | Forbidden; use CURRENT + capabilities |
| Parallel living “review” folders | Second source of truth; absorb then delete |
| Agents inventing product policy | Decision points exist so humans own forks |
| Meta deferred “until later” for shipped work | Later never comes; truth drifts |

---

## Agent / AI integration

This practice is especially valuable when coding agents share a repo with
humans.

### Contributor guide should say

- Progress docs are mandatory first-class work.  
- Session boot order (see above).  
- End-of-slice checklist.  
- Decision points: ask the human; do not invent policy.  
- No second handoff files.  
- Code wins; then fix docs.  

### Two lightweight agent skills (recommended)

1. **Session start** — force the read order; forbid inventing handoffs; require
   a short status paragraph before coding.  
2. **Progress docs** — force the end-of-slice checklist; define status
   vocabulary; refuse “done” without meta updates.

### Autonomy defaults that pair well

**Do without asking:** read/search; normal build/test; normal commits; update
progress docs when product progress lands.

**Confirm or stop before:** force-push / history rewrite; destructive infra;
production mutations the user did not request; committing secrets; inventing
answers to open decision points.

---

## Starter templates

### `CURRENT.md`

```markdown
# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Full model: [docs/progress-model.md](docs/progress-model.md).
Capability maturity: [docs/capabilities.md](docs/capabilities.md).

**Decisions agents must ask about:**
[docs/open-questions.md § Decision points](docs/open-questions.md#decision-points-ask-human).

**As of:** YYYY-MM-DD

---

## Now

1. …
2. …
3. …

**Explicit holds:** …

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance.

---

## Known dogfood state

| | **primary** |
|--|-------------|
| Role | … |
| Endpoint / host | … |
| Notes | … |

---

## Locked models

- … (do not re-litigate without an explicit decision)

---

## Pointers

- Capabilities: [docs/capabilities.md](docs/capabilities.md)
- Active plan: [docs/plans/…](docs/plans/)
- Active freeze: [docs/specs/…](docs/specs/)
- Roadmap: [docs/roadmap.md](docs/roadmap.md)
```

### `docs/capabilities.md` (header + one band)

```markdown
# Capabilities — as-built progress

**Purpose:** What exists vs what remains for each product capability.
Target design: [specs/](specs/). Session priority: root [CURRENT.md](../CURRENT.md).
Product docs: **shipped only**.

**Update this file in the same change** whenever status or gaps change.

**Status vocabulary:** `shipped` · `partial` · `spec’d` · `planned` · `idea`

**As of:** YYYY-MM-DD

---

## Priority band (example)

| ID | Capability | Status | Spec / plan | Dogfood | Gaps | Wiki |
|----|------------|--------|-------------|---------|------|------|
| example-cap | Example surface | partial | [freeze](specs/…) | env-a | Works for X. **Gaps:** Y, Z | no |
```

### `docs/architecture.md` (role blurb)

```markdown
# Architecture

**Role:** as-built system map (what the code and runtime look like **now**).
**Not** the place for multi-feature roadmaps or session priority.

| Concern | Document |
|---------|----------|
| Capability maturity | [capabilities.md](capabilities.md) |
| Target design freezes | [specs/](specs/) |
| Session priority + dogfood | Root [CURRENT.md](../CURRENT.md) |
| How docs fit together | [progress-model.md](progress-model.md) |
| Product docs | `<path>` — **shipped only** |

When implementation lands from a freeze, merge the **as-built** bits here and
update the freeze’s Implementation / Gaps header.
```

### `docs/open-questions.md` (decision point shape)

```markdown
# Open questions

Unresolved **design forks**. Not the implementation backlog
(that lives in roadmap / capabilities / plans).

## Decision points (ask human)

### D1 — Short title (P0)

**Context:** …

**Ask:**
1. …
2. …

**Until decided:** <safe default the team may follow>

**Related:** capability-id, freeze name

---

## Decision log

| Date | ID | Decision | Where recorded |
|------|-----|----------|----------------|
| YYYY-MM-DD | D1 | … | CURRENT locks / freeze / commit |
```

### `docs/roadmap.md` (phase legend)

```markdown
# Roadmap

**Program horizon** — coarse phase status over months.
Day-to-day maturity: [capabilities.md](capabilities.md).
Session priority: root [CURRENT.md](../CURRENT.md).

| Status | Meaning |
|--------|---------|
| **done** | Good enough to build on; polish ok |
| **partial** | Scaffold or subset shipped; important gaps remain |
| **active** | Current focus (`CURRENT.md`) |
| **next** | Queued after active |
| **planned** | Intended; not started |
| **unplanned** | Not scheduled; capture only |

Update phase status only when a **phase-level** flip happens.
Prefer capability rows for feature-level progress.

## Phase 0 — …

**Status: …**

- …
```

### Project-local `docs/progress-model.md`

Keep a short project file that:

1. Points at this practice (or inlines the thesis + four kinds).  
2. Maps roles to **your** paths (wiki location, archive folder, agent skills).  
3. Lists any project-specific defaults (e.g. “closure before breadth”).  
4. Holds a **deferred meta work** table so automation ideas do not become a
   second tracker:

```markdown
## Deferred meta work

| Item | Why deferred | When to revisit |
|------|----------------|-----------------|
| Auto-check product docs vs capabilities shipped | Manual discipline first | After matrix stabilizes |
| Generate architecture diagram from code | Hand map is enough | After topology settles |
```

---

## Adoption checklist (other projects)

1. **Create the spine:** `CURRENT.md`, `docs/capabilities.md`,
   `docs/architecture.md`, `docs/roadmap.md`, `docs/open-questions.md`,
   `docs/specs/`, `docs/plans/`, `docs/ideas/`.  
2. **Write the authority order** into `docs/README.md` and the contributor
   guide.  
3. **Seed capabilities** from what actually exists today (honest partial +
   gaps beat false shipped).  
4. **Seed CURRENT** with real next moves and runtime facts only.  
5. **Promote active design** into dated freezes with Implementation headers;
   park the rest in ideas.  
6. **Move history** out of the living set (archive folder; not authoritative).  
7. **Wire agents:** session-start + progress-docs skills or equivalent
   instructions.  
8. **Enforce the loop:** no “done” without the checklist; no second handoff.  
9. **Product docs boundary:** only document shipped (or limited partial)
   behavior.  
10. **Run one real slice** end-to-end with code + meta in the same change so
    the team feels the rhythm.

---

## What this practice optimizes for

- **One truth per question:** next action, maturity, map, decision, horizon.  
- **Agents that do not re-discover the world** every session.  
- **Operators/users who are not lied to** by aspirational docs.  
- **Humans who own policy** without blocking pure engineering progress.  
- **Thin dashboards** that stay updated because they stay short.

It does **not** optimize for beautiful static sites, exhaustive ADRs for every
micro-decision, or generating every matrix from code on day one. Those can
land later as deferred meta work — after the habit holds.

---

## License of the practice

Use freely. Adapt names, paths, and product-doc homes. Keep the hard rules:

1. Meta is first-class.  
2. Do not mix is and will-be.  
3. Partial requires gaps.  
4. Product docs = shipped.  
5. One living focus file.  
6. Same change as code.  
7. Ask humans on decision points.  
8. Code wins — then fix the docs.
