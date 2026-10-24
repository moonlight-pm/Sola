# Theme and toolkit

Every Sola application draws its windows with Iced through one shared
toolkit, `sola-kit`, and takes its colours, fonts, and shell chrome values
from one theme published on the bus. When the theme changes, each running
application applies it on its next frame. The theme styles the
applications' own controls; web pages shown inside Browser, web apps,
and Design are not restyled.

## sola-kit

`sola-kit` is a Rust library that every Sola application links, plus the
Kit application. It has no generic entry point: each application builds its
own `iced::application` or `iced::daemon` from the kit's parts.

| Part | What it gives an application |
|---|---|
| `app::startup` | The standard boot: logging, the Wayland socket, the GPU environment for a session started from a TTY, the system fonts, and a watch that restarts the application in place when its binary in `/opt/sola/bin` changes (skipped when `SOLA_NO_SELF_WATCH=1`) |
| `BusSetup`, `app::bus_subscription`, `app::bus` | The bus connection ([The bus](bus.md#using-the-bus-from-rust)) |
| `CallSetup`, `call_subscription` | Call-plane methods ([Call plane](call.md)) |
| `theme` | The palette, the shell style, and conversion from the bus theme |
| `fonts` | System font loading and the five font roles |
| `components` | Shared widgets: buttons, badges, cards, fields and forms, select, number input, colour picker, file picker, popover, context menu, sidebar, split, toolbar, titlebar, prose, JSON view, icons |
| `float`, `menu`, `app::window_settings` | The titlebar and drag for floating windows, the standard Window menu, and window settings that carry the application's Wayland `app_id` and turn off server decorations |

### The Kit application

Kit (`sola-kit`, **Kit** in the launcher) is the toolkit's storybook. Each
component has a page showing it in use, grouped as System (Overview, Theme,
Shell), Layout, and Components. Kit is also the theme editor
([Editing the theme](#editing-the-theme)). It remembers the last page in
`~/.config/sola/kit/storybook-page`.

## The theme on the bus

The theme travels as the persistent topic `Theme`
([topic reference](bus.md#theme-and-app-state)). Its payload is a `Theme`
value from `sola-core`:

- `palette.tokens` maps a token name to a token: a `kind` (`Color`,
  `FontFamily`, `TextSize`, `Space`, or `Radius`), a string `value`, and a
  list of `groups`.
- `components` maps components to slot bindings. Every theme Sola publishes
  has this map empty, and no application reads it.

A published theme replaces the previous one whole; there is no update of a
single token. Because `Theme` is persistent, the bus writes each published
theme to `~/.config/sola/theme/current.yaml`, restores it when the bus
starts, and replays it to every application that subscribes. When the shell
starts and that file does not exist, it publishes the built-in default
theme, which creates the file. Each token is stored like this:

```text
palette:
  tokens:
    accent:
      kind: Color
      value: '#3dd6f5'
      groups:
      - accent
```

The bus reads the file only when it starts, so edit the theme with Kit, not
in the file. Saved presets are separate records (`CustomTheme`), one file
each in `~/.config/sola/theme/presets/`.

## Palette tokens

Iced widgets take their colours from palette slots. The kit fills those
slots from ten of the tokens below and keeps the eleventh, `selection`,
beside them. Kit components and application styles read these; a colour an
application writes into its code does not follow the theme.

| Token | Default | Iced palette slots | Used for |
|---|---|---|---|
| `bg-primary` | `#0b0b0b` | `background.base`, `background.weakest` | Window canvas |
| `bg-secondary` | `#151515` | `background.weaker`, `background.weak` | Raised panels: sidebars, cards |
| `bg-hover` | `#1f1f1f` | `background.neutral`, `background.strong`, `secondary.weak` | Hover |
| `border` | `#2a2a2a` | `background.stronger`, `background.strongest`, `secondary.base`, `secondary.strong` | Hard edges and chrome |
| `text-primary` | `#ececec` | Text on every background slot and on `secondary.strong` | Primary labels |
| `text-tertiary` | `#949494` | Text on `secondary.base` and `secondary.weak` | Muted labels |
| `accent` | `#3dd6f5` | `primary`; Iced derives its lighter and darker shades | Focus, selection chrome, key status |
| `success` | `#3ecf8e` | `success` | Status |
| `warning` | `#e8b84a` | `warning` | Status |
| `danger` | `#f07178` | `danger` | Status |
| `selection` | `#292929` | None; read with `sola_kit::theme::selection()` | Selected rows and text selection |

The base is neutral grey. Until 2026-10-23 it was a blue-tinted slate
(`#0c0e12`, `#151922`, …). When the shell receives a saved theme that still has
those slate values, it swaps each one for its grey and saves the result.
Colours you picked yourself are kept.

Colour values are `#rrggbb` or `#rrggbbaa`. Terminal and Projects also
derive their terminal colours from these tokens: the default foreground and
background, the ANSI colours except magenta and cyan, and the selection
wash (from `accent`). Colours a program sets with 256-colour or 24-bit
codes are drawn as given, and the cursor colour is fixed.

The palette carries more tokens: `bg-tertiary`, `border-subtle`,
`text-secondary`, `text-muted`, `text-accent`, `accent-dim`, text sizes
(`text-caption` to `text-display`), spacing (`space-xs` to `space-xxl`),
and radii (`radius-sm` to `radius-lg`). They travel and are saved with the
theme, but no application reads them.

## Fonts

### Roles

Components never name a font family. They ask for a role each time they
draw, and the theme assigns a family to each role.

| Token | Role | Weight | Used for | Default family |
|---|---|---|---|---|
| `font-ui` | `fonts::ui()` | Regular | Body text, buttons, sidebar items | SF Pro Text |
| `font-ui-medium` | `fonts::ui_medium()` | Medium (500) | Emphasis: the focused app's title, the active row | SF Pro Text |
| `font-display` | `fonts::display()` | Medium (500) | Page titles and large headings | SF Pro Text |
| `font-chrome` | `fonts::chrome()` | Regular | Menubar labels and section headers | SF Pro Text |
| `font-mono` | `fonts::mono()` | Regular | Code, JSON, Terminal, Projects | Iosevka Term Slab |

### How a family is chosen

Sola bundles no fonts. `app::startup` loads every family fontconfig knows
into Iced's font database, once per process. A family named in the theme is
used only if it is in that database. When a role's token is missing, empty,
or names a family that is not installed, the role falls back:

- UI roles: SF Pro Text if installed, otherwise Inter.
- Mono: Iosevka Term Slab if installed, otherwise JetBrains Mono.

[Fonts](distribution.md) lists the families to install.

Each process keeps the five roles in one table. `fonts::install(table)`
replaces it, and the next frame draws with the new families; every `Theme`
message does this. Terminal and Projects also measure the new mono font's
cell size and reflow their panes.

## Shell tokens

The shell's chrome has its own tokens in the same palette, all in the group
`shell`. Only the shell applies them.

| Token | Default | Controls |
|---|---|---|
| `shell-menubar-bg` | `#050505` | Menubar background |
| `shell-backdrop-dim` | `#00000099` | Dimming behind the launcher |
| `shell-switcher-bg` | `#151515e6` | Switcher panel fill |
| `shell-switcher-border` | `#ffffff1a` | Switcher panel border |
| `shell-switcher-icon-bg` | `#ffffff2e` | Plate behind the selected app in the switcher |
| `shell-switcher-icon-fg` | `#ececec` | Tint of icons in the switcher |
| `shell-switcher-icon-fg-sel` | `#ececec` | Saved and editable; the switcher does not use it |
| `shell-switcher-pad` | `14px` | Switcher panel padding |
| `shell-switcher-tile-pad` | `8px` | Inset of each icon cell in the switcher |
| `shell-launcher-width` | `560px` | Launcher width |
| `shell-launcher-pad` | `8px` | Row padding in the launcher and the shortcuts overlay |

Colours may carry alpha (`#rrggbbaa`). Spacing values are a number followed
by `px`. The shell reads these tokens into its `ShellStyle` on every `Theme`
message. The [Shell](sola-shell.md) chapter describes the menubar,
launcher, and switcher.

## How an application applies the theme

On every `Theme` message, a kit application calls
`sola_kit::apply_theme_update(&message, &mut theme)`, which rebuilds its
Iced theme from the palette tokens (`theme::theme_from_bus`), installs the
font roles (`fonts::install`), and installs the selection colour, and
returns true when the message was a `Theme`. The application returns its
stored theme from Iced's `theme` callback, so the next frame uses it.

Before a theme arrives (between start and the bus replay, or while the bus
is down), an application draws with `sola_kit::default_theme()`, the
compile-time defaults in the tables above, with font roles resolved from
the default families.

### Fallbacks

| Missing or malformed | Result |
|---|---|
| No theme on the bus | Compile-time defaults, as before a theme arrives |
| A palette token, or a colour not in `#rrggbb` or `#rrggbbaa` form | That token's default |
| A font token, or a family that is not installed | The role's fallback family |
| A shell token, or a spacing value not in `<number>px` form | That token's default |
| A `theme/current.yaml` that does not match the theme schema | The bus restores no theme, and the shell publishes none because the file exists; applications draw the compile-time defaults until a theme is published from Kit (by choosing or editing a preset) |

## Editing the theme

Kit is the theme editor. Settings does not edit the theme or the fonts; it
follows the theme like any other application.

Kit's header holds the theme menu, **New Theme**, and **Delete**. The
presets are **Default**, the built-in values, which is read-only (edits are
ignored and **Delete** is disabled while it is selected), and your own,
each a `CustomTheme` record in `~/.config/sola/theme/presets/<name>.yaml`.

1. Choose **New Theme**, type a name of lowercase letters joined by single
   hyphens (such as `solar-flare`), and press **Save**. Kit copies the
   active preset under that name, saves it, and selects it.
2. On the **Theme** page, pick a family for each font role (from the
   families fontconfig reported when Kit started) and click a swatch to edit
   a colour; **reset** under a changed swatch restores its default. On the
   **Shell** page, edit the seven shell colours (the picker has an alpha
   rail) and the four spacing values: switcher padding 0 to 64 px, icon
   cell inset 0 to 48 px, launcher width 320 to 1280 px, launcher row
   padding 0 to 32 px.
3. Each change is published at once as `Theme`, so every running
   application restyles as you edit and the bus saves it to
   `theme/current.yaml`. The header shows **Edited •**.
4. **Save** writes the preset file. **Revert** returns the preset to its
   last saved state and publishes that.

Choosing another preset publishes it and discards unsaved edits to the one
you leave. **Delete**, then **Confirm?**, removes the preset's file and
publishes Default. Kit publishes nothing when it opens. It does not store
which preset is live: it selects the preset whose values equal the live
theme, and if none matches (after quitting with unsaved edits, which stay
live) it shows Default while every application keeps drawing the live
theme.

## Following the theme in a new application

1. Call `sola_kit::app::startup(APP_ID)` first; it loads the system fonts.
2. Include `TopicKind::Theme` in the `BusSetup` subscription.
3. Keep an `iced::Theme` in the application state, start it as
   `sola_kit::default_theme()`, and return it from Iced's `theme` callback.
   An application that floats with a transparent window returns
   `sola_kit::theme_for(floating, &theme)`, which clears the window fill
   while floating.
4. Map `sola_kit::app::bus_subscription()` into the application's
   subscription and pass each message to `sola_kit::apply_theme_update`.
5. Draw with kit components. In custom styles, take colours from
   `theme.extended_palette()` and `sola_kit::theme::selection()`, and fonts
   from the `sola_kit::fonts` role functions. A colour or family written
   into the application's code does not follow the theme.

[The bus](bus.md#using-the-bus-from-rust) has a code sketch of this
wiring.

## Limits

- The theme controls eleven colours, five font roles, and eleven shell
  values. Type sizes, spacing, and radii are fixed.
- The font database is loaded once per process. A family installed while an
  application runs is not available to it until it restarts.
- Iced's default font, used by widgets that set no font, is chosen when an
  application starts, before the theme arrives: the default UI family in
  most applications, Inter in Kit and the shell, the default mono family in
  Terminal and Projects. It does not follow `font-ui`. Text that a
  component draws with a role does follow the theme.
- Iced treats every theme as dark, whatever its colours.
- Kit is the only editor. `solactl emit Theme` can publish a whole theme
  as JSON, but there is no command to change one token.
