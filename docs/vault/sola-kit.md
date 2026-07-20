# sola-kit (iced)

The iced app kit that Sola primary apps and the shell build on.
**Production path** for desktop UI — pure Rust, iced 0.14 + wgpu +
wayland. The earlier CEF + Remix v3 kit is **removed** (not in tree,
not a fallback).

**Status (2026-07-19):** lib + `sola-kit` storybook binary. Active
consumers: `sola-monitor`, `sola-settings`, `sola-shell`,
`sola-terminal`, agent, browser-core, and the in-tree storybook
(dogfoods every kit component). Residual hardening work is tracked in
`docs/specs/2026-07-19-sola-kit-hardening-plan.md`.

Do **not** confuse this crate with:

- **`apocrypha/sola-app`** — the **legacy GTK4 + WebKit6** WebView
  host. Reference only; not a workspace member. See [[sola-app]].
- **`apocrypha/apps/*`** — retired WebView prototypes (agent, mail
  reference). Not on the iced kit path.
- The **removed CEF/Remix kit** — historical only; there is no second
  "sola-kit" web stack.

## Why iced (and not the CEF kit)

The CEF kit was a JavaScript runtime per app (CEF browser process,
Remix v3 components, swc transform, design-token CSS, storybook). It
proved correct but expensive on NVIDIA proprietary (CPU OSR memcpy per
paint — see [[sola-browser]] for engine-choice rationale).

Iced validated on NVIDIA proprietary with:

- iced 0.14 + wgpu + winit + smithay's wayland client
- `decorations: false` + `xdg_toplevel.app_id` + bus app-menu flow
  with sola-shell's menubar
- shared boilerplate (bus connect / subscribe / menu / window /
  fonts / theme) pulled into this kit

`sola-shell` and `sola-settings` (and the other consumers above) are
ported to this crate. With no consumers left, the CEF/Remix kit was
removed from the workspace.

## What lives in the kit (today)

```
crates/sola-kit/                  (library + storybook binary)
├── Cargo.toml                    iced 0.14 wayland/wgpu/tokio/svg/advanced
├── src/
│   ├── lib.rs                    re-exports + crate docs
│   ├── app.rs                    BusSetup, bus(), window_settings,
│   │                             startup(), bus_subscription(),
│   │                             apply_theme_update, is_self_quit
│   ├── fonts.rs                  system fonts via ensure_system_fonts;
│   │                             role table (ui / ui_medium / display /
│   │                             chrome / mono); Inter + JetBrains Mono
│   ├── theme.rs                  default_theme, theme_from_bus (pure),
│   │                             Atoms, FontSelection, ShellStyle,
│   │                             shell_style_from_bus_theme / bus_theme_with_shell
│   ├── float.rs                  FloatState — bus-driven float bit for
│   │                             client-drawn titlebars
│   ├── components/
│   │   ├── mod.rs                public surface
│   │   ├── badge.rs              status pill (Tone: Neutral/Accent/…)
│   │   ├── button.rs             named style fns + confirm_button
│   │   ├── card.rs               card / backplate / modal chrome
│   │   ├── color_picker.rs       stateful ColorPicker (SV + hue + alpha)
│   │   ├── divider.rs            vertical / horizontal (+ drag)
│   │   ├── field.rs              label + input + help text row
│   │   ├── icon.rs               icon / icon_svg / icon_colored helpers
│   │   ├── number_input.rs       unit-aware [−] value unit [+] stepper
│   │   ├── popover.rs            floating-panel chrome (+ anchored)
│   │   ├── readable.rs           max-width centered content column
│   │   ├── sidebar.rs            sidebar, panels, vertical tabs
│   │   ├── spectrum.rs           sv_square, hue/alpha strips (picker guts)
│   │   ├── split.rs              two-pane row with kit divider
│   │   ├── style.rs              RADIUS_*/SPACE_*, filled/hairline/dim
│   │   ├── swatch.rs             color preview tile
│   │   ├── text.rs               heading/body/caption/code + tone styles
│   │   ├── text_input/           forked iced text input (kit-styled)
│   │   ├── titlebar.rs           client-side window titlebar chrome
│   │   └── toolbar.rs            toolbar_button + style
│   ├── main.rs                   `sola-kit` storybook binary entry
│   └── storybook/                binary-only modules; not part of lib
│       ├── mod.rs                Storybook app, pages, theme/shell editors
│       └── pages/                one showcase per kit surface
│           ├── welcome-equivalent via theme.rs / shell.rs editors
│           ├── text, button, badge, card, field, …
│           ├── color_picker, number_input, readable, titlebar, icon
│           ├── sidebar, split, divider, popover, toolbar
│           └── shell (ShellStyle tokens)
```

Build / consume:

```bash
cargo make build
# or targeted:
cargo build --manifest-path crates/sola-kit/Cargo.toml
```

The kit is workspace-excluded (iced's transitive `smithay-clipboard`
flips wayland-sys into dlopen mode, which would break sola-river's
direct wayland linkage if unified). Iced consumers depend by path:

```toml
sola-kit = { path = "../sola-kit" }
```

## App-side scaffolding

There is **no** generic `run::<A>()` wrapper. Each app builds its own
`iced::application` / `iced::daemon` (update/view/subscription types
differ). The kit supplies the shared pre-iced boot and helpers:

```rust
startup(APP_ID);

BusSetup::new(APP_ID)
    .subscribe(TopicKind::ALL)
    .app_menu("Monitor", [("quit", "Quit Monitor", KeyCode::Q.meta())])
    .install();

let mut app = iced::application(...)
    .window(window_settings(APP_ID));
// fonts: ensure_system_fonts() already ran inside startup();
// components use fonts::ui() / mono() etc. by role name
app.run()
```

Canonical wiring: `crates/sola-monitor/src/main.rs`. Bus events:

```rust
fn subscription(&self) -> Subscription<Msg> {
    sola_kit::app::bus_subscription().map(Msg::Bus)
}

// In update, on Msg::Bus(msg):
sola_kit::apply_theme_update(&msg, &mut self.theme);
if sola_kit::is_self_quit(&msg, APP_ID) { /* exit */ }
```

Use `bus_subscription()` **or** a manual `bus().lock().try_recv()` loop,
never both (one receiver per process).

## Fonts

Sola **does not bundle** app UI fonts into the kit path. Fonts resolve
by family name through the system **fontconfig** database:

- `fonts::ensure_system_fonts()` — called from `startup()`; loads
  system faces into iced's global font db (iced 0.14 does not call
  cosmic-text's `load_system_fonts` by default).
- Defaults: **Inter** (all UI-shaped roles) + **JetBrains Mono**
  (mono/code). Must be installed system-wide — see
  [[Distribution]] / `docs/manual/distribution.md`.
- Semantic roles, not hard-coded families in components:
  `fonts::ui()`, `ui_medium()`, `display()`, `chrome()`, `mono()`.
- `fonts::install(Fonts)` hot-swaps the role table; bus theme path
  reinstalls via `fonts_from_bus_theme` (through `apply_theme_update`
  or an explicit pair with `theme_from_bus`).

## Theme & shell chrome

Three representations of the palette:

1. **Compile-time defaults** — `theme::hex::*` constants.
2. **`Atoms`** — editable `iced::Color`s (bg, bg_raised, bg_hover,
   border, fg, fg_muted, accent, success, warning, danger, selection).
3. **Bus theme** — `sola_core::theme::Theme`, sticky `Topic::Theme`.

Key APIs:

| API | Role |
| --- | --- |
| `default_theme()` | Pre-replay / offline iced theme |
| `theme_from_bus(&BusTheme)` | **Pure** map bus → iced `Theme` (no font side-effect) |
| `apply_theme_update(&msg, &mut theme)` | Theme + fonts + selection install on `Topic::Theme` |
| `to_bus_theme()` / `bus_theme_from_atoms` | Emit bus theme from kit defaults / atoms |
| `ShellStyle` + `shell_style_from_bus_theme` / `bus_theme_with_shell` | Shell chrome tokens (`shell-*` colors + spacing) |

`theme_from_bus` is pure on purpose — pair it with
`fonts::install(fonts_from_bus_theme(bus))` and selection install, or
use `apply_theme_update`. Shell keeps its own `on_theme` path that also
refreshes `ShellStyle`.

Specs: `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md`,
`docs/specs/2026-06-06-shell-customization-design.md`.

### Atom → iced slot map (summary)

| atom | iced slot(s) | typical use |
| ---- | ------------ | ----------- |
| `bg` | `background.base` / `weakest` | window canvas |
| `bg_raised` | `background.weaker` / `weak` | sidebars, cards |
| `bg_hover` | `background.neutral` / `strong` | hover / selected rows |
| `border` | `background.stronger` / `strongest` | hairlines |
| `fg` | `background.*.text` / `palette.text` | body text |
| `fg_muted` | `secondary.base.text` | captions |
| `accent` | `primary.base` | links, active rows |
| `success` / `warning` / `danger` | matching semantic slots | status |
| `selection` | process-wide via `install_selection` (no iced slot) | selection highlight |

### FloatState + titlebar

Client-drawn chrome for floating windows:

- `titlebar` component — dense window title strip.
- `FloatState` — learns this app's `window_id`s from `Topic::Windows`
  (match `(app_id, title)`), tracks `Topic::WindowFloating`, exposes
  `is_floating` / `is_floating_any` for view logic.

## Reusable components — policy

iced components are functions (or small stateful structs) returning
`Element` / `Container`. The bar for a new kit component is: **two
apps want the same thing**. Until then it lives in the app. No
speculative widgets.

Fork note: `components/text_input/` is a **forked** iced text-input
widget (not a thin style wrapper of `iced::widget::text_input`), with
kit `style` and free constructor `text_input::text_input`. Spectrum
primitives back `ColorPicker`.

## Where to look in code

| file | what it owns |
| ---- | ------------ |
| `crates/sola-kit/src/lib.rs` | crate docs + re-exports (`BusSetup`, `FloatState`, `default_theme`, iced, sola_bus) |
| `crates/sola-kit/src/app.rs` | startup, BusSetup, bus_subscription, apply_theme_update, is_self_quit, window_settings |
| `crates/sola-kit/src/fonts.rs` | ensure_system_fonts, Fonts roles, Inter / JetBrains Mono defaults |
| `crates/sola-kit/src/theme.rs` | Atoms, theme_from_bus, ShellStyle, bus round-trips |
| `crates/sola-kit/src/float.rs` | FloatState |
| `crates/sola-kit/src/components/` | widget surface (see tree above) |
| `crates/sola-kit/src/storybook/` | binary-only showcase + theme/shell editors |
| `crates/sola-monitor/src/main.rs` | canonical consumer wiring |

## Roadmap

### Done (relative to the original v0 roadmap)

- [x] Multiple iced consumers beyond the storybook — monitor, settings,
  shell, terminal, agent, browser-core.
- [x] Live `Topic::Theme` reload (`bus_subscription` +
  `apply_theme_update` / shell `on_theme`).
- [x] Grow components with demand — including `color_picker`,
  `number_input`, `readable`, `titlebar`, `spectrum`, `icon`, forked
  `text_input`, plus shell tokens / `ShellStyle` and `FloatState`.
- [x] CEF/Remix kit removed; shell + settings on iced sola-kit.
- [x] System fonts via fontconfig (`ensure_system_fonts`); Inter +
  JetBrains Mono defaults (no bundled SF Pro narrative for the kit).

### Residual / next

Tracked by **this plan**, not the old v0 bullet list:

→ **`docs/specs/2026-07-19-sola-kit-hardening-plan.md`**

Historical ideas that remain optional / non-goals unless the plan
says otherwise:

- Generic `run::<A>()` — still not shipped; apps own their iced
  builder. Promote only when real duplication justifies it (and the
  hardening plan allows it).
- Further component growth — only when a second consumer needs the
  same control.

Design language north star for chrome/style touch:
`docs/manual/design-language.md`.
