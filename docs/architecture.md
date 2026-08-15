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
bus, and multi-process **Iced** apps sharing `sola-kit`.

```text
                    ┌──────────────────────┐
                    │  sola (supervisor)   │
                    │  process manager     │
                    └──┬───┬───┬───┬───┬───┘
                       │   │   │   │   │
          ┌────────────┘   │   │   │   └────────────┐
          ▼                ▼   ▼   ▼                ▼
     ┌────────┐      ┌──────┐ ┌────────┐      ┌──────────┐
     │ River  │      │ bus  │ │ river  │      │ session  │
     │(comp.) │      │ host │ │ bridge │      │ manager  │
     └───┬────┘      └──┬───┘ └───┬────┘      └────┬─────┘
         │              │         │                │
         │              └────┬────┴────────────────┘
         │                   │  Unix socket (sola-bus)
         │              ┌────┴────────────────────────┐
         │              │  shell · settings · terminal │
         │              │  agent-terminal · browser    │
         │              │  agent · mail · …            │
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
| `crates/sola-core` | Shared primitives (env, process, config, log, …) |
| `crates/sola-river` | River ↔ bus bridge |
| `crates/sola-session` | User-app session manager (spawn / close / reap) |
| `crates/sola-shell` | Menubar, launcher, switcher, zoning (iced daemon) |
| `crates/sola-kit` | Iced app kit + storybook |
| `crates/sola-settings` | Settings panel (theme, apps, mail config, …) |
| `crates/sola-terminal` | Untitled-shell terminal (alacritty grid + iced). Also a **library** for the grid/PTY (`tmux::configure` for other sockets). |
| `crates/sola-agent-terminal` | Project / workspace rail + agent-aware PTYs (tmux `sola-at`). Catalog `~/.config/sola/agent-terminal/catalog.json`. Siblings under `<root>/.worktrees/`. `sat` on `$XDG_RUNTIME_DIR/sola-at-cli.sock`. Grok hooks on `$XDG_RUNTIME_DIR/sola-at-hooks.sock`; OSC 9999 stripped in the term lib. |
| `crates/sola-browser*` | Chrome + WPE (primary) / CEF (parallel) |
| `crates/sola-agent` | Coding agent UI (ACP → Grok leader) — not the start of agent-terminal |
| `crates/sola-mail` | Kit-native mail client |
| `crates/sola-monitor` | System monitor / bus audit |
| `crates/sola-kvm` | KVM / input bridge (Linux ↔ Mac) |
| `crates/sola-preview` | Image preview / selection capture handoff |
| `crates/sola-arcade` | Steam library browser + windowed-gamescope game launch |
| `crates/solactl` | CLI helpers |
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
| Surfaces / input | Wayland protocols via River |
| Launch | Dev: physical TTY → `/opt/sola/bin/sola`. Dist image: loginless `sola-desktop` → Sola |
| Binaries | Dev install `/opt/sola/bin/`; images stage from `target/release` |
| Logs | `/opt/sola/log/` (and tracing to TTY when run interactively) |
| Persistent stickies | Bus writes `~/.config/sola/state.toml` |
| Arcade library cache | `~/.config/sola/arcade-library.json` (scan snapshot; bg rescan on open) |
| Agent overlay | `~/.config/sola/agent/overlay.json` (pins, titles, sidebar width) |
| Agent-terminal catalog | `~/.config/sola/agent-terminal/catalog.json` (projects / workspaces / selected) |
| Agent-terminal CLI | `$XDG_RUNTIME_DIR/sola-at-cli.sock` (`sat`; fail if app down) |
| Grok sessions | `~/.grok/sessions/` + leader socket `~/.grok/leader.sock` |
| Self-update of apps | Binary watch → re-exec when `/opt/sola/bin/<name>` changes (`SOLA_NO_SELF_WATCH=1` skips) |

### Communication layers

1. **Sola Bus** — lifecycle, focus, themes, app menus, session commands,
   stickies. Control plane.  
2. **Wayland** — buffers, seats, layers, xdg surfaces. Pixel and input plane.

---

## Shell windows (as-built)

`sola-shell` is a single `iced::daemon` with on-demand windows:

| Kind | Role |
|------|------|
| Menubar | Top chrome, menus, stats, toasts |
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

## Browser engines (as-built)

| Engine | Crate | Role |
|--------|-------|------|
| WPE | `sola-browser-wpe` | **Primary** |
| CEF | `sola-browser-cef` | Parallel path; CEF `147.x` via `cef` crate |
| Shared chrome | `sola-browser-core` | Iced chrome |
| Dispatcher | `sola-browser` | Exec WPE or CEF |

CEF: do **not** enable `accelerated_osr`; dma-buf import is via Wayland
`zwp_linux_dmabuf_v1` and sola-river composition.

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
