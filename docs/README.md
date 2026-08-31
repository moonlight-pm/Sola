# Sola documentation

Canonical **engineering** documentation for the Iced-era Sola desktop.  
Historical WebView / early-stack material lives under
[`../apocrypha/`](../apocrypha/) and is not authoritative.

**Operator-facing product docs** live in [`manual/`](manual/) — **shipped
behavior only**. See [`progress-model.md`](progress-model.md).

## Session boot (agents and humans)

| Step | Document |
|-----:|----------|
| 1 | [`../AGENTS.md`](../AGENTS.md) — worktrees, install rules, kit notes |
| 2 | [`../CURRENT.md`](../CURRENT.md) — **living** priority + dogfood state |
| 3 | [`capabilities.md`](capabilities.md) — as-built maturity for the slice |
| 4 | [`open-questions.md`](open-questions.md) Decision points if the slice needs policy |
| 5 | One freeze under [`specs/`](specs/) or plan under [`plans/`](plans/) if needed |

When you finish real product work, update **`CURRENT.md`** and
**`capabilities.md`** (and manual / architecture / roadmap as required) in the
**same change**. Follow
[`.grok/skills/sola-progress-docs/SKILL.md`](../.grok/skills/sola-progress-docs/SKILL.md).
Do not create one-off handoff files. Do not invent answers to Decision points —
ask the human.

**Progress docs are first-class.** Full rules:
[`progress-model.md`](progress-model.md) · portable export:
[`progress-documentation-practice.md`](progress-documentation-practice.md).

## Document map

| File | Purpose | Kind |
|------|---------|------|
| [`../CURRENT.md`](../CURRENT.md) | Living priority, dogfood, locks | **Focus** |
| [`../PERFORMANCE.md`](../PERFORMANCE.md) | GPU / iced idle program: shipped, smoke, next | **As-built** track |
| [`capabilities.md`](capabilities.md) | Capability status + gaps | **As-built** |
| [`architecture.md`](architecture.md) | Processes, crates, bus, call plane, install layout | **As-built** map |
| [`progress-model.md`](progress-model.md) | How is / will-be / focus / manual fit | Meta |
| [`progress-documentation-practice.md`](progress-documentation-practice.md) | Portable practice (shareable) | Meta |
| [`roadmap.md`](roadmap.md) | Coarse multi-month phases | **Horizon** |
| [`open-questions.md`](open-questions.md) | Design forks + ask-human decisions | Design forks |
| [`specs/`](specs/) | Target freezes (dated) | **Target** |
| [`plans/`](plans/) | Implementation checklists (active + historical) | Build |
| [`ideas/`](ideas/) | Parked thoughts | Idea |
| [`manual/`](manual/) | Operator truth (fonts, kvm, **shell**, **arcade**, **browser**, **paint**, **monitor**, **wrapper**, **scope**, **solactl**, dist notes; ISO guide when dogfoodable) | **Product** (shipped only) |
| [`specs/2026-08-05-distribution-image-design.md`](specs/2026-08-05-distribution-image-design.md) | Dist installer freeze | **Target** |
| [`vault/`](vault/) | Early Obsidian notes — reference only | History |
| [`notes/`](notes/) | One-off investigations — not living handoff | History |
| [`visual/`](visual/) | Visual regression baselines | Engineering assets |

## Related trees (not under `docs/`)

| Path | Role |
|------|------|
| `AGENTS.md` | Contributor + agent guide |
| `CURRENT.md` | Only living session handoff |
| `PERFORMANCE.md` | GPU / idle program log (not a second CURRENT) |
| `INSTALL.md` | Shape 1 colleague install (shipped-path ops; tarball may 404) |
| `CONTRIBUTING.md` | From-source NixOS setup (`services.sola.installRelease = false`) |
| `nix/` | NixOS module + `nix/image/` ISO/qcow sources |
| `crates/sola-install` | Installer wizard binary |
| `var/images/` | Local ISO/qcow products (gitignored) |
| `apocrypha/` | Legacy WebView stack — not built |
| `.grok/skills/` | `sola-session-start`, `sola-progress-docs`, `sola-workspaces-cli` |
| `.grok/rules/active-work.md` | **Pointer** to `CURRENT.md` (auto-load reminder) |
| `/opt/sola/` | Install root (bin, log, share) — runtime, not git |

## Authority order

1. **Code that ships** (and tests)  
2. **Root `CURRENT.md`** for active priority and dogfood facts  
3. **`docs/capabilities.md`** for capability maturity  
4. This `docs/` suite for intent and map  
5. `apocrypha/`, vault, notes — ignore unless hunting history  
