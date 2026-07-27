# Active work

This file is auto-loaded every session (`.grok/rules/*.md`).

## When the user says go

If the user signals go-ahead **without naming a new task** — e.g. "go", "ok go",
"do it", "continue", "keep going", "ship it", "proceed", "lfg", "get on with it",
or similar — **do not re-plan, re-audit, or re-explore**. Execute **Current**
below from the listed next phase, using the linked plan / backlog.

If Current is `none`, ask what they want instead of inventing work.

## Current

**sola-kvm** — custom software KVM (novus server → ember macOS client)

- Spec: [`docs/specs/2026-07-27-sola-kvm-design.md`](../../docs/specs/2026-07-27-sola-kvm-design.md)
- Branch / worktree: `sola-kvm` (`/home/joshua/orca/workspaces/Sola/sola-kvm`)
- Base: local `master` @ `3d1d44b`

### Next phase

**Phase B — Mac agent** (or continue Phase C on novus if Mac inject is deferred)

1. UDP listen + CGEvent inject (motion/button/key) on ember
2. LaunchAgent in GUI session
3. Manual feed: `sola-kvm send-test --to 10.0.0.21:4242`

### Done this branch

**Phase A — Spec + skeleton**

- Design doc committed earlier (`8b2ebb0`)
- `crates/sola-kvm` — protocol, layout math, TOML config, UDP send/listen
- CLI: `show`, `init`, `server` (idle stub), `listen`, `send-test`
- 28 unit tests (encode/decode, layout bottoms-align, localhost UDP)
- Mac stub note: `apps/sola-kvm-mac/README.md`

### Later phases

- **C** — novus edge capture + exclusive grab + chord suppress + UDP emit
- **D** — autostart, stuck keys, polish; drop lan-mouse daily path

## Last completed (prior, master)

**app-icon-raster → master**: full-color app icons (path/PNG refs) in launcher + switcher; case-insensitive catalog lookup for Wayland app_id mismatches (e.g. orca).

**session-id-routing → master**: live ACP stream keyed by session UUID
(not title); OD session cards (graphite select, slim context bar, hover
× + time rail); kit `SidebarItem` card chrome + custom content; toolbar
RESET deletes open session and starts fresh.

### Future / follow-ups (other)

- Permission fan-out UX when TUI + sola-agent both attached (ask mode)
- Further Grok TUI presentation parity
- Storybook page parity for non-Overview tabs (on demand)
- Remaining worktrees: `libei-portal`, `app-icon-raster`

### Resume

```text
# sola-kvm worktree — Phase A done; next Phase B (Mac) or C (novus capture)
cd /home/joshua/orca/workspaces/Sola/sola-kvm
cargo test -p sola-kvm
cargo run -p sola-kvm -- show
```
