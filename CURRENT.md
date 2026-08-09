# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-09 (browser WPE-only; CEF removed — branch `browser-wpe-only`)

---

## Now

1. **Browser WPE-only (single crate)** — **on `naturalethic/browser`**  
   - Tag `pre-cef-removal` archives dual-engine tree.  
   - CEF removed; `sola-browser-core` folded into `sola-browser`
     (`src/wpe/` for engine/FFI). app_id `sola-browser`.  
   - **Next:** user install + smoke (`cargo make install browser shell`).
2. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (merged)  
   - **UI:** search + **A–Z / Recent** + **Ready to play only** (default on);
     lazy viewport banners; Play / Store / Uninstall; **Install** + faded
     uninstalled rows; Stop-on-row; scroll preserved.  
   - **Library:** `~/.config/sola/arcade-library.json` cache (instant open);
     background rescan every start; first-scan status when no cache.  
   - **Nest / river:** `--nested-steam` (no BPM); kill nest Steam on game quit;
     zone/float + Cinema exit; host label; `-S fit`.  
   - Manual: [`docs/manual/sola-arcade.md`](docs/manual/sola-arcade.md).  
   - **Next polish (backlog):** Portal-class nest fails; residual flicker;
     title contrast on bright heroes; never-played owned without API.  
3. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.  
4. **Progress docs** — keep this file + capabilities honest.  
5. **Follow-ups (unordered backlog):** float chrome, D1/D2, preview, mail,
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
| Branch | **`master`** (post–windowed-gamescope merge) | Feature work in worktrees / Orca workspaces |
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
| Browser engine | **WPE only** (`sola-browser`); CEF removed (tag `pre-cef-removal`) |
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
