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

## Web Frontends: Preact + signals + JSX

Sola-kit apps render their UI with **Preact** (`preact`, vendored at `crates/sola-kit/web/vendor/preact/`) and reactivity via `@preact/signals` (which is the Preact integration that wraps `@preact/signals-core`). JSX is **transformed server-side** by swc — there is no bundler, no Node, and no `tsc` in the loop.

### Build pipeline

Files end in `.tsx` (JSX) or `.ts` (no JSX). The asset server (`crates/sola-kit/src/strip.rs::transform`) handles the request:

- **`.tsx`** → swc parses TSX → JSX transform (classic runtime, `pragma="h"`, `pragmaFrag="Fragment"`) → TS strip → JS.
- **`.ts`** → swc parses TS → TS strip → JS. JSX transform is skipped.
- **`.jsx`** → JSX transform only, no type strip.

This means **every `.tsx` file must `import { h, Fragment } from 'preact'`** even if `Fragment` isn't used directly — JSX desugars to `h(...)` calls and `<>...</>` to `h(Fragment, null, ...)`.

### A component

```tsx
import { h, Fragment } from 'preact';
import { signal, computed, effect } from '@preact/signals';

// Module-scope signals survive unmount and are shared across components.
const count = signal(0);
const doubled = computed(() => count.value * 2);

effect(() => {
  document.title = `count: ${count.value}`;
});

export function Counter({ label }: { label: string }) {
  return (
    <button class="kit-btn" onClick={() => count.value++}>
      {label}: {count} (×2 = {doubled})
    </button>
  );
}
```

Two important Preact-vs-React differences:

- **`class`, not `className`.** Preact accepts both, prefer `class`.
- **Inline events are `onClick`/`onInput`/etc.** (camelCase), but Preact also accepts lowercase `onclick`.

### Signals: read with `.value`, except in JSX

```tsx
count.value++;          // write
const x = count.value;  // read in JS

<span>{count}</span>    // read in JSX — auto-unwrapped, auto-subscribed
<span>{count.value}</span>  // also works, also subscribes
```

The auto-unwrap and auto-tracking is what `@preact/signals` (the Preact integration package) adds on top of `@preact/signals-core`. Without the integration package, you'd write `.value` everywhere and components wouldn't re-render automatically.

**Mutate by replacing**, never in place:

```ts
items.value = [...items.value, newItem];  // ✅
items.value.push(newItem);                // ❌ no notification
```

### Slots = children. Events = props. No callback noise.

```tsx
function Card({ title, children }: { title: string; children: any }) {
  return <section><h2>{title}</h2><div>{children}</div></section>;
}

<Card title="Counters">
  <Counter label="A" />
  <Counter label="B" />
</Card>
```

### Lists

```tsx
{items.value.map(it => <li key={it.id}>{it.name}</li>)}
```

A stable `key` is required when items can reorder/insert/remove.

### CSS imports

Sola serves assets directly from Rust-embedded bytes — there's no bundler. **`import './foo.css'` in a `.tsx` file will fail** with `'text/css' is not a valid JavaScript MIME type` and kill the frontend. Declare component stylesheets with `<link rel="stylesheet" href="/src/components/foo.css">` in `index.html` (and register each CSS file in the `asset_bundle!` macro in the app's `main.rs`).

### Module imports

Both `import './foo'` and `import './foo.js'` work. The asset server (`AssetBundle::find` in `crates/sola-kit/src/assets.rs`) tries the literal path first, then `.js → .ts/.tsx/.jsx`, then — for extensionless paths — `.ts/.tsx/.jsx/.js/.mjs` in that order. The editor side accepts extensionless imports because tsconfig sets `"moduleResolution": "bundler"`.

Bare specifiers (`import 'foo'`) still need import-map entries — that's a browser rule, not ours. The current import map lives inline in `crates/sola-kit/src/lib.rs` and publishes:

- `preact`, `preact/hooks`
- `@preact/signals-core`, `@preact/signals`
- `@sola/ipc`, `@sola/store`, `@sola/kit`
- `~/` (prefix mapping to `/lib/`, so `import { x } from '~/components/foo'` reaches the public lib surface)

### Common pitfalls

- ❌ Forgetting `import { h, Fragment } from 'preact'` at the top of a `.tsx` file — JSX desugars to `h(...)` and you'll get a `ReferenceError`.
- ❌ Mutating `items.value.push(...)` — replace the value (`items.value = [...items.value, x]`).
- ❌ `import './x.css'` — use `<link>` in `index.html`.
- ❌ Using `@preact/signals-core` directly in components and expecting auto-tracking — that's what the `@preact/signals` integration package adds. Use `@preact/signals` in components.
- ❌ Treating Preact like React for non-trivial libraries — preact is API-compatible at the surface but `react`-named packages won't work; use `preact`-named ones (e.g. `@preact/signals`, not `@preact/signals/react`).
