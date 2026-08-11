# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-11 (`naturalethic/browser`) — plane frame-paced present; dogfood residual

---

## Now

1. **sola-browser hardening** — **priority**  
   - Shape: single crate WPE (`src/` chrome + `src/wpe/` + `src/content_plane/`);
     CEF gone (`pre-cef-removal`).  
   - Freeze:
     [`docs/specs/2026-08-11-sola-browser-content-plane-design.md`](docs/specs/2026-08-11-sola-browser-content-plane-design.md)
     — **implemented, partial quality**.  
   - **Paint path (as-built):** default `SOLA_BROWSER_CONTENT=plane` — iced
     chrome + Wayland subsurface dma-buf (River presents); empty input
     region; hold until `wl_buffer.release`; **frame-callback paced attach**
     (no force-release of on-screen buffers); content scale **2×** default
     (`SOLA_BROWSER_DPR` override); fallback `import`.  
   - **User dogfood (YT homepage):** re-check black/flicker after frame-pace
     fix; soft text / daily-driver bar still open until confirmed.  
   - **Dogfood URLs:** `solactl emit OpenUrl` only — **never** `solactl open`
     (Helium / D3).  
   - **D8 profiles** shipped (no switcher).  
   - **Still ask:** **D5** middle-click, **D6** search.  
   - **Next:** dogfood YT hard-scroll (black/flicker) → blur polish →
     cookie stickiness → vault session.  

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
| Browser | Content plane default; frame-paced present; **re-dogfood black/flicker**; soft text open; OpenUrl bus; D8 | — |

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
