# Sola

Sola is a Wayland desktop shell — a full compositor and desktop environment built in Rust with Smithay, using WebKit6 WebViews for all UI rendering.

## System Topology

```
┌───────────────────────────────────┐
│         sola (process manager)    │
│         Launches & restarts all   │
└──┬────────┬────────┬────────┬─────┘
   │        │        │        │
   ▼        ▼        ▼        ▼
┌──────┐ ┌────────┐ ┌──────┐ ┌──────────┐
│sola- │ │sola-   │ │sola- │ │sola-     │
│bus   │ │river   │ │shell │ │session   │
└──┬───┘ └───┬────┘ └──┬───┘ └──┬───────┘
   │         │         │        │
   └─────────┴─────────┴────────┘
      Unix socket: sola-bus
```

River (wlroots compositor) is spawned directly by sola as a prerequisite. All other components are managed processes.

All components are independently restartable. Shell apps are resilient to bus and compositor restarts.

## Components

| Component | Role | Process |
|---|---|---|
| [[sola]] | Process manager | Main binary |
| [[sola-bus]] | IPC bus + protocol definitions | Separate |
| sola-river | River ↔ bus bridge (wayland protocols) | Separate |
| sola-shell | Desktop shell (switcher, launcher, menubar, zoning) | Separate |
| sola-session | User app session manager + config store | Separate |
| [[sola-app]] | WebView app framework (GTK4 + WebKit6) | Library |
| sola-core | Shared primitives (log, env, process, config, keys) | Library |
| sola-make | Build/install orchestration | Dev tool |

## Communication

Two communication layers:

- **[[Sola Bus]]** — events over Unix socket. All Sola components. Control plane: shell events, lifecycle, config, metadata.
- **Wayland protocol** — surfaces, buffers, input. Data plane: pixels at 60fps.

The bus handles coordination. Wayland handles rendering.

## Configuration

Centralized config store managed by sola-session:

- Persisted as `~/.config/sola/sola.toml`
- Broadcast on the bus as `Topic::Config` (sticky, flat key-value snapshot)
- Apps mutate config via `Topic::MutateConfig` (validated by session)
- See [[Topics]] for details

## Key Design Decisions

- [[Process Model]] — every component is a separate process, sola supervises all
- [[Wire Format]] — Message: id (UUIDv7), topic (String), payload (postcard bytes), sticky, source
- [[Topics]] — typed via `define_topics!` macro, config types in sola-core
- Compositor bridge pattern — sola-river translates between River's wayland protocols and the bus
- Binary resolution via PATH lookup — no hardcoded paths, works on NixOS

## Workspace

```
crates/
  sola/                # Process manager
  sola-bus/            # Bus host + client library + protocol
  sola-core/           # Shared primitives (env, process, config, log, keys, watcher)
  sola-app/            # WebView app framework (GTK4 + WebKit6)
  sola-assets/         # Vendored icon/asset bundles
  sola-make/           # Build/install orchestration (xtask)
  sola-river/          # River compositor bridge (bus ↔ wayland)
  sola-session/        # User-app session manager + config store
  sola-shell/          # Desktop shell — launcher, switcher, menubar, zoning
apps/
  agent/               # AI agent frontend
  browser/             # WebKit browser
  mail/                # IMAP/SMTP mail client
  monitor/             # System monitor / bus audit
  settings/            # Settings panel
  terminal/            # Terminal emulator (tmux-backed, xterm.js)
docs/
  manual/              # Architecture docs, references
  specs/               # Design specs and implementation plans
  vault/               # This vault
```

## Runtime Environment

- **Platform:** NixOS (development machine: novus)
- **Binaries:** `/opt/sola/bin/`
- **Logs:** `/opt/sola/log/sola.log` (shared by all processes, rotated at 100KB)
- **Config:** `~/.config/sola/sola.toml`
- **Launch:** manually from a physical TTY — no display manager
