# Sola

Sola is a Wayland desktop shell — a full compositor and desktop environment
built in Rust. UI is pure Rust via **Iced** (`sola-kit`); River is the
Wayland compositor, bridged by `sola-river`.

## Active work

Queued work lives in [`.grok/rules/active-work.md`](.grok/rules/active-work.md)
(auto-loaded every session). If the user signals go-ahead without a new task
("go", "ok go", "continue", etc.), execute that file's **Current** work — do not
re-plan from scratch. Keep `active-work.md` updated as phases complete.

## Architecture

- **Process manager (`sola`):** Launches and supervises all components. No desktop or bus logic — pure process management.
- **Bus (`sola-bus`):** General-purpose IPC bus. Separate process. All Sola components communicate via bus events over a Unix socket.
- **Compositor:** River (external), bridged by `sola-river` (bus client).
- **Shell / apps:** Iced programs via `sola-kit` — Wayland clients + bus clients. Each is a separate process.
- **Browser:** Custom iced chrome with WPE (primary) or CEF (parallel) engines.
- **IPC:** Sola Bus (events over Unix socket) + Wayland protocols for surfaces/input
- **Build system:** `cargo make` (xtask pattern via `sola-make` crate)

All components are independently restartable. Sola apps are resilient to bus and compositor restarts.

## Workspace Structure

```
crates/
  sola/                # Process manager (binary entry point)
  sola-bus/            # IPC bus host + client library
  sola-core/           # Shared primitives (env, process, watcher, config, log, ...)
  sola-kit/            # Iced app kit + storybook binary
  sola-assets/         # Vendored icon/asset bundles
  sola-browser/        # Thin engine dispatcher (exec WPE or CEF)
  sola-browser-core/   # Shared iced browser chrome
  sola-browser-wpe/    # Primary browser engine
  sola-browser-cef/    # Parallel CEF engine
  sola-make/           # Build/install orchestration (xtask)
  sola-monitor/        # System monitor / bus audit
  sola-river/          # River compositor bridge (bus ↔ wayland)
  sola-session/        # User-app session manager
  sola-settings/       # Settings panel (incl. mail config)
  sola-shell/          # Desktop shell — launcher, switcher, menubar, zoning
  sola-terminal/       # Terminal emulator (alacritty grid + iced)
  sola-agent/          # Coding agent (iced + Fugu)
apocrypha/             # Reference-only: legacy WebView stack (not built)
  sola-app/            # Frozen GTK4 + WebKit6 host
  apps/agent/          # Retired WebView agent prototype
  apps/mail/           # Reference for future crates/sola-mail
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
- **After merging a worktree branch to master, always clean up:** remove the
  worktree (`git worktree remove .worktrees/<name>`) and delete the local
  branch (`git branch -d <branch>`). Do this in the same turn as the merge
  unless the user asks to keep it — no leftover feature worktrees/branches.

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

## UI Stack — Iced

Sola's app UI is built with **[Iced](https://iced.rs) 0.14** — pure Rust, no
web engine. Apps are `iced` programs (wgpu renderer, wayland feature, svg) that
run as wayland clients of `sola-river` and talk to the rest of the system over
the bus. There is no HTML/JS/CSS, no bundler, no WebView in the active stack.

The old GTK4 + WebKit6 host (`sola-app`) and its apps live under
`apocrypha/` for reference only — not workspace members, not installed.
Do **not** write new apps against that stack. The only remaining product
gap from that era is a kit-native mail client (`crates/sola-mail`); use
`apocrypha/apps/mail` as the logic/UI reference. CEF binding notes at the
end of this file apply to `sola-browser-cef`, not to the Iced kit.

## sola-kit (the Iced app kit)

`sola-kit` is the shared Iced app kit + a `sola-kit` **storybook** binary that
dogfoods every component (each widget has a showcase page, so regressions show
up there first). Library surface in `src/lib.rs`: `App`, `BusSetup`, `run`,
`default_theme`, plus re-exported `iced` and `sola_bus` (so consumers don't take
their own direct deps just to spell trait bounds or reference bus types).

### App scaffolding (`src/app.rs`)

- **`App` trait** — currently just `const APP_ID: &'static str`. One source of
  truth for the wayland `xdg_toplevel.app_id` and the bus app id.
- **`startup(app_id)`** — the standard boot flow every kit app runs *before*
  handing the thread to iced: log init → `activate_wayland_session` →
  `wait_for_wayland_socket` (cold-boot race guard) → `activate_gpu_env` (points
  NixOS GPU dispatch env at `/run/opengl-driver/` so wgpu/EGL/Vulkan init from a
  bare TTY) → `watch_own_binary` (re-exec in place when `/opt/sola/bin/<name>`
  changes, so `cargo make install` picks up new code live; skipped when
  `SOLA_NO_SELF_WATCH=1`).
- **`BusSetup`** — builder for the connect + subscribe + publish-app-menu dance.
  `BusSetup::new(id).subscribe(TopicKind::ALL).app_menu("Foo", [(...)]).install()`
  hands the connected client to the kit's global slot.
- **`bus_subscription()`** → `Subscription<Arc<Message>>` — apps `.map(...)` it
  into their own message enum. Internally a 8ms polling thread forwards the bus
  client's sync `recv` into an unbounded channel feeding `stream::channel`. Use
  this **or** a manual `bus().lock().try_recv()` loop, never both (one receiver
  per process).
- **`run::<A>()`** — still just a placeholder over `startup`. There is no generic
  iced wrapper yet: each app builds its own `iced::application`/`iced::daemon`
  because update/view/subscription types differ. Promote shared logic here only
  once a second app justifies it.

### Theme protocol (`src/theme.rs`)

The kit bridges three representations of the palette:

1. **Compile-time defaults** — the `hex::*` constants.
2. **`Atoms`** — 10 editable `iced::Color`s: `bg`, `bg_raised`, `bg_hover`,
   `border`, `fg`, `fg_muted`, `accent`, `success`, `warning`, `danger`. The
   editable bridge the storybook mutates.
3. **Bus theme** — `sola_core::theme::Theme`, broadcast as the persistent
   `Topic::Theme` and shared with every other sola process.

Conversions: `Atoms` → `iced::Theme` via `iced_theme_from_atoms`; `Atoms` →
bus via `bus_theme_from_atoms`. **`from_bus_theme(&BusTheme) -> iced::Theme`**
is the consumer path — it maps named palette tokens (`bg-primary`,
`bg-secondary`, `bg-tertiary`, `border`, `text-primary`, `text-tertiary`,
`accent`, `success`, `warning`, `danger`) onto an iced `Extended` palette,
falling back to compile-time defaults for any missing/malformed atom, **and as
a side effect installs the font role table** so font tokens land alongside
colours. Wire it into `update` on `Topic::Theme`, then return `self.theme.clone()`
from `App::theme`. `to_bus_theme()` is canned-default-only — the iced→bus
direction is lossy by design (iced derives many colours from a few atoms), so a
real theme editor should emit the bus value directly, not reverse it out of an
iced `Theme`.

Spec: `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md`.

**Shell chrome** rides the same palette as `shell-*` tokens (4 alpha-capable
colors + 4 spacing values, group `"shell"`). `ShellStyle` is the typed view:
`shell_style_from_bus_theme` extracts (per-token fallback to compile-time
defaults), `bus_theme_with_shell` writes back. The shell refreshes it on every
`Topic::Theme`; the storybook's Shell page is the editor. Colors round-trip as
`#rrggbbaa` when translucent. Spec:
`docs/specs/2026-06-06-shell-customization-design.md`.

### Fonts (`src/fonts.rs`)

- **Font files** ship under `/opt/sola/share/fonts/` (synced by
  `cargo make assets sync`). `load_all()` reads `FONT_FILES` off disk; missing
  files warn but don't fail. Pass the bytes to the iced builder's `.font(...)`.
  SF Pro and Iosevka are placed manually by the user (license), not synced.
- **Semantic roles, not families.** Components never name a family directly —
  they call role accessors: `fonts::ui()`, `ui_medium()`, `display()`,
  `chrome()`, `mono()`. The `Fonts` table (those 5 roles) lives in a process-wide
  `RwLock`; `fonts::install(Fonts)` hot-swaps it, and the bus theme path calls
  `install` on every `Topic::Theme` so a font edit propagates on the next render.
  Default is SF Pro for everything UI-shaped + JetBrains Mono for code.
- **`INSTALLED_FAMILIES`** is the font-picker vocabulary (the strings a settings
  UI offers and that `FontFamily` tokens carry on the bus). `fonts_from_families`
  builds a `Fonts` table from per-role family-name selections; `FontSelection`
  (in `theme.rs`) is the per-role wire form.

### Components (`src/components/`)

`badge`, `button`, `card`, `divider`, `field`, `icon`, `popover`, `sidebar`,
`split`, `swatch`, `text`, `text_input`, `toolbar`. Each is a reusable iced
widget/style with a matching storybook page under `src/storybook/pages/`. Grow
this surface only as real apps need shared pieces — no speculative widgets.

## sola-shell (the Iced desktop shell)

A single **`iced::daemon`** multi-window application. The daemon opens no
default window; a boot task opens the menubar, and the other three windows
(`WindowKind`: `Menubar`, `Menu`, `Launcher`, `Switcher`) open on demand. The
`Shell` struct holds the per-window `iced::window::Id`s, focused app/window,
MRU apps + per-app MRU windows, known windows, the application list, parsed app
menus, output size, menu open-state, switcher/launcher sub-state, and zoning
state. `Msg` covers bus events, window lifecycle, menu open/hover/close/action,
launcher (query/nav/launch), switcher (nav/hover/confirm/cancel + a focus-hover
timer), zoning, clock tick, and toasts. It path-depends on `sola-kit`.

Spec: `docs/specs/2026-05-22-sola-shell-iced-port-design.md`.

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
