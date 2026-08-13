# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-13 (this branch: agent-terminal Grok hooks, local smoke OK)

---

## Now

1. **sola-agent-terminal** — **partial (Grok hooks)** (this branch)  
   **Freeze:** [`docs/specs/2026-08-13-sola-agent-terminal-design.md`](docs/specs/2026-08-13-sola-agent-terminal-design.md)  
   **Product:** [`crates/sola-agent-terminal/PRODUCT.md`](crates/sola-agent-terminal/PRODUCT.md)  
   **Idea:** [`docs/ideas/2026-08-12-sola-agent-terminal.md`](docs/ideas/2026-08-12-sola-agent-terminal.md)  
   **Next:** projects + workspaces + spawn sibling.  
   **Do not invent:** D3 interims (name, worktree base, `sat` if down, Claude hooks).  
   **Install:** ask first. User installed; Grok marks + tmux reattach smoked.  
   **Now:** kit marks + Grok hooks + OSC 9999 + process-tree. Live PTY is
   `sat-ws-main` (stable; orphans adopted). Demo rows still in the rail.  
2. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on **master**)  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.  
3. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.  
4. **Follow-ups (unordered backlog):** float chrome, D1/D2, preview, mail,
   kvm clipboard, switcher FFM holdoff (`naturalethic/switcher-ffm-holdoff`
   unmerged), etc.  
   Orca Grok pane flash reclassified off Sola; shell Windows/composition
   hygiene + ordered multi-install merged from focus-flashing.

**Explicit holds:** none.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
warning cleanups; worktree hygiene the user asks for.

---

## Known dogfood state

| | **primary (local)** | **dist (QEMU)** |
|--|---------------------|-----------------|
| Role | Daily dogfood desktop | Installer / image engineering |
| Launch | Physical TTY → `/opt/sola/bin/sola` | `cargo make vm run` / `iso run` |
| Install root | `/opt/sola/bin/`, logs `/opt/sola/log/` | Guest image + `var/images/` products |
| Bus / UI | sticky `~/.config/sola/state.toml`; Iced + kit | Same stack inside guest when installed |
| Dist path | Shape 1 colleague module (`INSTALL.md`) | QEMU **vdb** install → loginless Sola **OK**; **ISO e2e pending** |
| Branch | This workspace: **`naturalethic/sola-agent-terminal`** (installed locally). Grok sidebar marks + `sat-ws-main` reattach smoked. Master dogfood: **`master`** | Feature work in worktrees / Orca workspaces |
| Arcade | Banner list + nest dogfooded (Core Keeper, PEAK); cache + ready-to-play filter + lazy banners; nest Steam exits on game quit; some titles still flaky | — |
| Nest paint | wayland+`-b`+`-S fit`; **no `-e`**; `--nested-steam` (no BPM) | — |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install. User installs and smokes.

**Useful:**

```bash
cargo make build
cargo make install arcade shell river   # only with your OK
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
```

---

## Locked models (do not re-litigate)

| Topic | Rule |
|-------|------|
| UI stack | **Iced + sola-kit** only for new apps; WebView host is apocrypha |
| Compositor | **River** external; **sola-river** is the bus ↔ Wayland bridge |
| IPC | **Sola Bus** (Unix socket events) + Wayland for surfaces/input |
| Process model | Multi-process; each app independently restartable |
| Theme | Bus `Topic::Theme` + kit semantic tokens/fonts; shell chrome tokens |
| Browser engines | **WPE primary**, CEF parallel; no `accelerated_osr` crate feature |
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents. **`sola-agent` is not the start of agent-terminal.** |
| Agent-terminal | Host **user-launched CLI agents in PTYs**. Spawn sibling is the fan-out verb. No ACP chat, no mailbox orchestration. |
| Agent-terminal CLI | **Grok is first-class.** Hooks, presence, OSC, and spawn always implement and test Grok first. Other CLIs are presence-only until Grok status is trustworthy. |
| Agent-terminal UI | Load **impeccable** (Operate) + **frontend-design** before any UI. Kit tokens/atoms/components may be refined; do not silently restyle other apps. |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
