# Shell Customization — Design

**Date:** 2026-06-06
**Status:** implemented (2026-06-06)
**Prereq reading:** `docs/specs/2026-05-07-sidebar-and-theme-protocol-design.md` (theme protocol),
`docs/specs/2026-06-04-sola-shell-kit-ification-plan.md` (shell surfaces on kit components)

## Goal

Make the shell's look user-adjustable from sola-kit: the colors the shell
uses (with alpha — e.g. the switcher backplate's translucent fill) and a
small set of layout knobs (paddings, launcher width) for the switcher and
launcher. Edits propagate live over the bus and are captured by theme
presets.

## Guiding decision

Things expressible as global theme atoms stay global atoms (those are
already editable on the storybook Theme page and the shell follows them
live). Only values that are genuinely shell-specific — today hardcoded in
shell views or kit style fns — become new tokens. The mechanism is the
existing two-layer theme protocol: **new `shell-*` palette tokens**, no
schema changes, no new bus topics. The `components` bindings layer is not
used (bindings point at tokens; they cannot hold values), and a separate
`Topic::ShellStyle` was rejected (second persistence path, presets would
not capture it).

A known trade-off, accepted: the switcher tint is today *derived* from the
accent (`primary @ 0.18`). As a literal token it is seeded to match the
current look but no longer follows accent edits. That is the cost of
direct alpha control; per-preset values keep it sane.

## 1. Protocol — 8 new `shell-*` tokens

Added to `Palette::seed` (`crates/sola-core/src/theme/palette.rs`), all in
group `"shell"`. Seed values reproduce today's hardcoded look exactly:

| Token | Kind | Seed value | Replaces |
|---|---|---|---|
| `shell-menubar-bg` | Color | `#000000` | `theme::menubar` palette.background = BLACK |
| `shell-backdrop-dim` | Color | `#00000066` | launcher backdrop `rgba(0,0,0,0.40)` |
| `shell-switcher-bg` | Color | `#00d4ff2e` | `accent_backplate_style` fill (primary @ 0.18) |
| `shell-switcher-border` | Color | `#00d4ff59` | `accent_backplate_style` border (primary @ 0.35) |
| `shell-switcher-pad` | Space | `36px` | switcher backplate `.padding(36)` |
| `shell-switcher-tile-pad` | Space | `16px` | switcher tile padding 16/20 (v/h) |
| `shell-launcher-width` | Space | `640px` | launcher card `Fixed(640.0)` |
| `shell-launcher-pad` | Space | `12px` | launcher row padding 12/16 (v/h) |

**Alpha in color values:** kit `theme::try_parse` accepts `#rrggbbaa`
(8 hex digits) in addition to `#rrggbb`; `color_to_hex` emits 8 digits
only when alpha < 1.0, so existing opaque values don't churn on
round-trip.

**Compatibility:** existing on-disk presets
(`~/.config/sola/theme/presets/*.yaml`) lack these tokens. Consumers fall
back per-token to the defaults below — a stale preset renders exactly as
today.

## 2. Kit — `ShellStyle`

In `crates/sola-kit/src/theme.rs`:

```rust
pub struct ShellStyle {
    pub menubar_bg: Color,
    pub backdrop_dim: Color,
    pub switcher_bg: Color,
    pub switcher_border: Color,
    pub switcher_pad: f32,       // 36.0
    pub switcher_tile_pad: f32,  // 16.0
    pub launcher_width: f32,     // 640.0
    pub launcher_pad: f32,       // 12.0
}
```

- `Default` = the seed table above (compile-time constants, same pattern
  as `Atoms`).
- `shell_style_from_bus_theme(&sola_core::theme::Theme) -> ShellStyle` —
  per-token extraction; missing or malformed tokens fall back to the
  default field. Space tokens parse `"<n>px"` (same convention as seeded
  space atoms).

**Asymmetric-padding rule:** where today's layout uses a vertical/
horizontal pair, the knob is the vertical value and **horizontal =
vertical + 4px**. This reproduces both current layouts exactly (switcher
tile 16/20, launcher row 12/16) from one knob each, and is documented at
each use site.

## 3. Kit component — parameterized backplate

`card::backplate_style(fill: Color, border: Color)` — the parameterized
form of `accent_backplate_style` (radius 16, border width 1, deep shadow
unchanged). `accent_backplate_style` becomes a thin wrapper passing its
current palette-derived defaults, so generic kit consumers and the
storybook demo are unaffected. A matching `backplate(content, fill,
border)` convenience mirrors `accent_backplate(content)`.

## 4. Shell consumption

- `Shell` gains `style: ShellStyle` (init `Default`), refreshed in the
  `Topic::Theme` arm beside the existing `theme_from_bus` call.
- `theme::menubar(base: &Theme, bg: Color)` — gains the bg parameter;
  the shell passes `style.menubar_bg`.
- View changes (literal → `shell.style.*`):
  - **switcher:** backplate via `card::backplate(content, style.switcher_bg,
    style.switcher_border)` padded `style.switcher_pad`; tiles padded
    `[style.switcher_tile_pad, style.switcher_tile_pad + 4.0]`.
  - **launcher:** card `Fixed(style.launcher_width)`; rows padded
    `[style.launcher_pad, style.launcher_pad + 4.0]`; backdrop fill
    `style.backdrop_dim`.

Live propagation falls out of the existing flow: storybook edit →
`Topic::Theme` broadcast → shell update arm → next render.

## 5. Storybook — `Page::Shell`

New `crates/sola-kit/src/storybook/pages/shell.rs` + `Page::Shell`
sidebar entry. Three sections — **Menubar / Launcher / Switcher** — each
listing its knobs:

- **Colors:** swatch + inline alpha-capable color picker, same popover
  pattern as the Theme page's atoms. A parallel `ShellColorField` enum
  (4 variants) mirrors `AtomField`; messages `EditShellColor(field)` /
  picker routing / close reuse the existing picker-state machinery.
- **Spacing:** kit `number_input` per knob (`SetShellSpace(field, f32)`,
  `ShellSpaceField` enum, 4 variants). Sane ranges clamped at the edge
  (e.g. pads 0–64, width 320–1280).

Edit rules identical to atoms: no-op on the Default preset (read-only);
edits mutate the active preset, then `persist_active_theme` + broadcast.

## 6. Preset round-trip

`ThemePreset` is currently a lossy `atoms + fonts` bundle — emitting a
preset reconstructs the bus theme from those alone, which would wipe
shell tokens. Fix:

- `ThemePreset` gains `shell: ShellStyle`.
- The emit path (`bus_theme_from`) writes the 8 shell tokens into the
  outgoing palette (colors via `color_to_hex` with alpha, spaces as
  `"<n>px"`).
- Preset load and external-theme adoption extract via
  `shell_style_from_bus_theme` (defaults for missing tokens).
- The Default preset's `shell` is `ShellStyle::default()`, immutable like
  its atoms.

## Testing

Kit unit tests:

- `try_parse` 8-hex round-trip (`#00d4ff2e` ↔ Color with a ≈ 0.18);
  6-hex behavior unchanged; `color_to_hex` emits 6 digits when opaque.
- `shell_style_from_bus_theme`: seeded theme yields exact current values;
  empty palette yields `ShellStyle::default()`; malformed token falls
  back per-field.
- Preset emit → extract round-trip preserves all 8 shell tokens.

Existing suites (60 kit, 39 shell) stay green. Manual smoke: edit
switcher bg alpha in the storybook Shell page → live shell switcher
updates; restart → preset persisted.

## Out of scope (first pass)

- Menu dropdown width, launcher list height, switcher tile radius,
  menubar fg (follows global atoms).
- Binding shell slots through the `components` layer.
- A shell page in `sola-settings` (storybook is the editor for now).
