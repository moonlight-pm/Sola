# Sola

Sola is a Wayland desktop shell — a full compositor and desktop environment built in Rust with Smithay, using WebKit6 WebViews for all UI rendering.

## Architecture

- **Compositor:** Smithay (pure Rust) — DRM/KMS backend, input handling, Wayland protocol, surface management
- **Renderer:** Smithay GlesRenderer (OpenGL ES) — composites Wayland client surfaces
- **Shell UI:** WebKit6 WebViews as privileged Wayland clients — all Sola chrome and apps
- **Web frontends:** Framework-agnostic. Any app or component can use any web framework (Svelte, React, vanilla, etc.)
- **IPC:** Wayland protocols + app-level socket/messages
- **Build system:** `cargo make` (xtask pattern via `sola-make` crate)

Reference codebase: `../Cogsworth` — Sola is a deliberate rebuild of Cogsworth, moving from X11 to Wayland.

## Workspace Structure

```
crates/
  sola/                # Binary entry point (clap CLI)
  sola-compositor/     # Smithay compositor
  sola-protocol/       # Shared types, wire format
  sola-make/           # Build/deploy orchestration (xtask)
apps/
  desktop/             # Shell UI (web tech, framework-agnostic)
docs/
  manual/              # Architecture docs, references
  specs/               # Design specs and implementation plans
```

## Development Rules

### Worktrees
- Always use `.worktrees/` for git worktrees.
- Only make code modifications in worktrees. Never commit code changes directly to master.
- Only merge worktree branches to master with explicit user permission.

### Deploying
- Only deploy when you have explicit user permission.
- Deploy target is **canto** (a separate physical machine accessible via SSH).
- `cargo make deploy canto` — builds release, rsync's binary to `/opt/sola/bin/` on canto.
- The user launches `sola` manually from a physical TTY on canto. Do not configure auto-start.

### Code Quality
- This is a deliberate, careful rebuild. The user reviews and approves all code.
- Keep modules small and focused. Prefer many small files over few large ones.
- No speculative abstractions — build what's needed now.

## Build System

Uses the xtask pattern with a `sola-make` crate:

```
cargo make build              # Build everything
cargo make build <target>     # Build a specific target
cargo make deploy canto       # Deploy to canto
```

Alias configured in `.cargo/config.toml`:
```toml
[alias]
make = "run -q -p sola-make --"
```

## Documentation

- All docs live under `docs/`.
- Architecture and reference docs go in `docs/manual/`.
- Design specs and implementation plans go in `docs/specs/`.
- Superpowers specs and plans also go in `docs/specs/`.

## Deploy Environment: Canto

- Separate physical machine with SSH access (`ssh canto`)
- NVIDIA GPU (same driver considerations as dev machine)
- Binaries deploy to `/opt/sola/bin/`
- User launches sola manually from a physical TTY — no display manager, no auto-login
