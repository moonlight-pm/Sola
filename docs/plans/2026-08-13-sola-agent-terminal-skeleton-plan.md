# Plan — sola-agent-terminal skeleton

**Freeze:** [`docs/specs/2026-08-13-sola-agent-terminal-design.md`](../specs/2026-08-13-sola-agent-terminal-design.md)  
**Slice:** build-order step 1 only.

## Done when

`cargo make build sola-agent-terminal` succeeds. Window boots (on user
install), shows one project / one workspace / one live PTY, own tmux
socket. No install from agents.

## Steps

1. [x] Expose `sola-terminal` as a library; parameterize tmux socket.
2. [x] New crate `sola-agent-terminal` — kit boot, bus, sidebar, one pane.
3. [x] Extend kit `SidebarIndicator` enough for a reserved idle/working/waiting/done slot (idle used now).
4. [x] Progress docs: capability `spec’d` → `partial` with gaps; CURRENT; architecture.

**Done** (2026-08-13). Next freeze build-order step: status chrome.

## Not this plan

Spawn, hooks, `sat`, persist, toasts.
