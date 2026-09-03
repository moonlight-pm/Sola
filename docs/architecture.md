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
         │              │  workspaces · browser        │
         │              │  wrapper · mail · scope · spotify │
         │              └─────────────────────────────┘
         └──── Wayland (surfaces / input) ─────────────┘
```

All managed components are **independently restartable**. Kit apps reconnect
to the bus and tolerate compositor restarts.

---

## Repository layout

| Path | Role |
|------|------|
| `crates/sola` | Process manager (binary entry) |
| `crates/sola-bus` | Bus host + client library + topics |
| `crates/sola-call` | Call host + client library (request/reply) |
| `crates/sola-core` | Shared primitives (env, process, config, log, …) |
| `crates/sola-river` | River ↔ bus bridge |
| `crates/sola-session` | User-app session manager (spawn / close / reap) |
| `crates/sola-shell` | Menubar, launcher, switcher, Super+K shortcuts overlay, zoning, notification HUD, Bluetooth popover, volume popover (iced daemon; BlueZ over system D-Bus; PipeWire via `pw-dump`/`wpctl`) |
| `crates/sola-kit` | Iced app kit + storybook (incl. `FilePicker`, shared **Window** menu, compositor clipboard helper for images) |
| `crates/sola-settings` | Settings panel (theme, apps, mail config, …) |
| `crates/sola-terminal` | Untitled-shell terminal (alacritty grid + iced). Also a **library** for the grid/PTY (`tmux::configure` for other sockets). |
| `crates/sola-workspaces` | Project / workspace rail + agent-aware PTYs (tmux `sola-ws`). Catalog `~/.config/sola/workspaces/catalog.json` (migrates `agent-terminal/`). Siblings under `<root>/.worktrees/`. Call owner `workspaces` (`solactl workspaces …`; methods: `ps`, `project.{list,add,rm,startup}`, `workspace.{list,spawn,set,rm,select,exec}`, `pane.{list,send,read,wait}`, `whoami`). Per-project `startup` script runs in a new worktree after spawn. `project.rm` unregisters a project + kills its tmux, leaves worktrees. Attach stamps `SOLA_WS_PATH`; restart attaches only on path match and quarantines leftovers. Grok hooks on `$XDG_RUNTIME_DIR/sola-ws-hooks.sock`; OSC 9999 stripped in the term lib. Compaction `×N` on the workspace row is the loudest Grok pane; reads `~/.grok/sessions/<encoded-cwd>/<sid>/` (`compaction/segment_*.md`, `compaction_checkpoints/`, then `signals.json` `compactionCount`). The rail mark rolls up Grok panes in a split (waiting > working > done > idle). |
| `crates/sola-browser` | Iced chrome + CEF engine (single crate). Default web MIME / `xdg-open` (`sola-browser.desktop`). Web `Notification` → `Topic::AppNotification`. |
| `crates/sola-wrapper` | Website wrappers as first-class apps (`sola-wrapper <id>`; CEF via sola-browser lib; catalog `kind`/`url` on `Topic::Application`) |
| `crates/sola-mail` | Kit-native mail client. Emits sticky `Topic::MailStatus` (inbox unread) for the menubar; retracts on quit. |
| `crates/sola-monitor` | System monitor: bus audit + call-plane observer |
| `crates/sola-kvm` | KVM / input bridge (Linux ↔ Mac) |
| `crates/sola-preview` | Argv / launcher image viewer (shell hotkeys copy screenshots to the clipboard) |
| `crates/sola-paint` | Default image viewer/editor (MIME, `solactl open`; singleton via `OpenImage`; tabs in `~/.config/sola/paint.yaml`) |
| `crates/sola-scope` | Pixel loupe: magnified grid around the pointer (`compositor.sample`) |
| `crates/sola-spotify` | Kit Spotify client: Web API + librespot Connect, MPRIS. Tokens + `skipped.json` + `liked.json` under `~/.local/state/sola/spotify/`; settings (last page + last track + last playlist) `~/.config/sola/spotify/settings.json`; page/audio/art cache `~/.cache/sola/spotify/`. |
| `crates/sola-arcade` | Steam library browser + windowed-gamescope game launch |
| `crates/solactl` | Operator CLI (`compositor`, `session`, emit, logs, …) |
| `crates/sola-install` | Kit installer wizard + apply orchestration (`sola-install-apply`) |
| `crates/sola-make` | `cargo make` xtask (build / install / publish / **vm** / **iso**) |
| `crates/sola-assets` | Vendored icons/assets |
| `crates/iced_winit-patched` | iced 0.14 `iced_winit` + Wayland opaque-region from window-fill alpha (not a workspace member; `[patch.crates-io]`) |
| `nix/patches/` | River + wlroots patches (Xwayland destroy heal; live `pointer_position`; screencopy omits software cursor) |
| `nix/module.nix` | NixOS module (`services.sola`) — Shape 1 + images |
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
| Surfaces / input | Wayland protocols via River. sola-river turns **NumLock on** for each keyboard (`river-xkb-config-v1`) so the number pad types digits; Super+Numpad zoning still matches both keysym sets. |
| Launch | Dev: physical TTY → `/opt/sola/bin/sola`. Dist image: loginless `sola-desktop` → Sola |
| Binaries | Dev install `/opt/sola/bin/`; images stage from `target/release` |
| Logs | `/opt/sola/log/` (and tracing to TTY when run interactively) |
| Persistent stickies | Bus writes `~/.config/sola/state.toml` |
| Arcade library cache | `~/.config/sola/arcade-library.json` (scan snapshot; bg rescan on open; `steamapps/` watch) |
| Arcade nest settings | `~/.config/sola/arcade-nest.json` (per-title Fit vs locked resolution; default 1080p) |
| Arcade singleton | `$XDG_RUNTIME_DIR/sola/arcade.lock.sock` (second spawn raises) |
| Spotify state | settings `~/.config/sola/spotify/settings.json` (last page + last track); tokens + skipped + liked URIs `~/.local/state/sola/spotify/`; page/audio/art cache `~/.cache/sola/spotify/` |
| Workspaces catalog | `~/.config/sola/workspaces/catalog.json` (projects / workspaces / selected; migrates `agent-terminal/`) |
| Workspaces calls | sola-call owner `workspaces` (`solactl workspaces …`). First-class: [`2026-08-18-workspaces-cli-design.md`](specs/2026-08-18-workspaces-cli-design.md) |
| Grok sessions | `~/.grok/sessions/` (Workspaces compaction `×N`; not an ACP leader socket) |
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
| Workspaces pointer gated | `sola-workspaces` `subscription` | No `window::frames()`; `CursorMoved` only while a split drag is live; ignored mouse is not `Msg::Input` | Split divider drag | Split resize only updates on press; whole window presents on every pixel over empty chrome |
| Morph2 drag pump | `sola-kit` `sidebar/strip.rs` | Idle: no vsync chain | While `dragging` / FLIP: `invalidate_layout` on pointer (ghost Y is layout) + `request_redraw` on `RedrawRequested` | Tab/group reorder stutters or ghost stuck; idle vsync if `request_redraw` is left on when not dragging |
| Shell overlays parked 2×2 | `sola-shell` `ensure_overlay_windows` + `zoning::overlay_frame` | Menu / launcher / switcher / shortcuts / selection / **notify** stay mapped after the menubar’s first Composition. **Dismissed = 2×2 swapchain off-output** (`OVERLAY_PARK_X/Y` −10000; winit Wayland min is 2×1 — 1×1 + `resizable=false` is `xdg_toplevel` invalid_size). **Shown = live Frame while hidden, Composition after iced `Resized` ≥64×64** (next-tick hop so view/present run first). Notify live Frame is a tight top-right card stack, not the full usable area. Menu and Super+K live Frames are the card + shadow pad, not the full usable area. | River hides any window not in last Composition; do not stack a parked buffer | GPU spike if a dismissed overlay is left at full output; overlay visible before iced `Resized`; Super+Space hangs if `Resized` never fires; 1920 placeholder jump if live Frame forgets output size; **shell panic-loop if park size is 1×1**; notify overlay covering apps if Frame is full-area instead of card-sized; Super+K / menu filling 1080p on software GL |
| Tiled kit opaque-region | patched `iced_winit` `State::synchronize` | Kit apps still create an ARGB swapchain (`window_settings_transparent` for float CSD). **Tiled:** `theme_for(false)` opaque `background.base` → `wl_surface.set_opaque_region` (full) so River GLES can scan out. **Float / shell overlay:** transparent base → region cleared | n/a | Idle GPU if tiled windows stay without opaque-region; float CSD square black corners if opaque-region left on while overlay theme is active; overlay launcher dimmed wrong if marked opaque |
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
| Menubar | Top chrome, menus, mail unread chip (when `sola-mail` is mapped), missed-notification bell, volume (hidden if no PipeWire), Bluetooth (hidden if no adapter), stats, whispers (`AppToast`) |
| Menu | Open application menus + calendar / stat / notification-pile / Bluetooth / volume panels (parked 2×2 while dismissed) |
| Launcher | App launch (parked 2×2 while dismissed) |
| Switcher | MRU window/app switch (parked 2×2 while dismissed) |
| Shortcuts | Super+K cheatsheet (parked 2×2 while dismissed; live Frame is the card + shadow pad, not the usable output) |
| Selection | Super+Shift+4 freeze-then-marquee (RGBA still of the live output, then crop; parked 2×2 while dismissed; live Frame is full output) |
| Notify | Live notification cards (tight Frame under the menubar, trailing edge with the clock; parked 2×2 while empty) |

Zoning / floating is coordinated with `sola-river` over the bus:
unassigned windows **default-float** (client size + `Topic::WindowFloating`);
saved zones restore frames; Meta+numpad snaps assign zones.

**Hide (Super+H):** sticky `Topic::AppHidden` omits that app’s surfaces from
`Topic::Composition` (River `hide` — not send-to-back). Restore: Super+Tab
(switcher still lists hidden apps), launcher on an already-running hidden app
(unhide + raise, no second spawn), or any `raise_app` path (OpenUrl, mail
unread, notification click). Last window of a hidden app retracts the sticky
so a later map is not stuck hidden. No menubar chip.

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
| Launch | `Topic::LaunchApp` → `sola-arcade --run <id> <w> <h> [fit]` → `gamescope … --cursor-scale-height <H> -- sola-arcade --nested-steam <id>` → desktop Steam `-applaunch` (no BPM). Play **refused** if a desktop Steam client is already running (no exclusive fullscreen). Nested helper kills nested Steam when game `AppId=` exits. `<w> <h>` from per-title nest (Fit or locked res). Host cursor is downsampled to desktop size (nested X cursors otherwise present 1:1 to River). |
| Fit follow | Arcade UI watches `Topic::Windows` / `WindowGeometry` for `app_id=gamescope` and pokes **nested** X only (`DISPLAY` from `--nested-steam`, never gamescope's host `:0`), debounced ~250 ms. Writes `GAMESCOPE_XWAYLAND_MODE_CONTROL` + focused window `0,0,w,h`. Locked res does not follow. |
| Session lock | Active Play → Stop on that row; other Plays disabled. Session end is `UserAppExited` on `steam-game-<id>` (`--run` waits on gamescope). Stop: `CloseApp` then Arcade-owned pids only (`--run` / `--nested-steam` / that gamescope), never `pkill AppId=`. |
| River | gamescope pre-init pin then zone/float; Cinema exit-fullscreen on next zone Frame; empty app_id → `gamescope` via pid; nest `-S fit` letterbox |
| AppHidden | Super+H + switcher/launcher restore (no menubar chip); Arcade UI does not expose hide-Steam |

Operator: [`manual/sola-arcade.md`](manual/sola-arcade.md).

---

## Browser (as-built)

| Piece | Role |
|-------|------|
| `crates/sola-browser` | **Product browser** — iced chrome (full-width bar: kit identity select + nav + omnibox + vault + downloads; etch tab strip with kit reorder; unified Bitwarden panel) + CEF CPU OSR under `src/cef/` |
| App id / binary | `sola-browser` → `/opt/sola/bin/sola-browser` (shell launcher: one “Browser” entry; one Wayland window). `dist/applications/sola-browser.desktop` is the xdg-open default for http(s), HTML, XHTML, `about:`, unknown. |
| Engine helpers | Per-profile headless `sola-browser --engine --profile=<uuid>` (no iced / no xdg_toplevel). Control socket `profiles/<uuid>/engine.sock`; pixel frames on `engine.frame.sock` (raw BGRA, not bincode). Page copy is JS extract → `FromEngine::Clipboard` on the control socket → chrome writes Wayland. Paste is `ToEngine::PasteImage` (`File` event) or `ToEngine::PasteText` (JS insert in the **focused** frame only). Chrome reads the compositor via `sola_kit::clipboard` (data-control), not iced, so an image offer is not dropped. Favicons: helper `DisplayHandler::on_favicon_urlchange` + `download_image` → `FromEngine::Favicon` (PNG) → chrome 16px leading slot (globe fallback). ⌘-click hit-test is JS → `FromEngine::OpenBackgroundTab` → chrome `open_tab_beside` (below current tab, same group; click not sent to CEF). `window.open` with features (`NEW_POPUP`) / `about:blank` is a windowless CEF browser (so `window.open` returns a `Window`); chrome adopts that engine tab and focuses it beside the opener. `target=_blank` / `NEW_WINDOW` cancel the native popup and open a focused chrome tab. ⌘T / `OpenUrl` / `xdg-open` use `open_tab` (loose, end of strip). An outside open (`chrome.sock` / `Topic::OpenUrl` with `activate`) asks the shell to raise the existing window (click-activation: MRU + composition + seat; sock probes are not activate). F12 / Browser → Developer Tools opens windowed CEF DevTools. Super+left/right are not WM pointer bindings (CSD titlebar still moves floats). Page right-click is `ContextMenuHandler::run_context_menu` (native cancelled) → `FromEngine::PageContext` → kit menu. Session history rides the tab snapshot (`TabInfo.history`); hold-nav uses `NavCmd::GoHistory`. IME caret is `OnImeCompositionRangeChanged` → `FromEngine::ImeCaret` (view px) so chrome can `request_input_method` at the composition box. Downloads: helper `DownloadHandler` → `FromEngine::Download` (any helper, including parked) → chrome list; cancel is `ToEngine::CancelDownload`. Passkey **get** / **create**: helper injects WebAuthn intercept in every frame → `FromEngine::WebAuthn` → chrome vault picker (same-site same-action coalesced; `create()` confirms then `Fido2Client::register` + POST/PUT cipher). One iced chrome (`chrome.sock`); a second process hands off a URL and exits. Helper death respawns CEF and restores tabs. Reap only orphan / pre-`exec_self` helpers. `<select>` is `PET_POPUP` blitted onto the VIEW CPU frame (not a second window). Only the front helper composites (`SetFront` + `was_hidden` + `windowless_frame_rate`). CEF `root_cache_path` = that profile’s `…/cef/` so cookies persist. |
| CEF pin | `cef` crate + workspace `cef-version`; `cargo make install-cef` unpacks the public tarball to `~/.cache/sola/cef-<ver>/`. H.264/AAC: replace that tree with `scripts/cef-codecs/` (same pin). |
| CEF switches | `--ozone-platform=wayland`; `--password-store=basic`; `--remote-allow-origins=*`; `--autoplay-policy=no-user-gesture-required`. Local `libcef` is a codecs rebuild of the same 147.0.10 pin (`proprietary_codecs=true ffmpeg_branding=Chrome`, `use_vaapi=false`; recipe `scripts/cef-codecs/`). Public cef-builds tarball had no H.264/AAC. No Widevine CDM; Steam store trailers are clear DASH, not EME. |
| Profiles (D8) | Registry `profiles.json`; data/cache under `profiles/<uuid>/`; session `session.json` (tabs + optional `group_id` + `groups[]` with optional `color` + `closed[]` for ⌘⇧T); shared downloads index `shared/downloads.json`; chrome parks tab-strip snapshots + last CPU composites (`FrameSlot.parked_frames`); switch points the router at the target helper (pages stay loaded). Eviction: `tab_cache` (idle 30m, max 4 parks, max 48 tabs total) |
| Tab / profile paint | `present_tab`: same-size parked frame → GPU this frame; miss → blank immediately (never keep the previous page). Helpers skip same-size `Resize` so the parked compositor stays live. Iced presents paints via the shader `request_redraw` pump (does not rebuild chrome at 60 Hz). Chrome `Tick` is `chrome_wake` from helper queues (plus a 250 ms timer only for copy-URL flash / TOTP / fill-wait), not an idle 16 ms or 250 ms pump. |

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
| Catalog | `Topic::Application` fields `kind: wrapper` + `url`. Command synthesized `/opt/sola/bin/sola-wrapper <id>`. Launch lookup: `state.yaml` (bus persistence). |
| Profile | Durable `~/.config/sola/wrapper/<id>/cef`; cache `$XDG_CACHE_HOME/sola/wrapper/<id>/`. Notification grants `…/notifications.json`; mic/camera `…/media.json`. `profiles::bind_external` so this is **not** `browser_data_root()`. |
| Chrome | Kit CSD while floating + one CEF page. **Edit** menubar (⌘X/C/V/A) via shell `MenuAction`; paste is compositor clipboard → `PasteImage` (`File`) or `PasteText`. Web `Notification` → desk card (`app_id` = wrapper id). Off-site `target=_blank` / ⌘-click / `window.open` → `sola_core::open_url` (sola-browser). Same-site / `about:blank` NEW_POPUP is a windowless CEF tab (huddle). `getUserMedia` → kit Allow / Block; Chromium `MEDIASTREAM_CAMERA` / `MEDIASTREAM_MIC` content settings follow `media.json`. |
| Singleton | `$XDG_RUNTIME_DIR/sola/wrapper/<id>.sock` — second spawn raises the live window. |

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
