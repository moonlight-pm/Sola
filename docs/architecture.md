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
| `crates/sola-make` | `cargo make` xtask (build / install) |
| `crates/sola-assets` | Vendored icons/assets |
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

Zoning / floating behavior is coordinated with `sola-river` over the bus.

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
```

Alias: `cargo make` → `cargo run -q -p sola-make --` (see `.cargo/config.toml`).

---

## History note

`docs/vault/` and early freezes may still describe the **WebView** era. Treat
those as history unless a freeze header says otherwise. Living map is **this
file** + code under `crates/`.
