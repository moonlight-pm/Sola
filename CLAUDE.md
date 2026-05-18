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

## Web Frontends — Two Stacks Coexisting

Two app frameworks live side-by-side in the workspace:

- **`sola-kit` (the future).** GTK-free CEF stack with off-screen rendering composited as Wayland surfaces via dma-buf. Apps use Remix v3 (`@remix-run/ui`) for component composition. This is where new app work happens. Documented below.
- **`sola-app` (legacy).** GTK4 + WebKit6 stack still hosting `sola-shell`, `sola-settings`, `sola-terminal`, and `sola-browser`. Retained until each is ported to the kit; do not write new apps against it.

The rest of this section is the kit; the legacy stack is self-contained and follows its own crate-local conventions.

## Web Frontends: Remix v3 (sola-kit)

Sola-kit apps render their UI with **Remix v3** (`@remix-run/ui`, vendored at `crates/sola-kit/web/vendor/remix-ui/`). JSX is **transformed server-side** by swc — there is no bundler, no Node, and no `tsc` in the loop at runtime.

### Build pipeline

Files end in `.tsx` (JSX) or `.ts` (no JSX). The asset server (`crates/sola-kit/src/strip.rs::transform`) handles the request:

- **`.tsx`** → swc parses TSX → resolver → JSX transform (automatic runtime, `import_source: "@remix-run/ui"`) → TS strip → JS.
- **`.ts`** → swc parses TS → resolver → TS strip → JS. JSX transform is skipped.
- **`.jsx`** → resolver → JSX transform only, no type strip.

The **automatic runtime** auto-injects `import { jsx, jsxs, Fragment } from "@remix-run/ui/jsx-runtime"` for any file containing JSX, so app code never imports a JSX factory by hand. The editor mirror is `tsconfig.json` with `"jsx": "react-jsx"` and `"jsxImportSource": "@remix-run/ui"` — both must agree.

### What the kit provides for free

Apps don't ship `index.html`, an importmap, theme bootstrap, or component CSS `<link>`s. The kit auto-injects all of that:

- **`index.html` + `index.tsx`** live in `crates/sola-kit/web/lib/` and are served via `platform_assets()`. `ctx::add_window` falls back to platform assets when an app has no own `index.html`.
- **Importmap** built per-app by `lib.rs::build_importmap` and injected into `<head>` before any module script. Publishes `@sola/ipc`, `@sola/kit`, `@sola/sidebar`, `@sola/app-root`, `@remix-run/ui`, `@remix-run/ui/jsx-runtime`. `@sola/app-root` is mapped per-app from `SolaApp::ROOT_COMPONENT` (default `/main.tsx`).
- **Component stylesheets** auto-discovered: `inject_kit_head` walks `platform_assets()` for every `Css` asset and emits a `<link rel="stylesheet">` for each. Adding a new kit component with a sibling `.css` file makes it appear in every kit app automatically.
- **`__solaRecv` queueing bootstrap** installed before any module script runs so Rust→JS pushes during early init don't drop on the floor.

The minimum an app needs is a `Cargo.toml`, a `SolaApp` impl, and a `web/main.tsx` that exports a Remix v3 component named `Main`. Override `ROOT_COMPONENT` only if the root file lives elsewhere.

### Component model (Remix v3)

Components are functions that take a `Handle<Props>` and return a `RenderFn`:

```tsx
import { type Handle, type RemixNode } from "@remix-run/ui";
import { on } from "@sola/kit";

interface CounterProps {
  label: string;
  children?: RemixNode;
}

export function Counter(handle: Handle<CounterProps>) {
  let count = 0;

  const onClick = () => {
    count++;
    handle.update();
  };

  return () => (
    <button class="kit-btn" mix={[on("click", onClick)]}>
      {handle.props.label}: {count} {handle.props.children}
    </button>
  );
}
```

Key points:

- **State is closure-captured.** Mutate locals; call `handle.update()` to schedule a re-render. The function body runs once per mount; the returned `RenderFn` runs every render.
- **`handle.props` is stable identity, fresh values.** Object reference doesn't change; field values are updated in place before each render.
- **Events attach via `mix={[on("click", handler)]}`.** Lowercase JSX attrs like `onclick=` are *not* typed on Remix v3's host elements — see "Pitfalls" below.
- **Children compose naturally** — `props.children` is `RemixNode` (renderable + arrays of renderable). Named slots use named props (`leading`, `trailing`, etc.), not HTML `<slot>`.
- **Conditionals and lists are plain JS** — `cond ? html`...` : null` and `items.map(item => <Item key={item.id} ... />)`.

### `@sola/kit` umbrella

`web/lib/kit.ts`, served at `/lib/kit.ts`, importmap entry `@sola/kit`. Two exports:

- **`setupKit()`** — call once from the kit's built-in `index.tsx` (apps almost never need to call it). Installs the constructable themeSheet that the bus-pump pushes CSS into.
- **`on()`** — typed wrapper around Remix v3's `on()` that pre-fixes `target = HTMLElement` and gives full event-type inference for handler params (`on("keydown", e => e.key)` infers `e: KeyboardEvent`). Always import from `@sola/kit`, not `@remix-run/ui`. The raw Remix `on` defaults its target to `Element`, whose `ElementEventMap` is missing keyboard events; inside Remix's own components that's not a problem because they're inside `createMixin<HTMLElement>` wrappers, but our direct-JSX `mix={[…]}` usage doesn't get that context.

### Theme protocol

Two layers, both in `sola-core::theme::Theme`, both broadcast as the persistent `Topic::Theme`:

1. **Palette** — flat `BTreeMap<TokenName, Token>`. Each token has a `kind` (Color / FontFamily / TextSize / Space / Radius), a value, and the *selection groups* it's eligible for (e.g. `["surface"]`, `["accent", "border"]`).
2. **Component bindings** — `BTreeMap<String, ComponentBindings>` keyed by component name. Each component declares slots; each slot points at a token and constrains itself to a selection group.

`Theme::to_css(&self)` lowers to a single `:root { … }` block in two sections — every palette token first (`--bg-secondary: #161b22;`), then every binding as a scoped var pointing at an atom (`--sola-sidebar-bg: var(--bg-secondary);`). **Component CSS only ever references the scoped vars** (`var(--sola-<component>-<slot>)`), never atoms directly. A binding swap is a one-line `:root` edit with no component-CSS change.

`Theme::validate()` enforces the four invariants (binding's token exists, token's groups contain binding's group, group→kind consistent, all referenced components present in bindings).

The kit's `BusPumpTask::execute` intercepts `Topic::Theme` deliveries: lowers via `to_css()` once, pushes to every kit-managed window via `__solaRecv` `{ event: "theme", css: … }`. The renderer-side `setupKit()` listener does `themeSheet.replaceSync(msg.css)` — single allocation, hot-reloadable on every theme update including the sticky replay at first connect.

Spec: `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md`.

### Multi-window apps

A single `SolaApp` can host multiple windows with different root
components and per-window seed state. On each `ctx.add_window(cfg)`:

- `cfg.root_component: Option<&'static str>` overrides
  `SolaApp::ROOT_COMPONENT` for that window's importmap entry of
  `@sola/app-root`. Lets one app mount different `Main` components
  per window (e.g. sola-shell's menubar/launcher/menu/switcher).
- `cfg.initial_state: Option<serde_json::Value>` is serialized into
  `<script>window.__solaInitial = <json>;</script>` and injected into
  the head of that window's `index.html`. The kit's `index.tsx` reads
  it synchronously and passes it to `Main` via the `initial` prop.
  `None` becomes `null`.

`Main`'s signature must accept the prop:
`function Main(handle: Handle<{ initial: T | null }>)`.

### CSS authoring

Kit components ship a `web/lib/components/<name>.css` next to their `.tsx`. Class-based selectors (not tag-based — Remix components render plain DOM). Reference only `var(--sola-<component>-<slot>)` slots; inherited typography (color, font-family, font-size) cascades from the surrounding `<Root>` via normal CSS inheritance — don't re-reference `--sola-root-*` from inside other components.

`import './foo.css'` in a `.ts(x)` file fails (`'text/css' is not a valid JavaScript MIME type`). The kit's auto-injection handles it — just add the CSS file to `platform_assets()` and a `<link>` appears in every app's `<head>`.

### Module imports

Both `import './foo'` and `import './foo.js'` work. The asset server (`AssetBundle::find` in `crates/sola-kit/src/assets.rs`) tries the literal path first, then `.js → .ts/.tsx/.jsx`, then — for extensionless paths — `.ts/.tsx/.jsx/.js/.mjs` in that order.

Bare specifiers (`import x from 'foo'`) need importmap entries. The kit's importmap (auto-injected) covers `@sola/*` and `@remix-run/ui*`. Apps with their own bare specifiers need their own kit-side extension (not yet built; pull when a real consumer needs it).

`tsconfig.json` carries:
- `"jsx": "react-jsx"` and `"jsxImportSource": "@remix-run/ui"` — must mirror swc's runtime config.
- `"allowImportingTsExtensions": true` — required because the vendored `@remix-run/ui` source uses explicit `.ts`/`.tsx` extensions in its own imports.
- `"paths"` mirroring the runtime importmap so the LSP resolves the same specifiers.

### Sidebar (the first kit component)

`web/lib/components/sidebar.tsx` exports three component factories — `Sidebar`, `SidebarSection`, `SidebarItem`. Slot-based composition, parent-controlled active state, `onSelect` callback prop, `leading` / `trailing` named-prop slots. CSS lives in the sibling `sidebar.css` and references only `--sola-sidebar-*` scoped vars. See the spec for the slot inventory and selection groups.

### Common pitfalls

- ❌ `import { on } from "@remix-run/ui"` — use `import { on } from "@sola/kit"` so HTMLElement event-map inference works without explicit type parameters.
- ❌ `<div onclick={fn}>` or `<div onClick={fn}>` — host elements have no event attribute typings in Remix v3. Use `mix={[on("click", fn)]}`.
- ❌ Mounting your own importmap or `<link>` for kit components in a kit app's `index.html` — the kit auto-injects both. Only one importmap is allowed per page.
- ❌ Re-importing or shipping `@remix-run/ui` in a kit app — the kit serves the vendored source via `platform_assets()`.
- ❌ Returning JSX directly from the component function (`function C() { return <div/> }`) — Remix factories return a `RenderFn` (`function C(handle) { return () => <div/> }`).
- ❌ Mutating closure state without `handle.update()` — the renderer doesn't auto-track. Call `handle.update()` to schedule the next render.
- ❌ Object literals at expression position (`async () => {a: 1}`) — they parse as block statements. Use `async () => ({a: 1})`. Only relevant inside `solactl eval` and similar wrappers.
- ❌ `import './foo.css'` — use a `<link>` (or, for kit-shipped CSS, just register in `platform_assets()` and the kit auto-injects).

## CEF binding choice

**Chosen crate:** `cef` `147.1.0+147.0.10` (tauri-apps/cef-rs, Apache-2.0 OR MIT)
- crates.io: <https://crates.io/crates/cef>
- docs.rs: <https://docs.rs/cef/147.1.0+147.0.10/cef/>
- GitHub: <https://github.com/tauri-apps/cef-rs>

This is the **only** active Rust CEF binding that tracks current CEF releases. `cef-rs` as a separate crate does not exist on crates.io. The crate is maintained by the Tauri team, ships pre-generated bindgen bindings (no headers needed — the tarball is enough), and has a working Linux `osr` example that exercises `on_accelerated_paint` with dma-buf import via Vulkan. Version `147.1.0+147.0.10` exactly matches our pinned CEF `147.0.10`.

**Do NOT enable the `accelerated_osr` feature.** That feature only gates the crate's wgpu/Vulkan-based dma-buf importer helper module — it pulls in `ash`, `wgpu`, `metal`, `objc`, etc. We don't need any of that. We import dma-bufs through `zwp_linux_dmabuf_v1::create_params` (sctk-managed `wl_buffer`) and let sola-river composite. The `AcceleratedPaintInfo` and `AcceleratedPaintNativePixmapPlaneInfo` structs live in the base bindgen output (`cef::sys::*`) and are available without features.

```toml
cef = "147.1.0+147.0.10"
```

### Binding name deltas vs. the design spec

The design spec uses generic CEF C-API names. The actual `cef` crate names differ:

| Spec / pseudocode | Actual `cef` crate |
|---|---|
| `CefSettings` | `Settings` |
| `CefMainArgs` | `MainArgs` (built via `cef::args::Args::new()`) |
| `CefBrowserSettings` | `BrowserSettings` |
| `CefWindowInfo` | `WindowInfo` |
| `CefBrowserHost::create_browser_sync(...)` | free fn `cef::browser_host_create_browser_sync(window_info, client, url, settings, extra_info, request_context)` |
| `frame.execute_javascript(...)` | `frame.execute_java_script(...)` (yes, with underscore) |
| `RenderHandler::get_view_rect(...) -> Rect` | `ImplRenderHandler::view_rect(&self, browser, rect: Option<&mut Rect>)` (out-param, no return) |
| `ResourceHandler::get_response_headers(...)` | `response_headers(...)` (no `get_` prefix) |
| `cef::post_task(ThreadId::UI, closure)` | `cef::post_task(thread_id, task: Option<&mut Task>)` — boxed `Task`, not closure (helper macro: `wrap_task!`) |
| `cef::CefClientBuilder::new().with_*()` | no builder — use `wrap_client!` macro with handler fields |
| `EventFlags::EVENTFLAG_*` | constants on `cef::sys::cef_event_flags_t` |
| `KeyEventType::{KeyDown, KeyUp, Char}` | `KeyEventType::{KEYDOWN, KEYUP, CHAR, RAWKEYDOWN}` (uppercase) |
| `MouseButtonType::{Left, Right, Middle}` | `MouseButtonType::{LEFT, RIGHT, MIDDLE}` |
| C `int` booleans on event structs | `c_int` in Rust — cast `true as _` or `1 / 0` |
| `CefString` is `Option<String>` in pseudocode | actual is `cef::CefString` — built from `&str` via its `From` impl |
| `execute_process` returning `< 0` for main | actually returns `-1` for main, `>= 0` for subprocess (matches plan's branching) |

`on_accelerated_paint` confirmed present in `ImplRenderHandler` with full dma-buf info (per-plane fd, stride, offset, size; struct-level DRM modifier; `cef::sys::cef_color_type_t` format = RGBA_8888 or BGRA_8888). No bindgen build step — the crate ships pre-generated bindings per target under `src/bindings/`.
