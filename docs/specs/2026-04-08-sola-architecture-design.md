# Sola Architecture Design

**Date:** 2026-04-08
**Status:** Approved

## Overview

Sola is a Wayland desktop shell — a full compositor and desktop environment. It is a deliberate rebuild of Cogsworth, replacing X11 with Wayland and establishing clean architecture from the start.

The primary problem with Cogsworth was that rapid development produced a chaotic codebase with intractable bugs. Sola aims to avoid this through careful, incremental, user-approved development.

## Core Architecture

### Compositor: Smithay (Full Ownership)

Sola owns the entire display pipeline via Smithay:

- **Backend:** DRM/KMS — direct hardware access, no host compositor
- **Input:** libinput via Smithay — keyboard, mouse, touch
- **Wayland protocol:** Smithay's built-in protocol handling
- **Surface management:** Compositor tracks and composites all client surfaces
- **Renderer:** GlesRenderer (OpenGL ES) — composites Wayland client buffers to scanout

No Winit/windowed development backend. All testing happens on real hardware (canto).

### Shell UI: WebKit6 WebViews

All Sola UI — desktop chrome, app windows, overlays — renders in WebKit6 WebViews that connect as Wayland clients to the compositor.

- Web frontends are framework-agnostic (Svelte, React, vanilla, anything)
- Each UI surface is a separate WebKit6 process/WebView
- Compositor treats shell WebViews as privileged clients (special positioning, input routing)

This mirrors Cogsworth's WebView-centric approach but with cleaner separation: the compositor knows nothing about web tech, WebViews know nothing about DRM/KMS.

### External Applications

Non-Sola Wayland applications run as regular clients. The compositor manages their surfaces like any other. Decoration strategy (CSD vs SSD) to be decided later.

## Workspace Layout

```
Sola/
├── crates/
│   ├── sola/                  # Binary entry point (clap CLI)
│   ├── sola-compositor/       # Smithay compositor (DRM/KMS, input, surface mgmt)
│   ├── sola-protocol/         # Shared types, wire format
│   └── sola-make/             # Build/deploy orchestration (xtask)
├── apps/
│   └── desktop/               # Shell UI (web tech, framework-agnostic)
├── docs/
│   ├── manual/                # Architecture docs, references
│   └── specs/                 # Design specs and implementation plans
├── .cargo/config.toml         # cargo make alias
├── Cargo.toml                 # Workspace root
└── CLAUDE.md
```

Additional crates (sola-app, sola-mail, etc.) will be added as scope grows.

## Build System

**xtask pattern** via `sola-make` crate, replacing Cogsworth's Makefile+scripts approach.

```
cargo make build              # Build everything
cargo make build <target>     # Build specific target
cargo make deploy canto       # Deploy to canto
```

Build orchestration logic (frontend compilation, asset embedding, hash-based caching, deploy) is written in Rust with clap, not shell scripts.

## Deploy

- **Target:** canto (physical machine, SSH access)
- **Binary location:** `/opt/sola/bin/`
- **Process:** `cargo make deploy canto` builds release, rsync's to canto
- **Launch:** User runs `/opt/sola/bin/sola` manually from a physical TTY

## Phase 1: Pixels on Screen

The immediate goal is a minimal working Wayland compositor:

1. `sola` binary starts `sola-compositor`
2. Compositor initializes DRM/KMS on canto's NVIDIA GPU
3. Compositor claims a Wayland display
4. Presents frames — solid color on screen, proving the compositor owns the display

### Phase 1 explicitly excludes:
- WebKit6 client launching
- Zone-based window management
- External app management
- App launcher/switcher
- Built-in apps
- IPC protocol
- Frontend build pipeline

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Compositor library | Smithay | Pure Rust, full control, no C FFI wrappers |
| Display backend | DRM/KMS only | Testing on real hardware (canto), no windowed dev mode |
| UI rendering | WebKit6 WebViews | Leverage web tech for all UI, framework-agnostic |
| Build system | cargo make (xtask) | Rust-native, replaces fragile Makefile+scripts |
| Web framework | None prescribed | Each app/component picks its own |
| Development flow | Build local, deploy to canto | SSH + rsync, manual launch from TTY |
