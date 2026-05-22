# sola-kit (iced)

The kit that Sola apps build on. v0 — small surface, grows with
demand. Successor to [[sola-kit-legacy]] (CEF + Remix v3); the two
coexist while shell-side apps migrate.

**Status (2026-05-22):** v0 ships a lib + the `sola-kit` storybook
binary. First external consumer is `sola-monitor`; the storybook is
the second consumer (dogfoods every kit component the moment it
lands). Component lineup approaches parity with the legacy kit's
visible vocabulary — see "What lives in the kit" below.

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
crates/sola-kit/                  (library + storybook binary)
├── Cargo.toml                    iced 0.14 wayland/wgpu/tokio
├── src/
│   ├── lib.rs                    re-exports + crate docs
│   ├── app.rs                    BusSetup, bus(), window_settings,
│   │                             startup(), App trait (marker)
│   ├── fonts.rs                  MONO/NORMAL/CONDENSED/CONDENSED_BOLD,
│   │                             load_all() reads /opt/sola/share/fonts
│   ├── theme.rs                  default_theme() + sola_extended generator
│   │                             — binds hex atoms to iced palette slots
│   ├── components/
│   │   ├── mod.rs                public surface
│   │   ├── badge.rs              status pill (Neutral/Accent/Success/...)
│   │   ├── button.rs             named style fns (primary/secondary/...)
│   │   ├── card.rs               elevated container chrome
│   │   ├── divider.rs            vertical_divider(on_press)
│   │   ├── field.rs              label + input + help text row
│   │   ├── popover.rs            floating-panel chrome
│   │   ├── sidebar.rs            sidebar(items) — vertical nav column
│   │   ├── split.rs              two-pane row with kit divider
│   │   ├── swatch.rs             color preview tile
│   │   ├── text.rs               heading/subheading/body/caption/code +
│   │   │                         muted/accent/success/warning/danger styles
│   │   ├── text_input.rs         kit-styled text input
│   │   └── toolbar.rs            toolbar_button + toolbar style fn
│   ├── main.rs                   `sola-kit` binary entry — boots iced
│   │                             with the storybook below
│   └── storybook/                binary-only modules; not part of lib
│       ├── mod.rs                Storybook app, Page enum, layout
│       └── pages/                one showcase per kit component
│           ├── welcome.rs        + theme.rs (palette atoms + slot map)
│           ├── text.rs           typography reference
│           ├── button.rs         + badge.rs / card.rs / field.rs
│           ├── divider.rs        + popover.rs / sidebar.rs / split.rs
│           └── toolbar.rs        stateful (click-count demo)
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

## Theme & styling — the design

We chose the pure-iced approach: every kit component's style is a
function of `&iced::Theme`. The kit's "binding layer" is a custom
extended-palette generator (`theme::sola_extended`) handed to
`Theme::custom_with_fn`. Component style fns then read from
`theme.extended_palette()` exactly like `iced::button::primary` does
— no global, no side struct, no parallel styling pipeline.

### How atoms map to iced slots

Iced's `Extended::Background` has 8 tiers (`weakest…strongest`); our
10 hex atoms compress into iced's vocabulary like this:

| atom         | iced slot(s)                              | typical use            |
| ------------ | ----------------------------------------- | ---------------------- |
| `BG`         | `background.base` / `weakest`             | window canvas          |
| `BG_RAISED`  | `background.weaker` / `weak`              | sidebars, cards        |
| `BG_HOVER`   | `background.neutral` / `strong`           | hover / selected rows  |
| `BORDER`     | `background.stronger` / `strongest`       | hairlines              |
| `FG`         | `background.*.text` / `palette.text`      | body text              |
| `FG_MUTED`   | `secondary.base.text`                     | captions, deemphasized |
| `ACCENT`     | `primary.base` (weak/strong auto-derived) | links, active rows     |
| `SUCCESS`    | `success.base`                            | confirmations          |
| `WARNING`    | `warning.base`                            | non-blocking issues    |
| `DANGER`     | `danger.base`                             | errors, destructive    |

The full mapping lives in `theme::sola_extended`. Rebinding a slot is
a one-line edit there — no component-code change.

### Live theme reload (v0.2)

Iced's `app.theme(&self) -> Theme` runs every render, so the wiring
for `Topic::Theme` is straightforward:

1. App stores `Arc<iced::Theme>` (or just `iced::Theme`) in state.
2. Bus subscription on `Topic::Theme` produces an
   `iced::Subscription` event; `update` rebuilds the theme via
   `sola_core::theme::Theme → iced::Theme::custom_with_fn(...)` and
   swaps it into state.
3. Next render iced calls `theme(&self)` and sees the new value.

We avoid global state because iced's existing mechanism is enough.

### Escape hatch

When an atom genuinely doesn't fit any iced slot (e.g. an overlay
glow tint), publish it as a `pub const` in `theme::hex` and
reference it directly from one component's style fn. Same shape as
the legacy kit's `--sola-…` escape hatches that bypassed the
binding system. Use sparingly — every direct `hex::*` reference is
debt against the unified theme surface.

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
| `crates/sola-kit/src/components/badge.rs`           | status pill (5 tones)                           |
| `crates/sola-kit/src/components/button.rs`          | primary / secondary / ghost / danger style fns  |
| `crates/sola-kit/src/components/card.rs`            | elevated container chrome                       |
| `crates/sola-kit/src/components/divider.rs`         | draggable column divider                        |
| `crates/sola-kit/src/components/field.rs`           | label + input + help text row                   |
| `crates/sola-kit/src/components/popover.rs`         | floating-panel chrome (visual only)             |
| `crates/sola-kit/src/components/sidebar.rs`         | vertical nav column with active row             |
| `crates/sola-kit/src/components/split.rs`           | two-pane row with kit divider                   |
| `crates/sola-kit/src/components/swatch.rs`          | color preview tile                              |
| `crates/sola-kit/src/components/text.rs`            | typography helpers + tone style fns             |
| `crates/sola-kit/src/components/text_input.rs`      | kit-styled text input                           |
| `crates/sola-kit/src/components/toolbar.rs`         | pre-styled toolbar button                       |
| `crates/sola-kit/src/main.rs`                       | `sola-kit` storybook binary entry               |
| `crates/sola-kit/src/storybook/`                    | binary-only showcase pages                      |
| `crates/sola-monitor/src/main.rs`                   | canonical first consumer — read for the pattern |
| `crates/sola-kit-legacy/`                           | mothballed CEF/Remix kit — see [[sola-kit-legacy]] |

## Roadmap (when work resumes)

In rough priority order:

1. **Live theme reloads.** Wire `Topic::Theme` into the kit so apps'
   `theme(&self)` callbacks return a fresh `iced::Theme` built from
   the bus payload. See "Live theme reload" above for the design.
2. **Third consumer.** Port one of: sola-settings (small, well-scoped),
   sola-shell's launcher (visual, exercises layout components), or a
   brand-new app that wants iced from day one. Whatever the third app
   wants, that's what gets promoted into the kit next. (Storybook is
   the second consumer; it dogfoods every existing component.)
3. **Refactor sola-monitor onto kit components.** It currently
   carries its own divider_style, hex helper, row containers etc. —
   small file, easy migration once we touch it.
4. **`run::<A>()` entry point.** Once two apps' `main()` functions look
   nearly identical (modulo the typed update/view/subscription
   closures), bundle them into one generic function. Don't ship before
   the duplication is real.
5. **Grow components inline** as the third consumer needs them
   (list-row, tabs, breadcrumbs, segmented control, slider styling,
   checkbox styling, ...). One at a time.

Items 1–3 unblock most other work. 4+ are improvements that pay off
proportionally to kit adoption.
