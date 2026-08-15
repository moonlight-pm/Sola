# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-14 (default URL → sola-browser)

---

## Now

1. **Call plane on master** — freeze
   [`docs/specs/2026-08-13-sola-call-plane-design.md`](docs/specs/2026-08-13-sola-call-plane-design.md).
   Host `sola-call`; `solactl compositor` / `session`; kit `CallSetup`; shell
   screenshot via call. Fake bus pairs removed. **Needs install to dogfood.**
   Next consumer: **sola-agent-terminal** (merge master, register methods).
   **D3** (confirm gates) is open. Later list is in the freeze.  
3. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.  
4. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.  
5. **Follow-ups (unordered backlog):** float chrome, D1/D2, preview, mail,
   kvm clipboard, switcher FFM holdoff (`naturalethic/switcher-ffm-holdoff`
   unmerged), open URL single-instance handoff, etc.  
   Kit Overview **desk** + Select chrome + Button/Field/Form/Select
   pages are on `naturalethic/kit-design` (**needs install**). Card /
   Theme / remaining labs still lag.

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
| Branch | **master** (call plane 2026-08-14; unified sidebar 2026-08-13) | Feature work in worktrees / Orca workspaces |
| Browser | One chrome window + per-profile `--engine` helpers; instant Profiles switch (menubar + chrome-bar select); visited tabs/profiles paint from parked last-frames (miss blanks); omnibox load line + no submit blank-flash; instant tab close (no strip bounce); YouTube persists after quit; Bitwarden unlock/fill; **Create login** (save-then-fill); page ⌘C/⌘V + triple-click; passkey **get** (Google); chrome interactive with animated tabs (parked helpers hidden; shader pump); OSR IME + Shift+wheel + `<select>` dropdowns dogfooded; **default http(s) open** via sola-browser only (no Helium; install to re-register MIME; no single-instance yet) | — |
| Arcade | Banner list + nest dogfooded (Core Keeper, PEAK); cache + ready-to-play filter + lazy banners; nest Steam exits on game quit; some titles still flaky | — |
| Nest paint | wayland+`-b`+`-S fit`; **no `-e`**; `--nested-steam` (no BPM) | — |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install. User installs and smokes.

**Useful:**

```bash
cargo make build
cargo make install browser shell   # only with your OK
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log
```

---

## Locked models (do not re-litigate)

| Topic | Rule |
|-------|------|
| UI stack | **Iced + sola-kit** only for new apps; WebView host is apocrypha |
| Compositor | **River** external; **sola-river** is the bus ↔ Wayland bridge |
| IPC | **Sola Bus** (fan-out) + **sola-call** (request/reply) + Wayland for surfaces/input |
| Process model | Multi-process; each app independently restartable |
| Theme | Bus `Topic::Theme` + kit semantic tokens/fonts; shell chrome tokens |
| Browser | **CEF** in single `sola-browser` crate; no `accelerated_osr`; WPE path retired |
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
