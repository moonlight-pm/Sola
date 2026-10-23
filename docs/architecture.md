# Architecture

**Role:** as-built system map (what the code and local runtime look like
**now**).  
**Not** the place for multi-feature roadmaps or session priority.

| Concern | Document |
|---------|----------|
| Capability maturity | [`capabilities.md`](capabilities.md) |
| Target design freezes | [`specs/`](specs/) |
| Session priority + dogfood | Root [`CURRENT.md`](../CURRENT.md) |
| How docs fit together | [`progress-model.md`](progress-model.md) |
| Operator product docs | [`manual/`](manual/) — **shipped only** |

When implementation lands from a freeze, merge the **as-built** bits here and
update the freeze’s Implementation / Gaps header.

---

## Overview

Sola is a **Wayland desktop environment**: River as compositor, a typed IPC
bus, a call host, and multi-process **Iced** apps sharing `sola-kit`.

```text
                    ┌──────────────────────┐
                    │  sola (supervisor)   │
                    │  process manager     │
                    └──┬───┬───┬───┬───┬───┘
                       │   │   │   │   │
          ┌────────────┘   │   │   │   └────────────┐
          ▼                ▼   ▼   ▼   ▼            ▼
     ┌────────┐      ┌──────┐ ┌──────┐ ┌────────┐ ┌──────────┐
     │ River  │      │ bus  │ │ call │ │ river  │ │ session  │
     │(comp.) │      │ host │ │ host │ │ bridge │ │ manager  │
     └───┬────┘      └──┬───┘ └──┬───┘ └───┬────┘ └────┬─────┘
         │              │        │         │           │
         │              └────┬───┴─────────┴───────────┘
         │                   │  Unix sockets (sola-bus, sola-call)
         │              ┌────┴────────────────────────┐
         │              │  shell · settings · terminal │
         │              │  projects · browser          │
         │              │  wrapper · mail · calendar · bots · scope · spotify │
         │              └─────────────────────────────┘
         └──── Wayland (surfaces / input) ─────────────┘
```

Every managed component except River is **independently restartable**; River
exiting ends the session. Kit apps reconnect to the bus and tolerate
`sola-river` restarts.

---

## Repository layout

| Path | Role |
|------|------|
| `crates/sola` | Process manager (binary entry) |
| `crates/sola-bus` | Bus host + client library + topics |
| `crates/sola-call` | Call host + client library (request/reply) |
| `crates/sola-bots` | Named informational LLM sessions. Daemon `sola-botsd` (supervisor `MANAGED`, owner `bots`, HTTP `0.0.0.0:27419`, SSE `GET /events`) + iced **Bots** (HTTP/SSE client, same as the phone). Homes `~/Bots/<slug>/`. ACP `grok agent stdio` (yolo). Phone: SolaBot on Ember. `solactl bots` stays on sola-call. Not Projects. |
| `crates/sola-core` | Shared primitives (env, process, config, log, …) |
| `crates/sola-river` | River ↔ bus bridge |
| `crates/sola-session` | User-app session manager (spawn / close / reap) |
| `crates/sola-shell` | Menubar, launcher, switcher, Super+K shortcuts overlay, zoning, notification HUD, Bluetooth popover, volume popover + 12-band LED spectrum (iced daemon; BlueZ over system D-Bus; PipeWire via `pw-dump`/`wpctl`/`pw-cat`; Lamzu mouse battery over hidraw). Flower menu: Restart Shell, Quit Sola, Restart Computer, Shut Down (logind; on Oath `sudo oath set host:local power=reboot|halt` + `oath apply --confirm host:local`). |
| `crates/sola-kit` | Iced app kit + storybook (incl. `FilePicker`, shared **Window** menu, compositor clipboard helper for images) |
| `crates/sola-settings` | Settings panel (theme, apps, mail config, calendar accounts + shelf, …) |
| `crates/sola-oath` | **Oath** catalog app: packages, Needs / Used by, pins, apply, over `oath --json` (sudo for writes). Launcher entry **Oath** only when `/oath/INDEX.md` exists |
| `crates/sola-terminal` | Untitled-shell terminal (alacritty grid + iced). Also a **library** for the grid/PTY (`tmux::configure` for other sockets). On `next`: an `iced::daemon` with a window per tab (each a tab in the shell's bar via kit `tabs::TabWindows`), no tab strip, one process (`$XDG_RUNTIME_DIR/sola/terminal.sock`; a second launch emits `MenuAction new_tab` and exits). |
| `crates/sola-projects` | **Projects** (named Workspaces until 2026-10-21; "workspace" now means a desktop workspace): a window per project (a tab in the shell's bar, whose row shows the project's activity) with its tabs on a top bar, + agent-aware PTYs (tmux `sola-ws`). Catalog `~/.config/sola/projects/catalog.json` (migrated once from `workspaces/`, before that `agent-terminal/`; a `workspaces` key still loads and saves as `tabs`). Siblings under `<root>/.worktrees/`, or root tabs (`kind: root`, `tab.spawn --root`) in the project root itself — no worktree, no startup script, never `git worktree remove`d. Call owner `projects` (`solactl projects …`; methods: `ps`, `project.{list,add,rm,startup,reorder}`, `tab.{list,spawn,set,rm,select,exec}`, `pane.{list,send,read,wait}`, `whoami`). `tab.set --name` `git worktree move`s to `.worktrees/<slug>` (id stays; restamps `SOLA_WS_PATH`); `--title` is the top-bar label (default slug); `--before` reorders in the project; `--branch` is `git branch -m`. `tab.rm --worktree` also `git worktree remove`s (after the tab drops; tmux kill + git run off the iced thread); a gone checkout reaps the tab. `pane.send` / `tab.exec --prompt` tmux `load-buffer` + `paste-buffer -p` then Enter (not raw client-PTY dump). Per-project `startup` script runs in a new worktree after spawn. `project.rm` unregisters a project + kills its tmux, leaves worktrees. Attach stamps `SOLA_WS_PATH`; restart attaches only on path match and quarantines leftovers. `last-status.json` keeps the Grok `session_id`; a new tmux (reboot) execs `grok -r` when Grok was running and the session dir exists. Tmux session `-t` is exact (`=name`) so a parent id is not a prefix of a split leaf; pane/window commands use `=name:` (a bare `=name` is an exact pane name and fails). Grok hooks on `$XDG_RUNTIME_DIR/sola-ws-hooks.sock` (`POST /hook/grok`); Codex hooks merge into `~/.codex/hooks.json` (`POST /hook/codex`) without dropping Impeccable groups. Claude hooks merge into `~/.claude/settings.json` (`POST /hook/claude`) without dropping permissions, plugins, or other hooks. On start, existing Codex / Claude entries are found by script file name and rewritten in place (statusMessage "Sola Projects status"). OSC 9999 stripped in the term lib. Compaction `×N` on the tab row is the loudest Grok, Codex, or Claude pane. Grok reads `~/.grok/sessions/<encoded-cwd>/<sid>/` (`compaction/segment_*.md`, `compaction_checkpoints/`, then `signals.json` `compactionCount`). Codex counts `type: compacted` records in `~/.codex/sessions/**/rollout-*-{session_id}.jsonl`. Claude counts `subtype: compact_boundary` in `~/.claude/projects/<encoded-cwd>/<session_id>.jsonl` (non-alphanumeric cwd bytes become `-`). A tab's mark rolls up Grok, Codex, and Claude panes in a split (waiting > working > done > idle); each project window sends its project's roll-up as sticky `Topic::TabActivity` (working / waiting only) for its row in the shell's bar. Unfocused done/waiting emit `Topic::AppNotification` (title `{project} · {tab}`, body `grok is done` / `codex is done` / `claude is done` / `needs attention`). |
| `crates/sola-browser` | Iced chrome + CEF engine (single crate). Default web + PDF MIME / `xdg-open` (`sola-browser.desktop`). Web `Notification` → `Topic::AppNotification`. Call owner `browser` (`solactl browser …`). |
| `crates/sola-wrapper` | Website wrappers as first-class apps (`sola-wrapper <id>`; CEF via sola-browser lib; catalog `kind`/`url` on `Topic::Application`) |
| `crates/sola-design` | HTML artboards on a canvas, edited by agents. Own project list `~/.config/sola/design/projects.json`; canvases `<root>/design/<canvas>/canvas.toml`. One CEF board tab per artboard in one `--engine --profile=sola-design` helper; idle boards are discarded (tab closed, picture kept) when free memory runs short (`/proc/meminfo`). No editing UI. Call owner `design` (`solactl design …`: `project.{list,add,remove}`, `open`, `canvas.close`, `status`, `selection`, `view`, `wait`, `reload`, `screenshot`, `inspect`). |
| `crates/sola-mail` | Kit-native mail client. Emits sticky `Topic::MailStatus` (inbox unread) for the menubar; retracts on quit. Subscribes to Theme, MenuAction, CloseApp, Windows, WindowFloating, MailConfig only. `Topic::MailConfig`: IMAP-enabled accounts combined into six canonical boxes; `aliases` + `from_hidden` + extra SMTP `accounts` + `primary_from`. Threads: UI; coordinator (`sola-mail-worker`: config, account slots, fan-out jobs, retry backoff); per account a read session (`-r-`), write session (`-w-`: MOVE / delete / erase / append / move rules) and IDLE (`-idle-`) — 3 IMAP logins; SMTP send on a short-lived thread. Rows are keyed account + mailbox + UID. Attachments: IMAP `BODYSTRUCTURE` + MIME parts; SMTP `multipart/mixed`; kit FilePicker. Last-session list snapshot (versioned) under `~/.local/state/sola/mail/snapshot.json`. Gmail `CREATE Archive` if unmapped. |
| `crates/sola-calendar` | Kit-native calendar. Local **On This Computer** plus Google Calendar API v3 (shipped Desktop OAuth client, PKCE), iCloud / generic CalDAV, and ICS/webcal URL feeds. Accounts and calendar shelf (hidden / alias / colour) from persistent `Topic::CalendarConfig` (Settings → Calendar; multiple of each kind). Events `~/.local/state/sola/calendar/store.json`; reminders `reminders.json` (local offsets). View prefs `~/.config/sola/calendar/settings.json`. Due reminders emit `AppNotification` with a `sola-persist:` tag so the desk card stays until dismissed. Launcher **Calendar** (`lucide/calendar`). |
| `crates/sola-monitor` | System monitor: bus audit + call-plane observer |
| `crates/sola-kvm` | KVM / input bridge (novus server → Linux or Mac client) |
| `crates/sola-preview` | Argv / launcher image viewer (shell hotkeys copy screenshots to the clipboard) |
| `crates/sola-paint` | Default image viewer/editor (MIME, `solactl open`; singleton via `OpenImage`; tabs in `~/.config/sola/paint.yaml`) |
| `crates/sola-scope` | Pixel loupe: magnified grid around the pointer (`compositor.sample`) |
| `crates/sola-spotify` | Kit Spotify client: Web API + librespot Connect, MPRIS. Catalogue reads (playlists, rootlist, albums, artist top tracks) prefer the librespot session (`session_reads.rs`; no per-app quota) and fall back to the shared Web API app, whose traffic is budgeted (`api/client.rs` cooldown/probe/background slots; `worker.rs` page freshness, single-flight library, incremental liked sync, slow remote poll nudged by Connect cluster pushes). Disk writes from the UI go through one writer thread (`cache.rs`). Tokens + `skipped.json` + `liked.json` + `profile.json` (last profile, so sign-in skips `/me`) under `~/.local/state/sola/spotify/`; settings (last page + last track + last playlist + Back/Forward stack, max 20 back) `~/.config/sola/spotify/settings.json`; page/audio/art cache `~/.cache/sola/spotify/`. |
| `crates/sola-arcade` | Steam library browser + windowed-gamescope game launch |
| `crates/solactl` | Operator CLI (`compositor`, `session`, `projects`, `browser`, `bots`, `design`, emit, logs, …). `Path` args resolve against the caller's cwd before the call. `solactl mcp`: stdio MCP server — live owners' methods as `<owner>_<method>` tools (schema from `ArgSpec`), `tools/list_changed` on a 2 s catalog rescan, calls on their own thread, a reply `path` to a PNG / JPEG returned as image content. |
| `crates/sola-install` | Kit installer wizard + apply orchestration (`sola-install-apply`) |
| `crates/sola-make` | `cargo make` xtask (build / install / publish / **vm** / **iso**) |
| `crates/sola-assets` | Vendored icons/assets |
| `crates/iced_winit-patched` | iced 0.14 `iced_winit` + Wayland opaque-region from window-fill alpha (not a workspace member; `[patch.crates-io]`) |
| `nix/patches/` | River + wlroots patches (Xwayland destroy heal; live `pointer_position`; screencopy omits software cursor) |
| `nix/module.nix` | NixOS module (`services.sola`) — Shape 1 + images; wheel polkit YES for logind reboot/power-off (flower menu; no polkit agent) |
| `nix/sola.nix` | Package from GitHub release tarball |
| `nix/image/` | VM/image profile: quiet boot, Plymouth `sola`, installer kiosk, stage package |
| `nix/image/plymouth/` | Flower splash theme (clockwise cyan petal gradient frames) |
| `var/images/` | Local image products only (gitignored; not source) |
| `apocrypha/` | Legacy GTK4+WebKit host — **not built** |
| `docs/` | Engineering + operator docs (this suite) |

---

## Process and runtime model

| Concern | As-built |
|---------|----------|
| Language | Rust (edition 2024 workspace) |
| UI | Iced 0.14, wgpu, Wayland client |
| Compositor | External **River**; bridge **sola-river** |
| IPC control plane | **Sola Bus** over a Unix socket |
| IPC call plane | **sola-call** over `$XDG_RUNTIME_DIR/sola-call` |
| Surfaces / input | Wayland protocols via River. sola-river turns **NumLock on** for each keyboard (`river-xkb-config-v1`) so the number pad types digits; Super+Numpad zoning still matches both keysym sets. CEF OSR maps iced `Location::Numpad` + produced key to `VK_NUMPAD*` (unmodified `key` is still Home/End). |
| Launch | Dev: physical TTY → `/opt/sola/bin/sola`. Dist image: loginless `sola-desktop` → Sola. **Oath:** PID 1 runs `svc:sola-{bus,call,river,shell,session,kvm}` (and tmux) as the desk user; `crates/sola` refuses to start there |
| Directories | `sola_core::paths`: `bin_dir` / `share_dir` / `log_dir`, each `SOLA_BIN_DIR` / `SOLA_SHARE_DIR` / `SOLA_LOG_DIR`, else the platform default — NixOS `/opt/sola/{bin,share,log}`; Oath (`/oath/INDEX.md`) `/bin`, `/oath/store/pkg/sola/share`, `$XDG_STATE_HOME/sola` |
| Binaries | Dev install `/opt/sola/bin/`; images stage from `target/release` |
| Logs | `paths::log_dir()` (`/opt/sola/log/` on NixOS); stderr too, coloured only on a terminal |
| Wayland socket | The PM publishes `$XDG_RUNTIME_DIR/sola-wayland`; without it (Oath) sola-river and the session use a live `wayland-*` socket |
| Persistent stickies | Bus writes `~/.config/sola/state.yaml` (some topics have their own files, e.g. `theme/current.yaml`) |
| Browser vault session | `~/.local/share/sola/browser/vault-session.json` (age-encrypted tokens + user key; **Log out** deletes). Prefs `~/.config/sola/browser/vault.json`. |
| Browser HTTP auth | Per-profile `profiles/<uuid>/http-auth.json` (age-encrypted Basic/Digest; kit Sign in). |
| Arcade library cache | `~/.config/sola/arcade-library.json` (scan snapshot; bg rescan on open; `steamapps/` watch) |
| Arcade nest settings | `~/.config/sola/arcade-nest.json` (per-title Fit vs locked resolution; default 1080p) |
| Arcade singleton | `$XDG_RUNTIME_DIR/sola/arcade.lock.sock` (second spawn raises) |
| Spotify state | settings `~/.config/sola/spotify/settings.json` (last page + last track + last playlist + nav history, max 20 back); tokens + skipped + liked URIs + last profile `~/.local/state/sola/spotify/`; page/audio/art cache `~/.cache/sola/spotify/` |
| Projects catalog | `~/.config/sola/projects/catalog.json` (projects / tabs / selected; migrated once from `~/.config/sola/workspaces/`, before that `agent-terminal/`; a `workspaces` key still loads). Hook scripts (`grok-hook.sh`, `codex-hook.sh`, `claude-hook.sh`) live in the same dir |
| Projects last-status | `~/.config/sola/projects/last-status.json` (per-pane status / agent / Grok or Claude `session_id`; a new tmux after reboot execs `grok -r` or `claude --resume`) |
| Projects calls | sola-call owner `projects` (`solactl projects …`; no `workspaces` alias). First-class: [`2026-08-18-workspaces-cli-design.md`](specs/2026-08-18-workspaces-cli-design.md) (old names in the body) |
| Browser calls | sola-call owner `browser` (`solactl browser …`). Snapshot/act by `--tab`; CSS-pixel click/hover; `fill --file` / `get` / `links`; `wait --control`; `fill`/`type` paste into contenteditable; `key --chord` (one char); `scroll`. A timeout while the owner is still advertised is a wedged chrome (close X / Flower, relaunch). First-class: [`2026-09-11-sola-browser-agent-control-design.md`](specs/2026-09-11-sola-browser-agent-control-design.md) |
| Grok sessions | `~/.grok/sessions/` (Projects compaction `×N` + reboot resume; not an ACP leader socket) |
| Codex sessions | `~/.codex/sessions/YYYY/MM/DD/rollout-*-{id}.jsonl` (Projects compaction `×N`; `CODEX_HOME` override) |
| Self-update of apps | Binary watch → re-exec when `/opt/sola/bin/<name>` changes (`SOLA_NO_SELF_WATCH=1` skips) |

### Communication layers

1. **Sola Bus** — lifecycle, focus, themes, app menus, session commands,
   stickies. Fan-out facts. No request/reply. Mail unread is
   `Topic::MailStatus` (sticky, not persisted).  
2. **sola-call** — live method registry; request id, timeout, error to the
   caller. `solactl compositor` / `session`; kit apps advertise via
   `CallSetup` / `BusSetup::calls`. Fail if the owner is not connected.
   `Role::Observer` is a long-lived auditor: host fans out `Catalog`
   snapshots and `Trace` copies of invoke/reply/timeout/advertise/unregister.
   sola-monitor is the consumer (`install_observer`). RPC still does not
   travel on the bus.  
3. **Wayland** — buffers, seats, layers, xdg surfaces. Pixel and input plane.
   `compositor.screenshot` full/region uses `wlr-screencopy`; `--app` uses
   `ext-image-copy-capture` of the foreign toplevel (no raise).

### D-Bus (deprecated)

**D-Bus is deprecated as a requirement (2026-10-18).** Add no new D-Bus
dependencies. The uses below are legacy; the target is Sola's own
replacement on Sola Bus (facts) and sola-call (requests). Its shape is
open — [T3](open-questions.md#t3--d-bus-replacement-on-sola-bus-p2).

| Where | Bus | Use | How |
|-------|-----|-----|-----|
| `sola-shell` Bluetooth chip (`bluetooth/`) | system | `org.bluez` — adapters, devices, pairing agent | `zbus` in process |
| `solactl media` (shell media keys) | session | finds the Playing MPRIS player | `zbus` in process |
| `sola-spotify` (`mpris.rs`) | session | registers `org.mpris.MediaPlayer2.sola-spotify` | `mpris-server` |
| `sola-shell` flower Restart / Shut Down (`power.rs`) | system | logind reboot / power-off | `loginctl` / `systemctl` |
| `sola-session` app launch / stop | user systemd | `systemd-run --user --scope`, `systemctl --user stop` | CLI |
| CEF (`sola-browser`, `sola-wrapper`, `sola-design`) | — | Chromium links `libdbus-1` (nix-ld list in `nix/module.nix`) | third-party |
| Installer image | session | `dbus-run-session` around cage (`nix/image/installer-session.nix`) | image |

---

## Iced present / GPU idle (as-built)

Iced 0.14 **GPU-presents every window in the process after any `Message`**.
A timer or `window::frames()` is therefore a full-window present loop, not
a cheap poll. On this dogfood box (River 0.4.5, NVIDIA, **5120×2160**) that
showed up as ~30–40% GPU with several kit apps open. **Do not re-introduce
always-on vsync pumps** to “fix” a gesture or helper drain.

| Mitigation | Where | Idle rule | Drag / live exception | Regression to watch |
|------------|--------|-----------|------------------------|---------------------|
| No 16 ms chrome timer | `sola-browser` `subscription` | Page copy + context menu wake iced via `chrome_wake::wake` from the CEF router | Morph2 tab reorder is **not** this timer | Idle chrome presenting ~60 Hz; copy/right-click delayed >1 frame if wake is dropped |
| Chrome `Tick` is not a 250 ms pump | `sola-browser` `Msg::Tick` | Helper queues (tabs, downloads, handoff, vault, ⌘-click, copy/menu) **wake** `Tick`. 250 ms `time::every` only while copy-URL flash, vault TOTP on the open item, or vault fill-wait | Loading titles/progress still wake on `FromEngine::Tabs` | Strip titles / load line frozen; handoff (`solactl open`) ignored until another chrome event; copy-URL check never clears |
| Working ring `At` ~20 Hz | `sola-kit` `status_mark` | `RedrawRequest::At(50ms)` on `RedrawRequested`, **not** `NextFrame` / `window::frames()` | n/a | Storybook Sidebar or a Working Grok row pins GPU; ring frozen (no `RedrawRequested` after status change) |
| Projects pointer gated | `sola-projects` `subscription` | No `window::frames()`; `CursorMoved` only while a split drag is live; ignored mouse is not `Msg::Input` | Split divider drag | Split resize only updates on press; whole window presents on every pixel over empty chrome |
| Morph2 drag pump | `sola-kit` `sidebar/strip.rs` | Idle: no vsync chain | While `dragging` / FLIP: `invalidate_layout` on pointer (ghost Y is layout) + `request_redraw` on `RedrawRequested` | Tab/group reorder stutters or ghost stuck; idle vsync if `request_redraw` is left on when not dragging |
| Browser HTML5 drag | `sola-browser` `FrameSlot.page_drag` + shader `should_pump` | Idle: hangover 50 ms after last paint | While helper `FrameMeta.drag`: `request_redraw` without `Msg::NewFrame` | Kanban ghost lags / smears if overlay full-uploads; idle vsync if `page_drag` sticks after drop |
| Shell overlays parked 2×2 | `sola-shell` `ensure_overlay_windows` + `zoning::overlay_frame` | Menu / launcher / switcher / shortcuts / selection / **notify** stay mapped after the menubar’s first Composition. **Dismissed = 2×2 swapchain off-output** (`OVERLAY_PARK_X/Y` −10000; winit Wayland min is 2×1 — 1×1 + `resizable=false` is `xdg_toplevel` invalid_size). **Shown = live Frame while hidden, Composition after iced `Resized` ≥64×64** (next-tick hop so view/present run first). Notify live Frame is a tight top-right card stack, not the full usable area. Menu and Super+K live Frames are the card + shadow pad, not the full usable area. | River hides any window not in last Composition; do not stack a parked buffer | GPU spike if a dismissed overlay is left at full output; overlay visible before iced `Resized`; Super+Space hangs if `Resized` never fires; 1920 placeholder jump if live Frame forgets output size; **shell panic-loop if park size is 1×1**; notify overlay covering apps if Frame is full-area instead of card-sized; Super+K / menu filling 1080p on software GL |
| Tiled kit opaque-region | patched `iced_winit` `State::synchronize` | Kit apps still create an ARGB swapchain (`window_settings_transparent` for float CSD). **Tiled:** `theme_for(false)` opaque `background.base` → `wl_surface.set_opaque_region` (full) so River GLES can scan out. **Float / shell overlay:** transparent base → region cleared | n/a | Idle GPU if tiled windows stay without opaque-region; float CSD square black corners if opaque-region left on while overlay theme is active; overlay launcher dimmed wrong if marked opaque |
| Design plane | `sola-design` `subscription` | No idle timers: a 200 ms tick only while a `wait` / `auto` measure is pending, 40 ms while a detail plan is due, frames while a glow animates or the camera glides to a tour step, 60 s while a canvas is live off screen, 5 s while an agent's call in the last 30 s keeps a canvas awake. Board frames wake once per batch; helper queues wake `Poll` via `chrome_wake`; the rail rescan thread messages only when the canvas list changed; the canvas watcher only on file events | Pointer moves message only while right-drag panning or over a live page | Idle presents if a timer creeps in; boards blank if the frame wake is lost |
| Scope live sample | `sola-scope` `Msg::Tick` | 100 ms `time::every` **only while the loupe process is running** (the job is live pixels). No `window::frames()` | n/a | Scope closed: no extra presents. Open: ~10 Hz is expected |

**Still open (next slices):** River NVIDIA knobs (after clients stop
presenting). Browser `LeftPressed` / `CursorReleased` still fire on every
click (one present per click — acceptable).

**Law for new iced work:** if a widget needs motion, subscribe or
`request_redraw` **only while that motion is live**. Helper threads must
`chrome_wake::wake()` (or equivalent) instead of a process-wide timer.

---

## Shell windows (as-built)

`sola-shell` is a single `iced::daemon`. **Menubar** is always mapped.
Menu / launcher / switcher / shortcuts / selection / notify are **parked at 2×2 off-output** after
the menubar maps (River hides them until Composition). Show **Frames to the
live size while still hidden**, then joins the stack on the tick after iced
reports a live `Resized` — so a stretched 2×2 buffer is never shown. The
**notify** overlay Frames a tight top-right card stack; **menu** and
**Super+K** Frame to the card + shadow pad. Launcher / switcher / selection
still Frame the usable (or full) output. Iced does not present full-output
swapchains in the background (see
[Iced present / GPU idle](#iced-present--gpu-idle-as-built)).

| Kind | Role |
|------|------|
| Menubar | Top chrome, menus, mail unread chip (when `sola-mail` is mapped), missed-notification bell, mouse battery (Lamzu receiver over hidraw; hidden until the mouse answers), Bluetooth (hidden if no adapter), 12-band LED spectrum (own phrase, hidden if no PipeWire; click opens volume), pixel-graph stats (CPU/GPU/MEM/RX/TX), whispers (`AppToast`) |
| Menu | Open application menus + calendar / stat / notification-pile / Bluetooth / volume panels (parked 2×2 while dismissed) |
| Launcher | App launch (parked 2×2 while dismissed) |
| Switcher | MRU window/app switch (parked 2×2 while dismissed) |
| Shortcuts | Super+K cheatsheet (parked 2×2 while dismissed; live Frame is the card + shadow pad, not the usable output) |
| Selection | Super+Shift+4 freeze-then-marquee (RGBA still of the live output, then crop; parked 2×2 while dismissed; live Frame is full output) |
| Notify | Live notification cards (tight Frame under the menubar, trailing edge with the clock; parked 2×2 while empty) |
| Tab bar (`next` only) | Next's one tab list ([spec](specs/2026-10-21-next-tab-bar-design.md)): docked left under the menubar (240 px, one gap in: 10, 38 on canto), always mapped, composed right above the menubar. Kit `SidebarPanel` on a kit `island` (rounded, rimmed, ground-black corners): a section per zone (Left, Right, Floating), a row per window (app icon, title); click shows, drag moves between zones. Model in `stacks.rs` |

**On `next` (Next tab bar):** every app window is a tab in a zone. The two
tiled zones (Left / Right, 50 / 50 of the content area right of the bar;
an empty zone gives its width away) each show one tab; the shell leaves
the other tabs out of `Topic::Composition`, so River hides them and they
keep running. Floats (Floating zone; the loupe, Preview, gamescope, and
apps floated with Super+KP_*) all show. A window with a River parent
(sola-river's sticky `Topic::WindowParent`) is a dialog: no row, composed
above its parent. A new window joins the focused zone; a window back from a
restart or a session restore takes its app's place from the persistent
`Topic::TabLayout` (`~/.config/sola/shell/tabs.yaml`). An app with a window
per tab announces each window before it opens (sticky `Topic::Tab`: key, pid,
`seq`); the shell pairs a process's tabs with its new windows in order
(`pairing.rs`), answers sticky `Topic::TabWindow` (key → sola-river id), and
remembers the tab by its key. An app marks a tab's row with sticky
`Topic::TabActivity` (key → idle / working / waiting; kit
`TabWindows::set_activity`): the bar draws the mark in the row's icon slot.
Projects sends each project window's roll-up (a tab waiting on a decision
beats a tab working). `Topic::Zones` is no
longer read or written on `next`. Super+KP_4 / KP_6 move a tab, KP_0 /
KP_Enter show it Fullscreen (content area) / Cinema (output).

**On `next`, workspaces (slice 5):** the shell keeps a desk of zones per
desktop workspace (`stacks/`; readers without a window answer for the
showing one) and composes only the showing workspace; windows elsewhere
are hidden by River and keep running. The workspaces (id, name, order) and
which one shows persist inside `TabLayout` and go out as sticky
`Topic::Workspaces`; each window's workspace as sticky
`Topic::WindowWorkspace`. The switcher is pills in the menubar
(`menubar/pills.rs`); Super+1…9 / Super+Ctrl+1…9 / Super+Ctrl+Tab. The
browser runs one CEF helper per profile, all live: its router keeps a page
→ profile map and routes commands by page (the rest to the active page's
helper), merges every helper's pages, and makes a helper front only while
it has a page on screen or the active one; helper-minted page ids sit in a
per-profile range above chrome's. Each workspace picks a profile
(`profiles.json` `workspaces`); a page runs its workspace's profile and
reopens in place when that differs (`app/pages.rs`); sessions are saved
per profile.

**On `next`, window frames (slice 6):** one 10 px gap (`zoning::GAP`)
around the output under the menubar, between the bar and the tiles, and
between tiles. Every kit window draws its own frame (`sola-kit`
`components/window_frame`): a 38 px titlebar with the app's menus (the kit
keeps the app's latest `AppMenuPayload` from `BusSetup` / `menu::publish`;
a dropdown is an overlay inside the window; a Window item goes on the bus as
`MenuAction` and the shell acts on its focused window, any other item is
handed back to the app's own bus stream), 12 px corners, a lit rim. Tiles
stay opaque with their corners painted the ground; floats clear them. The
menubar carries no app menus. The shell publishes sticky
`Topic::WindowChrome` (window id → tiled, Cinema, rim; `chrome.rs`):
sola-river sends `set_tiled` on change and a 1 px border on windows whose
app published no menu (foreign apps); kit apps drop the frame in Cinema
(`FloatState::chrome`).

**On master:** zoning / floating is coordinated with `sola-river` over the
bus: unassigned windows **default-float** (client size +
`Topic::WindowFloating`); saved zones restore frames; Meta+numpad snaps
assign zones.

**Hide (Super+H):** sticky `Topic::AppHidden` omits that app’s surfaces from
`Topic::Composition` (River `hide` — not send-to-back). Restore: Super+Tab
(switcher still lists hidden apps), launcher on an already-running hidden app
(unhide + raise, no second spawn), or any `raise_app` path (OpenUrl, mail
unread, notification click). Last window of a hidden app retracts the sticky
so a later map is not stuck hidden. No menubar chip.

**Shell re-exec z-order:** sola-shell rebuilds MRU from window-id order, which
would shuffle apps. River keeps the last relative stack when only the
menubar id changed (click-raise still honors a new Composition).

A hard-killed `sola-shell` can leave parked surfaces in sola-river (no
`closed`). River prunes entries whose `/proc/<pid>` is gone so a respawn
can map a new menubar. Shell composition also ignores dead-pid windows.

### Games / Arcade (as-built)

| Piece | Role |
|-------|------|
| `sola-arcade` | Kit app: Steam library gallery (search; A–Z / Recent; Ready-to-play filter default on; Install on uninstalled; Stop-on-row). Singleton (`$XDG_RUNTIME_DIR/sola/arcade.lock.sock`); second spawn raises. |
| Library data | Offline: ACF manifests + `localconfig` activity + `appinfo.vdf` names; cache `~/.config/sola/arcade-library.json`; UI opens from cache, full scan always in background; debounced non-recursive watch on each `steamapps/` (ACF + `libraryfolders.vdf`) |
| Gallery prefs | `~/.config/sola/arcade-prefs.json` — A–Z / Recent sort (default A–Z) |
| Banners | Lazy viewport decode (+ overscan); paths resolved when row visible |
| Launch | `Topic::LaunchApp` → `sola-arcade --run <id> <w> <h> [fit]` → `gamescope … --cursor-scale-height <H> -- sola-arcade --nested-steam <id>` → desktop Steam `-applaunch` (no BPM). Play **refused** if a desktop Steam client is already running (no exclusive fullscreen). Nested helper kills nested Steam when the **game** `AppId=` reaper exits (not `Install=1` / `iscriptevaluator`). `<w> <h>` from per-title nest (Fit or locked res). Host cursor is downsampled to desktop size (nested X cursors otherwise present 1:1 to River). |
| Fit follow | Arcade UI watches `Topic::Windows` / `WindowGeometry` for `app_id=gamescope` and pokes **nested** X only (`DISPLAY` from `--nested-steam`, never gamescope's host `:0`), debounced ~250 ms. Writes `GAMESCOPE_XWAYLAND_MODE_CONTROL` + focused window `0,0,w,h`. Locked res does not follow. |
| Session lock | Active Play → Stop on that row; other Plays disabled. Session end is `UserAppExited` on `steam-game-<id>` (`--run` waits on gamescope). Stop: `CloseApp` then Arcade-owned pids only (`--run` / `--nested-steam` / that gamescope), never `pkill AppId=`. |
| River | gamescope pre-init pin then zone/float; Cinema exit-fullscreen on next zone Frame; empty app_id → `gamescope` via pid; nest `-S fit` letterbox |
| AppHidden | Super+H + switcher/launcher restore (no menubar chip); Arcade UI does not expose hide-Steam |

Operator: [`manual/sola-arcade.md`](manual/sola-arcade.md).

---

## Browser (as-built)

| Piece | Role |
|-------|------|
| `crates/sola-browser` | **Product browser** — iced chrome (a window per page, each a shell tab (`page_windows.rs`); the active page's window has the full-width bar: nav + omnibox + vault + downloads; the others a read-only address bar; unified Bitwarden panel with item edit) + CEF CPU OSR under `src/cef/` |
| App id / binary | `sola-browser` → `/opt/sola/bin/sola-browser` (shell launcher: one “Browser” entry; one Wayland window). `dist/applications/sola-browser.desktop` is the xdg-open default for http(s), HTML, XHTML, PDF, `about:`, unknown. |
| Engine helpers | Per-profile headless `sola-browser --engine --profile=<uuid>` (no iced / no xdg_toplevel). Control socket `profiles/<uuid>/engine.sock`; pixel frames on `engine.frame.sock` (raw BGRA, not bincode). Page copy is JS extract → `FromEngine::Clipboard` on the control socket → chrome writes Wayland. Paste is `ToEngine::PasteImage` (`File` event) or `ToEngine::PasteText` (synthetic `ClipboardEvent` + focused-frame fallback; not Chromium `insertText` of the whole payload — that flattens newlines). Chrome reads the compositor via `sola_kit::clipboard` (data-control), not iced, so an image offer is not dropped. Favicons: helper `DisplayHandler::on_favicon_urlchange` + `download_image` → `FromEngine::Favicon` (PNG) → chrome 16px leading slot (globe fallback). ⌘-click hit-test is JS → `FromEngine::OpenBackgroundTab` → chrome `open_tab_beside` (below current tab, same group; click not sent to CEF). `window.open` with features (`NEW_POPUP`) / `about:blank` is a windowless CEF browser (so `window.open` returns a `Window`); chrome adopts that engine tab and focuses it beside the opener. `target=_blank` / `NEW_WINDOW` cancel the native popup and open a focused chrome tab. ⌘T / `OpenUrl` / `xdg-open` use `open_tab` (loose, end of strip). An outside open (`chrome.sock` / `Topic::OpenUrl` with `activate`) asks the shell to raise the existing window (click-activation: MRU + composition + seat; sock probes are not activate). Browser → Developer Tools (and page-menu Inspect) docks a second windowless OSR browser under the page (bundled frontend over the helper `/json/list` websocket; dual `FrameSlot` page + inspector paints; `FromEngine::DevTools`). Chrome-runtime `ShowDevTools` cannot be windowless and is not used. It is not a strip tab. Super+left/right are not WM pointer bindings (CSD titlebar still moves floats). Page right-click is `ContextMenuHandler::run_context_menu` (native cancelled) → `FromEngine::PageContext` → kit menu. Session history rides the tab snapshot (`TabInfo.history`); hold-nav uses `NavCmd::GoHistory`. IME caret is `OnImeCompositionRangeChanged` → `FromEngine::ImeCaret` (view px) so chrome can `request_input_method` at the composition box. JS `alert` / `confirm` / `prompt` / leave-page: helper `JsDialogHandler` → `FromEngine::JsDialog` → kit modal; chrome replies `ToEngine::JsDialog`. Continue drops the pending-callback `RefCell` before calling CEF (OnReset re-enters). Park / navigate resets pending callbacks. Find: helper `FindHandler` + `BrowserHost::find` → `FromEngine::FindResult`; chrome `ToEngine::Find` / `StopFind`. Idle omnibox is a label until click / `⌘L` (no query/fragment; clipped). Edit paste uses the tracked omnibox flag — `is_focused` is `None` while the field is unmounted. Downloads: helper `DownloadHandler` → `FromEngine::Download` (any helper, including parked) → chrome list; cancel is `ToEngine::CancelDownload`. Passkey **get** / **create**: renderer V8 `OnContextCreated` eval (document-start) plus load-handler inject in every frame → `FromEngine::WebAuthn` → chrome vault picker (same-site same-action coalesced; advertises a platform authenticator so Google does not wait on Chromium’s OSR-invisible sheet; `create()` confirms then `Fido2Client::register` + POST/PUT cipher). One iced chrome (`chrome.sock`); a second process hands off a URL and exits. Chrome start **Shutdown**-flushes helpers (cookie store) before SIGTERM leftovers. Helper death respawns CEF and restores chrome’s tab list (does not resurrect closed tabs from a stale helper snapshot). Reap only orphan / pre-`exec_self` helpers. `<select>` is `PET_POPUP` blitted onto the VIEW CPU frame (not a second window). Only the front helper composites (`SetFront` + `was_hidden` + `windowless_frame_rate`). CEF `root_cache_path` = that profile’s `…/cef/` so cookies persist. |
| CEF pin | `cef` crate + workspace `cef-version`; `cargo make install-cef` unpacks the public tarball to `~/.cache/sola/cef-<ver>/`. H.264/AAC: replace that tree with `scripts/cef-codecs/` (same pin). |
| CEF switches | `--ozone-platform=wayland`; `--password-store=basic`; `--remote-allow-origins=*`; `--autoplay-policy=no-user-gesture-required`. Local `libcef` is a codecs rebuild of the same 147.0.10 pin (`proprietary_codecs=true ffmpeg_branding=Chrome`, `use_vaapi=false`; recipe `scripts/cef-codecs/`). Public cef-builds tarball had no H.264/AAC. No Widevine CDM; Steam store trailers are clear DASH, not EME. |
| Profiles (D8) | Registry `profiles.json`; data/cache under `profiles/<uuid>/`; session `session.json` (pages with stable ids + `closed[]` for ⌘⇧T; older files' groups and sidebar width are ignored); shared downloads index `shared/downloads.json`; visit history `shared/history.json` (omnibox); chrome parks page-list snapshots + last CPU composites (`FrameSlot.parked_frames`); switch points the router at the target helper (pages stay loaded). Eviction: `tab_cache` (idle 30m, max 4 parks, max 48 tabs total) |
| Tab / profile paint | `present_tab`: same-size parked frame → GPU this frame; miss → blank immediately (never keep the previous page). Helpers skip same-size `Resize` so the parked compositor stays live. Iced presents paints via the shader `request_redraw` pump (does not rebuild chrome at 60 Hz). Partial `on_paint` dirty copies only onto a uniquely-owned complete last-frame (the pixel ring does not pin that Arc); otherwise the CPU buffer full-blits from CEF’s view. Latest-wins hops absorb dropped-frame dirty into the survivor so GPU damage is continuous since the last texture upload. HTML5 OSR drag keeps `last_frame` clean and composites a host ghost with union dirty (`FrameMeta.drag`); skipped overlays keep ghost/view damage until the mailbox is empty; chrome does not park overlay frames; shader pumps while `page_drag` on the painted tab. Chrome `Tick` is `chrome_wake` from helper queues (plus a 250 ms timer only for copy-URL flash / TOTP / fill-wait), not an idle 16 ms or 250 ms pump. |

Former split (`sola-browser-core` / `-wpe` / `-cef` dispatcher) and the WPE
content-plane path are **retired**. CEF: do **not** enable `accelerated_osr`
(CPU `on_paint` path only).

---

## Wrapper (as-built)

| Piece | Role |
|-------|------|
| `crates/sola-wrapper` | Kit iced chrome (CSD while floating) + one CEF page. Not sola-browser (no tabs/omnibox/vault). |
| Binary / argv | `sola-wrapper <id>`; helper `sola-wrapper --engine --profile=<id>` (same binary, `current_exe`). |
| App id | The configured id (`slack`), set on kit `startup` / `window_settings_transparent`. |
| Catalog | `Topic::Application` fields `kind: wrapper` + `url`. Command synthesized `paths::bin_path("sola-wrapper")` (`/bin` on Oath, `/opt/sola/bin` on NixOS) + ` <id>`. Launch lookup: `state.yaml` (bus persistence). |
| Profile | Durable `~/.config/sola/wrapper/<id>/cef`; cache `$XDG_CACHE_HOME/sola/wrapper/<id>/`. Notification grants `…/notifications.json`; mic/camera `…/media.json`. `profiles::bind_external` so this is **not** `browser_data_root()`. |
| Chrome | Kit CSD while floating + one CEF page. **Edit** menubar (⌘X/C/V/A) via shell `MenuAction`; paste is compositor clipboard → `PasteImage` (`File`) or `PasteText`. Web `Notification` → desk card (`app_id` = wrapper id). Off-site `target=_blank` / ⌘-click / `window.open` → `sola_core::open_url` (sola-browser). Same-site / `about:blank` NEW_POPUP is a windowless CEF tab (huddle). `getUserMedia` → kit Allow / Block; Chromium `MEDIASTREAM_CAMERA` / `MEDIASTREAM_MIC` content settings follow `media.json`. |
| Singleton | `$XDG_RUNTIME_DIR/sola/wrapper/<id>.sock` — second spawn raises the live window. |

---

## Design (as-built)

| Piece | Role |
|-------|------|
| `crates/sola-design` | Kit iced app: project rail (kit sidebar, drag-reorder within sections) + plane. Not sola-browser (no tabs/omnibox/vault). |
| Binary / argv | `sola-design`; helper `sola-design --engine --profile=sola-design` (same binary). Singleton `$XDG_RUNTIME_DIR/sola/design.sock` — a second launch raises the window. |
| Profile | `profiles::bind_external("sola-design", ~/.config/sola/design, ~/.cache/sola/design)`; CEF user data `~/.config/sola/design/cef/`. Helper switches `--hide-scrollbars` and `--disable-gpu-compositing` (CPU frames: software compositing redraws only the damage). |
| Origin | `serve.rs`: std `TcpListener` on 127.0.0.1:0, thread per connection, GET/HEAD, single byte `Range` (206 / 416), `Connection: close`, `no-store`. A canvas `root` registers a web root per board host (looked up first, then the project root). `Host` = `<board>--<canvas>.<project>.localhost` → registered project root (`set_projects` from the catalog); path under root, no `..` / dotfile segments, dir → `index.html`. Board `file://` URLs are rewritten to it on every layout; settle resources map back through `local_path`. Falls back to `file://` if the server cannot start. |
| Document | Files on disk: `<root>/design/<canvas>/canvas.toml` lists `[[artboard]]` (`id`, `src` file or URL, `x`, `y`, `width`, `height` or `"auto"`, `title`) and annotations (`[[section]]`, `[[note]]`, `[[link]]`, `[[flow]]` — `annot.rs`, checked against the board ids); pages are plain HTML/CSS, exits are `data-design-to` attributes in them. `canvas.new` / the rail `+` scaffold it. |
| Annotations | `marks.rs`: what a page declares, from each settle reply (exits, plain links, pinned-note anchors; `board_for_url` matches a link to a board by path + query, the fragment to tell shared pages apart; own page / ambiguous → none) → `Board::marks`. `scene.rs`: canvas-space `Scene` (sections, arrows, clickable `hots`, notes, tour `choices`) built per view from the shown canvas. `routes.rs`: geometry — section boxes; connectors laid out in canvas units through the gutters (port level with the exit on the facing side, stub, orthogonal grid on lines beside every board's edges, Dijkstra over node × heading with a bend cost, target ports spread), memoised per layout in `scene.rs`; on screen: `nudge` (shared gutter runs into lanes), label spots, card placement. `annotate.rs`: `Sections` canvas **under** the board shader (`LoadOp::Load`) for regions; section titles, arrows, hot outlines, and notes drawn by the overlay. Overlay clicks on a hot region / note marker are the app's (`Event::Follow` / `ToggleNote`); → ← Esc 1–9 N with no page focused (`Event::Nav`). `tour.rs` + `app/tour.rs`: `Tour` (path of board ids), `Tween` glide (`camera::frame_board`, `lerp`, `ease`; human camera), tour bar + flows list above the pill. `report.rs`: `status` `annotations` / `exits` / `board_links`. The share viewer mirrors all of it in JS (exits read from its same-origin frames). |
| Board tabs | sola-browser `Cmd::OpenBoard` / `Cmd::SetBoard` carry a `BoardSize` (CSS size, raster scale, OSR `surface` px ≥ the board's pixels) and `Cmd::BoardDetail` a view request (+ `ToEngine` twins). Helper side in `cef/board.rs`: the page is zoomed by the scale (`set_zoom_level`; device scale stays 1 — this CEF's OSR ignores it) and its layout is **pinned** with `Emulation.setDeviceMetricsOverride` (width/height = board px), so `view_rect` can be the larger surface without the page seeing a resize. A detail request adds the DevTools `viewport` override (document px = (x + scroll)·scale, scale relative to the raster; a compositor transform — no relayout; input is not transformed). Frames carry `CefFrame::view`: the newest request `seq` Chromium acknowledged. `Cmd::BoardPace` sets how often it paints (`BoardView::pace`): Live = shown at 60 fps, Slow = shown at 1 fps, Asleep = `was_hidden(1)` (waking invalidates the view); `SetBoard` keeps the pace. The helper waits for (then SIGTERMs) a previous helper of its profile still shutting down (`engine.pid`) before CEF starts — one CEF per cache dir. |
| Plane | `plane.rs` wgpu shader: per board a `Bgra8UnormSrgb` texture with a mip chain (the board's own picture, cropped from the frame's top-left with `cpu_import::upload_cropped`) and an optional one-level detail texture (the frame as is, placed by its request); frames are routed by `view` (`detail::View` table, current + previous request), dirty rects trusted only between frames of one view. `overlay.rs` iced canvas draws frames, titles, the pick outline, agent glows, and owns input (right-drag pan, wheel zoom, Super+click pick, IME preedit/commit, everything else to the page), wrapped by `ime.rs` (requests the input method at the page caret while a board has the keyboard). Floating status pill: zoom % + pick. Board frames arrive whole once per size / view, then as **patches** (only the 32 px tiles that changed, packed; `CefFrame::patch`) written straight into the texture (`cpu_import::upload_patch`), mips rebuilt only over the changed region; a patch with no picture under it → `Cmd::BoardRepaint`. Each board's frame rate follows the view (`pace.rs` → `Cmd::BoardFrameRate`: 60 on screen at a readable zoom, 10 tiny, 4 off screen / hidden canvas, 60 while loading); only on-screen boards' frames wake iced, paced to the fastest of them. |
| Raster | Per board `boards::raster_scale`: 2×, else 1×, else the hundredth that fits (8192 px texture side / 160 MiB frame), lowered to the least on its host (`host_scales`; served boards each have their own host). Canvas zoom only scales textures; the plane snaps to whole pixels only at 1:1. **Detail renders** (`detail.rs`): 120 ms after the camera rests past a board's raster scale, the visible part (+192 px margin, origin on a screen pixel) is asked for at screen scale; a board whose surface cannot hold it is first grown to the plane (`SetBoard`, rounded to 256 px, grow-only, frames from before the grow dropped). Zoom limit 3200%. |
| Settle / auto height | After each load (`loads.rs`: seen loading → idle, or 1 s grace; a generation per load so late replies are ignored) every board runs `inspect::settle(pins)` (`AgentOp::Eval` with `awaitPromise`: `document.fonts.ready`, two frames → `{height, resources, status, url, exits, links, anchors}`; `pins` = its notes' `at` selectors; re-read as `Pending::Marks` when notes move or the height changes). Status ≥ 400 → board error. `auto` boards are resized by `SetBoard`, then settled again (≤ 2 per load); after the first settle the page runs `inspect::WATCH_HEIGHT` (a pending eval: `ResizeObserver` on root / body / body's children, resolves `{height, reflow}` when the content height changes) and Design resizes and re-arms on each answer — no timer; `reflow` answers are capped at `loads::MAX_REFLOWS` in a row. The helper drops a closed browser's DevTools jobs (`cef::agent::forget_browser`). Local resources outside `design/` become the board's linked files. Design's CEF has `--hide-scrollbars`. A 200 ms tick runs only while a settle or `wait` is pending. |
| Pick | Super+click (Super from River bus chords via `sola_browser::input`) → `AgentOp::Eval` (`Runtime.evaluate`, `returnByValue`) runs `document.elementFromPoint` → `{selector, tag, text, rect, html}`; re-queried by selector 400 ms after a reload or re-layout. |
| Watch | `notify` on `design/` (recursive) + the linked files' directories: a burst is `Change { layout, content }`; `canvas.toml` → re-layout (`boards::sync`), other files there, shared ones outside every canvas, or linked files → `ReloadIgnoreCache`, skipped when `watch::newest_input` is not newer than `shown_at` (the file times the boards last loaded). `wait` reloads first when they are; the canvas's own `canvas.toml` is not an input (a layout edit must not reload — that cancelled a pending `src` navigation). A reload hard-reloads a tab on its intended URL, else loads the intended URL (repairs drift). Rail rescans projects every 2 s off-thread. |
| Page chrome | None: JS dialogs auto-accepted, HTTP auth cancelled, popups / notifications / favicons dropped. |
| Call owner | `design` (`calls.rs` specs, `app/cli.rs` handlers). `canvas.new` / `open` register the project (`ensure_project`) and make the canvas live off screen (`--raise` shows it); `share` / `share.rotate` give its link; solactl / MCP fill `canvas.new --path` with the caller's cwd. `screenshot` = `AgentOp::Screenshot` (CDP PNG); `pdf` = `AgentOp::Pdf` (`Emulation.setEmulatedMedia` screen → `Page.printToPDF` → restore); `inspect` = `AgentOp::Eval` page script; `wait` / `reload` reply when every target board is painted, not loading, and was seen loading (or idle 1 s). Board tabs skip the agent reveal; Design wakes a sleeping board before it sends an op. |
| Agent / human | Agent reads never follow the camera: `screenshot` / `pdf` hold their board (`Details::hold`: whole-board view, no detail requests until the reply; the old detail stays drawn) and send the page a pointer-leave first. `view` yields once the human has panned / zoomed the canvas (`human_camera`; `moved: false`). `glow.rs` shimmers what agents touch (inspect rects, read selection, captured board, reloading board until settled) on the overlay only. |
| Canvases | `app/canvases.rs`: `live: Vec<Live>` (boards, links, `shown_at`, parked camera, `touched`, `agent_at`) and `shown` (the human's). Agents' `open` / `canvas.new` call `ensure_live` (off screen); the rail and `--raise` call `show_canvas` (parks the old view: camera, focus, pick, detail renders retired). Page calls resolve `--canvas` → `canvas::locate` → `ensure_live`, else the shown canvas. Off-screen canvases idle 10 min close (`Msg::Evict`, 60 s tick; never with a call in flight). Every live board keeps its texture; only the shown canvas draws. **Pace** (`pace.rs`, `App::pace_boards` after every update): busy boards (loading, settling, a call / pick / settle in flight — not a height watch) Live; shown canvas in view (plane + 256 px) Live while the window has focus (`window::Event::Focused`), else Slow; out of view Asleep; off-screen canvases Asleep unless an agent called within 30 s (Slow). Sent only on change. Rail row says `agent` within 30 s of an agent call. `solactl mcp` keeps each agent's last opened / made canvas and fills `canvas` on page calls. |
| Share | `share.rs`: second listener (`share.toml` `listen`, default 127.0.0.1:47820) for the tunnel. Tokens (128-bit base32, `share.json`, 0600) per canvas key, minted on rescan, rotatable. `/<token>/` → `share_viewer.html` (iframes per artboard, live pages, Design's controls — right-drag pans and wheel zooms over the pages via capture-phase listeners in each same-origin frame, touch mode — pages inert (`pointer-events: none`), one finger pans, a pinch only zooms, no hint; own `auto` measuring, `~version` poll every 2 s → reload); `~canvas.json` layout (board files as token URLs; `auto` boards carry Design's measured height — `Share::set_height` on each settle — so the web layout matches; unmeasured ones are measured in the frame with Design's formula); frames get no scrollbars; files via `serve::lookup` (web root, then project root) then `allowed` on real paths: canvas folder, `design/` outside other canvases, web root, persisted linked files. Root-absolute requests 307 under the `Referer` token. `Referrer-Policy: same-origin`, `X-Robots-Tag: noindex`; ≤ 64 connections. `share/tunnel.rs` runs `cloudflared tunnel run` (`TUNNEL_TOKEN` env, pdeathsig, restart with backoff, log `/opt/sola/log/design-tunnel.log`). Cloudflare: remotely managed tunnel `sola-design` (ingress `design.moonlight.pm` → `http://127.0.0.1:47820`), proxied CNAME `design`. |
| Tab snapshot | CEF url / title / loading callbacks `mark_snapshot_dirty` and post a command drain, so the snapshot (and `is_loading`) reaches chrome even when no commands are flowing. |
| Helper restart | Router kills a still-running helper before respawning (one CEF per profile). Design reopens its boards when the helper's tab list drops them. `AgentReply.data` crosses the bincode IPC as JSON text. |

---

## Build

```text
cargo make build              # release (default)
cargo make build --debug      # unoptimized
cargo make install            # needs explicit user permission; release
cargo make install <app>…     # targeted install; --debug for debug
cargo build --release         # you own Rust build before vm
cargo make vm build           # stage target/release → nix qcow2 (no cargo)
cargo make vm run             # installed if present, else installer
cargo make iso build          # installer ISO (stage + nix)
cargo make iso run            # QEMU: ISO + blank disk
```

Alias: `cargo make` → `cargo run -q -p sola-make --` (see `.cargo/config.toml`).

### Distribution (as-built, post-merge)

| Path | What |
|------|------|
| Shape 1 | Flake `packages.sola` + `nixosModules.default`; `services.sola.installRelease` (default true) installs the tarball; colleague ops in root [`INSTALL.md`](../INSTALL.md). From-source: [`CONTRIBUTING.md`](../CONTRIBUTING.md) (`installRelease = false`) |
| Shape 2 (harness) | `nixosConfigurations.sola-vm` + `packages.sola-vm-qcow2`; `SOLA_VM_STAGE` + impure stage |
| Shape 3 (product) | `nixosConfigurations.sola-iso` + `packages.sola-iso` → `var/images/sola.iso` |
| Stage source | Always **`target/release`** (this tree); never `/opt/sola/bin`; guest ELFs patchelf’d |
| Live stack | Quiet boot + Plymouth `sola` + cage kiosk → `sola-install` (`live-common` shared by qcow + ISO) |
| Splash | `nix/image/plymouth/` — flower alpha mask, cyan ripple / clockwise petal walk |
| Target system | `nixosConfigurations.sola-installed` — quiet boot + loginless `sola-desktop` |
| Apply | `sola-install-apply`: GPT ESP+root, `nixos-install --system`, user (no password), autologin |
| Policy v1 | US EN, Mac keyboard, hostname `sola`, interim TZ US/Mountain, wizard = username + disk |
| Desktop seat | `sola-desktop`: ensure user → `runuser` → `/opt/sola/bin/sola` |
| Local products | `var/images/` qcow/ISO/target disks (gitignored) |
| `cargo make vm` | `build` / `install` (wipe target) / `run` (installed if present else installer) |
| `cargo make iso` | `build` / `run` (QEMU: `-cdrom` + blank virtio disk) |

---

## History note

`docs/vault/` and early freezes may still describe the **WebView** era. Treat
those as history unless a freeze header says otherwise. Living map is **this
file** + code under `crates/`.
