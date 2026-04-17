# Sola

Sola is a Wayland desktop shell — a full compositor and desktop environment built in Rust with Smithay, using WebKit6 WebViews for all UI rendering.

## Architecture

- **Process manager (`sola`):** Launches and supervises all components. No desktop or bus logic — pure process management.
- **Bus (`sola-bus`):** General-purpose IPC bus. Separate process. All Sola components communicate via bus events over a Unix socket.
- **Compositor (`sola-compositor`):** Smithay (pure Rust) — DRM/KMS backend, input handling, Wayland protocol, surface management, XWayland hosting. Separate process, bus client.
- **Renderer:** Smithay GlesRenderer (OpenGL ES) — composites Wayland client surfaces
- **Shell apps:** WebKit6 WebViews as Wayland clients + bus clients. Each is a separate process (switcher, launcher, panel, etc.).
- **Web frontends:** Framework-agnostic. Any app or component can use any web framework (Svelte, React, vanilla, etc.)
- **IPC:** Sola Bus (events over Unix socket) + Wayland protocols for surfaces/input
- **Build system:** `cargo make` (xtask pattern via `sola-make` crate)

All components are independently restartable. Sola apps are resilient to bus and compositor restarts.

Reference codebase: `../Cogsworth` — Sola is a deliberate rebuild of Cogsworth, moving from X11 to Wayland.

## Workspace Structure

```
crates/
  sola/                # Process manager (binary entry point)
  sola-bus/            # Bus host process + protocol definitions
  sola-compositor/     # Smithay compositor + XWayland host (bus client)
  sola-app/            # WebView app framework (Rust host + JS platform lib)
  sola-make/           # Build/deploy orchestration (xtask)
apps/
  switcher/            # App switcher (WebView, bus client)
  terminal/            # Terminal emulator (tmux-backed, xterm.js, bus client)
docs/
  manual/              # Architecture docs, references
  specs/               # Design specs and implementation plans
  vault/               # Obsidian vault — architecture docs
```

## Development Rules

### Worktrees
- Always use `.worktrees/` for git worktrees.
- Only make code modifications in worktrees. Never commit code changes directly to master.
- Only merge worktree branches to master with explicit user permission.

### Deploying
- Only deploy when you have explicit user permission.
- Deploy target is **canto** (a separate physical machine accessible via SSH).
- `cargo make deploy --canto` — builds release, rsync's all binaries to `/opt/sola/bin/` on canto.
- `cargo make deploy <app> --canto` — builds and deploys a single app.
- `cargo make deploy <app> --canto --watch` — watches for changes, rebuilds, and redeploys automatically.
- The user launches `sola` manually from a physical TTY on canto. Do not configure auto-start.

### Building
- Always use `cargo make build` and `cargo make deploy --canto` — never raw `cargo build` or `rsync`.
- This ensures our build system stays tested and current.

### Debugging
- Before adding debug logging or guessing at fixes, look up how reference implementations handle the same problem. Check niri, anvil, cosmic-comp, or Smithay docs first.
- Read the actual Smithay source for the API you're calling — don't assume signatures or behavior.
- One targeted fix based on understanding beats five speculative attempts.

### Code Quality
- This is a deliberate, careful rebuild. The user reviews and approves all code.
- Keep modules small and focused. Prefer many small files over few large ones.
- No speculative abstractions — build what's needed now.

## Build System

Uses the xtask pattern with a `sola-make` crate:

```
cargo make build                                  # Build everything
cargo make build <target>                         # Build a specific target
cargo make deploy --canto                         # Deploy all to canto
cargo make deploy <app> --canto                   # Deploy one app to canto
cargo make deploy <app> --canto --watch           # Watch + redeploy on change
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

## Debugging and Logging

### Principles
- All errors must be diagnosable after the fact. Never lose output to a TTY.
- Persistent log files at `/opt/sola/log/` on canto. Always write logs there.
- Use `tracing` with structured fields — always include relevant context (device node, connector, crtc, etc.).
- Errors should explain *what went wrong* and *what was being attempted*. Don't swallow errors silently.
- When SSH'd to canto, you can run sola on its display for debugging. Use this.

### Remote Debugging Workflow
```bash
# SSH to canto, run sola with debug logging, logs go to file AND terminal
ssh canto "RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log"

# Check recent logs
ssh canto "tail -100 /opt/sola/log/sola.log"
```

### Log Levels
- `error` — something broke, action needed
- `warn` — unexpected but handled (e.g., GPU quirk worked around)
- `info` — lifecycle events (startup, device found, output connected, shutdown)
- `debug` — detailed flow (event loop ticks, input events, frame timing)
- `trace` — extremely verbose (every VBlank, every Wayland message)

## Deploy Environment: Canto

- Separate physical machine with SSH access (`ssh canto`)
- **Two AMD Radeon R7 370 GPUs** (amdgpu driver), display on card2-DP-10
- Binaries deploy to `/opt/sola/bin/`
- Logs go to `/opt/sola/log/`
- User launches sola manually from a physical TTY — no display manager, no auto-login

## Code Intelligence

Two MCP-based knowledge graphs are available. They have different strengths — use the right one for the task.

### When to Use Which

| Task | Tool | Why |
|------|------|-----|
| How does X flow through the system? | GitNexus `query` | Returns ranked execution flows across modules |
| What breaks if I change X? | GitNexus `impact` | Blast radius with depth-grouped affected symbols |
| Give me context to implement Y | CodeGraph `context` / `explore` (via Explore agent) | Returns entry points + source code for a task |
| Find a symbol by name | CodeGraph `codegraph_search` | Fast, returns locations |
| Trace callers/callees | CodeGraph `codegraph_callers` / `codegraph_callees` | Direct call graph |
| Safe multi-file rename | GitNexus `rename` (dry_run first) | Graph-aware, not text replace |
| Pre-commit scope check | GitNexus `detect_changes` | Verify only expected symbols changed |

### Before Editing Code

- Run `gitnexus_impact` on symbols you're about to modify (direction: `upstream`). If risk is HIGH or CRITICAL, warn the user before proceeding.
- Update all d=1 (direct callers/importers) affected by your changes.

### Before Committing

- Run `gitnexus_detect_changes()` to verify changes match expected scope.

### CodeGraph in Main Session

Never call `codegraph_explore` or `codegraph_context` directly in the main session — they return too much source code. Spawn an Explore agent instead, with this instruction in the prompt:

> Use `codegraph_explore` as your PRIMARY tool. Follow the call budget in the tool description. Do NOT re-read files that codegraph_explore already returned source for.

The main session may use these lightweight CodeGraph tools directly:
- `codegraph_search` — find symbols by name
- `codegraph_callers` / `codegraph_callees` — trace call flow
- `codegraph_impact` — check what's affected
- `codegraph_node` — single symbol details

### Keeping Indexes Fresh

After committing, both indexes become stale:
```bash
gitnexus analyze    # re-index GitNexus
```
