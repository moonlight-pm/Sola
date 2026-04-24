# Sola

Sola is a Wayland desktop shell — the River compositor hosted under a
supervisor, with a typed IPC bus coordinating a small set of long-
running processes and WebKit6 WebView shell apps.

## System Topology

```
┌────────────────────────────────────┐
│         sola (process manager)     │
│         Launches & restarts all    │
└──┬──────┬──────────┬────────┬──────┘
   │      │          │        │
   ▼      ▼          ▼        ▼
┌──────┐ ┌──────┐ ┌────────┐ ┌────────┐ ┌────────┐
│River │ │sola- │ │sola-   │ │sola-   │ │sola-   │
│      │ │bus   │ │river   │ │shell   │ │session │
└──────┘ └──┬───┘ └───┬────┘ └──┬─────┘ └──┬─────┘
            │         │         │          │
            └─────────┴─────────┴──────────┘
                Unix socket: sola-bus
```

River is spawned as a direct child of `sola` and is a prerequisite
for everything else. The remaining processes are managed children;
each connects to the bus and speaks to River (for wayland) through
the sola-river bridge.

All components are independently restartable. Shell apps tolerate
bus and compositor restarts.

## Components

| Component       | Role                                                    | Process   |
|-----------------|---------------------------------------------------------|-----------|
| [[sola]]        | Process manager                                         | Main      |
| [[sola-bus]]    | IPC bus host + client library + protocol definitions    | Separate  |
| sola-river      | River ↔ bus bridge (window manager, xkb bindings)       | Separate  |
| sola-shell      | Desktop shell — launcher, switcher, menubar, zoning     | Separate  |
| sola-session    | User app session manager (spawn, close, reap)           | Separate  |
| [[sola-app]]    | WebView app framework (GTK4 + WebKit6)                  | Library   |
| sola-core       | Shared primitives — log, env, process, keys, encrypted  | Library   |
| sola-assets     | Vendored icon/asset bundles                             | Library   |
| [[sola-make]]   | Build/install orchestration (xtask)                     | Dev tool  |

## Communication

Two layers:

- **[[Sola Bus|sola-bus]]** — events over a Unix socket. Coordination,
  lifecycle, persistent config snapshots. Control plane.
- **Wayland protocol** — surfaces, buffers, input. River delivers
  pixels; sola-river translates seat/window events into bus topics.

## Configuration

State is **not** a central manager anymore. Each owner emits its
state as a persistent [[Topics#Behavior|sticky]] bus topic, and the
bus writes those to a single file on disk.

- File: `~/.config/sola/state.toml`
- One `[Section]` per persistent [[Topics|topic kind]]
- On bus startup the file is parsed and each section is restored as a
  sticky message tagged `source = "sola-bus"`; new subscribers get
  the latest value via normal sticky replay
- Apps never read or write state.toml themselves; they subscribe to
  the topic and emit updates. The bus owns the disk.

The first migrated persistent topic is `Zones` (shell's zone
assignments). The spec at
`docs/specs/2026-04-24-persistent-bus-design.md` has the full
rationale.

Secrets in persistent topics use `sola_core::Encrypted<T>`, which
encrypts on human-readable serializers (TOML) and passes through on
binary (postcard wire). See [[sola-bus#Encrypted payloads]].

## Key Design Decisions

- [[Process Model]] — every component is a separate process under
  sola's supervision
- [[Wire Format]] — `Message { id, topic, payload, sticky, source }`;
  postcard on the socket, length-prefixed
- [[Topics]] — typed via `define_topics!` with per-variant
  `#[sticky]` / `#[persistent]` annotations controlling retention
- River bridge pattern — sola-river speaks River's
  `river_window_manager_v1` + `river_xkb_bindings_v1` protocols and
  re-surfaces everything as typed bus topics
- Binary resolution via `$PATH` — works on NixOS and traditional
  distros without hardcoding

## Workspace

```
crates/
  sola/                # Process manager
  sola-bus/            # Bus host + client library + protocol
  sola-core/           # Shared primitives (env, process, log, keys, encrypted, watcher)
  sola-app/            # WebView app framework (GTK4 + WebKit6)
  sola-assets/         # Vendored icon/asset bundles
  sola-make/           # Build/install orchestration (xtask)
  sola-river/          # River compositor bridge (bus ↔ wayland)
  sola-session/        # User-app session manager (spawn/close/reap)
  sola-shell/          # Desktop shell — launcher, switcher, menubar, zoning
docs/
  specs/               # Design specs and implementation plans
  vault/               # Canonical architecture docs (this vault)
```

`apps/*` (browser, mail, terminal, monitor, agent, settings) are
temporarily excluded from the workspace. They'll be re-added as each
is rewritten against the new bus model.

## Runtime Environment

- **Platform:** NixOS, developer launches from a physical TTY
- **Binaries:** `/opt/sola/bin/`
- **Logs:** `/opt/sola/log/sola.log` (shared, rotated at 100KB)
- **Persistent state:** `~/.config/sola/state.toml`
- **Encryption key:** `~/.config/sola/key` (mode 0600, auto-generated)
