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
         │              │  browser · agent · mail · …  │
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
| `crates/sola-terminal` | Terminal (alacritty grid + iced) |
| `crates/sola-browser*` | Chrome + WPE (primary) / CEF (parallel) |
| `crates/sola-agent` | Coding agent UI (ACP → Grok leader) |
| `crates/sola-mail` | Kit-native mail client |
| `crates/sola-monitor` | System monitor / bus audit |
| `crates/sola-kvm` | KVM / input bridge (Linux ↔ Mac) |
| `crates/sola-preview` | Image preview / selection capture handoff |
| `crates/solactl` | CLI helpers |
| `crates/sola-install` | Installer wizard UI (kit; dry-run until real apply) |
| `crates/sola-make` | `cargo make` xtask (build / install / publish / **vm**) |
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
| Launch | User starts `/opt/sola/bin/sola` from a **physical TTY** |
| Binaries | Install to `/opt/sola/bin/` |
| Logs | `/opt/sola/log/` (and tracing to TTY when run interactively) |
| Persistent stickies | Bus writes `~/.config/sola/state.toml` |
| Agent overlay | `~/.config/sola/agent/overlay.json` (pins, titles, sidebar width) |
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
```

Alias: `cargo make` → `cargo run -q -p sola-make --` (see `.cargo/config.toml`).

### Distribution (as-built)

| Path | What |
|------|------|
| Shape 1 | Flake `packages.sola` + `nixosModules.default`; colleague hosts import module |
| Shape 2 | Flake `nixosConfigurations.sola-vm` + `packages.sola-vm-qcow2`; `SOLA_VM_STAGE` + `--impure` for local stage |
| Stage source | Always `target/release` (this tree); never `/opt/sola/bin`; guest ELFs patchelf’d for image glibc |
| Image profile | `nix/image/configuration.nix` — quiet boot, Plymouth theme `sola`, cage kiosk → `sola-install` |
| Splash | `nix/image/plymouth/` — 5 frames, clockwise cyan shade gradient on petals |
| Target system | `nixosConfigurations.sola-installed` — quiet boot + loginless sola-desktop |
| Apply | `sola-install-apply` (sudo): GPT ESP+root, `nixos-install --system`, install-user |
| Desktop seat | `sola-desktop` unit: ensure user → `runuser` → `/opt/sola/bin/sola` |
| Local products | `var/images/sola-vm.qcow2`, overlay, `sola-install-target.qcow2` (never committed) |
| vm run | Boot installed target if present; else live installer + vdb |
| vm install | Wipe target qcow + boot live installer |
| Product ISO | Not built yet |

---

## History note

`docs/vault/` and early freezes may still describe the **WebView** era. Treat
those as history unless a freeze header says otherwise. Living map is **this
file** + code under `crates/`.
