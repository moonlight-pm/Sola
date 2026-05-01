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

## Workspace Structure

```
crates/
  sola/                # Process manager (binary entry point)
  sola-bus/            # IPC bus host + client library
  sola-core/           # Shared primitives (env, process, watcher, config, log, ...)
  sola-app/            # WebView app framework (GTK4 + WebKit6)
  sola-kit/            # WebView app framework + design-token kit + storybook (parallel to sola-app)
  sola-assets/         # Vendored icon/asset bundles
  sola-browser/        # WebKit browser
  sola-make/           # Build/install orchestration (xtask)
  sola-monitor/        # System monitor / bus audit
  sola-river/          # River compositor bridge (bus ↔ wayland)
  sola-session/        # User-app session manager
  sola-settings/       # Settings panel
  sola-shell/          # Desktop shell — launcher, switcher, menubar, zoning
  sola-terminal/       # Terminal emulator (xterm.js + tmux)
apps/
  agent/               # AI agent frontend (not in workspace yet)
  mail/                # IMAP/SMTP mail client (not in workspace yet)
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

### Installing
- **NEVER run `cargo make install` (or any variant) without express user permission for that specific install.** This applies to subagents too — if you delegate work, your prompt MUST tell the subagent not to install. Permission for one install is not permission for the next; ask each time.
- Use `cargo make build` (or `cargo build`) to verify a change compiles. Stop there. Do not install just because a plan or task description says "install and smoke" — that step is for the user to run.
- Install is local: binaries go to `/opt/sola/bin/`.
- `cargo make install` — builds and copies all binaries to `/opt/sola/bin/`.
- `cargo make install <app>` — builds and installs a single app.
- `cargo make install <app> --watch` — watches for changes, rebuilds, and reinstalls automatically.
- The user launches `sola` manually from a physical TTY. Do not configure auto-start.

### Building
- Always use `cargo make build` — never raw `cargo build` or `cp`.
- This ensures our build system stays tested and current.
- Building is fine to do without permission. Installing is not — see the Installing rule above.

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
cargo make install                                # Build + install all to /opt/sola/bin
cargo make install <app>                          # Build + install a single app
cargo make install <app> --watch                  # Watch + reinstall on change
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
- Persistent log files at `/opt/sola/log/`. Always write logs there.
- Use `tracing` with structured fields — always include relevant context (device node, connector, crtc, etc.).
- Errors should explain *what went wrong* and *what was being attempted*. Don't swallow errors silently.

### Debugging Workflow
```bash
# Run sola from a TTY with debug logging, logs go to file AND terminal
RUST_LOG=debug /opt/sola/bin/sola 2>&1 | tee /opt/sola/log/sola.log

# Check recent logs
tail -100 /opt/sola/log/sola.log
```

### Log Levels
- `error` — something broke, action needed
- `warn` — unexpected but handled (e.g., GPU quirk worked around)
- `info` — lifecycle events (startup, device found, output connected, shutdown)
- `debug` — detailed flow (event loop ticks, input events, frame timing)
- `trace` — extremely verbose (every VBlank, every Wayland message)

## Runtime Environment

- Binaries install to `/opt/sola/bin/`
- Logs go to `/opt/sola/log/`
- User launches sola manually from a physical TTY — no display manager, no auto-login

## Web Frontends: Arrow.js

Sola apps render their UI with `@arrow-js/core` (vendored at `crates/sola-app/web/vendor/arrow/`). Arrow is small and has its own conventions — do NOT assume Lit, Svelte, or React idioms. Before writing or reviewing any `.ts` web file, check these rules.

### Template basics

```ts
import { html, reactive, watch } from '@arrow-js/core';

const state = reactive({ count: 0, name: '' });

html`<div>${() => state.count}</div>`(targetEl);   // mount into targetEl
```

`html\`...\`(target)` **appends** to `target`. To swap content, call the mounting function once per target; don't re-invoke `html\`...\`(sameTarget)`. Guard mounts with a boolean flag.

### Reactivity: closures vs. plain values

- **Closure `${() => expr}`** — reactive. Re-runs when any reactive state it reads changes.
- **Plain `${value}`** — static. Captured once at template creation.

Rule of thumb: if a value can change after mount, wrap it in `() => …`. If it's captured inside an outer reactive closure, the outer closure's re-run produces a new template and the "plain" interpolation is fine.

### Attributes

- **Regular attribute:** `class="${() => state.cls}"` — Arrow replaces the attribute value each time the closure changes.
- **Boolean attribute:** Arrow does NOT support Lit's `?attr="…"` syntax. Use `attr="${() => cond ? 'attr-name' : false}"`. Returning `false` removes the attribute entirely.
  - ✅ `disabled="${() => busy ? 'disabled' : false}"`
  - ❌ `?disabled="${() => busy}"` — throws `InvalidCharacterError: '?disabled'` at runtime and aborts the render.
- **Event handler:** `@event="${handler}"`, e.g. `@click="${() => onClick()}"`. Handler can be a plain function or an inline arrow.

### CSS imports

Sola serves assets directly from Rust-embedded bytes — there's no bundler. **`import './foo.css'` in a `.ts` file will fail** with `'text/css' is not a valid JavaScript MIME type` and kill the frontend. Declare component stylesheets with `<link rel="stylesheet" href="/src/components/foo.css">` in `index.html` (and register each CSS file in the `asset_bundle!` macro in the app's `main.rs`).

### List rendering

```ts
html`<ul>
  ${() => state.items.map(item => html`<li>${() => item.name}</li>`)}
</ul>`
```

Re-assign arrays (`state.items = [...state.items, newItem]`) to trigger re-render — mutating in place doesn't always notify.

### Nested templates MUST be in closures

**Critical:** A plain nested template expression — `${childTemplate}` or `${cond ? tplA : tplB}` — is mounted once and **never re-patched** when the parent chunk is reused on a reactive re-render. Only `${() => childTemplate}` (a function expression) installs the update observer that Arrow uses to re-render nested templates.

This is the #1 cause of "the first message shows, but clicking a second message still shows the first" stale-content bugs.

```ts
// ❌ Stale on parent re-render — no update observer installed
<div>${msg.cc ? html`<div>${msg.cc}</div>` : html``}</div>

// ✅ Re-patched every time the parent closure re-runs
<div>${() => msg.cc ? html`<div>${() => msg.cc}</div>` : html``}</div>
```

The same applies to nested text interpolations that need to follow reactive changes: write `${() => msg.from}`, not `${msg.from}`, when the enclosing template might be reused.

### Keys

Keys aren't needed to force re-patching in reactive single-template contexts — the closure rule above is what you want there. Use `.key(id)` when rendering **lists of templates**: Arrow's list-diffing path consults keys to match items across renders. Single-template reactive expressions go through the `patch` fast-path that reuses the prev chunk by raw-strings signature, so keys are a no-op there.

### watch()

```ts
watch(() => {
  // reads any reactive props; re-runs on their changes
  if (state.composing) mountCompose();
});
```

Arrow's reactive setter **emits on every write**, even if the new value equals the old. If a `watch` handler has side-effects (e.g. remounting DOM), guard against no-op transitions:

```ts
let prev = state.flag;
watch(() => {
  if (state.flag === prev) return;
  prev = state.flag;
  // …actual work…
});
```

### Reactive store wrapper

`createStore<T>(initial)` from `@sola/store` is a thin wrapper over `reactive()`. Use it for typed app state. Persistence helpers: `save()` and `persist()` in the same module.

### Common pitfalls recap

- ❌ `?bool-attr="${…}"` — use `attr="${() => truthy ? 'attr' : false}"`
- ❌ `import './x.css'` — use `<link>` in `index.html`
- ❌ `state.x = x` assuming equality check — Arrow always emits
- ❌ Mounting `html\`\`(target)` twice — it appends
- ❌ Nested `${childTemplate}` or `${cond ? tplA : tplB}` — wrap in `${() => ...}` so Arrow re-patches on parent re-render
