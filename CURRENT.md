# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-10 (`naturalethic/browser`) — scroll no crash; quality telem live

---

## Now

1. **sola-browser hardening** — **priority**  
   - Shape: single crate WPE (`src/` chrome + `src/wpe/`); CEF gone
     (`pre-cef-removal`); installed to `/opt/sola/bin/sola-browser` + shell.  
   - Full review + backlog:
     [`docs/plans/2026-08-09-sola-browser-hardening.md`](docs/plans/2026-08-09-sola-browser-hardening.md).  
   - **Paint / YouTube crash fixed (P0):**  
     [`docs/plans/2026-08-10-browser-paint-investigation.md`](docs/plans/2026-08-10-browser-paint-investigation.md)
     — SEGV + scroll freeze fixed (buffer ref + RLIMIT_NOFILE + lean holds).  
   - **Paint quality (active):** headless→iced path improved but not
     daily-driver. **Next architecture:** content plane freeze
     [`docs/specs/2026-08-11-sola-browser-content-plane-design.md`](docs/specs/2026-08-11-sola-browser-content-plane-design.md)
     — iced chrome + Wayland content surface (River presents); **awaiting
     human lock** then Phase 0 spike (G1–G4).  
   - **D8 profiles** shipped (no switcher).  
   - **Still ask:** **D5** middle-click, **D6** search; **content-plane
     freeze lock**.  
   - **Next:** lock freeze → content-plane spike → cookie stickiness →
     vault → history → NV12 (P1).  

2. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.  
3. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.  
4. **Progress docs** — keep this file + capabilities honest.  
5. **Follow-ups (unordered backlog):** float chrome, D1/D2, preview, mail,
   kvm clipboard, switcher FFM holdoff (`naturalethic/switcher-ffm-holdoff`
   unmerged), etc.  
   Shell Windows/composition hygiene + ordered multi-install on master.

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
| Branch | **`naturalethic/browser`** (merged master) | Feature work in worktrees / Orca workspaces |
| Arcade | Banner list + nest dogfooded (Core Keeper, PEAK); cache + ready-to-play filter + lazy banners; nest Steam exits on game quit; some titles still flaky | — |
| Nest paint | wayland+`-b`+`-S fit`; **no `-e`**; `--nested-steam` (no BPM) | — |
| Browser | Paint: headless render_buffer hijack + owned blit; YT home scroll dogfood OK metrics; OpenUrl; D8 | — |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install — **except** standing OK to install
`sola-browser` after each finish on this branch (user 2026-08-10).

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
