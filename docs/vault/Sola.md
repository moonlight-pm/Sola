# Sola

Sola is a Wayland desktop shell — a full compositor and desktop environment built in Rust with [Smithay](https://smithay.github.io/), using WebKit6 WebViews for all UI rendering.

A deliberate rebuild of Cogsworth, moving from X11 to Wayland.

## System Topology

```
┌───────────────────────────────────┐
│         sola (process manager)    │
│         Launches & restarts all   │
└──┬────────┬────────┬────────┬─────┘
   │        │        │        │
   ▼        ▼        ▼        ▼
┌──────┐ ┌────────┐ ┌──────┐ ┌──────┐
│sola- │ │sola-   │ │sola- │ │shell │
│bus   │ │compos- │ │x     │ │apps  │
│      │ │itor   │ │      │ │      │
└──┬───┘ └───┬────┘ └──┬───┘ └──┬───┘
   │         │         │        │
   └─────────┴─────────┴────────┘
      Unix socket: sola-bus
```

All components are independently restartable. Shell apps are resilient to bus and compositor restarts. No launch ordering required.

## Components

| Component           | Role                           | Process     |
| ------------------- | ------------------------------ | ----------- |
| [[sola]]            | Process manager                | Main binary |
| [[sola-bus]]        | IPC bus + protocol definitions | Separate    |
| [[sola-compositor]] | Wayland compositor             | Separate    |
| [[sola-x]]          | XWayland host / bridge         | Separate    |
| [[sola-switcher]]   | App switcher (Super+Tab)       | Separate    |
| [[sola-make]]       | Build/deploy orchestration     | Dev tool    |

## Communication

Two communication layers:

- **[[Sola Bus]]** — events over Unix socket. All Sola components. Control plane: shell events, lifecycle, metadata.
- **Wayland protocol** — surfaces, buffers, input. Data plane: pixels at 60fps.

The bus handles coordination. Wayland handles rendering.

## Key Design Decisions

- [[Input Routing]] — Super held = bus, everything else = focused Wayland client
- [[Process Model]] — every component is a separate process, sola supervises all
- [[Wire Format]] — three fields: id (UUIDv7), topic (String), payload (`Option<Bytes>`)
- No sola-shell coordinator — each app coordinates its own flow
- Compositor is "dumb" — no knowledge of what shortcuts mean

## Workspace

```
crates/
  sola/                # Process manager
  sola-bus/            # Bus host + protocol
  sola-compositor/     # Smithay compositor
  sola-x/              # XWayland host (in progress)
  sola-make/           # Build system
apps/
  switcher/            # App switcher
  wtest/               # Wayland test client
  xtest/               # X11 test client
docs/
  vault/               # This vault
  specs/               # Design specs
  manual/              # Architecture references
```

## Deploy Environment

Target machine: **canto** (separate physical hardware, SSH access)

- Two AMD Radeon R7 370 GPUs (amdgpu), display on card2-DP-10
- Binaries: `/opt/sola/bin/`
- Logs: `/opt/sola/log/`
- Launched manually from a physical TTY
