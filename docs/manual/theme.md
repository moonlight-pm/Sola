# Theme and toolkit

Every Sola application draws its windows with Iced through one shared
toolkit, `sola-kit`, and takes its colours, fonts, and shell chrome values
from one theme published on the bus. When the theme changes, each running
application applies it on its next frame. The theme styles the
applications' own controls; web pages shown inside Browser, web apps,
and Design are not restyled.

Two places change the theme. **Settings → Appearance** sets the colours
from the background picture, from four colours of your own, or both
([Appearance settings](#appearance-settings)). **Kit** edits every token,
the fonts included, and keeps named presets
([Editing the theme in Kit](#editing-the-theme-in-kit)).

## sola-kit

`sola-kit` is a Rust library that every Sola application links, plus the
Kit application. It has no generic entry point: each application builds its
own `iced::application` or `iced::daemon` from the kit's parts.

| Part | What it gives an application |
|---|---|
| `app::startup` | The standard boot: logging, the Wayland socket, the system fonts, and a watch that restarts the application in place when its own binary changes (skipped when `SOLA_NO_SELF_WATCH=1`) |
| `BusSetup`, `app::bus_subscription`, `app::bus` | The bus connection ([The bus](bus.md#using-the-bus-from-rust)) |
| `CallSetup`, `call_subscription` | Call-plane methods ([Call plane](call.md)) |
| `theme` | The palette, the shell style, and conversion from the bus theme |
| `fonts` | System font loading and the five font roles |
| `components` | Shared widgets: buttons, badges, cards, fields and forms, select, number input, colour picker, file picker, popover, context menu, sidebar, status marks, split, scroll, toolbar, prose, JSON view, icons, and the window frame (titlebar, menus, rim) |
| `menu`, `float`, `app::window_settings` | The app's menus in its titlebars and the standard Window menu; moving and resizing floating windows; window settings that carry the application's Wayland `app_id` and turn off server decorations |
| `tabs` | A window per document: tells the Shell which window is which tab, so it keeps its place in the bar, puts an activity mark on its row, and asks the Shell to show it |

### The Kit application

Kit (`sola-kit`, **Kit** in the launcher) is the toolkit's storybook. Each
component has a page showing it in use, grouped as System (Overview, Theme,
Shell), Layout, and Components. Kit is also a theme editor
([Editing the theme in Kit](#editing-the-theme-in-kit)). It remembers the
last page in `~/.config/sola/kit/storybook-page`.

## The theme on the bus

The theme travels as the persistent topic `Theme`
([topic reference](bus.md#the-desks-look)). Its payload is a `Theme`
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

The bus reads the file only when it starts, so change the theme with
Settings or Kit, not in the file. Kit's presets are separate records
(`CustomTheme`), one file each in `~/.config/sola/theme/presets/`.

## Palette tokens

Iced widgets take their colours from palette slots. The kit fills those
slots from ten of the tokens below and keeps the last two, `selection` and
`highlight`, beside them. Kit components and application styles read
these; a colour an application writes into its code does not follow the
theme.

| Token | Default | Iced palette slots | Used for |
|---|---|---|---|
| `bg-primary` | `#0b0b0b` | `background.base`, `background.weakest` | Window canvas |
| `bg-secondary` | `#151515` | `background.weaker`, `background.weak` | Raised panels: sidebars, cards, titlebars |
| `bg-hover` | `#1f1f1f` | `background.neutral`, `background.strong`, `secondary.weak` | Hover |
| `border` | `#2a2a2a` | `background.stronger`, `background.strongest`, `secondary.base`, `secondary.strong` | Hard edges and chrome |
| `text-primary` | `#ececec` | Text on every background slot and on `secondary.strong` | Primary labels |
| `text-tertiary` | `#949494` | Text on `secondary.base` and `secondary.weak` | Muted labels (see below) |
| `accent` | `#3dd6f5` | `primary`; Iced derives its lighter and darker shades | Focus, selection chrome, key status |
| `success` | `#3ecf8e` | `success` | Status |
| `warning` | `#e8b84a` | `warning` | Status |
| `danger` | `#f07178` | `danger` | Status |
| `selection` | `#292929` | None; read with `sola_kit::theme::selection()` | Selected rows and text selection |
| `highlight` | `#353535` | None; read with `sola_kit::theme::highlight()` | A sidebar column's active row: the bar's focused window, Settings' page |

`text-tertiary` has two defaults. A fresh desk, and Kit's Default preset,
write `#949494`; the palette's seed value, which the Shell uses whenever it
manages the colours (from the picture, or your own), is `#737373`.

Themes saved by earlier versions of Sola use a blue-tinted slate base
(`#0c0e12`, `#151922`, …). When the shell receives a theme that still has
slate values, it swaps each one for its grey and saves the result. Colours
you picked yourself are kept.

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
| `font-ui-medium` | `fonts::ui_medium()` | Medium (500) | Emphasis: badges, section caps, toolbar labels | SF Pro Text |
| `font-display` | `fonts::display()` | Medium (500) | Page titles and large headings | SF Pro Text |
| `font-chrome` | `fonts::chrome()` | Regular | Titlebar menus and titles, the top row's labels and clock | SF Pro Text |
| `font-mono` | `fonts::mono()` | Regular | Code, JSON, Terminal, Projects | Iosevka Term Slab |

### How a family is chosen

Sola bundles no fonts. `app::startup` loads every family fontconfig knows
into Iced's font database, once per process. A family named in the theme is
used only if it is in that database. When a role's token is missing, empty,
or names a family that is not installed, the role falls back:

- UI roles: SF Pro Text if installed, otherwise Inter, otherwise Noto Sans.
- Mono: Iosevka Term Slab if installed, otherwise JetBrains Mono,
  otherwise Noto Sans Mono.

[Fonts](distribution.md) lists the families to install.

Each process keeps the five roles in one table. `fonts::install(table)`
replaces it, and the next frame draws with the new families; every `Theme`
message does this. Terminal and Projects also measure the new mono font's
cell size and reflow their panes.

## Shell tokens

The shell's chrome has its own tokens in the same palette, all in the group
`shell`. Only the shell applies them, and it draws with four of them.

| Token | Default | Controls |
|---|---|---|
| `shell-flower` | `#ffb800` | The flower at the left of the top row. A picture's scheme (not a grey picture's) and your Accent set it to the accent |
| `shell-backdrop-dim` | `#00000099` | Dimming behind the launcher |
| `shell-launcher-width` | `560px` | Launcher width |
| `shell-launcher-pad` | `8px` | Row padding in the launcher and the shortcuts overlay |

Eight more are editable in Kit and saved with its themes, but nothing
draws with them:
`shell-menubar-bg` (`#050505`; the top row is transparent), and the
switcher's `shell-switcher-bg` (`#151515e6`), `shell-switcher-border`,
`shell-switcher-icon-bg`, `shell-switcher-icon-fg`,
`shell-switcher-icon-fg-sel`, `shell-switcher-pad` and
`shell-switcher-tile-pad`. The switcher they styled is gone.

Colours may carry alpha (`#rrggbbaa`). Spacing values are a number followed
by `px`. The shell reads these tokens into its `ShellStyle` on every `Theme`
message. The [Shell](sola-shell.md) chapter describes the top row and the
launcher.

## How an application applies the theme

On every `Theme` message, a kit application calls
`sola_kit::apply_theme_update(&message, &mut theme)`, which rebuilds its
Iced theme from the palette tokens (`theme::theme_from_bus`), installs the
font roles (`fonts::install`), installs the colours that have no Iced slot
(`selection`, `highlight`, and the chrome surface drawn from the base and
raised fills), and returns true when the message was a `Theme`. The
application returns its stored theme from Iced's `theme` callback, so the
next frame uses it.

Before a theme arrives (between start and the bus replay, or while the bus
is down), an application draws with `sola_kit::default_theme()`, the
compile-time defaults in the tables above, with font roles resolved from
the default families.

The compositor follows the theme too: the rims it draws around windows of
other programs come from `highlight` and `bg-primary`
([Rims and rows](#rims-and-rows)).

### Fallbacks

| Missing or malformed | Result |
|---|---|
| No theme on the bus | Compile-time defaults, as before a theme arrives |
| A palette token, or a colour not in `#rrggbb` or `#rrggbbaa` form | That token's default |
| A font token, or a family that is not installed | The role's fallback family |
| A shell token, or a spacing value not in `<number>px` form | That token's default |
| A `theme/current.yaml` that does not match the theme schema | The bus restores no theme, and the shell publishes none because the file exists; applications draw the compile-time defaults until a theme is published from Kit (by choosing or editing a preset); until then Settings → Appearance changes no colours |

## Appearance settings

Settings' **Appearance** page holds, top to bottom:

| Section | Settings |
|---|---|
| Light or dark | **Mode**: **Dark** (at first) or **Light**: what web pages are told you prefer ([Browser](sola-browser.md#light-or-dark)). Sola's own windows stay dark |
| Background picture | The picture's file name and **Clear**, or "None — the black ground." A picture is set from Paint (File → Set as Background) or `solactl background` |
| Colors | **Adapt colors to the background**, **Strength**, and a card for each look |
| Your colors | **Window**, **Panels**, **Highlight**, **Accent** |
| Desk | **Opacity** of the windows, the bar and the top row over the background: 50–100%, 90% at first |
| Idle | **Fade the windows when idle**, **Start after**, **Fade over**, **Then dim the background**, **Dim after**, **Dim over** |

The settings travel as the persistent topic `Appearance`, saved in
`~/.config/sola/state.yaml`. The Shell acts on the colours, rewriting
`Theme`; the compositor acts on the opacity, the idle fade and the dim
([Shell § The idle fade](sola-shell.md#the-idle-fade) has their defaults
and ranges; [Compositor](compositor.md) how they are drawn); the browser
acts on light or dark. There is no light theme for Sola itself: every look
is dark, and **Light** changes only what web pages are told.

## Colors from the background picture

**Adapt colors to the background** makes the theme's colours follow the
background picture. The Shell samples the picture and publishes `Theme`
with new colours, so every application changes at once:

- The surfaces (window fills, titlebars, the bar and other sidebars, the
  top row's islands, borders, the selection and the highlight) take a
  tint of one of the picture's colours; the text a fainter one. Each keeps
  the lightness of its grey, so contrast does not change. The darkest
  surfaces take a little less of the tint.
- The accent takes another of the picture's colours, at least 60° of hue
  from the tint where the picture has one (otherwise the tint's own hue),
  as light as the default accent and as vivid as the picture's own colour
  there allows.
- The flower in the top row takes the accent too.
- Status colours, fonts, sizes and spacing do not change.

The picture offers a **scheme** for each colour it is made of: up to four,
the best first, as far apart in hue as the picture allows. The colours
are the picture's own (the ones many of its pixels share, preferring vivid
ones), not averages over it, so a dark picture still gives colourful
schemes. Each scheme is named by its hue: Rose, Ember, Amber, Gold, Olive,
Green, Teal, Sky, Blue, Violet or Magenta. A picture with no colour of
its own offers one scheme, **Grey**: the greys and the amber flower stay,
and the accent is a cyan close to the default.

When the picture offers more than one scheme, each shows as a card under
**Strength**: a small window in its colours, with the picture's colour
beside its name. Click one to use it; the one in use is ringed in the
accent.

**Strength** (0–100%, 60% at first) is how much of the picture colour's
own colourfulness the surfaces take, up to a fixed limit; at 0% they stay
grey. Every window follows as you drag the slider.

Turning **Adapt colors to the background** off, or clearing the picture,
puts the greys back (with your colours over them, if a look of yours is in
use).

### Your colors and your looks

**Your colors** go over the greys or over the picture's scheme, adapting
on or off. Click a swatch for a picker, then **Use** (or **Cancel**).

| Color | Sets |
|---|---|
| Window | `bg-primary` |
| Panels | `bg-secondary` (bars, titlebars, sidebars), and `shell-switcher-bg` at 90% opacity |
| Highlight | `highlight` (the bar's focused row); `selection` takes its hue at its own lightness |
| Accent | `accent`, `text-accent`, `accent-dim` at 10%, and the flower (`shell-flower`) |

Changing a colour while a scheme (or the greys) is in use makes a **look**
of your own from it, named **Custom** (then Custom 2, …), shown as a card
beside the schemes, and leaves the scheme as it was: click the scheme's
card to go back to it, and the Custom card to come back to yours. While
one of your looks is in use, changes go to it. **Reset** beside a colour
gives that colour back to the look's scheme, and **Remove Custom** (in the
section's heading) forgets the look; its scheme stays in use. You keep up
to six looks; with six, remove one to make another.

### What the Shell rewrites

While the Shell manages the colours (adapting with a picture, or a look of
yours in use), it makes them from scratch on every change: the seed greys,
then the picture's scheme over them, then your colours over that. It
rewrites these tokens: `bg-primary`, `bg-secondary`, `bg-tertiary`,
`bg-hover`, `selection`, `highlight`, `border`, `border-subtle`,
`text-primary`, `text-secondary`, `text-tertiary`, `text-muted`,
`accent`, `text-accent`, `accent-dim`, `shell-menubar-bg`,
`shell-switcher-bg` and `shell-flower`. An edit to one of them from Kit is
replaced at once; Kit's edits to the rest (status colours, fonts, the
other shell tokens) stay. With adapting off and no look of yours in use,
the Shell leaves the theme alone.

### Rims and rows

Hover and shown rows in a sidebar are lifts of the column that keep its
hue, so they follow a tinted or custom panel colour instead of turning
grey. A focused window's rim is the highlight a step lighter, so it takes
the scheme's or your highlight's hue (`#3d3d3d` on the grey theme). An
unfocused rim is opaque: the window's face 7% lighter in its hue. The
compositor draws the same rims around windows of other programs.

## Editing the theme in Kit

Kit edits every token the toolkit reads, the fonts, and the shell tokens.
Settings does not edit the fonts.

Kit's header holds the theme menu, **New Theme**, and **Delete**. The
presets are **Default**, the built-in values, which is read-only (edits are
ignored and **Delete** is disabled while it is selected), and your own,
each a `CustomTheme` record in `~/.config/sola/theme/presets/<name>.yaml`.

1. Choose **New Theme**, type a name of lowercase letters joined by single
   hyphens (such as `solar-flare`), and press **Save**. Kit copies the
   active preset under that name, saves it, and selects it.
2. On the **Theme** page, pick a family for each font role (from the
   families fontconfig reported when Kit started) and click a swatch to edit
   one of the twelve colours; **reset** under a changed swatch restores its
   default. On the **Shell** page, edit the eight shell colours (the picker
   has an alpha rail) and the four spacing values: switcher padding 0 to
   64 px, tile padding 0 to 48 px, launcher width 320 to 1280 px, launcher
   row padding 0 to 32 px.
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
live, or while the Shell manages the colours) it shows Default while every
application keeps drawing the live theme.

A theme Kit publishes is its preset's values over the seed palette: the
tokens no application reads go back to their seed values.

## Following the theme in a new application

1. Call `sola_kit::app::startup(APP_ID)` first; it loads the system fonts.
2. Include `TopicKind::Theme` in the `BusSetup` subscription.
3. Keep an `iced::Theme` in the application state, start it as
   `sola_kit::default_theme()`, and return it from Iced's `theme` callback.
   A kit window returns `sola_kit::theme_for(chrome, &theme)` (its
   `FloatState::chrome`), which clears the window fill so the frame's
   rounded corners show what is under it, except in Cinema.
4. Map `sola_kit::app::bus_subscription()` into the application's
   subscription and pass each message to `sola_kit::apply_theme_update`.
5. Draw with kit components. In custom styles, take colours from
   `theme.extended_palette()`, `sola_kit::theme::selection()`,
   `sola_kit::theme::highlight()` and
   `sola_kit::components::style::chrome_surface()`, and fonts from the
   `sola_kit::fonts` role functions. A colour or family written into the
   application's code does not follow the theme.

[The bus](bus.md#using-the-bus-from-rust) has a code sketch of this
wiring.

## Limits

- The theme controls twelve colours, five font roles, and the four shell
  values the shell draws with. Type sizes, spacing, and radii are fixed.
- There is no light theme; Iced treats every theme as dark, whatever its
  colours.
- The font database is loaded once per process. A family installed while an
  application runs is not available to it until it restarts.
- Iced's default font, used by widgets that set no font, is chosen when an
  application starts, before the theme arrives: the default UI family in
  most applications, Inter in Kit and the shell, the default mono family in
  Terminal and Projects. It does not follow `font-ui`. Text that a
  component draws with a role does follow the theme.
- While the Shell manages the colours, Kit cannot change the colour tokens
  it rewrites.
- There is no command to change one token. `solactl emit Theme` can
  publish a whole theme as JSON.
