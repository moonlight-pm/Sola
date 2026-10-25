---
name: Sola
description: Graphite, as built. Windows with their own menus on a 10 px gap grid, islands across the top, a grey base that can take its colors from the background picture.
colors:
  field-well: "#050505"
  bg-primary: "#0b0b0b"
  bg-chrome: "#141414"
  bg-secondary: "#151515"
  island: "#1a1a1a"
  bg-hover: "#1f1f1f"
  bg-tertiary: "#1f1f1f"
  selection: "#292929"
  highlight: "#353535"
  border: "#2a2a2a"
  border-subtle: "#1f1f1f"
  hairline: "#252525"
  hairline-strong: "#313131"
  field-edge: "#232323"
  rim-focus: "#3d3d3d"
  rim-rest: "#1c1c1c"
  island-rim: "#353535"
  text-primary: "#ececec"
  text-ghost: "#b3b3b3"
  text-secondary: "#949494"
  text-tertiary: "#737373"
  text-muted: "#2a2a2a"
  accent: "#3dd6f5"
  on-accent: "#041018"
  success: "#3ecf8e"
  warning: "#e8b84a"
  danger: "#f07178"
  on-danger: "#1a0608"
  close-dot: "#ff5f57"
  shell-backdrop-dim: "#00000099"
  shell-switcher-bg: "#151515e6"
  shell-switcher-border: "#ffffff1a"
  shell-switcher-icon-bg: "#ffffff2e"
  term-cursor: "#ffb800"
  sol: "#ffb800"
  enamel-teal: "#61adb8"
  enamel-iris: "#7a8ac7"
  enamel-clay: "#b8856b"
  enamel-sage: "#75a380"
  enamel-dusk: "#ad7a94"
  enamel-olive: "#949e66"
  enamel-ink: "#6685b3"
  enamel-brass: "#bd9e66"
  phosphor-1: "#38fa6b"
  phosphor-2: "#1febe0"
  phosphor-3: "#6185ff"
  phosphor-4: "#c752ff"
  phosphor-5: "#ff942e"
typography:
  heading:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "22px"
    fontWeight: 500
  subheading:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "15px"
    fontWeight: 500
  prose:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "14px"
    fontWeight: 400
  body:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  small:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  title:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "12.5px"
    fontWeight: 400
  caption:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "11px"
    fontWeight: 400
  caps:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "10px"
    fontWeight: 500
    letterSpacing: "1px"
  code:
    fontFamily: "Iosevka Term Slab, JetBrains Mono, monospace"
    fontSize: "12px"
    fontWeight: 400
rounded:
  row: "4px"
  sm: "5px"
  md: "7px"
  lg: "10px"
  window: "12px"
  xl: "14px"
  island: "14px"
  pill: "999px"
spacing:
  space-2xs: "2px"
  space-xs: "4px"
  space-sm: "8px"
  space-md: "12px"
  space-lg: "16px"
  space-xl: "20px"
  space-xxl: "24px"
  gap: "10px"
  island-height: "28px"
  top: "48px"
  tab-bar-width: "240px"
  titlebar-height: "38px"
  sidebar-width: "220px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 14px"
  button-secondary:
    backgroundColor: "#1e1e1e"
    borderColor: "#393939"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 14px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text-ghost}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "7px 14px"
  button-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.on-danger}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 14px"
  button-danger-outline:
    backgroundColor: "#2b1e1f"
    borderColor: "#713c3f"
    textColor: "#f28288"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 14px"
  button-small:
    typography: "{typography.small}"
    padding: "5px 11px"
  field:
    backgroundColor: "{colors.field-well}"
    borderColor: "{colors.field-edge}"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 12px"
  titlebar:
    backgroundColor: "{colors.bg-secondary}"
    typography: "{typography.body}"
    height: "38px"
    padding: "0 10px 0 13px"
  titlebar-menu:
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    height: "24px"
    padding: "0 8px"
  dropdown:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.window}"
    padding: "5px"
    minWidth: "200px"
  dropdown-row:
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    height: "26px"
    padding: "0 10px"
  island:
    backgroundColor: "{colors.island}"
    typography: "{typography.body}"
    rounded: "{rounded.island}"
    height: "28px"
    padding: "0 12px"
  tab-bar:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.window}"
    width: "240px"
  tab-bar-head:
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    height: "28px"
  tab-bar-row:
    typography: "{typography.body}"
    rounded: "{rounded.row}"
    height: "32px"
    padding: "6px 10px"
  tab-bar-row-focused:
    backgroundColor: "{colors.highlight}"
    textColor: "{colors.text-primary}"
  menu:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.md}"
    padding: "4px"
    width: "240px"
  menu-row:
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "3px 10px"
  launcher:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.xl}"
    width: "560px"
  switcher:
    backgroundColor: "{colors.shell-switcher-bg}"
    rounded: "{rounded.xl}"
    padding: "14px 22px"
  notification:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.md}"
    width: "340px"
    height: "76px"
  agent-card:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.md}"
    padding: "12px"
    width: "440px"
  settings-group:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.lg}"
    padding: "4px"
  settings-row:
    typography: "{typography.body}"
    height: "32px"
    padding: "2px 8px 2px 10px"
  badge:
    typography: "{typography.caps}"
    rounded: "{rounded.pill}"
    padding: "3px 8px"
  count-mark:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.caps}"
    rounded: "{rounded.pill}"
    height: "18px"
---

# Design System: Sola

## Overview

**Graphite, as built.** Sola is a desktop for one person who keeps
terminals, agents, mail and a browser open at once. Every window carries its
own menus in a 38 px titlebar and sits one 10 px gap from its neighbors and
the edges. The top row is a handful of 28 px islands on the ground, each the
thing you click. Under everything is the ground: black, or a background
picture whose colors the theme can take on. The base is neutral grey; the
cyan accent is sparse; the flower is the one warm mark. "Graphite" stays the
name of the style.

The source of truth is the sola-design canvas
[`design/foundation/`](design/foundation/) (open it in Design): a cover,
then 01 Color, 02 Type, 03 Space, shape, edge, depth, 04 Controls, 05 The
window, 06 The top row, 07 The tab bar and workspaces, 08 The desk, 09
Surfaces, 10 Settings and the picture, and 11 Open. The boards draw with
shared parts in `design/foundation/foundation.css` and `foundation.js`, so
every board shows the same desk. 01 to 10 are what is built on `next`
(2026-10-24). 11 lists what is decided but not built, and where the build
disagrees with itself.

Every color, size and radius is a token in
[`design/tokens.css`](design/tokens.css), shared by every canvas in
`design/`. Color names are the bus palette keys in `sola-core`
`Palette::seed`. Values the kit derives in code (hairlines, the field well,
rims, the island fill) are tokens too, computed with the kit's own math.
Values printed on the boards are read live from the CSS, so they follow
`tokens.css`. The tokens name SF Pro Text and Iosevka Term Slab, with the
kit's fallbacks (Inter, JetBrains Mono).

[`docs/manual/design-language.md`](docs/manual/design-language.md) remains
the craft reference (macOS dark density, what iced can draw).

## Rules

- **Menus ride the window.** Every kit window has a 38 px titlebar: the
  close dot, the app's menus (its name first, in semibold), the title at the
  right in tertiary ink. There is no global menubar.
- **One gap.** 10 px at the output's edges, between windows, and above and
  below the islands. Tiles and the tab bar start at y 48.
- **Islands, not a bar.** The top row is pills on the ground with nothing
  behind them: the flower and agents at the left, a toast in the center, an
  island per instrument group at the right.
- **One tab list.** The tab bar lists the showing workspace's windows by
  zone. Its head is the workspace switcher.
- **Focus is the window.** The focused window's rim is the highlight a step
  lighter, its titlebar a step brighter, its close dot red. The rest dim
  their menus to 55% and stay where they are.
- **Sparse accent.** Cyan marks focus in fields, the one primary action,
  live work and unread counts. It is never a region, a heading, or
  decoration. A focused row is the grey highlight, not cyan.
- **Shape first.** Working is a ring, waiting a diamond, done a check. A row
  reads without color.
- **Only floats cast.** Floats, popovers, menus and the launcher have
  shadows. Tiles, the tab bar and the islands are flat with rims.
- **Pixel meters.** The top row's meters are 2 px LEDs on a 3 px pitch.
  Numbers live in the panels.
- **The picture can set the colors.** With Adapt on, the theme takes the
  picture's modal colors as schemes; your own colors sit on top.

## Colors

### The ladder

| Token | Hex | Role |
|-------|-----|------|
| `field-well` | `#050505` | Text field well (canvas × 0.45) |
| `bg-primary` | `#0b0b0b` | Window face |
| `bg-chrome` | `#141414` | Sidebar columns, Projects' top bar, a browser's toolbar. Not a token: base → raised at 90% |
| `bg-secondary` | `#151515` | Raised: titlebars, dropdowns, popovers, the tab bar |
| `island` | `#1a1a1a` | An island at rest: raised ↔ hover midpoint |
| `bg-hover` | `#1f1f1f` | Hover; an island whose panel is open |
| `selection` | `#292929` | Text selection, a chosen row in a select |
| `highlight` | `#353535` | A column's active row: the tab bar's focused window, Settings' page |
| `border` | `#2a2a2a` | Hard edges |
| `hairline` | `#252525` | White 7% over raised. Rules, popover edges |
| `hairline-strong` | `#313131` | White 12% over raised. Secondary buttons |
| `field-edge` | `#232323` | White 12% over the well |

Edges are opaque mixes, not translucent white: iced blends in linear light
and a translucent 1 px white reads about three times brighter.

### Rims

| Token | Hex | Role |
|-------|-----|------|
| `rim-focus` | `#3d3d3d` | The focused window: `highlight` lifted 4% in its hue |
| `rim-rest` | `#1c1c1c` | Other windows: the face lifted 7% in its hue. Opaque, so the picture does not show through the edge |
| `island-rim` | `#353535` | An island's 1 px top edge: its fill 12% whiter |

The tab bar's rim is its fill lifted 12% (`#313131`). sola-river draws the
same rims on other applications' windows.

### Text

| Token | Hex | On raised | Role |
|-------|-----|-----------|------|
| `text-primary` | `#ececec` | 15.5:1 | Labels, values, titles |
| `text-ghost` | `#b3b3b3` | 8.7:1 | Ghost buttons at rest |
| `text-secondary` | `#949494` | 6.0:1 | Secondary labels, placeholders |
| `text-tertiary` | `#737373` | 3.9:1 | The seed's muted text. The kit's muted text falls back to `#949494`, which a fresh desk writes (see 11 Open) |
| `accent` | `#3dd6f5` | 10.6:1 | Unread counts, live marks |

In a titlebar the unfocused menus are ink at 55% and the title at 38%.

### Signal and status

| Token | Hex | Use | Soft fill, edge |
|-------|-----|-----|-----------------|
| `accent` | `#3dd6f5` | Field focus, the primary button, working, RX, unread | 14%, 28% |
| `success` | `#3ecf8e` | Done, connected, TX | 14%, 28% |
| `warning` | `#e8b84a` | Waiting on you | 14%, 28% |
| `danger` | `#f07178` | Delete, failed, invalid | 14%, 28% |

Label on accent is `on-accent` `#041018` (11.1:1); a dark accent you pick
takes a white label. Label on danger is `on-danger` `#1a0608`. The close dot
is `#ff5f57`, lit only on the focused window. The meters and the stat panels
turn `warning` past 75% and `danger` past 90%.

### The picture's colors

With Settings → Appearance → Adapt on, the shell samples the background
picture and finds its modal colors (narrow OKLCH cells, scored by share and
chroma). It offers up to four schemes, each named for its hue (Ember, Blue,
Amber, Rose, …), at least 30° apart. In a scheme the accent sits at least
60° from the tint, around L 0.82. Surfaces keep their grey's lightness and
take the tint's chroma, at most 0.08 × Strength (60 by default). Text takes
30% of the tint. Status colors stay as they are. Colors apply in order:
greys, then the scheme, then your colors. Your colors (Window, Panels,
Highlight, Accent) are kept as looks beside the schemes, up to six.

### Shell materials

`shell-backdrop-dim` `#00000099` is the dim under the launcher.
`shell-switcher-bg` `#151515e6` is the switcher's backplate, with a 1 px
`shell-switcher-border` `#ffffff1a`; `shell-switcher-icon-bg` `#ffffff2e`
is the plate under the selected icon. `shell-menubar-bg` is still seeded,
but the top row's window is transparent and does not read it.

### Enamel

Eight hues pulled toward graphite: teal, iris, clay, sage, dusk, olive, ink,
brass. A profile, theme or project gets one plate, hashed from a stable seed
(`select::enamel`). A plate names a thing. It never reports a state.

### The two exceptions

- **Sol** `#ffb800`: the flower and the terminal cursor. Never status, never
  a fill, never text.
- **Phosphor**: the audio spectrum's five LED rows, green at the bottom to
  orange at the top. These hues appear nowhere else.

### Terminal

Background, foreground, red, green, yellow and blue come from `bg-primary`,
`text-primary`, `danger`, `success`, `warning` and `accent`, and follow a
scheme change. The cursor is `term-cursor`. Selection is `accent` at 55%.
Magenta `#aa33aa` and cyan `#33aaaa` are literals (P9).

## Typography

**UI face:** SF Pro Text (fallback Inter, then Noto Sans). **Mono face:**
Iosevka Term Slab (fallback JetBrains Mono). Components never name a family;
they call a role (`ui`, `ui_medium`, `display`, `chrome`, `mono`), and the
bus theme swaps the family behind it. `ui_medium` and `display` are SF Pro
Text Medium.

| Role | Spec | Kit | Use |
|------|------|-----|-----|
| Heading | 22 / 500 | `text::heading` | Panel and dialog titles, Settings pages |
| Subheading | 15 / 500 | `text::subheading` | Card and section titles |
| Prose | 14 / 400 | `text::prose` | Mail bodies |
| Body | 13 / 400 | `text::body`, `fonts::chrome()` | Buttons, rows, menus, titlebar menus, the top row |
| Small | 12 / 400 | `labeled_sm` | Small buttons, shortcuts, Projects' tab chips |
| Window title | 12.5 / 400 | titlebar | The title at a titlebar's right |
| Caption | 11 / 400 | `text::caption` | Sources, timestamps, Settings section heads |
| Caps | 10 / 500 / +1 caps | `ui_medium` | Sidebar section caps, badges |
| Code | 12 mono | `text::code` | Paths, ids, JSON |

A titlebar's app name (its first menu) is semibold. An agent island's name
is medium. Stat labels in the top row are text at 62%. The launcher's query
is 16 and its rows 14.

## Layout and density

Space steps by four, with a two for nudges: 2, 4, 8, 12, 16, 20, 24. The
kit's `SPACE_*` constants run one name behind the bus tokens (`SPACE_SM` is
4, `space-xs` is 4).

| Measure | Value | Where |
|---------|-------|-------|
| Gap | 10 | Edges, between tiles, above and below the islands |
| Top | 48 | Gap, a 28 px island, gap. Tiles and the tab bar start here |
| Tab bar | 240 | At x 10; the content area starts at x 260 |
| Halves | (content − 10) / 2 | Left and Right; Full is the whole content area |
| Titlebar | 38 | Close 12 at x 13, 9 to the first menu; menu pills 24 tall, pad 8, 1 apart |
| Island | 28 | Pad 12 each end, items 12 apart, islands 6 apart |
| `PAD_CONTROL` | 7 × 14 | Buttons |
| `PAD_CONTROL_SM` | 5 × 11 | Small and toolbar buttons |
| Field | 7 × 12 | Text inputs |
| Tab bar row | 6 × 10, 32 tall | 16 px leading slot (icon or status mark) |
| Dropdown row | 26 tall, pad 10 | A titlebar menu's dropdown |
| Menu row | 3 × 10 | Shell menus (flower, workspace) |
| Settings row | 32 tall | Label left, control right; groups pad 4 |

The launcher is 560 wide. The switcher cell is 96 with a 72 icon. Desk
cards are 340 × 76, 12 from the right edge, 6 under the top row, at most
three. The agent card is 440 wide. A kit app's own sidebar is 220.

## Elevation and depth

| Level | Shadow | Where |
|-------|--------|-------|
| Flat | none | Tiles, the tab bar, islands. Rims only |
| Popover | y 2, blur 10, black 28% | Menus, dropdowns, island panels, desk cards, the agent card |
| Floating window | y 3, blur 11, black 28% | Drawn by sola-river under the surface |
| Card | y 4, blur 16, black 28% | Kit cards |
| Modal | y 8, blur 28, black 38% | Launcher, dialogs |
| Primary glow | accent 35%, y 4, blur 14 | The one filled primary |

Islands carry a sheen: 4% whiter at the top, down to the fill at 55% (8% while
open). Cards and the launcher carry a 1 px bevel, dark at the top left to
light at the bottom right.

## Shapes

4 for tab bar rows, 5 for ghost buttons and menu rows, 7 for buttons,
fields, menu pills, popovers and dropdown rows, 10 for Settings groups and
the switcher's selected plate, 12 for windows, the tab bar and dropdown
sheets, 14 for cards, the launcher, the switcher and the islands (a full
pill). Badges and count marks are pills. A window's corners are cleared, so
the ground shows there.

## Components

- **Buttons.** Primary is an accent fill with a dark label and a soft glow,
  one per group. Secondary is a quiet fill (`#1e1e1e`) with a strong
  hairline (`#393939`). Ghost rests at a muted label and lifts to grey, never
  cyan. Danger outline arms a destructive action and Danger confirms it.
  Disabled halves the alpha.
- **Fields.** An inset well darker than the face, a 12% edge, and an edge of
  accent 55% into the well on focus. Errors put a danger edge on the field
  and an 11 px danger caption under it.
- **Choices.** Toggle on is an accent track with a dark knob. Checkbox on is
  accent with a dark check.
- **Badges.** A soft tint with a matching edge. The count mark is the one
  filled cyan chip.
- **Titlebar.** 38 px on raised (focused) or the face/raised midpoint
  (not). The close dot, the menus as 24 px pills (open: ink at 14%), the
  title at the right. Menus that do not fit are left off; the title is cut,
  or dropped under 32 px of room.
- **Dropdown.** Hangs under its menu inside the window: raised, a 10% edge,
  12 px corners, 5 px pad, 26 px rows; hover is accent at 14%.
- **Tab bar row.** Focused: the highlight pill. Shown: a 5% lift in the
  column's hue. Shown while its zone is off screen: full ink, no lift.
  Behind: ink at 70%. Hover: a 7% lift and a red stoplight that closes the
  window. A working or waiting tab shows its mark in the icon's place.
- **Island.** 28 px pill, fill between raised and hover, a sheen, a 1 px top
  rim. Open: the hover grey.
- **Settings group.** Sections of grouped rows (raised gradient, 7% edge,
  10 px corners, 1 px rules between rows). An opening row reveals its editor
  in place under it.
- **Menus.** Shell menus are 240 wide, 4 px inset, popover chrome, grey
  hover. Shortcuts at 55%.

## Shell

- **The top row.** Islands on the ground. Left: the flower (system menu),
  then an island per Projects session that asks or has finished (mark,
  name, what it wants; +N past half the row). Center: a toast while one
  shows. Right: extras (mail and missed counts in the accent, the mouse's
  battery, Bluetooth), the 12-band spectrum, CPU · GPU · MEM, RX · TX, the
  clock. Panels hang from their island.
- **The tab bar.** A 240 px island at the left. Its head is the showing
  workspace's name and menu (switch, New, Rename…, Move, Delete; attention
  marks beside the name). Sections Full, Left, Right, Floating, a row per
  window.
- **Windows.** Every kit window has the titlebar. Zones are Full or Left and
  Right halves; Full and the halves take turns on screen. Floats sit over
  the tiles with the only shadow. Other applications' tiles get the same
  corners and rim from sola-river; their floats stay square. Cinema is
  bare.
- **Workspaces.** Each is its own desk, named, with its own browser profile.
  Super+1…9 switches, Super+Ctrl+1…9 sends the window.
- **Launcher.** Super+Space. A modal card over the dim, a 16 px query, then
  rows. The selected row is the hover grey.
- **Switcher.** Super+Tab. A glass backplate, no dim. The selected icon sits
  on a white 18% plate with its name under it. Count marks ride the icon.
- **The picture and the fade.** sola-river draws the picture under
  everything (fill, center crop); without one the ground is black. When
  idle (5 minutes by default) the picture rises over the desk over 30
  minutes; the flower and the agent islands stay in full sight; any
  input brings the desk back over a second.

## Do's and Don'ts

### Do

- **Do** take every color from a bus token or a named kit derivation.
- **Do** put an app's menus on its window's titlebar.
- **Do** keep cyan to field focus, one primary, and live state.
- **Do** write state as a shape before a color.
- **Do** keep meters as pixels and numbers in the panels.
- **Do** keep edges opaque mixes on their surface.

### Don't

- **Don't** fill a region, a heading, or a focused row with cyan.
- **Don't** add a shadow to tiles, the tab bar or the islands.
- **Don't** use enamel or phosphor to report state.
- **Don't** add a global menu or a band behind the top row.
- **Don't** add a hex literal in a view when a token exists. Add a token.

## Open

Board 11 Open lists what Next decided and the build has not reached (the
zone glyph, menus folding into ···, the live center, the launcher grown
from the flower, a workspace overview, foreign titlebars), where the build
disagrees with itself, and Graphite v1's ten proposals:

| # | Proposal | Status |
|---|----------|--------|
| P1 | Sol gold flower | Done |
| P2 | SF Pro Display at 20 px and up | Open |
| P3 | Type tokens the kit reads | Open |
| P4 | A focus halo on fields | Open |
| P5 | Tertiary text at 4.5:1 | Open |
| P6 | Meters and stat panels in kit hues | Open |
| P7 | One scrim | Superseded (the rail it was about is gone) |
| P8 | Selection marquee without `#00d4ff` | Done |
| P9 | Terminal magenta and cyan from enamel | Open |
| P10 | Calendar's fallback color from enamel | Open |
