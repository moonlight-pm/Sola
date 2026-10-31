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
         │              │  projects · browser · mail   │
         │              │  calendar · bots · scope     │
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
| `crates/sola-bus` | Bus host + client library + topics (an app brings its own window forward with ephemeral `Topic::ShowWindow(window_id)` → the shell; `Topic::Focus` stays the shell's word to sola-river) |
| `crates/sola-call` | Call host + client library (request/reply) |
| `crates/sola-bots` | Named informational LLM sessions. A bot is a directory `~/Bots/<slug>/` (`bot.json`: name, AI, model, session; `dialog.json`), listed by scanning `~/Bots` like Projects. Daemon `sola-botsd` (supervisor `MANAGED` on NixOS; `pkg:sola` carries it on Oath, no `svc` yet; owner `bots`; HTTP + SSE `GET /events` on the Unix socket `$XDG_RUNTIME_DIR/sola-bots`, no token) + iced **Bots** (its client). Each bot's AI is fixed at creation: Grok (`acp.rs`, `grok agent stdio` per bot, yolo), Claude (`claude.rs`, `claude -p` per turn on the bot's session, subscription login), Gab AI (`gab.rs`, the chat API, a key). `solactl bots` stays on sola-call. Not Projects. |
| `crates/sola-core` | Shared primitives (env, process, config, log, `login` — Oath's login service at `/oath/run/login.sock`: status, check, set, auto-login, desk down —, `open_url`, `omnibox` — typed words as an address or a Kagi search, and the browser's visit history read and ranked — …) |
| `crates/sola-river` | River ↔ bus bridge |
| `crates/sola-session` | User-app session manager (spawn / close / reap; `SessionApps` restore; apps that exit with their compositor — `display.rs`, the Wayland socket's identity — stay saved and start again on the next one; a restarted session adopts the running saved apps — `adopted.rs`) |
| `crates/sola-shell` | Menubar, launcher (a running app comes forward — `activate_window` on its last-used window — except Terminal, which opens a new window; picking Browser opens the Browser field (`launcher/web.rs`: an address or a search over the browser's visits via `sola_core::omnibox`, the pick handed to `sola_core::open_url`); while an app starts, a ghost row in the tab bar at `Stacks::landing_slot`; its Projects chooser lists `~/Projects` and calls Projects' `project.list` / `project.open` on the call plane), switcher, Super+K shortcuts overlay, zoning, notification HUD, Bluetooth popover, volume popover + 12-band LED spectrum (iced daemon; BlueZ over system D-Bus; PipeWire via `pw-dump`/`wpctl`/`pw-cat`; Lamzu mouse battery over hidraw). Flower menu: Exit Sola, Restart Computer, Shut Down (logind; on Oath `sudo oath set host:local power=reboot|halt` + `oath apply --confirm host:local`). |
| `crates/sola-kit` | Iced app kit, a library (the storybook binary retired 2026-10-30) (incl. `TabWindows` — per-document windows that take the compositor's close requests; `FilePicker` — the one Open / Save / Choose Folder panel, keys and several files; `style::BAR_H` for app bars; `show_window` → `Topic::ShowWindow`; shared **Window** menu, compositor clipboard helper for images; `scroll` — the kit scrollbar, a pill in a rounded channel, which apps take through `scroll::scrollable`; `prose_letter` — prose with rules and lines of pictures the app supplies by key) |
| `crates/sola-settings` | Settings panel (theme, apps, mail config, calendar accounts + shelf, Login — the password and auto-login through Oath's login service, …) |
| `crates/sola-oath` | **Oath** catalog app: packages, Needs / Used by, pins, apply, over `oath --json` (sudo for writes). Launcher entry **Oath** only when `/oath/INDEX.md` exists |
| `crates/sola-terminal` | Untitled-shell terminal (alacritty grid + iced). Also a **library** for the grid/PTY (`tmux::configure` for other sockets). On `next`: an `iced::daemon` with a window per tab (each a tab in the shell's bar via kit `tabs::TabWindows`), no tab strip, one process (`$XDG_RUNTIME_DIR/sola/terminal.sock`; a second launch emits `MenuAction new_tab` and exits). |
| `crates/sola-projects` | **Projects** (named Workspaces until 2026-10-21): **a window per session** (sessions anywhere, 2026-10-30) — each a tab in the shell's bar, titled `<project> › <session> \| <host>` (`sola_core::window_label`; the shell draws the host as a chip in its prompt color, `sola_core::host_color`), its row showing that session's activity. A session is a tmux session on the computer it runs on (socket `sola-ws` there, the one that computer's own Projects uses), `sws-<id>`, started in its project's folder; any computer's Projects can open it (several clients; `window-size latest`, Projects sizes its client only and unpins a session it attaches). Keys here: `<id>` for this computer's, `<id>@<host>` for another's (`catalog::key_for` / `bare_id` / `session_name`). A session says what it is in tmux user options (`@sola-project`, `@sola-name`, `@sola-title`, `@sola-kind`; `sessions.rs` stamps them and lists a host's sessions, here or over ssh, an unstamped one placed by its pane's folder); `project.open` opens every session of the project on its computer, else starts its main one. Closing a window or quitting detaches; End Session / `tab.rm` kills (off the UI thread). Projects makes no worktrees (a session makes its own); no splits, no startup scripts. Another computer's sessions: each window's PTY runs `ssh -t <host>` into that tmux (`sola_terminal::remote`: the attach script makes a new session detached, stamps env and options, then attaches; an existing session is attached as it is; sessions on the pre-2026-10-30 `sola-ws-remote` socket are still found, `remote::pick`). Status lives on the host (slice 2): every agent hook appends its event to that computer's hook log `~/.local/state/sola/projects/hooks.log` (one line, `<agent>\t<SOLA_PANE_ID>\t<base64 payload>`, under `flock`, rotated at 8 MiB; `hooks/log.rs`), and every computer watching its sessions follows it — this one by reading the file, another through its link (`link.rs`: Projects' own `ssh host tail -F`, after it learns the host and installs the hooks there); events carry their host; a computer that looks at or answers an island appends `seen\t<id>\t` so every other one lets it go. `machine.rs` (`Machine`) runs git and session-file reads there; `remote.rs` runs what waits on ssh off the UI thread (attach, a 3 s status tick per host, remote calls, island scans). Catalog `~/.config/sola/projects/catalog.json`: the sessions open here (an older catalog's split leaves become sessions, remote tabs are re-keyed `@host`). Call owner `projects` (`solactl projects …`; methods: `ps`, `project.{list,open,close}`, `tab.{list,spawn,set,rm,select,exec}`, `pane.{list,send,read,wait}`, `whoami`; a tab is a session). Single instance: `$XDG_RUNTIME_DIR/sola/projects.sock`. `pane.send` / `tab.exec --prompt` tmux `load-buffer` + `paste-buffer -p` then Enter. `last-status.json` keeps the agent `session_id`; a new tmux (reboot) execs `grok -r` / `claude --resume` when that agent was running and its session is on disk. Tmux targets are exact (`=name`, panes `=name:`). Grok hooks in `~/.grok/hooks/sola-status.json`; Codex hooks merge into `~/.codex/hooks.json`, Claude hooks into `~/.claude/settings.json`, found again by script file name on start. Compaction `×N` (Grok session dir, Codex rollout `compacted`, Claude `compact_boundary`). Each window sends its session's status as sticky `Topic::TabActivity` (working / waiting) for its row. Unfocused done/waiting emit `Topic::AppNotification`. Agent islands (`islands.rs`): a session that asks or finishes is sticky `Topic::AgentIsland`; `Topic::AgentIslandAction` types a choice or a reply into it, or shows its window. |
| `crates/sola-browser` | Iced chrome + CEF engine (single crate). Default web + PDF MIME / `xdg-open` (`sola-browser.desktop`). Web `Notification` → `Topic::AppNotification`. A page's file chooser: the helper's CEF `DialogHandler` holds the callback and sends `FromEngine::FileDialog`; the chrome shows the kit `FilePicker` over the page (`file_dialog.rs`) and answers `ToEngine::FileDialog` (paths, or cancel). Call owner `browser` (`solactl browser …`). Hosts **web apps** (`sola-browser --app <id>`; [Web apps](#web-apps-as-built)). |
| `crates/sola-design` | HTML artboards on a canvas, edited by agents. Own project list `~/.config/sola/design/projects.json`; canvases `<root>/design/<canvas>/canvas.toml`. One CEF board tab per artboard in one `--engine --profile=sola-design` helper; idle boards are discarded (tab closed, picture kept) when free memory runs short (`/proc/meminfo`). No editing UI. Call owner `design` (`solactl design …`: `project.{list,add,remove}`, `open`, `canvas.close`, `status`, `selection`, `view`, `wait`, `reload`, `screenshot`, `inspect`). |
| `crates/sola-mail` | Kit-native mail client. Emits sticky `Topic::MailStatus` (inbox unread) for the menubar; retracts on quit. Subscribes to Theme, MenuAction, CloseApp, Windows, WindowFloating, MailConfig only. `Topic::MailConfig`: IMAP-enabled accounts combined into six canonical boxes; `aliases` + `from_hidden` + extra SMTP `accounts` + `primary_from`. Threads: UI; coordinator (`sola-mail-worker`: config, account slots, fan-out jobs, retry backoff); per account a read session (`-r-`), write session (`-w-`: MOVE / delete / erase / append / move rules) and IDLE (`-idle-`) — 3 IMAP logins; SMTP send on a short-lived thread. Rows are keyed account + mailbox + UID. Attachments: IMAP `BODYSTRUCTURE` + MIME parts; SMTP `multipart/mixed`; kit FilePicker. Last-session list snapshot (versioned) under `~/.local/state/sola/mail/snapshot.json`. Gmail `CREATE Archive` if unmapped. UI in `ui/` drawn as Next's mail (`style.rs`: the face from `palette().background`): a bar across the window (`bar.rs`), the mailboxes beside the list from a 1040 px window, else a menu under the bar's title. HTML letters: `protocol/html/` (html5ever → own `Rc` DOM → a safe subset walked into kit prose blocks; link labels), pictures decoded on the worker (`protocol/picture.rs`, `image`); web pictures fetched on **Load Pictures** by `worker/images.rs` (`ureq`, threads `sola-mail-img-*`, `MailCmd::FetchImages` → `MailEvent::Image` / `ImagesDone`). Ships in `pkg:sola` on Oath (on `next`). |
| `crates/sola-calendar` | Kit-native calendar. Local **On This Computer** plus Google Calendar API v3 (shipped Desktop OAuth client, PKCE), iCloud / generic CalDAV, and ICS/webcal URL feeds. Accounts and calendar shelf (hidden / alias / colour) from persistent `Topic::CalendarConfig` (Settings → Calendar; multiple of each kind). Events `~/.local/state/sola/calendar/store.json`; reminders `reminders.json` (local offsets). View prefs `~/.config/sola/calendar/settings.json`. Due reminders emit `AppNotification` with a `sola-persist:` tag so the desk card stays until dismissed. Launcher **Calendar** (`lucide/calendar`). Single instance: `$XDG_RUNTIME_DIR/sola/calendar.sock` (a later launch writes its argv paths, one per line, and exits; an empty connect is a probe). `.ics` paths (argv or handoff) become a new-event draft (`invite.rs`). Default `text/calendar` handler (`dist/applications/sola-calendar.desktop`); `sola_core::open_calendar` is the opener for Mail attachment Open and `solactl open`. Publishes persistent `Topic::CalendarAgenda` (`agenda.rs`; namespace `calendar/agenda` → `~/.config/sola/calendar/agenda.yaml`): the shown calendars' events from today through the next week, a day at a time, for the menubar clock; `NotificationActivate { app_id: sola-calendar, url: id }` (a reminder card, the clock's panel) shows that event on its day, held until the store loads. Board in `ui/board.rs`. Ships in `pkg:sola` on Oath (on `next`). |
| `crates/sola-monitor` | System monitor: bus audit + call-plane observer |
| `crates/sola-kvm` | KVM / input bridge (server → Linux or Mac client). The server captures through the compositor (`capture.rs`: layer-shell edge strip, exclusive keyboard, pointer lock, relative motion, shortcuts inhibitor), not `/dev/input`. `pair` swaps static public keys over ssh (`~/.config/sola-kvm/key`, `key.pub`, `peer.pub`); input and clipboard run Noise IK between them (`link.rs`, also built into `apps/sola-kvm-mac`); neither side starts unpaired |
| `crates/sola-lock` | The lock screen: `ext-session-lock-v1` via smithay-client-toolkit, drawn with tiny-skia + cosmic-text (no iced: it cannot make lock surfaces), the password checked by `sola_core::login`. Started by the shell (`sola-shell/src/lock.rs`): Super+L, the desk's start (password and no auto-login; once per compositor socket), the idle fade's end (`Appearance.lock_idle`); started again if it dies while locked. Exit Sola's last step on Oath is `login::desk_down` once sola-session is gone |
| `crates/sola-preview` | Argv / launcher image viewer (shell hotkeys copy screenshots to the clipboard) |
| `crates/sola-paint` | Default image viewer/editor (MIME, `solactl open`; singleton via `OpenImage`; on `next` a window per image, kit `TabWindows` keyed by path; open images in sticky `PaintSession`; ships in `pkg:sola` on Oath) |
| `crates/sola-scope` | Pixel loupe: magnified grid around the pointer (`compositor.sample`) |
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
| Persistent stickies | Bus writes `~/.config/sola/state.yaml` (some topics have their own files, e.g. `theme/current.yaml`; the shell seeds `Topic::Theme` when that file is missing and, on `next`, re-emits a saved theme still on the retired slate values with their greys) |
| Browser vault session | `~/.local/share/sola/browser/vault-session.json` (age-encrypted tokens + user key; **Log out** deletes). Prefs `~/.config/sola/browser/vault.json`. |
| Browser HTTP auth | Per-profile `profiles/<uuid>/http-auth.json` (age-encrypted Basic/Digest; kit Sign in). |
| Projects catalog | `~/.config/sola/projects/catalog.json` (projects / tabs / selected; migrated once from `~/.config/sola/workspaces/`, before that `agent-terminal/`; a `workspaces` key still loads). Hook scripts (`grok-hook.sh`, `codex-hook.sh`, `claude-hook.sh`) live in the same dir |
| Projects on another computer | Recent hosts `~/.config/sola/shell/remote-hosts.json` (the launcher's chooser; eight, latest first). Shared ssh connections `$XDG_RUNTIME_DIR/sola-ssh/%C` (`ControlMaster`, 10 min). On the host: tmux socket `sola-ws` (the host's own; sessions opened from elsewhere before 2026-10-30 on `sola-ws-remote` until they end), sessions `sws-<id>` with `@sola-*` options, config `~/.config/sola/sola-ws.tmux.conf`; its hook log `~/.local/state/sola/projects/hooks.log` (followed by `ssh host tail -F`), hook scripts `~/.config/sola/remote/{claude,codex,grok}-hook.sh` (or `~/.config/sola/projects/` when a Projects runs there) |
| Projects last-status | `~/.config/sola/projects/last-status.json` (per-pane status / agent / Grok or Claude `session_id`; a new tmux after reboot execs `grok -r` or `claude --resume`) |
| Projects calls | sola-call owner `projects` (`solactl projects …`; no `workspaces` alias). First-class: [`2026-08-18-workspaces-cli-design.md`](specs/2026-08-18-workspaces-cli-design.md) (old names in the body) |
| Browser calls | sola-call owner `browser` (`solactl browser …`). Snapshot/act by `--tab`; CSS-pixel click/hover; `fill --file` / `get` / `links`; `wait --control`; `fill`/`type` paste into contenteditable; `key --chord` (one char); `scroll`. A timeout while the owner is still advertised is a wedged chrome (close X / Flower, relaunch). First-class: [`2026-09-11-sola-browser-agent-control-design.md`](specs/2026-09-11-sola-browser-agent-control-design.md) |
| Grok sessions | `~/.grok/sessions/` (Projects compaction `×N` + reboot resume; not an ACP leader socket) |
| Codex sessions | `~/.codex/sessions/YYYY/MM/DD/rollout-*-{id}.jsonl` (Projects compaction `×N`; `CODEX_HOME` override) |
| Self-update of apps | Binary watch → re-exec when `/opt/sola/bin/<name>` changes (`SOLA_NO_SELF_WATCH=1` skips) |

### Communication layers

1. **Sola Bus** — lifecycle, focus, themes, app menus, session commands,
   stickies. Fan-out facts. No request/reply. Mail unread is
   `Topic::MailStatus` (sticky, not persisted). Calendar's coming days are
   `Topic::CalendarAgenda` (persistent), for the clock's panel.  
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
| `sola-shell` flower Restart / Shut Down (`power.rs`) | system | logind reboot / power-off | `loginctl` / `systemctl` |
| `sola-session` app launch / stop | user systemd | `systemd-run --user --scope`, `systemctl --user stop` | CLI |
| CEF (`sola-browser`, `sola-design`) | — | Chromium links `libdbus-1` (nix-ld list in `nix/module.nix`) | third-party |
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
| Working ring `At` ~20 Hz | `sola-kit` `status_mark` | `RedrawRequest::At(50ms)` on `RedrawRequested`, **not** `NextFrame` / `window::frames()` | n/a | A Working Grok row pins GPU; ring frozen (no `RedrawRequested` after status change) |
| Projects pointer gated | `sola-projects` `subscription` | No `window::frames()`; `CursorMoved` only while a split drag is live; ignored mouse is not `Msg::Input` | Split divider drag | Split resize only updates on press; whole window presents on every pixel over empty chrome |
| Morph2 drag pump | `sola-kit` `sidebar/strip.rs` | Idle: no vsync chain | While `dragging` / FLIP: `invalidate_layout` on pointer (ghost Y is layout) + `request_redraw` on `RedrawRequested` | Tab/group reorder stutters or ghost stuck; idle vsync if `request_redraw` is left on when not dragging |
| Browser HTML5 drag | `sola-browser` `FrameSlot.page_drag` + shader `should_pump` | Idle: hangover 50 ms after last paint | While helper `FrameMeta.drag`: `request_redraw` without `Msg::NewFrame` | Kanban ghost lags / smears if overlay full-uploads; idle vsync if `page_drag` sticks after drop |
| Shell overlays parked 2×2 | `sola-shell` `ensure_overlay_windows` + `zoning::overlay_frame` | Menu / launcher / switcher / shortcuts / selection / **notify** stay mapped after the menubar’s first Composition. **Dismissed = 2×2 swapchain off-output** (`OVERLAY_PARK_X/Y` −10000; winit Wayland min is 2×1 — 1×1 + `resizable=false` is `xdg_toplevel` invalid_size). **Shown = live Frame while hidden, Composition after iced `Resized` ≥64×64** (next-tick hop so view/present run first). Notify live Frame is a tight top-right card stack, not the full usable area. Menu and Super+K live Frames are the card + shadow pad, not the full usable area. | River hides any window not in last Composition; do not stack a parked buffer | GPU spike if a dismissed overlay is left at full output; overlay visible before iced `Resized`; Super+Space hangs if `Resized` never fires; 1920 placeholder jump if live Frame forgets output size; **shell panic-loop if park size is 1×1**; notify overlay covering apps if Frame is full-area instead of card-sized; Super+K / menu filling 1080p on software GL |
| Kit opaque-region | patched `iced_winit` `State::synchronize` | Kit apps create an ARGB swapchain (`window_settings_transparent`). **Cinema:** `theme_for(Chrome::Bare)` opaque `background.base` → `wl_surface.set_opaque_region` (full) so River GLES can scan out. **Tiled / float / shell overlay:** transparent base (the frame clears its corners so the ground shows) → region cleared | n/a | Cinema without opaque-region cannot scan out; square black corners on a tile or float if opaque-region is left on while the overlay theme is active (the ground hidden); overlay launcher dimmed wrong if marked opaque |
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
| Tab bar (`next` only) | Next's one tab list ([spec](specs/2026-10-21-next-tab-bar-design.md)): docked left under the menubar (240 px, one gap in: 10, 38 on canto), always mapped, composed right above the menubar. Kit `SidebarPanel` on a kit `island` (rounded, rimmed, ground-black corners): one list of the workspace's windows in its own order, not grouped by zone (2026-10-28), a row per window (app icon, title); click shows, drag reorders (the window keeps its zone). Model in `stacks/` (a desk per workspace: its tabs in bar order, each with its zone) |

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
longer read or written on `next`.

**Zones (2026-10-31):** the content area is a grid of
equal cells from the persistent `Topic::ZoneLayout` (Settings → Zones →
shell; `sola_bus::topics::ZoneLayout`, which also knows overlaps, cell
fractions and place names). A zone is `Zone::Grid(id)` (a rectangle of
cells), Full (`Zone::Fullscreen`, every cell), Cinema (the output) or
Floating; the old named zones are gone (saved Left / Right deserialize as
zones 1 / 2). Each desk keeps its zones' pick order (`TabLayout.picks`;
an older file's `full` reads as `[Full]`); `Desk::on_screen` walks it and
takes each zone with a tab unless one taken overlaps it. `zoning::tile_frame`
cuts the cells with one gap between neighbours. A Cinema tab on screen is
the whole composition (`Shell::push_desk` is skipped), the only tab framed
fullscreen (`Shell::cinema_shown`), and sola-river drops the background
surface while a fullscreen window is composed (`background::sync_on_render`).
sola-river grants an app's own fullscreen request at once and emits
`Topic::FullscreenRequest`; the shell moves the tab to Cinema and back
(`app/zones.rs`, `Stacks::cinema_return`).

**Keys (2026-10-31):** `sola_core::keys::KEYS` is the
one key table (keycode, keysym, NumLock-off keysym, name). The shell's system
actions are one table (`sola-shell/src/bindings.rs`: id, group, label,
default; zone actions built from the layout): `shell_key_chords` registers
from it, `on_chord` → `run_act` dispatches by it after the modal gates,
Super+K, the Workspaces menu and the Window menus the shell draws read it.
The shell publishes its actions as persistent `Topic::KeyActions` (owner
`system`); Settings writes your overrides as persistent `Topic::Keybindings`;
`sola_kit::keys::resolve` puts them together (your choice over a default, the
system over an app, two apps may share). While Settings waits for a key it
holds sticky `Topic::KeyCapture` and the shell registers no chords (20 s
timeout of its own).

**On `next`, workspaces (slice 5):** the shell keeps a desk of zones per
desktop workspace (`stacks/`; readers without a window answer for the
showing one) and composes only the showing workspace; windows elsewhere
are hidden by River and keep running. The workspaces (id, name, order) and
which one shows persist inside `TabLayout` and go out as sticky
`Topic::Workspaces`; each window's workspace as sticky
`Topic::WindowWorkspace`. The switcher is the tab bar's head and its
menu (`tabbar/head.rs`, `menu/workspace.rs`; the menu overlay hangs under
the head, `MenuOverlaySpec.y`); Super+1…9 / Super+Ctrl+1…9 /
Super+(Shift+)Tab. The
browser runs one CEF helper per profile, all live: its router keeps a page
→ profile map and routes commands by page (the rest to the active page's
helper), merges every helper's pages, and makes a helper front only while
it has a page on screen or the active one; helper-minted page ids sit in a
per-profile range above chrome's. Light or dark (`Appearance.light`) goes
to every helper as `Cmd::ColorScheme` (told again to each new one); a
helper emulates `prefers-color-scheme` on each page's DevTools session
(`Emulation.setEmulatedMedia`). Each workspace has a profile of its
own, paired by the workspace's id (`profiles.json` `workspaces`,
`profiles::pair`; a profile is only a uuid, so a workspace renames
freely; no Profiles menu since 2026-10-26); a page runs its
workspace's profile and reopens in place when that differs
(`app/pages.rs`); a deleted workspace's profile stays on disk, its helper
stopped; sessions are saved per profile.

**On `next`, window frames (slice 6):** one 10 px gap (`zoning::GAP`)
around the output under the menubar, between the bar and the tiles, and
between tiles. Every kit window draws its own frame (`sola-kit`
`components/window_frame`): a 38 px titlebar with the app's menus (the kit
keeps the app's latest `AppMenuPayload` from `BusSetup` / `menu::publish`;
a dropdown is an overlay inside the window; a Window item goes on the bus as
`MenuAction` and the shell acts on its focused window, any other item is
handed back to the app's own bus stream), 12 px corners, a lit rim. Tiles
and floats alike clear their corners (`theme_for(chrome)`: a clear fill
unless Cinema), so the ground shows there. The ground is the background
picture when one is set (persistent `Topic::Background`, Sola's copy under
`~/.config/sola/background/` via `sola_core::background`, which keeps every
picture set — `kept` / `forget`, Settings → Appearance's gallery sets one
again; set by Paint, Browser or `solactl background`): sola-river draws it as a River shell surface at the
bottom of the render list (`client/background.rs`, decoded and scaled off
the loop), else the compositor's black. With Settings → Appearance's
*Adapt colors to the background* on (persistent `Topic::Appearance`), the
shell samples the picture (`adapt.rs`, off the UI thread) and rewrites
`Topic::Theme`'s color tokens from it (`sola_core::theme::Scheme`,
`Theme::adapt`: the grey's lightness, one of the picture's modal colors —
narrow OKLCH cells scored as Material Color Utilities' `Score`; `unadapt`
when off; a scheme per color, `Appearance.variation` picks, the shell
publishes the choices as sticky `Topic::ThemeVariations` for Settings;
your colors over it, `Appearance.colors`, kept as looks in
`Appearance.customs`; each kept picture's look in `Appearance.pictures`,
which the shell (`pictures.rs`) updates and wears on a picture change);
the kit's chrome surface follows the theme (`style::chrome_surface`,
`theme::install_atoms`). The desk's opacity and the idle fade are
sola-river's (`client/fade.rs`): a shell surface at the top of the render
list shows the picture (or black) at `100 − Appearance.opacity` % at rest
(none at 100 %, none over a fullscreen window; made again when the
picture, the output size or the kept rectangle change);
`ext_idle_notifier_v1` says idle / resumed, and the opacity rises from
there over `fade_over_min` (`wp_alpha_modifier_v1`), stepped back down to
the rest over a second on input. Once it is full, the dim (`client/fade/dim.rs`) waits `dim_after_min`, then raises a black output-sized surface over it to 90 % over `dim_over_min` (a tenth of the picture's light left), and steps it down with the return. The overlay is cut in pieces (sub-buffers of the picture's memory) around
the shell's sticky `Topic::IdleKeep` — the top row's flower and agent
islands — whose piece stays at the rest. Sticky `Topic::IdleFade`
says it is fading and, once full, `out`: the shell then reports every tab
window off screen (`TabWindow.shown`), so the browser's pages rest. The menubar carries no
app menus. The shell publishes sticky
`Topic::WindowChrome` (window id → tiled, Cinema, rim; `chrome.rs`):
sola-river sends `set_tiled` on change and a 1 px border on windows whose
app published no menu (foreign apps) — tiled ones get rounded corners
instead, four decorations above the window painting the corners the ground
(the picture's own pixels under them, `background::sample`) and the rim
along them (`client/corners.rs`); kit apps drop the frame in
Cinema (`FloatState::chrome`).

**On master:** zoning / floating is coordinated with `sola-river` over the
bus: unassigned windows **default-float** (client size +
`Topic::WindowFloating`); saved zones restore frames; Meta+numpad snaps
assign zones.

**Hide (Super+H):** sticky `Topic::AppHidden` omits that app’s surfaces from
`Topic::Composition` (River `hide` — not send-to-back). Restore: a tab bar
row or Super+Backtick (showing a tab unhides its app), launcher on an already-running hidden app
(unhide + raise, no second spawn), or any `raise_app` path (OpenUrl, mail
unread, notification click). Last window of a hidden app retracts the sticky
so a later map is not stuck hidden. No menubar chip.

**Shell re-exec z-order:** sola-shell rebuilds MRU from window-id order, which
would shuffle apps. River keeps the last relative stack when only the
menubar id changed (click-raise still honors a new Composition).

A hard-killed `sola-shell` can leave parked surfaces in sola-river (no
`closed`). River prunes entries whose `/proc/<pid>` is gone so a respawn
can map a new menubar. Shell composition also ignores dead-pid windows.

### Games

`sola-arcade` (the Steam library in a window, each game in a gamescope
nest) was retired on 2026-10-30; it stays in git history. sola-river and
the shell still give any `gamescope` window its nest size and float it.

---

## Browser (as-built)

| Piece | Role |
|-------|------|
| `crates/sola-browser` | **Product browser** — iced chrome (a window per page, each a shell tab (`page_windows.rs`); the active page's window has the full-width bar: nav + omnibox + vault + downloads; the others a read-only address bar; unified Bitwarden panel with item edit) + CEF CPU OSR under `src/cef/`. New page windows open at the last page window's size; the engine re-checks a page 300 ms after a resize or show and, with no frame at its window's size yet, hides and shows it (`check_page_size`) |
| App id / binary | `sola-browser` → `/opt/sola/bin/sola-browser` (shell launcher: one “Browser” entry; one Wayland window). `dist/applications/sola-browser.desktop` is the xdg-open default for http(s), HTML, XHTML, PDF, `about:`, unknown. |
| Engine helpers | Per-profile headless `sola-browser --engine --profile=<uuid>` (no iced / no xdg_toplevel). Control socket `profiles/<uuid>/engine.sock`; pixel frames on `engine.frame.sock` (raw BGRA, not bincode). Page copy is JS extract → `FromEngine::Clipboard` on the control socket → chrome writes Wayland. Paste is `ToEngine::PasteImage` (`File` event) or `ToEngine::PasteText` (synthetic `ClipboardEvent` + focused-frame fallback; not Chromium `insertText` of the whole payload — that flattens newlines). Chrome reads the compositor via `sola_kit::clipboard` (data-control), not iced, so an image offer is not dropped. Favicons: helper `DisplayHandler::on_favicon_urlchange` + `download_image` → `FromEngine::Favicon` (PNG) → chrome 16px leading slot (globe fallback). ⌘-click hit-test is JS → `FromEngine::OpenBackgroundTab` → chrome `open_tab_beside` (below current tab, same group; click not sent to CEF). `window.open` with features (`NEW_POPUP`) / `about:blank` is a windowless CEF browser (so `window.open` returns a `Window`); chrome adopts that engine tab and focuses it beside the opener. `target=_blank` / `NEW_WINDOW` cancel the native popup and open a focused chrome tab. ⌘T / `OpenUrl` / `xdg-open` use `open_tab` (loose, end of strip). An outside open (`chrome.sock` / `Topic::OpenUrl` with `activate`) asks the shell to raise the existing window (click-activation: MRU + composition + seat; sock probes are not activate). Browser → Developer Tools (and page-menu Inspect) docks a second windowless OSR browser under the page (bundled frontend over the helper `/json/list` websocket; dual `FrameSlot` page + inspector paints; `FromEngine::DevTools`). Chrome-runtime `ShowDevTools` cannot be windowless and is not used. It is not a strip tab. Super+left/right are not WM pointer bindings (CSD titlebar still moves floats). Page right-click is `ContextMenuHandler::run_context_menu` (native cancelled) → `FromEngine::PageContext` → kit menu. Session history rides the tab snapshot (`TabInfo.history`); hold-nav uses `NavCmd::GoHistory`. IME caret is `OnImeCompositionRangeChanged` → `FromEngine::ImeCaret` (view px) so chrome can `request_input_method` at the composition box. JS `alert` / `confirm` / `prompt` / leave-page: helper `JsDialogHandler` → `FromEngine::JsDialog` → kit modal; chrome replies `ToEngine::JsDialog`. Continue drops the pending-callback `RefCell` before calling CEF (OnReset re-enters). Park / navigate resets pending callbacks. Find: helper `FindHandler` + `BrowserHost::find` → `FromEngine::FindResult`; chrome `ToEngine::Find` / `StopFind`. Idle omnibox is a label until click / `⌘L` (no query/fragment; clipped). Edit paste uses the tracked omnibox flag — `is_focused` is `None` while the field is unmounted. Downloads: helper `DownloadHandler` → `FromEngine::Download` (any helper, including parked) → chrome list; cancel is `ToEngine::CancelDownload`. Passkey **get** / **create**: renderer V8 `OnContextCreated` installs a native `window.__solaVaultPost` (`V8Handler` → `CefProcessMessage` from that frame) and evals the hook (document-start) in every frame. The helper's `Client::on_process_message_received` takes origin, tab and frame from CEF (never the page's JSON), refuses an rpId the frame may not use (`vault::scope::rp_id_allowed`, PSL), and sends `FromEngine::WebAuthn` (random per-request nonce; `cancel` when the page times out) → chrome vault picker (same-site same-action coalesced; advertises a platform authenticator so Google does not wait on Chromium’s OSR-invisible sheet; `create()` confirms then `Fido2Client::register` + POST/PUT cipher). Answers go back as `Cmd::Target { tab, EvaluateJsIn { scope: FrameScope::Frame(id) } }` — that frame only. Vault fills are `EvaluateJsIn` too: `FrameScope::Site` (logins, codes, identities: main frame still on the asking origin, then same-origin frames and frames the login's own websites match) or `FrameScope::Page` (cards: every frame, origin still checked). A fill script's found/not-found report is `FromEngine::VaultFilled`. One iced chrome (`chrome.sock`); a second process hands off a URL and exits. Chrome start **Shutdown**-flushes helpers (cookie store) before SIGTERM leftovers. Helper death respawns CEF and restores chrome’s tab list (does not resurrect closed tabs from a stale helper snapshot). Reap only orphan / pre-`exec_self` helpers. `<select>` is `PET_POPUP` blitted onto the VIEW CPU frame (not a second window). Only the front helper composites (`SetFront` + `was_hidden` + `windowless_frame_rate`). CEF `root_cache_path` = that profile’s `…/cef/` so cookies persist. |
| CEF pin | CEF 156.0.14: `cef` crate `156.2.0` + workspace `cef-version`; the program asks libcef for API version 15600 (`CEF_API_VERSION_LAST`), not the experimental 999999. `cargo make install-cef` unpacks the public tarball to `~/.cache/sola/cef-<ver>/`. H.264/AAC: replace that tree with `scripts/cef-codecs/` (same pin). On Oath that cache is a link to `pkg:cef`'s live slot, and `crates/download-cef-stub` (download-cef 3.x) hands cef-dll-sys that tree plus the one-line `include/cef_api_versions.h` it reads (Oath's pack has no headers). Since 156 ANGLE is inside `libcef.so` (no `libEGL.so` / `libGLESv2.so` beside it). |
| CEF switches | `--ozone-platform=wayland`; `--password-store=basic`; `--remote-allow-origins=*`; Chromium's own autoplay policy (muted video plays while in view; sound after a click). A page off screen is `was_hidden` (the active one too). H.264/AAC need a codecs build (`proprietary_codecs=true ffmpeg_branding=Chrome`, `use_vaapi=false`); the public cef-builds tarball has neither. On Oath, `pkg:cef` is one: Oath builds CEF 156.0.14 from Chromium's and CEF's source with Chrome's codecs (Oath R40, `sha256-2a2cc750`, on the bucket); canto runs it since 2026-10-29. On NixOS, `scripts/cef-codecs/` rebuilds the `cef-version` pin into the cache. No Widevine CDM; Steam store trailers are clear DASH, not EME. |
| Profiles (D8) | Registry `profiles.json`; data/cache under `profiles/<uuid>/`; session `session.json` (pages with stable ids + `closed[]` for ⌘⇧T; older files' groups and sidebar width are ignored); shared downloads index `shared/downloads.json`; visit history `shared/history.json` (omnibox); chrome parks page-list snapshots + last CPU composites (`FrameSlot.parked_frames`); switch points the router at the target helper (pages stay loaded). Nothing is evicted for idle time or tab count (the `tab_cache` policy is gone, 2026-10-25) |
| Tab / profile paint | `present_tab`: same-size parked frame → GPU this frame; miss → blank immediately (never keep the previous page). Helpers skip same-size `Resize` so the parked compositor stays live. Iced presents paints via the shader `request_redraw` pump (does not rebuild chrome at 60 Hz). Partial `on_paint` dirty copies only onto a uniquely-owned complete last-frame (the pixel ring does not pin that Arc); otherwise the CPU buffer full-blits from CEF’s view. Latest-wins hops absorb dropped-frame dirty into the survivor so GPU damage is continuous since the last texture upload. HTML5 OSR drag keeps `last_frame` clean and composites a host ghost with union dirty (`FrameMeta.drag`); skipped overlays keep ghost/view damage until the mailbox is empty; chrome does not park overlay frames; shader pumps while `page_drag` on the painted tab. Chrome `Tick` is `chrome_wake` from helper queues (plus a 250 ms timer only for copy-URL flash / TOTP / fill-wait), not an idle 16 ms or 250 ms pump. |

Former split (`sola-browser-core` / `-wpe` / `-cef` dispatcher) and the WPE
content-plane path are **retired**. CEF: do **not** enable `accelerated_osr`
(CPU `on_paint` path only).

---

## Web apps (as-built)

A page the browser hosts as its own app ([spec](specs/2026-10-23-web-apps-design.md);
branch `next`). `sola-wrapper` (one CEF process and profile per app) is
retired there; master still has it.

| Piece | Role |
|-------|------|
| Catalog | `Topic::Application` kind `web` (the old `wrapper` kind reads as `web`): app id `web-<uuid>`, `label`, `icon` (the page's favicon saved under `~/.local/share/sola/icons/<id>.png`, else `lucide/globe`), `url` (start), `profile` (the browser profile it was wrapped from). Command synthesized: `paths::bin_path("sola-browser")` + ` --app <id>`. The browser writes the entry (Wrap) and retracts it (Remove). |
| Registry | `crates/sola-browser/src/app/webapps.rs` — `WebApps`: the catalog's web apps, page → app, the Wrap and Remove sheets. The session keeps each page's `app` (`SessionTab.app`). |
| Windows | `PageWindows::open` with the app's id: kit `TabWindows::open_as` (tab key `<app_id>/<page_id>`, Wayland app id = the app's). The shell pairs one process's tabs and windows by `(pid, app_id)`. Titlebar: the app's name and menus (`WindowFrame::menus`); the shell gets them through `menu::publish_for` (shortcuts). A restored app page waits up to 2 s for its catalog entry, then opens (as a browser page if the app is gone). |
| Launch | `sola-browser --app <id>`: a running browser gets the `@app <id>` handoff (instance socket) and raises the app's window or opens its start URL on its profile. With none up, the `--app` process emits `LaunchApp` for `sola-browser` (so `sola-session` tracks and restores the browser as itself), waits for its socket, hands off and exits; without a bus it becomes the browser. |
| Links / popups | Engine `Cmd::AppSite { id, start }` per app page (router replays it to a restarted helper): `on_before_browse` cancels a user-gesture, non-redirect main-frame navigation to another site and asks chrome for a browser page. `target=_blank` and popups from an app page join the app when same-site (`same_site`: last two host labels). |
| Notifications | `notify::to_bus_for(app_id)` with the app's label as source; activation shows the app's page. |
| Profile | The browser profile's — no profile of its own; removing the app keeps its sign-in. |

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
