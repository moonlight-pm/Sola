# CURRENT — living product focus

**Only session handoff.** Update when priority, next slice, or known runtime
state changes. Read after `AGENTS.md`. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Capability maturity:
[`docs/capabilities.md`](docs/capabilities.md).

**Decisions agents must ask about:**  
[`docs/open-questions.md` § Decision points](docs/open-questions.md#decision-points-ask-human).
Do not invent product policy.

**As of:** 2026-08-31 (Window menu + Super+K overlay **installed** `kit`+`shell`+`paint`+`terminal`; screenshot freeze-then-crop + `--app` toplevel capture; mail dest-UID undo + compose table; Super+H hide + river dead-pid prune; wrapper Edit/links/notify/huddle; codecs CEF + autoplay + vault/⌘G; shell Bluetooth + volume; `crates/sola-agent` retired; GPU idle → [`PERFORMANCE.md`](PERFORMANCE.md))

---

## Now

1. **Window menu + Super+K** — kit Window menu (zones, float, hide, cycle);
   shell injects it when an app omits it; Super+K shortcuts overlay
   (Omarchy chord). Freeze
   [`docs/specs/2026-08-31-window-menu-and-shortcuts-design.md`](docs/specs/2026-08-31-window-menu-and-shortcuts-design.md).
   Paint Crop moved to **⌘⇧K**. Terminal publishes the kit Window menu
   (after Edit). **Installed** `kit shell paint terminal` (debug, 2026-08-31).
   **Next:** desk-smoke Window menu + Super+K. Gaps: no zone checkmarks; chords not remappable.
2. **Screenshots** — Super+Shift+4 freeze-then-crop (RGBA still, no dim, GPU-ready before show). `solactl compositor screenshot --app` copies the window scene (`ext-image-copy-capture`); does not raise. **Installed** `river` + `shell` debug 2026-08-31. Desk: freeze picker smoked; `--app sola-arcade` 1434×2132 PNG. Gaps: occluded / Super+H-hidden `--app`; multi-output. Super+H hide is on master.
3. **Browser** — codecs CEF (H.264/AAC `canPlayType` `probably`) +
   `--autoplay-policy=no-user-gesture-required`; unified Bitwarden panel;
   **⌘G** new group (name focused+selected; hover pencil). Freeze
   [`docs/specs/2026-08-28-sola-browser-vault-panel-design.md`](docs/specs/2026-08-28-sola-browser-vault-panel-design.md).
   **Installed** `browser --release` 2026-08-30. **Next:** Steam store
   trailer desk smoke. Gaps: vault desk smoke; no item edit; notify no
   sound / actions / D-Bus.
4. **Shell Bluetooth + volume** (on master) —
   Bluetooth freeze
   [`docs/specs/2026-08-29-shell-bluetooth-menubar-design.md`](docs/specs/2026-08-29-shell-bluetooth-menubar-design.md);
   volume freeze
   [`docs/specs/2026-08-29-shell-audio-menubar-design.md`](docs/specs/2026-08-29-shell-audio-menubar-design.md).
   Nearby inquiry omits 6-pair hex addresses (`AA-BB-…`). Volume chip left of
   Bluetooth (`Panel::Audio`): default-sink level on the icon, popover slider
   + mute + output/input device pick (`pw-dump` / `wpctl`). Media keys still
   `solactl media`. **Installed** `shell` (debug, 2026-08-29). WH-CH520 paired
   on the desk. **Next:** desk-smoke volume keys vs chip, sink/source switch,
   dashed-MAC nearby filter.
5. **GPU idle** — living track:
   [`PERFORMANCE.md`](PERFORMANCE.md)
   (architecture regression table + capabilities row `gpu-idle`).
   Desk sample 2026-08-25: **mean 17.7%** at P8 / 21 W (was ~30–40%).
   Opaque-region **not fully smoked** (session apps did not re-exec).
   **Next:** restart those windows and re-measure, then River NVIDIA knobs.
6. **kit Morph2 sidebar** — frozen. sola-browser tab strip: groups and
   loose tabs **intermix** (no groups-on-top). **Installed** `kit` + `browser`.
7. **sola-terminal** — **partial** (on **master**; `terminal-polish` merged).
   Grid selection is kit neon `accent` (`#3dd6f5` @ 55%), not the graphite
   `selection` atom. Workspaces PTYs share the palette. **Installed**
   `terminal` (2026-08-21); Window menu **installed** 2026-08-31.
8. **sola-monitor** — **partial** (on **master**; `monitor-polish` merged).
   Bus + Call inspector on kit chrome (left plane rail, `list_item` log,
   inspector well, last-known stickies / live owners). Call traffic via
   `Role::Observer` + `Wire::Trace` (not RPC on the bus). Kit JSON
   highlighter. **Installed** `call` + `monitor` (debug, 2026-08-21).
   Desk smoke pending. GPU menubar ranking (SM % + VRAM) also lands.
9. **sola-mail** — **partial** (on **master**).
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
   Optimistic delete (row leaves immediately; rapid `d`/Trash does not
   wait on IMAP). A long scrolled list keeps its place (keyed rows +
   silent refresh of the loaded window).
   **This slice:** undo reverse-moves the **destination** IMAP UID
   (Trash UIDs are per-mailbox — `u` no longer restores a different
   trash row). Action toast auto-hides after 5s. Compose From/To/Cc/Subject
   is a caption-size table at full reader-pane width (body 13px). Inbox
   still tracks other clients (IDLE + 45s poll).
   **Install:** `mail` (debug, 2026-08-31). Self-restarts.
10. **Marketing site (sola.computer)** — **teaser live** at
   [https://sola.computer/](https://sola.computer/). Implemented as a Thoxa
   container (`Thoxa` repo `containers/sola`) on Wicket aulos (workload
   `sola`, image `sola-landing`). Paper
   [file](https://app.paper.design/file/01KZF8TSPFDJZ4APR05E2ADXBJ)
   **Teaser · Desktop / Mobile**; ISO notify form (SQLite `news`). Copy
   authority [`docs/marketing/PRODUCT.md`](docs/marketing/PRODUCT.md).
   **Gaps:** full Landing artboard not shipped; ISO download still unreleased.
   Root [`PRODUCT.md`](PRODUCT.md) remains the **desktop** product truth —
   do not overwrite it with site messaging.
11. **sola-workspaces** — **partial** (on master)  
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
   parent from `$SOLA_PANE_ID`) — **desk smoke pending**. CLI
   `workspace.spawn` is background unless `--select` (UI + / ⌘T still
   jump). Skill `sola-workspaces-cli`: “review/work ticket”, “create
   worktree”, “tell that grok” → spawn/exec, never steal the rail.
   Per-project
   startup script after sibling spawn (**Project → Startup Script…** /
   `project.startup`). Rail: Add project expands `~`;
   groups stack at the top; no grok/agent label on the row. Sibling
   hover close is the kit ×; root has no row close (Drop Project is menu-only).
   Launcher builtin **Workspaces** is in shell (`lucide/folders`).
   Shortcuts: ⌘T spawn, ⌘N new project, ⌘⇧↓ split down, ⌘⇧→ split
   right, ⌘W close pane. A workspace is one rail row even when split;
   the mark rolls up every Grok pane (waiting / needs-attention beats
   working beats done beats idle). Last pane close keeps the workspace.
   Dead last pane shows **Start new shell**; a split leaf that exits
   retracts. Quiet `×N` on the workspace row is the loudest Grok
   session in the tab (segments / checkpoints; `signals.json` can stay
   0). Switching a split attaches every leaf; hover does not spawn. Restart binds tmux by `SOLA_WS_PATH` / cwd
   — leftover sessions from a deleted workspace are quarantined, not
   attached to the next tab. Working ring spins (kit mark uses ms
   phase, not `as_secs_f32`). Rail marks reclaim on Grok
   `SessionStart` / `UserPromptSubmit` after `/new` or `grok -r`
   (was frozen on the old session). `StopCancelled` → done.
   Super-chord no longer latches LOGO: ⌘T / ⌘N / ⌘V used to swallow
   every later key until quit (River eats Super-up; the union was
   written back into `keyboard_mods`). Exiting Grok back to the shell
   idles the rail mark (grey disc; SessionEnd used to leave done).
   `workspace.rm` replies before it kills tmux (a pane closing itself
   no longer hangs `solactl` / leaves the working ring). Installed
   (self-restart).  
12. **sola-paint** — default MIME / `solactl open` dest; crop / rotate /
   flip / save; left tabs; kit `FilePicker`; **single-instance** (second
   spawn hands off); **zoom/pan**. Crop is **⌘⇧K** (⌘K is Super+K overlay).
   Screenshots stay on **preview**.
   Stage cache + off-thread decode; tabs persist via `PaintSession`.
   Reinstall `paint` to dogfood. Gaps: no clipboard image.
13. **Call plane** — on **master**. Host + `solactl compositor` / `session` +
   kit helper + `Role::Observer`. Workspaces registers owner `workspaces`
   (desk smoke pending). **D3** (confirm gates) is open. Catalog sticky
   on the bus still later (monitor observes the call socket instead).  
14. **sola-arcade / windowed gamescope** — **partial, dogfoodable** (on master)  
   Per-title nest: **Fit to window** or a locked resolution (default **1080p**).
   Fit follows the gamescope host frame on zone/float (nested mode-control +
   focused window at 0,0). Keep the game **fullscreen** for Fit. Nest passes
   `--cursor-scale-height` (desktop-sized host pointer; Factorio was huge).
   A–Z / Recent sort persists (`arcade-prefs.json`).  
   **Install:** standing OK to `install arcade` after each finished round.  
   - Fit rezone dogfooded (Factorio, fullscreen on).  
   - Backlog: Portal-class nest fails; residual flicker; title contrast;
     never-played owned without API.
15. **Distribution follow-through** (when resumed)** — ISO e2e, TZ, tarball.
16. **Follow-ups (unordered backlog):** IPC contract handshake
   ([idea](docs/ideas/2026-08-30-ipc-contract-compat.md) — mixed worktree
   installs; not Now); create-card; vault item edit;
   float chrome, D2, preview, kvm clipboard, switcher FFM holdoff
   (`naturalethic/switcher-ffm-holdoff` unmerged). Browser: org-vault fill
   **desk smoke**; hover × follows the pointer after close; scheme-less
   localhost / loopback is `http://`. Passkey **create** smoked. Page
   menu DevTools / Inspect Element; HTML5 drag/drop. Tab-group pockets
   **installed**: flush members, hairline rim, header drag moves the
   pocket, title drop is invalid, hole matches etch row height. Loose
   is a real section (no box). Extra opens only on the well under the
   ghost. **⌘G** new group **installed** 2026-08-29.

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
| Dist path | Shape 1 colleague module (`INSTALL.md`); tarball v0.1.1 **404**. From-source: `CONTRIBUTING.md` (`installRelease = false`) | QEMU **vdb** install → loginless Sola **OK**; **ISO e2e pending** |
| Branch | **master** (workspaces + wrapper + **scope**). Feature work in worktrees | Feature work in worktrees / Orca workspaces |
| Scope | **On master.** Pixel loupe: live follow, zoom 65×65…3×3, hex copy, remembered float. Desktop pointer visible; grid omits the sprite (patched wlroots + River). Installed `scope`+`river`+`shell`; compositor `/opt/sola/bin/river`. | — |
| Shell | Super+Shift+4 freeze-then-crop (no dim) + `--app` toplevel capture (no raise) **installed** `river`+`shell` debug 2026-08-31 (`sola-arcade` smoke). Super+H hide. Dead-pid prune; menubar remapped. Window menu + Super+K overlay **installed** `kit`+`shell`+`paint`+`terminal` debug 2026-08-31. | — |
| Paint | Installed first-pass; singleton + zoom/pan need `install paint`. Screenshots stay on preview (`install shell` if dest was flipped) | — |
| Browser | One chrome window + per-profile `--engine` helpers; instant Profiles switch; parked last-frames; omnibox load line; **copy URL** (left of field; committed page URL; check flash — **installed**); outside open **raises** the window (**installed** browser+shell); scheme-less localhost / loopback uses http; instant tab close (hover × opaque chip; follows pointer after close); **drag-reorder tabs** + width-aware titles (dogfooded); **tab groups** (kit inset pocket, flush members, hairline rim; header drag moves the pocket; title drop ignored; **⌘G** new group on a loose tab, name focused+selected; hover pencil to rename; strip has no right-click); **⌘V once** (focused-frame JS, not all-frames); **⌘-click** dogfooded (IMDb): Super+drag bindings **removed** (CSD titlebar still moves floats); JS href → chrome background tab **below current** (same group); ⌘T / xdg-open / `solactl open` append **loose at the bottom**. Super+Tab untouched. **page context menu** (kit; cancels empty CEF OSR strip); **hold back/forward** for session history; YouTube persists after quit; Bitwarden **unified vault** (one toolbar icon — lock / key / shield when this page has TOTP / fingerprint on passkey; search + type chips + item records; **Create login** via **+**; fill/cards/identities/TOTP/passkeys decrypt **org vaults** too — **installed** `browser` --release 2026-08-28, desk smoke next; create still personal); **downloads** (auto-save `~/Downloads`; toolbar icon + progress; flat panel; persist `shared/downloads.json`; dogfooded); unlock lifts the vault icon, accent = open panel; page ⌘C/⌘V + triple-click; passkey **get** (Google + **Gemini Exchange 2FA**; all-frames intercept; same-site coalesce — dogfooded); passkey **create** (vault confirm; new login or attach — **smoked**); OSR IME + Shift+wheel + `<select>`; **default http(s) open** via sola-browser only (no Helium); **single iced chrome** (`chrome.sock` handoff; second process does not reap helpers); tab strip no phantom `↓ N` chip. **⌘⇧T** reopens the most recently closed tab (stack of 25, per profile; **installed** `browser` 2026-08-28). **Notifications:** KenHerbert Allow → displayed + top-right desk card (wrap fix installed 2026-08-27). Steam store autoplay: codecs CEF + `--autoplay-policy` **installed** 2026-08-30 (`canPlayType` AAC/H.264 `probably`; reload trailer). | — |
| Monitor | **On master** (installed debug 2026-08-21): Bus/Call inspector, kit chrome, call observer. Desk smoke pending. GPU panel SM%/VRAM ranking lands with this merge | — |
| Mail | **On master, installed** `mail` (debug, 2026-08-31): dest-UID undo, 5s toast TTL, compose table at full pane width. No HTML engine / attachments | — |
| Terminal | **On master** (installed 2026-08-21): grid selection is neon accent wash (`#3dd6f5` @ 55%). Workspaces PTYs share the palette | — |
| Wrapper | **On master.** Slack paints. Edit, off-site links, desk notifications, huddle OSR + mic **smoked** 2026-08-29 (`wrapper` debug). | — |
| Arcade | **Installed** 2026-08-25. Banner list + nest dogfooded (Core Keeper, PEAK, Factorio); per-title **Fit / resolution** (default 1080p); live Fit follow dogfooded (Factorio rezone, fullscreen on). Standing OK to reinstall after each arcade update | — |
| Agent | **Retired** (this worktree). Crate gone; launcher has no **Agent**; settings no longer treats `sola-agent` as a system app. Leftover `/opt/sola/bin/sola-agent` from 2026-08-25 bulk until a later install. Daily agent work is Workspaces. | — |
| Nest paint | wayland+`-b`+`-S fit`; **no `-e`**; `--nested-steam` (no BPM); `-w/-h` from per-title nest; Fit live-pokes nested X only (never host `:0`) | — |

**Install policy:** agents never run `cargo make install` without explicit
permission for that install — except standing OK for **arcade** and
**workspaces** after each finished round of those apps. User smokes.

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
| Wrapper | **`sola-wrapper <id>`**; `app_id` is the configured id; per-id CEF profile under `~/.config/sola/wrapper/<id>/`; Applications catalog (`kind` + `url`); not sola-browser chrome |
| Agent product | **Workspaces** (`grok` CLI in PTYs). The iced ACP/Grok-leader GUI (`crates/sola-agent`) is **retired** — do not rebuild it or a multi-client ACP chat. |
| Workspaces | Host **user-launched CLI agents in PTYs**. Spawn sibling is the fan-out verb. No ACP chat, no mailbox orchestration. |
| Workspaces CLI | **Grok is first-class.** Hooks, presence, OSC, and spawn always implement and test Grok first. Other CLIs are presence-only until Grok status is trustworthy. |
| Workspaces UI | Load **impeccable** (Operate) + **frontend-design** before any UI. Kit tokens/atoms/components may be refined; do not silently restyle other apps. |
| Workspaces worktrees | **`<project-root>/.worktrees/<name>`** (D4.2). App may append `/.worktrees/` to the project's `.gitignore` on first spawn. |
| Workspaces merge / drop | Never remove a git worktree or rail tab unless asked. Merge/LGTM is merge only. "Clean up this worktree" / "merge and clean up" / "remove this worktree, don't merge" also close the tab. If the worktree goes, the tab goes unless they say keep it. |
| Workspaces names | Crate / app id **`sola-workspaces`**. Window **Workspaces**. Owner **`workspaces`**. Tmux **`sola-ws`** / **`sws-`**. Config **`~/.config/sola/workspaces/`**. |
| Workspaces calls | Register on **sola-call** as owner `workspaces`. Face is `solactl workspaces …`. No `sat` binary. Fail if app/host down. First-class: verbs/payloads stay in lockstep with the app ([CLI freeze](docs/specs/2026-08-18-workspaces-cli-design.md)). |
| Gamescope host | Windowed only (`-W`/`-H`, never host `-f`); product path is Arcade nest |
| Super+K | Keyboard shortcuts overlay (Omarchy chord). Paint Crop is Super+Shift+K. |
