# Parked ideas

Thoughts **not** yet active. Promote into a freeze + plan + CURRENT **Now**
when work starts. Status in the capability matrix: `idea`.

Do not implement from this folder without promotion.

| Idea | Notes |
|------|--------|
| [common-send-feedback](2026-11-02-common-send-feedback.md) | Parked (2026-11-02). Ways to show that a send to Common happened, since nothing moves: row pulse, island pulse, window flash, flying icon, toast, and a shake for refused actions. Waits on whether Common stays at all. |
| [signal-client](2026-10-31-signal-client.md) | Parked (2026-10-31). Research only: a kit Signal client as a linked device. Feasible, never set-and-forget. If unparked: signal-cli (JSON-RPC, no D-Bus) under a Vault-shaped `sola-signald` + kit app, engine-neutral so presage can replace it. No history on link and no calls with either engine. Earlier attempt not found. |
| [vault-later](2026-10-31-vault-later.md) | Parked (2026-10-31). Vault's later pieces: an SSH agent with a prompt per signature, Mail passwords from the vault, import, attachments, Send, the right-click menu over other applications' windows (V4), the username generator's own options, the shell's own fields, memory locking (needs an Oath rlimit on `svc:`). Declined: vault search in the launcher. |
| [design-memory-budget](2026-10-13-design-memory-budget.md) | Partly promoted → sola-design **slice 7** (budget in free memory, discard idle boards with picture kept, refuse the agent with the reason, `canvas.close`, `status` memory — built 2026-10-14). Still parked: renderer-death recovery, cheaper boards (1× off screen), shared renderers, cgroup fence, keep the previous log. |
| [desks](2026-10-12-desks.md) | Parked. A desk is a working mode: shell sidebar, one tiled plane of heterogeneous groups, panels are pages / terminals / agent sessions. Super+Tab cycles the plane against floats and fullscreen. Do not implement from the note. Forks (name, exclusive layers, where a new panel lands) are unsettled. Promoting this replaces zones and the app switcher as the spatial model. |
| [osr-cpu-drag-mitigations](2026-09-14-osr-cpu-drag-mitigations.md) | Parked remainder. 2026-09-14 shipped unique last_frame, overlay ring, damage continuity, partial GPU on drag; Shortcut kanban dogfood good. Still parked: ghost shader quad, overlay restore-ghost + writer handback, `read_frame` recycle, wgpu 256-byte pad check, credits. Do not enable `accelerated_osr` or a 16 ms chrome timer. |
| [omakade-arcade](2026-09-01-omakade-arcade.md) | Parked remainder. 2026-09-01 shipped watch, UserAppExited session, narrow Stop, singleton raise, refuse Play while Steam is open. Still parked: VDF caps, decode bounds, art cache, stale-install, nest/Fit/Portal. Do not steal Qt or URI-only Play. |
| [ipc-contract-compat](2026-08-30-ipc-contract-compat.md) | Parked. Mixed worktree installs into `/opt/sola/bin` can silently remap bus `TopicKind`. Fail-fast preamble + contract hashes + exit 76; do not restart. Promote a slice before any code. |
| [omarchy-consideration](2026-08-22-omarchy-consideration.md) | Parked remainder. Super+K cheatsheet **promoted** → [freeze](../specs/2026-08-31-window-menu-and-shortcuts-design.md). Other steal: solactl parity, clipboard, capture, lock/idle, theme packs, ISO feel. Do not steal tiling / QML / ricing. |
| [sola-agent-terminal](2026-08-12-sola-agent-terminal.md) | Promoted → [freeze](../specs/2026-08-13-sola-agent-terminal-design.md). Product is now Projects (`sola-projects`; `sola-workspaces` until 2026-10-21). |
