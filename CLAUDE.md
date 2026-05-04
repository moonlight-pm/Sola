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

## Web Frontends: lit + signals

Sola apps are built from custom elements. The lit stack (`lit-html` + `lit-element` + `@lit/reactive-element`, vendored under `crates/sola-kit/web/vendor/lit*/`) provides the element base; `@preact/signals-core` (vendored at `crates/sola-kit/web/vendor/signals/`) provides shared reactive state. Together ~15 KB.

**Component-first**: every reusable view is a custom element, not a function returning a template. Lib components are `<sola-X>`; storybook-internal widgets are `<kit-X>`. The shell is `<kit-app>`. Don't write helper functions that return templates when an element will do — element registration costs nothing and gives you proper composition (slots, events, encapsulation).

### KitElement

`KitElement` (from `@sola/kit`) is the base class:
- Extends `LitElement` (so you get update batching, lifecycle, render scheduling)
- Wrapped in `SignalWatcher` mixin (~10 lines inlined from `@lit-labs/preact-signals`) — signal reads inside `render()` auto-schedule re-renders
- Light DOM via `createRenderRoot() { return this; }` — global CSS variables drive the theme

```ts
import { html } from 'lit-html';
import { signal } from 'signals';
import { KitElement } from '@sola/kit';

const count = signal(0);

class MyCounter extends KitElement {
  render() {
    return html`
      <button @click=${() => count.value++}>Count: ${count.value}</button>
    `;
  }
}
customElements.define('my-counter', MyCounter);
```

Drop `<my-counter></my-counter>` in HTML; importing the module registers the tag.

### Properties (no decorators)

We use the static-properties API — no decorators (so swc strip-types works without transforms):

```ts
class SolaButton extends KitElement {
  static properties = {
    label: { type: String },
    variant: { type: String },
    disabled: { type: Boolean, reflect: true },
  };
  declare label: string;
  declare variant?: 'primary' | 'default' | 'ghost' | 'danger';
  declare disabled?: boolean;

  render() {
    return html`<button
      class=${`kit-btn kit-btn-${this.variant ?? 'default'}`}
      ?disabled=${this.disabled}
    >${this.label}</button>`;
  }
}
customElements.define('sola-button', SolaButton);
```

Use `declare` (not `let`/`const`/`=`) for property type declarations — avoids `useDefineForClassFields` clobbering Lit's getters/setters. Our `tsconfig.json` sets `useDefineForClassFields: false` defensively.

### Slots, not template props

Pass content via slots, not props that contain templates:

```ts
// ❌ Old style
form({ body: html`...`, actions: html`...` })

// ✅ Component style
html`<sola-form>
  <sola-field-row label="Email"><sola-field></sola-field></sola-field-row>
  <sola-button slot="actions" label="Save"></sola-button>
</sola-form>`
```

In the element, `<slot></slot>` for default content, `<slot name="actions"></slot>` for named slots.

### Events, not callback props

Components dispatch CustomEvents instead of accepting callback props:

```ts
// In the element
this.dispatchEvent(new CustomEvent('sola-input', {
  detail: { value: newValue },
  bubbles: true,
}));

// At the call site
<sola-field @sola-input=${(e: CustomEvent) => set(e.detail.value)}></sola-field>
```

For native events (`click`, `submit`, etc.) just let them bubble — `@click=${handler}` on the host catches them.

### Property bindings

Lit-html has four prefixes for interpolations on a custom element:

- `attr=${value}` — attribute (string-only)
- `?attr=${bool}` — boolean attribute (presence)
- `.prop=${value}` — DOM property (any type — use this for objects, arrays, numbers passed to a property declared in `static properties`)
- `@event=${handler}` — event listener

**For object/array props always use `.prop`**: `<sola-tab .index=${1}>` not `<sola-tab index="1">` (the latter would set the attribute as a string, and `type: Number` coercion only kicks in for attributes that exist on the HTML).

### Routing

Inside a `KitElement` `render()`, just dispatch on a signal:

```ts
class KitApp extends KitElement {
  render() {
    const sel = selected.value;
    return html`<main>${
      sel === 'home' ? html`<my-home></my-home>` :
      sel === 'about' ? html`<my-about></my-about>` :
      html`<div>Not found</div>`
    }</main>`;
  }
}
```

Lit-html atomically swaps the subtree — no swap-path bugs. Custom-element instances unmount cleanly via their disconnectedCallback.

### Conditionals and lists

Plain JavaScript expressions inside `${}` slots:

```ts
html`<div>
  ${state.error ? html`<p class="error">${state.error}</p>` : ''}
  ${state.items.map(item => html`<li>${item.name}</li>`)}
</div>`
```

Use `''` (empty string) rather than `null`/`undefined` for "render nothing" — slightly cleaner DOM.

For keyed lists where items reorder, import `repeat` from `lit-html/directives/repeat.js`. For our usual case (small lists, append/prepend), plain `.map()` is fine — lit-html does positional reuse.

### State

Use `signal()` for any value that changes over time and is read by templates. Mutate via `.value =` (immutable updates for object/array contents):

```ts
const items = signal<Item[]>([]);

// ✅ replace, signals see the change
items.value = [...items.value, newItem];

// ❌ mutate in place — signal sees no change, no re-render
items.value.push(newItem);
```

For derived state, `computed()` returns a read-only signal that recomputes when its dependencies change:

```ts
import { computed } from 'signals';
const count = signal(0);
const doubled = computed(() => count.value * 2);
```

### Side effects outside of render

Most reactive work happens automatically through `render()`. For side effects that aren't rendering — DOM listeners, timers, persistence — `effect(fn)` from `signals` returns a dispose function. Call it to stop. The effect's `fn` may also return a cleanup function that runs before the next call (and on dispose):

```ts
const stop = effect(() => {
  const id = setInterval(() => count.value++, 1000);
  return () => clearInterval(id);
});

// later: stop();
```

For DOM event listeners that should outlive the effect, attach in module scope; only attach inside an effect if you also clean up.

### CSS imports

Sola serves assets directly from Rust-embedded bytes — there's no bundler. **`import './foo.css'` in a `.ts` file will fail** with `'text/css' is not a valid JavaScript MIME type`. Declare component stylesheets with `<link rel="stylesheet" href="/src/components/foo.css">` in `index.html` (and register each CSS file in the `asset_bundle!` macro in the app's `main.rs`).

### Module imports

Sola's asset server has a fallback for `.js` → `.ts` lookups. Bare imports like `import { x } from './foo'` (no extension) **will 404**. Always write `import { x } from './foo.js'` — the strip-types layer turns the `.ts` source into JS at request time, and the import path needs to match.

### Reactive store wrapper

`@sola/store` re-exports `signal`, `computed`, `effect`, `batch`, `untracked` from `@preact/signals-core`, plus `persistedSignal(initial, key)` for localStorage-backed state.

### Common pitfalls

- ❌ Writing `function foo(opts) { return html\`...\` }` for a reusable view — make it a custom element.
- ❌ Initializing properties with `=` instead of `declare` — clobbers Lit's getter/setter setup unless `useDefineForClassFields: false`.
- ❌ `index="1"` on a custom element prop declared `type: Number` — use `.index=${1}` for non-string props.
- ❌ `value=${x}` on a controlled input — use `.value=${x}` so the property updates, not just the attribute.
- ❌ `?disabled="${bool}"` (quoted boolean) — use `?disabled=${bool}`.
- ❌ Mutating signal contents in place (`items.value.push(x)`) — replace the value (`items.value = [...items.value, x]`).
- ❌ `import './foo'` (no extension) — use `import './foo.js'`.
- ❌ `import './x.css'` — use `<link>` in `index.html`.
