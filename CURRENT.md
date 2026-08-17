# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-17 (browser polish: ⌘-click + vault authenticator)

---

## Now

1. **sola-workspaces** — **partial** (on master; this worktree kept open)  
   **Freeze:** [`docs/specs/2026-08-13-sola-agent-terminal-design.md`](docs/specs/2026-08-13-sola-agent-terminal-design.md)  
   **Call plane:** [`docs/specs/2026-08-13-sola-call-plane-design.md`](docs/specs/2026-08-13-sola-call-plane-design.md)  
   **Product:** [`crates/sola-workspaces/PRODUCT.md`](crates/sola-workspaces/PRODUCT.md)  
   **Next:** dogfood install (`sola-call`, app, `solactl`, shell). Polish:
   rename/recolor/reorder.  
   **Do not invent:** D4 Claude hooks; call-plane **D3** confirm.  
   **Install:** ask first.  
   **Now:** persist + spawn + done toast. Crate/app id `sola-workspaces`.
   Methods on sola-call owner `ws` (`solactl ws ps` / `workspace.spawn` / …).
   Config `~/.config/sola/workspaces/` (migrates `agent-terminal/`). Tmux
   `sola-ws` / `sws-`. Hooks + old `sat-ws-main` reattach smoked earlier;
   spawn UI / call methods not smoked.  
2. **sola-paint** — default MIME / `solactl open` dest; crop / rotate /
   flip / save; left tabs; kit `FilePicker`; **single-instance** (second
   spawn hands off); **zoom/pan**. Screenshots stay on **preview**.
   Stage cache + off-thread decode; tabs persist via `PaintSession`.
   Reinstall `paint` to dogfood. Gaps: no clipboard image.
3. **Call plane** — on **master**. Host + `solactl compositor` / `session` +
   kit helper. Workspaces already registers `ws` (unsmoked). **Needs install
   to dogfood.** **D3** (confirm gates) is open.  
4. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.
5. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.
6. **Follow-ups (unordered backlog):** create-card; float chrome, D1/D2,
   preview, mail, kvm clipboard, switcher FFM holdoff
   (`naturalethic/switcher-ffm-holdoff` unmerged). Browser polish on this
   worktree: **⌘-click** dogfooded (IMDb) — Super+drag removed; chrome
   opens a background tab **below the current tab** (same group). Vault
   **authenticator** (site-matched TOTP, click-to-copy) installed.
   Passkey **create** still needs Outline smoke.

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
| Branch | **master** (workspaces + browser polish + paint). This worktree `naturalethic/browser-polish` is synced with master | Feature work in worktrees / Orca workspaces |
| Paint | Installed first-pass; singleton + zoom/pan need `install paint`. Screenshots stay on preview (`install shell` if dest was flipped) | — |
| Browser | One chrome window + per-profile `--engine` helpers; instant Profiles switch; parked last-frames; omnibox load line; instant tab close (hover × opaque chip); **drag-reorder tabs** + width-aware titles (dogfooded); **tab groups** (on master); **⌘V once** (focused-frame JS, not all-frames); **⌘-click** dogfooded (IMDb): Super+drag bindings **removed** (CSD titlebar still moves floats); JS href → chrome background tab **below current** (same group). Super+Tab untouched. **page context menu** (kit; cancels empty CEF OSR strip); **hold back/forward** for session history; YouTube persists after quit; Bitwarden unlock/fill + **Create login**; **cards** (separate toolbar button; list + checkout fill; dogfooded); **authenticator** (shield; site-matched TOTP; click-to-copy); **downloads** (auto-save `~/Downloads`; toolbar icon + progress; flat panel; persist `shared/downloads.json`; dogfooded); unlock lifts both icons, accent = open panel; page ⌘C/⌘V + triple-click; passkey **get** (Google + **Gemini Exchange 2FA**; all-frames intercept; same-site coalesce — dogfooded); passkey **create** (vault confirm; new login or attach — **needs Outline dogfood**); OSR IME + Shift+wheel + `<select>`; **default http(s) open** via sola-browser only (no Helium); **single iced chrome** (`chrome.sock` handoff; second process does not reap helpers); tab strip no phantom `↓ N` chip. | — |
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
| Agent backend | Attach to **shared Grok leader** — do not spawn private turn-loop agents. **`sola-agent` is not the start of Workspaces.** |
| Workspaces | Host **user-launched CLI agents in PTYs**. Spawn sibling is the fan-out verb. No ACP chat, no mailbox orchestration. |
| Workspaces CLI | **Grok is first-class.** Hooks, presence, OSC, and spawn always implement and test Grok first. Other CLIs are presence-only until Grok status is trustworthy. |
| Workspaces UI | Load **impeccable** (Operate) + **frontend-design** before any UI. Kit tokens/atoms/components may be refined; do not silently restyle other apps. |
| Workspaces worktrees | **`<project-root>/.worktrees/<name>`** (D4.2). App may append `/.worktrees/` to the project's `.gitignore` on first spawn. |
| Workspaces names | Crate / app id **`sola-workspaces`**. Window **Workspaces**. Owner **`ws`**. Tmux **`sola-ws`** / **`sws-`**. Config **`~/.config/sola/workspaces/`**. |
| Workspaces calls | Register on **sola-call** as owner `ws`. Face is `solactl ws …`. No `sat` binary. Fail if app/host down. |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
