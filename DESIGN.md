---
name: Sola
description: Instruments in graphite. Quiet chrome that reads at a glance, one cyan signal, and a single warm mark.
colors:
  field-well: "#050608"
  bg-primary: "#0c0e12"
  bg-chrome: "#121722"
  bg-secondary: "#151922"
  bg-hover: "#1e2533"
  selection: "#2c333e"
  border: "#2a3344"
  hairline: "#252931"
  hairline-strong: "#31353d"
  text-primary: "#e9ecf2"
  text-ghost: "#acb3c2"
  text-secondary: "#8b94a8"
  text-tertiary: "#6b7385"
  accent: "#3dd6f5"
  on-accent: "#041018"
  success: "#3ecf8e"
  warning: "#e8b84a"
  danger: "#f07178"
  on-danger: "#1a0608"
  shell-menubar-bg: "#050608"
  shell-backdrop-dim: "#00000099"
  shell-switcher-bg: "#151922e6"
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
  display:
    fontFamily: "SF Pro Display, Inter, sans-serif"
    fontSize: "32px"
    fontWeight: 600
    letterSpacing: "-0.5px"
  heading:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "22px"
    fontWeight: 500
  title:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "15px"
    fontWeight: 500
  prose:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.55
  body:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  small:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  caption:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "11px"
    fontWeight: 400
  badge:
    fontFamily: "SF Pro Text, Inter, sans-serif"
    fontSize: "10px"
    fontWeight: 500
    letterSpacing: "0.8px"
  code:
    fontFamily: "Iosevka Term Slab, JetBrains Mono, monospace"
    fontSize: "12px"
    fontWeight: 400
  terminal:
    fontFamily: "Iosevka Term Slab, JetBrains Mono, monospace"
    fontSize: "15px"
    fontWeight: 400
rounded:
  xs: "4px"
  sm: "5px"
  md: "7px"
  lg: "10px"
  xl: "14px"
  pill: "999px"
spacing:
  space-2xs: "2px"
  space-xs: "4px"
  space-sm: "8px"
  space-md: "12px"
  space-lg: "16px"
  space-xl: "20px"
  space-xxl: "24px"
  menubar-height: "28px"
  rail-width: "240px"
  sidebar-width: "220px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 14px"
  button-secondary:
    backgroundColor: "#1e222b"
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
    backgroundColor: "#2b222b"
    textColor: "#f28288"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 14px"
  button-small:
    typography: "{typography.small}"
    padding: "5px 11px"
  field:
    backgroundColor: "{colors.field-well}"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "7px 12px"
  badge:
    typography: "{typography.badge}"
    rounded: "{rounded.pill}"
    padding: "3px 8px"
  count-mark:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.badge}"
    rounded: "{rounded.pill}"
    height: "18px"
  rail-row:
    textColor: "#ffffffb3"
    typography: "{typography.body}"
    rounded: "{rounded.xs}"
    padding: "6px 10px"
    height: "32px"
  rail-row-selected:
    backgroundColor: "#333741"
    textColor: "#ffffff"
  menu:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.md}"
    padding: "4px"
    width: "240px"
  menu-row:
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "{rounded.sm}"
    padding: "3px 10px"
  card:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.xl}"
    padding: "18px"
  notification:
    backgroundColor: "{colors.bg-secondary}"
    rounded: "{rounded.md}"
    padding: "8px"
    width: "340px"
    height: "76px"
  menubar:
    backgroundColor: "{colors.shell-menubar-bg}"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    height: "28px"
  launcher:
    rounded: "{rounded.xl}"
    width: "560px"
  switcher-pill:
    backgroundColor: "{colors.shell-switcher-bg}"
    rounded: "{rounded.xl}"
    padding: "14px 22px 12px"
  titlebar:
    backgroundColor: "{colors.bg-secondary}"
    typography: "{typography.title}"
    height: "38px"
---

# Design System: Sola

## Overview

**Creative North Star: "Instruments in graphite"**

Sola is a desktop for one person who keeps terminals, agents, mail, and a
browser open at once on a 5120 × 2160 desk. The chrome is quiet graphite and
reads at a glance, like a panel of instruments at night. One cyan signal marks
what is focused, primary, or working. The only warm things are the mark and the
terminal cursor.

The source of truth is the sola-design canvas
[`design/foundation/`](design/foundation/) (open it in Design). **Components**
holds the sola-kit parts. **Graphite** is the sheet: a cover, then 01 Color,
02 Type, 03 Space, shape, edge, depth, 04 Controls, 05 Rail, menus, panels,
06 Shell, 07 The desk, and 08 Proposed, one artboard each. Every part is a
class in `design/foundation/components.css`, and the sheet uses those classes,
so a change on the Components board reaches every section. An element's inline
style holds only what that instance overrides.

Every color, size, radius, and space on the sheet is a token: a CSS custom
property in [`design/tokens.css`](design/tokens.css), shared by every canvas
in `design/`. Color names are the bus palette keys in `sola-core`
`Palette::seed`, so a theme preset edits exactly these. Values the kit derives
in code (`hairline`, `field-well`, `on-accent`, the button fills) are also
tokens, named for their role. The tokens set Inter for SF Pro Text and
JetBrains Mono for Iosevka Term Slab. Those are the kit's own fallbacks.

[`docs/manual/design-language.md`](docs/manual/design-language.md) remains
the craft reference (macOS dark density, what iced can draw). This file and the
sheet are the visual authority.

**Code status:** sections 01 to 07 draw the kit and shell as they are in the
code on 2026-10-12, except where a "Proposed" badge says otherwise. Section 08
lists ten proposals (P1 to P10). None of them is in the code yet.

## Rules

- **One signal.** Cyan (`accent`) marks focus, the one primary action in a
  group, and live work: the working ring, RX, unread. It is never a region, a
  heading, or decoration. Selection is graphite (`selection`), except in
  terminal and Projects grids, where it is cyan at 55%.
- **The ladder.** Depth is a step up the graphite ladder: well, canvas, chrome,
  raised, hover, selection. Edges are opaque white mixes at 7% (`hairline`)
  and 12% (`hairline-strong`), because iced inflates translucent white on a 1px
  border.
- **Only floats cast.** Popovers, cards, the launcher, and floating windows
  have shadows. The menubar, the rail, and zoned windows are flat.
- **Shape first.** Working is a ring, waiting a diamond, done a check, active
  a dot, idle a faint dot. A rail column reads without color.
- **Pixel meters.** Chrome meters are 2px LEDs on a 1px gutter. Numbers live in
  the dropdown, never in the bar.
- **Zones, no titles.** Zoned windows meet each other and the zone edge bare,
  with no gap. Floating windows keep their own size, wear the kit titlebar,
  and round all four corners at 14.

## Colors

### Graphite ladder

| Token | Hex | Role |
|-------|-----|------|
| `field-well` | `#050608` | Text field well (canvas × 0.45). Also the menubar fill. |
| `bg-primary` | `#0c0e12` | Window canvas |
| `bg-chrome` | `#121722` | Rail column and full-width chrome strip (`CHROME_SURFACE`) |
| `bg-secondary` | `#151922` | Raised: menus, cards, popovers, panels, titlebar |
| `bg-hover` | `#1e2533` | Hover, selected launcher row, toggle track off |
| `selection` | `#2c333e` | Text selection, chosen row in a select menu |
| `border` | `#2a3344` | Hard edges: checkbox off |
| `hairline` | `#252931` | White 7% into raised. Rules, menus, popovers. |
| `hairline-strong` | `#31353d` | White 12%. Secondary buttons, fields. |

`selection` is more neutral than the rest of the ladder. That is deliberate:
a darkened cyan went muddy.

### Text

| Token | Hex | On raised | Role |
|-------|-----|-----------|------|
| `text-primary` | `#e9ecf2` | 14.9:1 | Labels, values, titles |
| `text-ghost` | `#acb3c2` | 8.4:1 | Ghost buttons at rest |
| `text-secondary` | `#8b94a8` | 5.8:1 | Secondary labels, placeholders |
| `text-tertiary` | `#6b7385` | 3.7:1 | Caps and meta. Fails AA at 11px (see P5). |
| `accent` | `#3dd6f5` | 10.2:1 | Signal text: unread, live counts |

### Signal and status

| Token | Hex | Use | Soft fill, edge |
|-------|-----|-----|-----------------|
| `accent` | `#3dd6f5` | Focus, primary, working, RX, unread, count marks | 14%, 28% |
| `success` | `#3ecf8e` | Done, connected, TX | 14%, 28% |
| `warning` | `#e8b84a` | Waiting on you, meter warn | 14%, 28% |
| `danger` | `#f07178` | Delete, failed, invalid, meter crit | 14%, 28% |

Label on accent is `on-accent` `#041018` (11.1:1). Label on danger is
`on-danger` `#1a0608`.

### Shell materials

`shell-menubar-bg` `#050608` is opaque until the compositor can blur.
`shell-backdrop-dim` `#00000099` is the scrim under the launcher and every
modal. `shell-switcher-bg` `#151922e6` is the switcher pill with a 1px
`shell-switcher-border` `#ffffff1a`. `shell-switcher-icon-bg` `#ffffff2e` is
the plate under the selected icon.

### Enamel

Eight kiln hues pulled toward graphite: patina teal, slate iris, kiln clay,
oxidized sage, dusk enamel, olive brass, ink blue, warm brass. A profile,
theme, or project gets one plate, hashed from a stable seed
(`select::enamel`). A plate names a thing. It never reports a state.

### The two exceptions

- **Sol** `#ffb800`: the terminal cursor today, and the flower mark if P1 is
  taken. Never status, never a fill, never text.
- **Phosphor**: the audio spectrum's five LED rows, green base to amber peak.
  These hues appear nowhere else.

### Terminal

Background, foreground, red, green, yellow, and blue come from `bg-primary`,
`text-primary`, `danger`, `success`, `warning`, and `accent`. The cursor is
`term-cursor`. Selection is `accent` at 55%. Magenta `#aa33aa` and cyan
`#33aaaa` are literals (see P9).

## Typography

**UI face:** SF Pro Text (fallback Inter). **Mono face:** Iosevka Term Slab
(fallback JetBrains Mono). Components never name a family. They call a role
(`ui`, `ui_medium`, `display`, `chrome`, `mono`), and the bus theme swaps the
family behind it.

| Role | Spec | Kit | Use |
|------|------|-----|-----|
| Display | 32 / 600 / −0.5 | none yet (proposed) | Settings page titles, empty states, installer |
| Heading | 22 / 500 | `text::heading` | Panel and dialog titles |
| Title | 15 / 500 | `text::subheading` | Card titles, notification titles |
| Prose | 14 / 400 / 1.55 | `text::prose` | Mail bodies |
| Body | 13 / 400 | `text::body`, `fonts::chrome()` | Buttons, rows, menus, the menubar |
| Small | 12 / 400 | `labeled_sm` | Small buttons, shortcuts, notification body |
| Caption | 11 / 400 | `text::caption` | Sources, timestamps, field help |
| Badge | 10 / 500 / +0.8 caps | `ui_medium` | Badges, count marks, rail section caps |
| Code | 12 mono | `text::code` | Paths, ids, JSON |
| Terminal | 15 mono | terminal grid | Terminal and Projects panes |

The menubar is all 13px chrome at regular weight. The focused app name is the
only bold word in the bar. Stat labels are text at 62%.

## Layout and density

Space steps by four, with a two for nudges: 2, 4, 8, 12, 16, 20, 24. The
kit's `SPACE_*` constants run one name behind the bus tokens (`SPACE_SM` is 4,
`space-xs` is 4).

| Pad | Value | Use |
|-----|-------|-----|
| `PAD_CONTROL` | 7 × 14 | Buttons |
| `PAD_CONTROL_SM` | 5 × 11 | Small and toolbar buttons |
| Field | 7 × 12 | Text inputs |
| Rail row | 6 × 10, 32 tall | Rail rows; 12px status slot |
| Menu row | 3 × 10 | Dropdown rows; 14px check column |
| Launcher row | 8 × 12 | 20px icon, 14px label |

The menubar is 28px. Menu titles take 9px on each side, extras 5px, stats
6px, with 12px between phrases. The launcher is 560 wide. The switcher cell is
96 with a 72 icon. Desk cards are 340 wide, 12px from the right edge and 6px
under the bar, at most three.

## Elevation and depth

| Level | Shadow | Where |
|-------|--------|-------|
| Docked | none | Menubar, rail, zoned windows |
| Popover | y2, blur 10, black 28% | Menus, stat drops, desk cards |
| Floating window | y3, blur 11, black 28% | Drawn by River under the surface |
| Card | y4, blur 16, black 28% | Kit cards |
| Modal | y8, blur 28, black 38% | Launcher, dialogs |
| Primary glow | accent 35%, y4, blur 14 | The one filled primary |

Cards and the launcher also carry a 1px bevel, dark at the top left to light
at the bottom right, and a 4% top highlight in the fill.

## Shapes

Radius grows with how far a thing floats: 4 for rail rows, 5 for ghost
buttons and menu rows, 7 for buttons, fields, and menus, 10 for panels and
the switcher plate, 14 for cards, the launcher, the switcher pill, and
floating windows. Badges and count marks are pills. The open menubar title is
square so its fill meets the top of the bar.

## Components

- **Buttons.** Primary is a cyan fill with a dark label and a soft glow, one
  per group. Secondary is a quiet fill with a strong hairline. Ghost rests at a
  muted label and lifts to grey, never cyan. Danger outline arms a destructive
  action and Danger confirms it. Disabled halves the alpha of fill and label.
- **Fields.** An inset well darker than the canvas, a 12% hairline, and a
  cyan-mixed edge on focus. Errors put a danger edge on the field and an 11px
  danger caption under it.
- **Choices.** Toggle on is a cyan track with a dark knob; off is a hover
  track with a canvas knob. Checkbox on is cyan with a dark check.
- **Select.** A trigger with an enamel plate; the menu hangs below at the
  trigger's width; the chosen row is a `selection` wash with a check.
- **Badges.** A soft tint with a matching edge, never a solid slab. The count
  mark is the one filled cyan chip.
- **Rail.** `bg-chrome`, uppercase 10px section caps, 32px rows with a status
  slot. Rest text is white at 70%. Hover lifts the row 7% toward white
  (`#232731`); selected lifts it 14% (`#333741`) inside a 1px rim at 24%
  (`#4b4f57`).
- **Menus.** 240 wide, 4px inset, hairline edge, popover shadow. Shortcuts at
  55%. Separators are a hairline with 4px above and below.
- **Desk cards.** A popover with an icon well, an 11px source line, a 13px
  medium title, and a 12px body.
- **Titlebar.** 38px on `bg-secondary`, a 12px close dot at the left, the title
  centered at 13px medium.

## Shell

- **Menubar.** Flower, focused app (bold), app menus. On the right: extras,
  spectrum, CPU, GPU, MEM, RX, TX, clock. An open title fills with text at
  18%. A whisper toast is plain 13px text in the middle of the bar.
- **Launcher.** Super+Space. A modal card over the scrim, a 16px query, a
  hairline, then rows. The selected row is `bg-hover`.
- **Switcher.** Super+Tab. A neutral glass pill, no scrim. The selected icon
  sits on a white 18% plate with its name under it. Count marks ride the icon.

## Do's and Don'ts

### Do

- **Do** take every color from a bus token or a named kit derivation.
- **Do** keep cyan to focus, one primary, and live state.
- **Do** write state as a shape before a color.
- **Do** keep meters as pixels and numbers in the dropdown.
- **Do** update the Components board (its class in `components.css`) first;
  the sheet follows.

### Don't

- **Don't** fill a region, a heading, or a hover with cyan.
- **Don't** add a shadow to docked chrome.
- **Don't** use enamel or phosphor to report state.
- **Don't** put titles on zoned windows.
- **Don't** add a hex literal in a view when a token exists. Add a token.

## Proposed (section 08)

Each is a separate decision. The sheet already draws the proposed side.

| # | Proposal | Touches |
|---|----------|---------|
| P1 | Sol gold flower in the menubar (new token `sol` `#ffb800`) | `sola-shell` menubar view, `sola-install`, `sola-core` palette |
| P2 | `font-display` seeds SF Pro Display; headings ≥ 20px use it | `sola-core` palette, `sola-kit` `fonts.rs` |
| P3 | One type scale: seed the role scale in the bus `text-*` tokens and have `text.rs` read them; add Display 32 | `sola-core` palette, `sola-kit` `components/text.rs` |
| P4 | Focus halo on fields: accent 14%, blur 8, around the existing edge | `sola-kit` `text_input/mod.rs` |
| P5 | `text-tertiary` `#6b7385` → `#7d8597` (3.7:1 → 4.8:1 on raised) | `sola-core` palette |
| P6 | Meter warn/crit and stat drops use `warning`, `danger`, `accent`, `success`, and the ladder | `sola-shell` `stats/mod.rs`, `stats/view.rs` |
| P7 | Every modal veil reads `shell-backdrop-dim` | `sola-projects` `sidebar.rs`, `sola-browser`, `sola-arcade` |
| P8 | Capture marquee reads `accent`, not `#00d4ff` | `sola-shell` `selection/view.rs` |
| P9 | Terminal magenta and cyan from `enamel-dusk` and `enamel-teal` | `sola-terminal` `term_view.rs` |
| P10 | Calendar fallback color is the calendar id's enamel plate, not `#7aa3f7` | `sola-calendar` `ui.rs`, `sola-settings` `calendar.rs` |
