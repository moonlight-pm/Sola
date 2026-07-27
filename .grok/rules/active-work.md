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
- Operator: [`docs/manual/sola-kvm-operator.md`](../../docs/manual/sola-kvm-operator.md)
- Branch / worktree: `sola-kvm` (`/home/joshua/orca/workspaces/Sola/sola-kvm`)
- Base: local `master` @ `3d1d44b`

### Next phase

**Phase B — Mac agent** (parallel / remaining) **or** Phase D polish

1. Phase B: UDP listen + CGEvent inject on ember; LaunchAgent
2. Phase D: layer-shell barriers + sola-river chord suppress port; compositor warp; autostart

### Done this branch

**Phase A — Spec + skeleton**

- Design doc (`8b2ebb0`)
- `crates/sola-kvm` — protocol, layout math, TOML config, UDP send/listen
- CLI: `show`, `init`, `server` (idle), `listen`, `send-test`

**Phase C — Novus edge + virtual cursor + UDP emit**

- Pure `Session` state machine: Local ↔ Remote, enter/leave, abs Motion, stuck-key recovery
- Layout `try_enter_from_motion` edge hit helpers
- Server loop wired: `--input feed|demo|evdev`
- Evdev EVIOCGRAB spike while remote (needs `/dev/input` access)
- Operator doc; design §10 Phase C notes
- Unit tests for edge math + state machine

### Later phases

- **B** — Mac agent (if not done in parallel)
- **D** — layer-shell path, sola-river hooks, autostart, drop lan-mouse

### Resume

```text
cd /home/joshua/orca/workspaces/Sola/sola-kvm
cargo test -p sola-kvm
cargo make build sola-kvm
# smoke (no HID):
#   sola-kvm listen --bind 127.0.0.1:4242
#   sola-kvm server --input demo   # peer must match or use listen on same port carefully
printf 'abs 5119 2000\nrel 3 0\nrel 40 10\nleave\n' | sola-kvm server --input feed
```

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
