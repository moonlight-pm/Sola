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
         │              │  agent · mail · paint · …    │
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
| `crates/sola-shell` | Menubar, launcher, switcher, zoning (iced daemon) |
| `crates/sola-kit` | Iced app kit + storybook (incl. `FilePicker`) |
| `crates/sola-settings` | Settings panel (theme, apps, mail config, …) |
| `crates/sola-terminal` | Untitled-shell terminal (alacritty grid + iced). Also a **library** for the grid/PTY (`tmux::configure` for other sockets). |
| `crates/sola-workspaces` | Project / workspace rail + agent-aware PTYs (tmux `sola-ws`). Catalog `~/.config/sola/workspaces/catalog.json` (migrates `agent-terminal/`). Siblings under `<root>/.worktrees/`. Call owner `workspaces` (`solactl workspaces …`; methods: `ps`, `project.{list,add,rm,startup}`, `workspace.{list,spawn,set,rm,select,exec}`, `pane.{list,send,read,wait}`, `whoami`). Per-project `startup` script runs in a new worktree after spawn. `project.rm` unregisters a project + kills its tmux, leaves worktrees. Attach stamps `SOLA_WS_PATH`; restart attaches only on path match and quarantines leftovers. Grok hooks on `$XDG_RUNTIME_DIR/sola-ws-hooks.sock`; OSC 9999 stripped in the term lib. Compaction `×N` reads `~/.grok/sessions/<encoded-cwd>/<sid>/` (`compaction/segment_*.md`, `compaction_checkpoints/`, then `signals.json` `compactionCount`). |
| `crates/sola-browser` | Iced chrome + CEF engine (single crate) |
| `crates/sola-agent` | Coding agent UI (ACP → Grok leader) — not the start of Workspaces |
| `crates/sola-mail` | Kit-native mail client. Emits sticky `Topic::MailStatus` (inbox unread) for the menubar; retracts on quit. |
| `crates/sola-monitor` | System monitor / bus audit |
| `crates/sola-kvm` | KVM / input bridge (Linux ↔ Mac) |
| `crates/sola-preview` | Screenshot + standalone argv image viewer |
| `crates/sola-paint` | Default image viewer/editor (MIME, `solactl open`; singleton via `OpenImage`; tabs in `~/.config/sola/paint.yaml`) |
| `crates/sola-arcade` | Steam library browser + windowed-gamescope game launch |
| `crates/solactl` | Operator CLI (`compositor`, `session`, emit, logs, …) |
| `crates/sola-install` | Kit installer wizard + apply orchestration (`sola-install-apply`) |
| `crates/sola-make` | `cargo make` xtask (build / install / publish / **vm** / **iso**) |
| `crates/sola-assets` | Vendored icons/assets |
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
| Surfaces / input | Wayland protocols via River |
| Launch | Dev: physical TTY → `/opt/sola/bin/sola`. Dist image: loginless `sola-desktop` → Sola |
| Binaries | Dev install `/opt/sola/bin/`; images stage from `target/release` |
| Logs | `/opt/sola/log/` (and tracing to TTY when run interactively) |
| Persistent stickies | Bus writes `~/.config/sola/state.toml` |
| Arcade library cache | `~/.config/sola/arcade-library.json` (scan snapshot; bg rescan on open) |
| Agent overlay | `~/.config/sola/agent/overlay.json` (pins, titles, sidebar width) |
| Workspaces catalog | `~/.config/sola/workspaces/catalog.json` (projects / workspaces / selected; migrates `agent-terminal/`) |
| Workspaces calls | sola-call owner `workspaces` (`solactl workspaces …`). First-class: [`2026-08-18-workspaces-cli-design.md`](specs/2026-08-18-workspaces-cli-design.md) |
| Grok sessions | `~/.grok/sessions/` + leader socket `~/.grok/leader.sock` |
| Self-update of apps | Binary watch → re-exec when `/opt/sola/bin/<name>` changes (`SOLA_NO_SELF_WATCH=1` skips) |

### Communication layers

1. **Sola Bus** — lifecycle, focus, themes, app menus, session commands,
   stickies. Fan-out facts. No request/reply. Mail unread is
   `Topic::MailStatus` (sticky, not persisted).  
2. **sola-call** — live method registry; request id, timeout, error to the
   caller. `solactl compositor` / `session`; kit apps advertise via
   `CallSetup` / `BusSetup::calls`. Fail if the owner is not connected.  
3. **Wayland** — buffers, seats, layers, xdg surfaces. Pixel and input plane.

---

## Shell windows (as-built)

`sola-shell` is a single `iced::daemon` with on-demand windows:

| Kind | Role |
|------|------|
| Menubar | Top chrome, menus, mail unread chip (when `sola-mail` is mapped), stats, toasts |
| Menu | Open application menus |
| Launcher | App launch |
| Switcher | MRU window/app switch |

Zoning / floating is coordinated with `sola-river` over the bus:
unassigned windows **default-float** (client size + `Topic::WindowFloating`);
saved zones restore frames; Meta+numpad snaps assign zones.

### Games / Arcade (as-built)

| Piece | Role |
|-------|------|
| `sola-arcade` | Kit app: Steam library gallery (search; A–Z / Recent; Ready-to-play filter default on; Install on uninstalled; Stop-on-row) |
| Library data | Offline: ACF manifests + `localconfig` activity + `appinfo.vdf` names; cache `~/.config/sola/arcade-library.json`; UI opens from cache, full scan always in background |
| Banners | Lazy viewport decode (+ overscan); paths resolved when row visible |
| Launch | `Topic::LaunchApp` → `sola-arcade --run <id>` → `gamescope … -- sola-arcade --nested-steam <id>` → desktop Steam `-applaunch` (no BPM; kill Steam when game `AppId=` exits) |
| Session lock | Active Play → Stop on that row; other Plays disabled; `session_alive` via `/proc` cmdline |
| River | gamescope pre-init pin then zone/float; Cinema exit-fullscreen on next zone Frame; empty app_id → `gamescope` via pid; nest `-S fit` letterbox |
| AppHidden | Bus sticky still exists (shell hide chip path); Arcade UI does not expose hide-Steam |

Operator: [`manual/sola-arcade.md`](manual/sola-arcade.md).

---

## Browser (as-built)

| Piece | Role |
|-------|------|
| `crates/sola-browser` | **Product browser** — iced chrome (full-width bar: kit identity select + nav + omnibox + downloads; etch tab strip with kit reorder; vault logins + cards) + CEF CPU OSR under `src/cef/` |
| App id / binary | `sola-browser` → `/opt/sola/bin/sola-browser` (shell launcher: one “Browser” entry; one Wayland window) |
| Engine helpers | Per-profile headless `sola-browser --engine --profile=<uuid>` (no iced / no xdg_toplevel). Control socket `profiles/<uuid>/engine.sock`; pixel frames on `engine.frame.sock` (raw BGRA, not bincode). Page copy is JS extract → `FromEngine::Clipboard` on the control socket → chrome writes Wayland. Paste is `ToEngine::PasteText` (JS insert in the **focused** frame only). ⌘-click hit-test is JS → `FromEngine::OpenBackgroundTab` → chrome `open_tab_beside` (below current tab, same group; click not sent to CEF). ⌘T / `OpenUrl` / `xdg-open` use `open_tab` (loose, end of strip). An outside open (`chrome.sock` / `Topic::OpenUrl` with `activate`) asks the shell to raise the existing window (click-activation: MRU + composition + seat; sock probes are not activate). F12 / Browser → Developer Tools opens windowed CEF DevTools. Super+left/right are not WM pointer bindings (CSD titlebar still moves floats). Page right-click is `ContextMenuHandler::run_context_menu` (native cancelled) → `FromEngine::PageContext` → kit menu. Session history rides the tab snapshot (`TabInfo.history`); hold-nav uses `NavCmd::GoHistory`. IME caret is `OnImeCompositionRangeChanged` → `FromEngine::ImeCaret` (view px) so chrome can `request_input_method` at the composition box. Downloads: helper `DownloadHandler` → `FromEngine::Download` (any helper, including parked) → chrome list; cancel is `ToEngine::CancelDownload`. Passkey **get** / **create**: helper injects WebAuthn intercept in every frame → `FromEngine::WebAuthn` → chrome vault picker (same-site same-action coalesced; `create()` confirms then `Fido2Client::register` + POST/PUT cipher). One iced chrome (`chrome.sock`); a second process hands off a URL and exits. Helper death respawns CEF and restores tabs. Reap only orphan / pre-`exec_self` helpers. `<select>` is `PET_POPUP` blitted onto the VIEW CPU frame (not a second window). Only the front helper composites (`SetFront` + `was_hidden` + `windowless_frame_rate`). CEF `root_cache_path` = that profile’s `…/cef/` so cookies persist. |
| CEF pin | `cef` crate + workspace `cef-version`; install tarball under `~/.cache/sola/cef-<ver>/` via `cargo make install-cef` |
| Profiles (D8) | Registry `profiles.json`; data/cache under `profiles/<uuid>/`; session `session.json` (tabs + optional `group_id` + `groups[]`); shared downloads index `shared/downloads.json`; chrome parks tab-strip snapshots + last CPU composites (`FrameSlot.parked_frames`); switch points the router at the target helper (pages stay loaded). Eviction: `tab_cache` (idle 30m, max 4 parks, max 48 tabs total) |
| Tab / profile paint | `present_tab`: same-size parked frame → GPU this frame; miss → blank immediately (never keep the previous page). Helpers skip same-size `Resize` so the parked compositor stays live. Iced presents paints via the shader `request_redraw` pump (does not rebuild chrome at 60 Hz). |

Former split (`sola-browser-core` / `-wpe` / `-cef` dispatcher) and the WPE
content-plane path are **retired**. CEF: do **not** enable `accelerated_osr`
(CPU `on_paint` path only).

---

## Build

```text
cargo make build              # all / subset
cargo make install            # needs explicit user permission
cargo make install <app>…     # targeted install
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
| Shape 1 | Flake `packages.sola` + `nixosModules.default`; colleague ops in root [`INSTALL.md`](../INSTALL.md) |
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
