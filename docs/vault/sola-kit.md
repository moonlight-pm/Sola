# sola-kit (iced)

The kit that Sola apps build on. v0 — small surface, grows with
demand. Successor to [[sola-kit-legacy]] (CEF + Remix v3); the two
coexist while shell-side apps migrate.

**Status (2026-05-21):** v0 shipped. First consumer is `sola-monitor`.
Surface intentionally minimal — we add to it when a second consumer
arrives and we can see the shape of the duplication.

## Why a new kit

`sola-kit-legacy` was a JavaScript runtime (CEF browser process per app,
Remix v3 components, swc transform pipeline, design-token CSS lowering,
storybook). The CEF stack proved correct but expensive on NVIDIA
proprietary (CPU OSR memcpy per paint, ~720 MiB/s memory traffic —
see [[sola-browser]] for the bench numbers and engine-choice rationale).

The iced sola-monitor is the first app that ran outside the legacy
kit. It validated:

- iced 0.14 + wgpu + winit + smithay's wayland client works fine on
  NVIDIA proprietary
- the `decorations: false` + `xdg_toplevel.app_id` + bus app-menu
  flow integrates cleanly with sola-shell's menubar
- the boilerplate every app would repeat (bus connect / subscribe /
  menu publish / window settings / font load / theme palette) is
  ~80 lines of nearly identical code per app — exactly the kind of
  thing a kit exists to eliminate

So the iced kit went in. The legacy kit stays alive for `sola-shell`,
`sola-settings`, and any `sola-app`-based crates. We don't break what
works.

## What lives in the kit (today)

```
crates/sola-kit/                  (library only, no binary)
├── Cargo.toml                    iced 0.14 wayland/wgpu/tokio
├── src/
│   ├── lib.rs                    re-exports + crate docs
│   ├── app.rs                    BusSetup, bus(), window_settings,
│   │                             startup(), App trait (marker)
│   ├── fonts.rs                  MONO/NORMAL/CONDENSED/CONDENSED_BOLD,
│   │                             load_all() reads /opt/sola/share/fonts
│   ├── theme.rs                  default_theme() → iced::Theme,
│   │                             parse() (#rrggbb), hex atom constants
│   └── components/
│       ├── mod.rs                public surface
│       ├── divider.rs            vertical_divider(on_press)
│       └── toolbar.rs            toolbar_button(label)
```

Build / consume:

```bash
cargo build --manifest-path crates/sola-kit/Cargo.toml
# or — through the build orchestrator
cargo make build
```

The kit is workspace-excluded (iced's transitive `smithay-clipboard`
flips wayland-sys into dlopen mode, which would break sola-river's
direct wayland linkage if unified across the workspace). Every
iced consumer is excluded for the same reason and depends on the kit
by path:

```toml
sola-kit = { path = "../sola-kit" }
```

## App-side boilerplate, before and after

Before the kit (the original `sola-monitor-iced::main()`, ~80 lines):

```rust
sola_core::log::init(APP_ID);
sola_core::env::activate_wayland_session(10_000);

let mut client = BusClient::new();
client.connect_blocking(Duration::from_millis(250));
client.subscribe(TopicKind::ALL).ok();
client.emit(Topic::SetAppMenu(AppMenuPayload {
    app_id: APP_ID.into(),
    menus: vec![MenuDefinition { /* … */ }],
})).ok();
BUS.set(Arc::new(Mutex::new(client))).map_err(|_| ()).unwrap();

let mut app = iced::application(...)
    .window(iced::window::Settings {
        decorations: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: APP_ID.into(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    });
for relative in FONT_FILES {
    let path = format!("{FONT_DIR}/{relative}");
    match std::fs::read(&path) {
        Ok(bytes) => app = app.font(bytes),
        Err(e) => tracing::warn!("font missing: {e}"),
    }
}
app.run()
```

After:

```rust
startup(APP_ID);

BusSetup::new(APP_ID)
    .subscribe(TopicKind::ALL)
    .app_menu("Monitor", [("quit", "Quit Monitor", KeyCode::Q.meta())])
    .install();

let mut app = iced::application(...)
    .window(window_settings(APP_ID));
for bytes in fonts::load_all() {
    app = app.font(bytes);
}
app.run()
```

iced still owns the `application(...)` builder — that's where the
typed `update`/`view`/`theme`/`subscription` fns thread through, and
wrapping it generically without macros runs into HRTBs that defeat
the point. We'll revisit a full `kit::run::<A>()` when a second app
shows up and we can see what's common.

## Reusable components — the design problem

iced components are functions returning `Element<'a, Message>`. There
is no class system or external runtime. So "kit component" means:

- a constructor function (`toolbar_button(label)`)
- optionally style helpers (`divider_style(theme)`)
- optionally a struct for components with non-trivial state

What we explicitly *don't* do:

- speculative widgets — we add `Sidebar`, `Card`, `Tabs`, etc. when
  a second consumer needs them, not before. The trap of the legacy
  kit was inventing a component library ahead of demand.
- wrapping iced's own widgets just to brand them — `iced::button` is
  already fine; we wrap only when there's real, repeated styling to
  centralise (e.g. `toolbar_button` bundles the condensed-bold font +
  fixed padding so a row of them aligns).
- generic "Message" plumbing — the kit doesn't try to be a state
  framework. Apps own their `Msg` enum; the kit just gives them
  reusable views.

The bar for a new kit component is: **two apps want it, and they want
the same thing**. Until then it lives in the app.

## Theme publishing — the design problem

The legacy kit ships a token system (palette + per-component bindings)
broadcast over `Topic::Theme` as CSS. iced can't consume CSS, so the
existing bus protocol doesn't lower cleanly into an iced app.

Today the iced kit ships a single hardcoded palette resolved at
startup (`default_theme()` in `theme.rs`). That's enough for v0 —
the monitor's colors all come from `sola_kit::theme::parse(hex::*)`
constants, and we can change them in one place.

For v0.2 we want bus-driven theme:

1. `sola-core::theme::Theme` already defines the token vocabulary
   (atoms + selection groups + per-component bindings). Both kits
   should resolve from this shared shape.
2. The iced kit subscribes to `Topic::Theme` and recomputes
   `iced::Theme::custom(...)` whenever it changes — this needs a
   `theme()` method that reads from a hot-swappable cell rather
   than returning the same value every call. Iced 0.14 supports
   this through the `theme` callback; the cell can live next to
   the bus singleton in `app.rs`.
3. Per-component slot resolution (`var(--sola-sidebar-bg)`) maps to
   per-component `style` fn closures. We'll publish a small set of
   slot-aware style fns that look up scoped bindings the same way
   the legacy kit's CSS does, but resolve to `iced::Color` instead
   of CSS variables.

None of that is built yet — the issue is that we need a kit consumer
that actually wants live theme reloads before we design the surface.
The monitor will likely be that consumer once we move it onto the
sidebar component.

See [[Topics]] for the `Topic::Theme` shape and `sola-core/src/theme/`
for the shared type story.

## Migration story (sola-shell, sola-settings)

`sola-shell` and `sola-settings` still depend on `sola-kit-legacy`
via the package-rename trick — `Cargo.toml` says
`sola-kit = { path = "../sola-kit-legacy", package = "sola-kit-legacy" }`
so their internal `use sola_kit::…` imports keep working unchanged.

When each crate ports to the new kit, that one line gets replaced
with `sola-kit = { path = "../sola-kit" }` and the call sites get
the obvious diff (no `cef::short_circuit_if_subprocess`, no
`asset_bundle!`, no `AppCtx` — iced apps own their state directly).

Order is up to demand. Shell + settings work today; the new
direction proves itself app-by-app rather than as a big-bang
migration.

## Where to look in code

| file                                                | what it owns                                    |
| --------------------------------------------------- | ----------------------------------------------- |
| `crates/sola-kit/src/lib.rs`                        | crate docs + re-exports                         |
| `crates/sola-kit/src/app.rs`                        | `BusSetup`, `bus()`, `window_settings`, etc.    |
| `crates/sola-kit/src/fonts.rs`                      | font constants + `load_all()`                   |
| `crates/sola-kit/src/theme.rs`                      | `default_theme()`, hex atoms, `parse()`         |
| `crates/sola-kit/src/components/divider.rs`         | draggable column divider                        |
| `crates/sola-kit/src/components/toolbar.rs`         | pre-styled toolbar button                       |
| `crates/sola-monitor/src/main.rs`                   | canonical first consumer — read for the pattern |
| `crates/sola-kit-legacy/`                           | mothballed CEF/Remix kit — see [[sola-kit-legacy]] |

## Roadmap (when work resumes)

In rough priority order:

1. **Live theme reloads.** Subscribe to `Topic::Theme`, recompute
   `iced::Theme::custom(...)` on change, push to running app.
2. **Second consumer.** Port one of: sola-settings (small, well-scoped),
   sola-shell's launcher (visual, exercises layout components), or a
   brand-new app that wants iced from day one. Whatever the second app
   wants, that's what gets promoted into the kit next.
3. **Slot-aware style fns.** Component-level `var(--sola-…)` resolution
   in iced — `kit::components::sidebar::style(theme, slot)` etc.
4. **`run::<A>()` entry point.** Once two apps' `main()` functions look
   nearly identical (modulo the typed update/view/subscription
   closures), bundle them into one generic function. Don't ship before
   the duplication is real.
5. **Inline shared widgets** as the second consumer needs them
   (sidebar, list-row, badge, status pill, etc.). One at a time.

Items 1–2 unblock most other work. 3+ are improvements that pay off
proportionally to kit adoption.
