# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-08 (`windowed-gamescope` — Arcade nest dogfoodable)

---

## Now

1. **sola-arcade / windowed gamescope (this worktree)** — **partial, dogfoodable**  
   - **UI:** search-only chrome; banner rows; Play / Store / Uninstall; Stop on
     active row; **scroll preserved** on launch/stop; session stickiness via
     `/proc` cmdline (NUL-normalized).  
   - **Nest:** cold Steam → `gamescope … -- sola-arcade --nested-steam <id>`
     (no BPM; prepare/shaders in nest; **kill nested Steam when game exits**;
     never host `-f`; no `-e`).  
   - **River/shell:** gamescope zone/float; Cinema exits fullscreen on next
     zone; float uses nest 16:9; Meta-resize free during drag; empty `app_id`
     → `gamescope`; Arcade game title for menubar/switcher.  
   - Manual: [`docs/manual/sola-arcade.md`](docs/manual/sola-arcade.md).  
   - **Next polish:** Portal-class nest fails; residual flicker; title contrast
     on bright heroes.  
2. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.  
3. **Progress docs** — keep this file + capabilities honest.  
4. **Follow-ups (unordered backlog):** float chrome, D1/D2, preview, mail,
   kvm clipboard, etc.

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
| Branch | **`windowed-gamescope`** | Feature work in worktrees / Orca workspaces |
| Arcade | Banner list + nest dogfooded (Core Keeper, PEAK); nest Steam exits on game quit; some titles still flaky | — |
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
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
