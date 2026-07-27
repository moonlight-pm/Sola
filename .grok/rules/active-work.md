# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**sola-kvm** — Phases A–C on branch `sola-kvm` (not merged to master)

- Spec: [`docs/specs/2026-07-27-sola-kvm-design.md`](../../docs/specs/2026-07-27-sola-kvm-design.md)
- Operator: [`docs/manual/sola-kvm-operator.md`](../../docs/manual/sola-kvm-operator.md)
- Mac client: [`apps/sola-kvm-mac/README.md`](../../apps/sola-kvm-mac/README.md)

### Done (orchestrated parallel B+C)

| Phase | Commit | Notes |
|-------|--------|-------|
| A skeleton | `7761b0b` | protocol, layout, config, CLI |
| B Mac agent | `76c942d` | UDP + CGEvent inject (Linux stub; needs ember smoke) |
| C novus capture | `f2de5d8` | Session machine, feed/demo/evdev, edge enter/leave |

**44** `sola-kvm` unit tests pass.

**lan-mouse purge (novus):** stopped/disabled; wrapper + config quarantined
(`.disabled`); removed from user nix profile. Docs note sola-kvm is the path.

### Next

1. **Desk smoke** — build Mac agent on ember + Accessibility; novus `sola-kvm server` against peer
2. **Layer-shell / chord suppress** — pull or port from `libei-portal` if Meta still eaten during remote
3. **Phase D** — autostart, primary size from bus, real Wayland warp on leave
4. Merge to master only with explicit user approval

### Orchestration (completed)

| Task | Worker | Status |
|------|--------|--------|
| `task_be40a25f306f` Phase B | `term_5bd77c38…` | completed |
| `task_e63e110a48df` Phase C | `term_25f471de…` | completed |

## Last completed (prior, master)

app-icon-raster, session-id-routing, sidebar-hover-trash, float-shadows,
agent-session-perf, focus-follows-mouse, agent-ui-fixes, agent-leader.

### Resume

```text
cd /home/joshua/orca/workspaces/Sola/sola-kvm
git log --oneline master..HEAD
cargo test -p sola-kvm
# desk: see docs/manual/sola-kvm-operator.md + apps/sola-kvm-mac/README.md
```
