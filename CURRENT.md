# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-21 (`terminal-polish` merged: neon grid selection; monitor + mail on master)

---

## Now

1. **sola-terminal** — **partial** (on **master**; `terminal-polish` merged).
   Grid selection is kit neon `accent` (`#3dd6f5` @ 55%), not the graphite
   `selection` atom. Workspaces PTYs share the palette. **Installed**
   `terminal` (2026-08-21).
2. **sola-monitor** — **partial** (on **master**; `monitor-polish` merged).
   Bus + Call inspector on kit chrome (left plane rail, `list_item` log,
   inspector well, last-known stickies / live owners). Call traffic via
   `Role::Observer` + `Wire::Trace` (not RPC on the bus). Kit JSON
   highlighter. **Installed** `call` + `monitor` (debug, 2026-08-21).
   Desk smoke pending. GPU menubar ranking (SM % + VRAM) also lands.
3. **sola-mail** — **partial** (on **master**; `mail-polish` merged).
   Letter reading (kit `prose`, HTML preferred); Mail.app list (bold
   unread, one-line subjects); always-on reader toolbar (icons +
   tooltips; message actions muted until a row is selected); scroll
   loads the next page; list selection is kit `list_item` graphite lift;
   list pointer is the default arrow (not an I-bar; no drag-copy of row
   text). IMAP lists via `SELECT`+`FETCH`. Empty Junk/Trash batches +
   toasts. In-body drag-select + Edit Copy / Select All (visible text;
   Copy Message still flattens URLs). Magic-link / long first-party URLs
   stay visible (Wicket “Sign in” mail). Menubar inbox unread chip
   (accent; click raises mail; hidden when mail is closed).
   **Install:** ask; last slice was `install mail`.
4. **Marketing site (sola.computer)** — **teaser live** at
   [https://sola.computer/](https://sola.computer/). Implemented as a Thoxa
   container (`Thoxa` repo `containers/sola`) on Wicket aulos (workload
   `sola`, image `sola-landing`). Paper
   [file](https://app.paper.design/file/01KZF8TSPFDJZ4APR05E2ADXBJ)
   **Teaser · Desktop / Mobile**; ISO notify form (SQLite `news`). Copy
   authority [`docs/marketing/PRODUCT.md`](docs/marketing/PRODUCT.md).
   **Gaps:** full Landing artboard not shipped; ISO download still unreleased.
   Root [`PRODUCT.md`](PRODUCT.md) remains the **desktop** product truth —
   do not overwrite it with site messaging.
5. **sola-workspaces** — **partial** (on master)  
   **Freeze:** [`docs/specs/2026-08-13-sola-agent-terminal-design.md`](docs/specs/2026-08-13-sola-agent-terminal-design.md)  
   **Call plane:** [`docs/specs/2026-08-13-sola-call-plane-design.md`](docs/specs/2026-08-13-sola-call-plane-design.md)  
   **Product:** [`crates/sola-workspaces/PRODUCT.md`](crates/sola-workspaces/PRODUCT.md)  
   **CLI freeze:** [`docs/specs/2026-08-18-workspaces-cli-design.md`](docs/specs/2026-08-18-workspaces-cli-design.md)  
   **Next:** desk-smoke `solactl workspaces` (needs `install workspaces` + `solactl`). Polish:
   rename/recolor/reorder.  
   **Do not invent:** D4 Claude hooks; call-plane **D3** confirm.  
   **Install:** standing OK to `install workspaces` after each finished
   round. Ask for any other target.  
   **Now:** persist + spawn + done toast. Crate/app id `sola-workspaces`.
   Methods on sola-call owner `workspaces` (`solactl workspaces ps` / `workspace.spawn` /
   `workspace.exec` / `pane.wait` / `whoami` / …). Control plane is
   first-class: verb changes update `calls.rs` + dispatch + tests +
   `docs/manual/solactl.md` together.
   Config `~/.config/sola/workspaces/` (migrates `agent-terminal/`). Tmux
   `sola-ws` / `sws-`. App installed and dogfooded (rail, splits,
   drop-project, dead-pane, `×N`). `solactl workspaces` implemented (richer
   payloads, `--prompt-file`, `project.add`, `workspace.select` /
   `workspace.exec`, `pane.wait`, `whoami`; Grok-leaf targeting;
   parent from `$SOLA_PANE_ID`) — **desk smoke pending**. Per-project
   startup script after sibling spawn (**Project → Startup Script…** /
   `project.startup`). Rail: Add project expands `~`;
   groups stack at the top; no grok/agent label on the row. Sibling
   hover close is the kit ×; root has no row close (Drop Project is menu-only).
   Launcher builtin **Workspaces** is in shell (`lucide/folders`).
   Shortcuts: ⌘T spawn, ⌘N new project, ⌘⇧↓ split down, ⌘⇧→ split
   right, ⌘W close pane. Split leaves appear under the workspace
   (`grok` / `shell`); last pane close keeps the workspace. Dead last
   pane shows **Start new shell**; a split leaf that exits retracts.
   Quiet `×N` only on a Grok leaf (session dir segments /
   checkpoints; `signals.json` can stay 0 after a compact). Split
   labels follow presence. Switching a split attaches every leaf;
   hover does not spawn. Restart binds tmux by `SOLA_WS_PATH` / cwd
   — leftover sessions from a deleted workspace are quarantined, not
   attached to the next tab. Working ring spins (kit mark uses ms
   phase, not `as_secs_f32`). Rail marks reclaim on Grok
   `SessionStart` / `UserPromptSubmit` after `/new` or `grok -r`
   (was frozen on the old session). `StopCancelled` → done.
   Installed and restarted.  
6. **sola-paint** — default MIME / `solactl open` dest; crop / rotate /
   flip / save; left tabs; kit `FilePicker`; **single-instance** (second
   spawn hands off); **zoom/pan**. Screenshots stay on **preview**.
   Stage cache + off-thread decode; tabs persist via `PaintSession`.
   Reinstall `paint` to dogfood. Gaps: no clipboard image.
7. **Call plane** — on **master**. Host + `solactl compositor` / `session` +
   kit helper + `Role::Observer`. Workspaces registers owner `workspaces`
   (desk smoke pending). **D3** (confirm gates) is open. Catalog sticky
   on the bus still later (monitor observes the call socket instead).  
8. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.
9. **Distribution follow-through (when resumed)** — ISO e2e, TZ, tarball.
10. **Follow-ups (unordered backlog):** create-card; float chrome, D1/D2,
   preview, kvm clipboard, switcher FFM holdoff
   (`naturalethic/switcher-ffm-holdoff` unmerged). Browser: Bitwarden
   fill decrypts **org vaults** (desk smoke after `install browser`);
   hover × follows the pointer after close; scheme-less localhost /
   loopback is `http://`. Passkey **create** smoked. Page menu
   DevTools / Inspect Element; HTML5 drag/drop.

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
| Branch | **master** (workspaces + browser + paint + mail + monitor + **terminal**). Worktree `browser-polish` kept open | Feature work in worktrees / Orca workspaces |
| Paint | Installed first-pass; singleton + zoom/pan need `install paint`. Screenshots stay on preview (`install shell` if dest was flipped) | — |
| Browser | One chrome window + per-profile `--engine` helpers; instant Profiles switch; parked last-frames; omnibox load line; **copy URL** (left of field; committed page URL; check flash — **installed**); outside open **raises** the window (**installed** browser+shell); scheme-less localhost / loopback uses http; instant tab close (hover × opaque chip; follows pointer after close); **drag-reorder tabs** + width-aware titles (dogfooded); **tab groups** (kit inset pocket + nested members; selected title no longer shifts — **installed**); **⌘V once** (focused-frame JS, not all-frames); **⌘-click** dogfooded (IMDb): Super+drag bindings **removed** (CSD titlebar still moves floats); JS href → chrome background tab **below current** (same group); ⌘T / xdg-open / `solactl open` append **loose at the bottom**. Super+Tab untouched. **page context menu** (kit; cancels empty CEF OSR strip); **hold back/forward** for session history; YouTube persists after quit; Bitwarden unlock/fill + **Create login** (fill/cards/TOTP/passkeys now decrypt **org vaults** too — **desk smoke pending** after `install browser`; create still personal); **cards** (separate toolbar button; list + checkout fill; dogfooded); **authenticator** (shield; site-matched TOTP; click-to-copy); **downloads** (auto-save `~/Downloads`; toolbar icon + progress; flat panel; persist `shared/downloads.json`; dogfooded); unlock lifts both icons, accent = open panel; page ⌘C/⌘V + triple-click; passkey **get** (Google + **Gemini Exchange 2FA**; all-frames intercept; same-site coalesce — dogfooded); passkey **create** (vault confirm; new login or attach — **smoked**); OSR IME + Shift+wheel + `<select>`; **default http(s) open** via sola-browser only (no Helium); **single iced chrome** (`chrome.sock` handoff; second process does not reap helpers); tab strip no phantom `↓ N` chip. | — |
| Monitor | **On master** (installed debug 2026-08-21): Bus/Call inspector, kit chrome, call observer. Desk smoke pending. GPU panel SM%/VRAM ranking lands with this merge | — |
| Mail | **On master:** letter pane; HTML preferred; unread **bold**; always-on icon toolbar; scroll-to-load; graphite `list_item`; SEARCH-free folder lists; empty batches; in-body drag-select + copy (list rows stay the default pointer, not copyable); magic-link / long first-party URLs stay clickable; menubar unread chip (`Topic::MailStatus`). Still no HTML engine / attachments | — |
| Terminal | **On master** (installed 2026-08-21): grid selection is neon accent wash (`#3dd6f5` @ 55%). Workspaces PTYs share the palette | — |
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
| Workspaces names | Crate / app id **`sola-workspaces`**. Window **Workspaces**. Owner **`workspaces`**. Tmux **`sola-ws`** / **`sws-`**. Config **`~/.config/sola/workspaces/`**. |
| Workspaces calls | Register on **sola-call** as owner `workspaces`. Face is `solactl workspaces …`. No `sat` binary. Fail if app/host down. First-class: verbs/payloads stay in lockstep with the app ([CLI freeze](docs/specs/2026-08-18-workspaces-cli-design.md)). |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
