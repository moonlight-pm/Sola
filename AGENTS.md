# Sola

Sola is a Wayland desktop shell — a full compositor and desktop environment
built in Rust. UI is pure Rust via **Iced** (`sola-kit`); River is the
Wayland compositor, bridged by `sola-river`.

## Session start

1. This file.
2. [`CURRENT.md`](CURRENT.md) — living priority and dogfood/runtime state.
3. [`docs/capabilities.md`](docs/capabilities.md) — as-built maturity for the
   slice you will touch. GPU / iced idle: also [`PERFORMANCE.md`](PERFORMANCE.md).
4. [`docs/open-questions.md`](docs/open-questions.md) — any **Decision points**
   for the slice? If yes, **ask the human**; do not invent product policy.
5. Only the freeze or plan needed for the active domain.

If the user signals go-ahead without a new task ("go", "ok go", "continue",
etc.), execute **CURRENT.md → Now** — do not re-plan from scratch.

Update `CURRENT.md` and `docs/capabilities.md` when direction, capability
status, or known runtime state changes. **No** one-off handoff files.
[`.grok/rules/active-work.md`](.grok/rules/active-work.md) is only a
pointer to `CURRENT.md` (auto-load reminder).

**Skills (Grok):** `.grok/skills/` —

- `sola-session-start` — boot order above  
- `sola-progress-docs` — **mandatory** end-of-slice doc updates  
- `sola-workspaces-cli` — `solactl workspaces` control plane (review/work
  ticket, create worktree, tell a pane, merge/cleanup — fan-out stays off
  the rail)  

## Progress documentation is first-class (mandatory)

Describing the system and its progress is **paramount**. Incomplete meta work
means incomplete product work. Full model:
[`docs/progress-model.md`](docs/progress-model.md). Portable practice:
[`docs/progress-documentation-practice.md`](docs/progress-documentation-practice.md).

| Kind | Home | Role |
|------|------|------|
| Focus | Root `CURRENT.md` | Priority, next moves, dogfood facts, locks |
| GPU / idle | Root `PERFORMANCE.md` | Present/scanout track (not a second CURRENT) |
| As-built progress | `docs/capabilities.md` | Capability status + gaps |
| As-built map | `docs/architecture.md` | Processes, crates, paths, IPC |
| Target design | `docs/specs/*` | Freezes (desired shape) |
| Horizon | `docs/roadmap.md` | Phase-level program status |
| Product docs | `docs/manual/` | **Shipped** operator truth only |

**End of every real product slice (same change as code):**

1. Update capability row(s) (status and/or gaps).  
2. Update `CURRENT.md` if priority or dogfood changed. GPU/idle slices
   also update root [`PERFORMANCE.md`](PERFORMANCE.md).  
3. Update `docs/manual/` if operator-visible **shipped** behavior changed.  
4. Update `architecture.md` if the system map changed.  
5. Flip `roadmap.md` phase status only when phase-level status changes.  
6. Follow [`.grok/skills/sola-progress-docs/SKILL.md`](.grok/skills/sola-progress-docs/SKILL.md).

Do not invent `STATUS.md` / `HANDOFF.md` / session diaries. Deferred
*improvements* to this meta system are listed only in
[`docs/progress-model.md`](docs/progress-model.md#deferred-meta-work).

**Code wins** over stale docs — then fix the docs immediately.

## Architecture (summary)

Living map: [`docs/architecture.md`](docs/architecture.md).

- **Process manager (`sola`):** Launches and supervises all components. No desktop or bus logic — pure process management.
- **Bus (`sola-bus`):** General-purpose IPC bus. Separate process. Fan-out facts over a Unix socket.
- **Call (`sola-call`):** Request/reply host. Live method registry. Not the bus.
- **Compositor:** River (external), bridged by `sola-river` (bus client).
- **Shell / apps:** Iced programs via `sola-kit` — Wayland clients + bus clients. Each is a separate process.
- **Browser:** Custom iced chrome + CEF engine in a single `sola-browser` crate.
- **IPC:** Sola Bus (events over Unix socket) + **sola-call** (request/reply) + Wayland protocols for surfaces/input
- **Build system:** `cargo make` (xtask pattern via `sola-make` crate)

All components are independently restartable. Sola apps are resilient to bus and compositor restarts.

## Workspace Structure

```
crates/
  sola/                # Process manager (binary entry point)
  sola-bus/            # IPC bus host + client library
  sola-call/           # Call host + client (request/reply; not the bus)
  sola-core/           # Shared primitives (env, process, watcher, config, log, ...)
  sola-kit/            # Iced app kit + storybook binary
  sola-assets/         # Vendored icon/asset bundles
  sola-browser/        # Iced browser chrome + CEF engine (single crate)
  sola-make/           # Build/install orchestration (xtask)
  sola-monitor/        # System monitor / bus audit
  sola-river/          # River compositor bridge (bus ↔ wayland)
  sola-session/        # User-app session manager
  sola-settings/       # Settings panel (incl. mail config)
  sola-shell/          # Desktop shell — launcher, switcher, menubar, zoning
  sola-terminal/       # Untitled-shell terminal (alacritty grid + iced); also a lib
  sola-workspaces/     # Project / workspace rail + agent-aware PTYs (this branch)
  sola-agent/          # Coding agent (iced + ACP / Grok leader) — not Workspaces
  sola-mail/           # Kit-native mail client
  sola-kvm/            # KVM / input bridge
  sola-preview/        # Simple image viewer (argv)
  sola-paint/          # Default image viewer/editor (MIME; screenshots stay on preview)
  sola-wrapper/        # Website wrappers as first-class apps (CEF via sola-browser lib)
  sola-install/        # Kit installer wizard (image media)
  solactl/             # CLI helpers
nix/                   # NixOS module (Shape 1) + image/ISO profiles
apocrypha/             # Reference-only: legacy WebView stack (not built)
CURRENT.md             # Living session focus (only handoff)
INSTALL.md             # Shape 1 colleague install (NixOS module + tarball)
docs/
  README.md            # Docs map + session boot
  progress-model.md    # How progress docs work
  capabilities.md      # As-built capability matrix
  architecture.md      # As-built system map
  roadmap.md           # Program horizon
  open-questions.md    # Design forks + ask-human decisions
  manual/              # Operator product docs (shipped only)
  specs/               # Target freezes (dated)
  plans/               # Implementation checklists
  ideas/               # Parked thoughts
  vault/               # Historical Obsidian notes (not authoritative)
```

## Development Rules

### Worktrees
- Always use `.worktrees/` for git worktrees.
- Only make code modifications in worktrees. Never commit code changes directly to master.
- Only merge worktree branches to master with explicit user permission.
- **Approval = merge in the same turn, not cleanup.** When the user signals
  the work is good — e.g. "nailed it", "looks good", "merge that", "ship it",
  "LGTM", "perfect" — commit any uncommitted work if needed and merge the
  branch to master. Do **not** wait for a second explicit "please merge"
  (unless they say hold off). Leave the git worktree **and** the Workspaces
  tab unless they asked to remove them.
- **Do not remove a worktree or workspace tab unless they asked.** There is
  no default cleanup after merge. If a git worktree is removed, close the
  tab unless they said keep it. Phrases and sequence:
  [`.grok/skills/sola-workspaces-cli/SKILL.md`](.grok/skills/sola-workspaces-cli/SKILL.md)
  (Merge / cleanup).

### Installing
- **NEVER run `cargo make install` (or any variant) without express user permission for that specific install.** This applies to subagents too — if you delegate work, your prompt MUST tell the subagent not to install. Permission for one install is not permission for the next; ask each time.
- Use `cargo make build` (or `cargo build`) to verify a change compiles. Stop there. Do not install just because a plan or task description says "install and smoke" — that step is for the user to run.
- Install is local: binaries go to `/opt/sola/bin/`.
- `cargo make install` — builds and copies all binaries to `/opt/sola/bin/`.
- `cargo make install <app>…` — builds and installs one or more apps (e.g. `shell kit`).
- Multi-target installs **replace binaries in restart order** (`sola-bus` →
  `sola-call` → `sola-river` → `sola-shell` → `sola-session` → `sola-kvm` → `sola`, then
  other apps) with a **1s settle gap** between actual replaces so the
  process manager can restart each component before the next kill.
  CLI order is ignored for that sequence (`install shell river` still
  copies river first).
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
cargo make install <app>…                         # Build + install one or more apps
cargo make install <app> --watch                  # Watch + reinstall on change
```

Alias configured in `.cargo/config.toml`:
```toml
[alias]
make = "run -q -p sola-make --"
```

## Documentation

- Map and authority order: [`docs/README.md`](docs/README.md).
- **Focus:** root `CURRENT.md` (only living handoff).
- **As-built:** `docs/capabilities.md`, `docs/architecture.md`.
- **Target freezes:** `docs/specs/` (dated). **New plans:** `docs/plans/`.
- **Product / operator docs:** `docs/manual/` — **shipped behavior only**.
- **Horizon / forks / ideas:** `docs/roadmap.md`, `docs/open-questions.md`,
  `docs/ideas/`.
- Historical vault/notes are not authoritative.

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
end of this file apply to `sola-browser` (CEF engine module), not to the Iced kit.

## sola-kit (the Iced app kit)

`sola-kit` is the shared Iced app kit + a `sola-kit` **storybook** binary that
dogfoods every component (each widget has a showcase page, so regressions show
up there first). Library surface in `src/lib.rs`: `App`, `BusSetup`, `CallSetup`, `run`,
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
  `BusSetup::new(id).subscribe(TopicKind::ALL).app_menu("Foo", [(...)]).calls("workspaces", methods).install()`
  Advertises call-plane methods; fold `call_subscription()` into iced. Or use
  `CallSetup::new(owner, app_id).methods(…).install()` alone.
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

- **System fonts only** — kit does **not** bundle or register font files from
  `/opt/sola/share/fonts/`. `fonts::ensure_system_fonts()` loads the host
  fontconfig set into iced’s global font db once per process (iced 0.14 does
  not do this by itself). Preferred faces: **SF Pro Text** (UI) and
  **Iosevka Term Slab** (mono) when installed; fall back to Inter /
  JetBrains Mono. Licensed SF/Iosevka stash lives gitignored at
  `.local/fonts/` and must be installed system-wide (`fc-cache`) — see
  `docs/manual/distribution.md`.
- **Semantic roles, not families.** Components never name a family directly —
  they call role accessors: `fonts::ui()`, `ui_medium()`, `display()`,
  `chrome()`, `mono()`. The `Fonts` table (those 5 roles) lives in a process-wide
  `RwLock`; `fonts::install(Fonts)` hot-swaps it, and the bus theme path calls
  `install` on every `Topic::Theme` so a font edit propagates on the next render.
- **`INSTALLED_FAMILIES` / `pickable_families()`** is the font-picker vocabulary
  (settings UI + bus `FontFamily` tokens). `fonts_from_families` builds a
  `Fonts` table from per-role family-name selections; `FontSelection` (in
  `theme.rs`) is the per-role wire form.

### Components (`src/components/`)

`badge`, `button`, `card`, `divider`, `field` (incl. `form_row`, error
caption, checkbox/toggle styles), `icon`, `popover`, `prose` (letter
reading: paragraphs, quotes, inline links), `readable`, `sidebar`,
`split`, `swatch`, `text`, `text_input` (fork — style/padding only; see
module docs), `toolbar`. Prefer `button::labeled` / `labeled_sm` and named
`PAD_CONTROL*` pads. Each has a storybook page under `src/storybook/pages/`.
**Always update that page in the same change** as the component (see
`.grok/rules/kit-storybook-pages.md`). Grow this surface only as real
apps need shared pieces — no speculative widgets.

## sola-shell (the Iced desktop shell)

A single **`iced::daemon`** multi-window application. The daemon opens no
default window; a boot task opens the menubar, then parks menu / launcher /
switcher / selection at 2×2 (`WindowKind`). Show is Frame to live output, not
a new map. The
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
