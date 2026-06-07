# Shell Customization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the shell's chrome user-adjustable from sola-kit — alpha-capable `shell-*` color tokens plus switcher/launcher spacing knobs, edited on a new storybook Shell page, propagated live over `Topic::Theme`, captured by presets.

**Architecture:** Eight new `shell-*` tokens in the existing theme palette (no schema change). Kit gains a typed `ShellStyle` (extract + write-back), a parameterized `backplate`, and an alpha-capable `#rrggbbaa` color round-trip. The shell reads `ShellStyle` on every theme delivery. The storybook's `ThemePreset` grows a `shell` field so presets round-trip the new tokens.

**Tech Stack:** Rust, iced 0.14, sola-bus `Topic::Theme`/`Topic::CustomTheme`, `cargo make`.

**Spec:** `docs/specs/2026-06-06-shell-customization-design.md`

**Build/test commands (memorize):**
- Workspace build: `cargo make build` — NEVER raw `cargo build`, NEVER `cargo make install` (install is the user's action, always).
- sola-kit is workspace-excluded: build `cargo make build sola-kit`, test `cargo test --manifest-path crates/sola-kit/Cargo.toml`.
- sola-shell tests: `cargo test -p sola-shell`.
- rust-analyzer diagnostics in this repo are frequently stale — trust `cargo make build` output only.

---

### Task 1: `shell-*` seed tokens + `#rrggbbaa` color round-trip

**Files:**
- Modify: `crates/sola-core/src/theme/palette.rs` (end of `Palette::seed`, after the Radius block ~line 131)
- Modify: `crates/sola-kit/src/theme.rs` — `parse` (~line 175), `try_parse` (~line 187), `color_to_hex` (~line 450)
- Test: `crates/sola-kit/src/theme.rs` `mod tests`

- [ ] **Step 1: Write the failing tests** (in `mod tests` at the bottom of `crates/sola-kit/src/theme.rs`)

```rust
#[test]
fn try_parse_eight_hex_roundtrip() {
    let c = try_parse("#00d4ff2e").expect("8-hex parses");
    assert!((c.a - 0.18).abs() < 0.005, "alpha ≈ 0.18, got {}", c.a);
    assert_eq!(color_to_hex(c), "#00d4ff2e");
}

#[test]
fn color_to_hex_opaque_stays_six_digits() {
    let c = try_parse("#0d1117").expect("6-hex parses");
    assert_eq!(c.a, 1.0);
    assert_eq!(color_to_hex(c), "#0d1117");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml try_parse_eight -- --nocapture`
Expected: FAIL — `try_parse` returns `None` for 8-char input (`expect("8-hex parses")` panics).

- [ ] **Step 3: Extend `try_parse`, `parse`, `color_to_hex`**

Replace the three functions in `crates/sola-kit/src/theme.rs`:

```rust
/// Parse `#rrggbb` / `#rrggbbaa` into an iced `Color`. Panics on
/// malformed input — the inputs are compile-time constants in this
/// crate, so the panic is a self-check rather than a runtime concern.
pub fn parse(s: &str) -> Color {
    try_parse(s).unwrap_or_else(|| panic!("expected #rrggbb or #rrggbbaa, got {s:?}"))
}

/// Try to parse `#rrggbb` or `#rrggbbaa`, returning `None` on malformed
/// input. Used when ingesting bus theme atoms whose values arrive as
/// untrusted strings — a malformed swatch becomes a fallback rather than
/// a panic — and by the color picker's free-text hex field.
pub fn try_parse(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 && s.len() != 8 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    let a = if s.len() == 8 {
        u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0
    } else {
        1.0
    };
    Some(Color { r, g, b, a })
}

/// Format an iced `Color` as `#rrggbb`, or `#rrggbbaa` when alpha < 1 —
/// so existing opaque values don't churn on round-trip. The inverse of
/// [`parse`]; shared with the storybook's atom grid so it doesn't
/// reimplement the conversion.
pub fn color_to_hex(c: Color) -> String {
    let r = (c.r * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (c.g * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (c.b * 255.0).round().clamp(0.0, 255.0) as u8;
    if c.a < 1.0 {
        let a = (c.a * 255.0).round().clamp(0.0, 255.0) as u8;
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}
```

- [ ] **Step 4: Add the 8 seed tokens**

In `crates/sola-core/src/theme/palette.rs`, after the Radius block (after the `radius-lg` insert, before the final `palette`):

```rust
        // Shell — sola-shell's customizable chrome. Colors carry alpha
        // (#rrggbbaa). Seed values reproduce the shell's original
        // hardcoded look exactly; see
        // docs/specs/2026-06-06-shell-customization-design.md.
        palette
            .tokens
            .insert("shell-menubar-bg".into(), Token::new(TokenKind::Color, "#000000", &["shell"]));
        palette.tokens.insert(
            "shell-backdrop-dim".into(),
            Token::new(TokenKind::Color, "#00000066", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-bg".into(),
            Token::new(TokenKind::Color, "#00d4ff2e", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-border".into(),
            Token::new(TokenKind::Color, "#00d4ff59", &["shell"]),
        );
        palette
            .tokens
            .insert("shell-switcher-pad".into(), Token::new(TokenKind::Space, "36px", &["shell"]));
        palette.tokens.insert(
            "shell-switcher-tile-pad".into(),
            Token::new(TokenKind::Space, "16px", &["shell"]),
        );
        palette.tokens.insert(
            "shell-launcher-width".into(),
            Token::new(TokenKind::Space, "640px", &["shell"]),
        );
        palette
            .tokens
            .insert("shell-launcher-pad".into(), Token::new(TokenKind::Space, "12px", &["shell"]));
```

(Alpha bytes: 0.18→`2e`, 0.35→`59`, 0.40→`66` — already computed, don't re-derive.)

- [ ] **Step 5: Run tests + builds, verify green**

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml` — expect ALL pass (60 existing + 2 new).
Run: `cargo make build` — expect clean (sola-core change ripples through the workspace).
Run: `cargo make build sola-kit` — expect clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-core/src/theme/palette.rs crates/sola-kit/src/theme.rs
git commit -m "feat(theme): seed shell-* tokens; support #rrggbbaa in kit color round-trip"
```

---

### Task 2: Kit `ShellStyle` — extract from / write to the bus theme

**Files:**
- Modify: `crates/sola-kit/src/theme.rs` (new section after the `Atoms`/`ATOM_BINDINGS` machinery, near `atoms_from_bus_theme` ~line 411)
- Test: same file, `mod tests`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn shell_style_from_seed_matches_defaults() {
    // BusTheme::default() seeds the shell-* tokens (Task 1), and
    // ShellStyle::default() parses the same constant strings — the
    // two must agree byte-for-byte for preset resync matching.
    assert_eq!(shell_style_from_bus_theme(&BusTheme::default()), ShellStyle::default());
}

#[test]
fn shell_style_defaults_from_empty_palette() {
    let empty = BusTheme { palette: Default::default(), components: Default::default() };
    assert_eq!(shell_style_from_bus_theme(&empty), ShellStyle::default());
}

#[test]
fn shell_style_bus_roundtrip() {
    let mut style = ShellStyle::default();
    // parse() quantizes to u8 channels, so equality after the string
    // round-trip is exact. Don't use raw f32 alphas here (0.5 → 0x80
    // → 128/255 ≠ 0.5).
    style.switcher_bg = parse("#ffb80080");
    style.switcher_pad = 48.0;
    let bus = bus_theme_with_shell(BusTheme::default(), &style);
    assert_eq!(shell_style_from_bus_theme(&bus), style);
}

#[test]
fn shell_style_malformed_token_falls_back() {
    let mut bus = BusTheme::default();
    bus.palette.tokens.get_mut("shell-switcher-pad").unwrap().value = "garbage".into();
    let style = shell_style_from_bus_theme(&bus);
    assert_eq!(style.switcher_pad, ShellStyle::default().switcher_pad);
}
```

- [ ] **Step 2: Run to verify they fail to compile** (`ShellStyle` doesn't exist)

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml shell_style`
Expected: compile error `cannot find type ShellStyle`.

- [ ] **Step 3: Implement**

Add to `crates/sola-kit/src/theme.rs` (one new section; keep it adjacent to `atoms_from_bus_theme` so the read/write pairs stay together):

```rust
// ── Shell customization (shell-* tokens) ────────────────────────────

/// Seed string values for the `shell-*` tokens. Must match
/// `Palette::seed` in sola-core byte-for-byte — `resync_active_theme`
/// matches presets to live themes by value equality, so a drift here
/// would unmatch every untouched preset.
const SHELL_MENUBAR_BG: &str = "#000000";
const SHELL_BACKDROP_DIM: &str = "#00000066";
const SHELL_SWITCHER_BG: &str = "#00d4ff2e";
const SHELL_SWITCHER_BORDER: &str = "#00d4ff59";
const SHELL_SWITCHER_PAD: f32 = 36.0;
const SHELL_SWITCHER_TILE_PAD: f32 = 16.0;
const SHELL_LAUNCHER_WIDTH: f32 = 640.0;
const SHELL_LAUNCHER_PAD: f32 = 12.0;

/// sola-shell's customizable chrome, extracted from the bus theme's
/// `shell-*` tokens. Colors carry alpha (the switcher backplate fill is
/// translucent by design); spacing values are plain pixels.
///
/// Where the shell's original layout used a vertical/horizontal padding
/// pair, the knob is the vertical value and **horizontal = vertical +
/// 4px** — this reproduces both original layouts exactly (switcher tile
/// 16/20, launcher row 12/16) from one knob each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellStyle {
    pub menubar_bg: Color,
    pub backdrop_dim: Color,
    pub switcher_bg: Color,
    pub switcher_border: Color,
    pub switcher_pad: f32,
    pub switcher_tile_pad: f32,
    pub launcher_width: f32,
    pub launcher_pad: f32,
}

impl Default for ShellStyle {
    fn default() -> Self {
        Self {
            menubar_bg: parse(SHELL_MENUBAR_BG),
            backdrop_dim: parse(SHELL_BACKDROP_DIM),
            switcher_bg: parse(SHELL_SWITCHER_BG),
            switcher_border: parse(SHELL_SWITCHER_BORDER),
            switcher_pad: SHELL_SWITCHER_PAD,
            switcher_tile_pad: SHELL_SWITCHER_TILE_PAD,
            launcher_width: SHELL_LAUNCHER_WIDTH,
            launcher_pad: SHELL_LAUNCHER_PAD,
        }
    }
}

/// One color token, falling back on missing/malformed.
fn shell_color(bus: &BusTheme, token: &str, fallback: Color) -> Color {
    bus.palette
        .tokens
        .get(token)
        .and_then(|t| try_parse(&t.value))
        .unwrap_or(fallback)
}

/// One `"<n>px"` space token, falling back on missing/malformed.
fn shell_space(bus: &BusTheme, token: &str, fallback: f32) -> f32 {
    bus.palette
        .tokens
        .get(token)
        .and_then(|t| t.value.trim().strip_suffix("px")?.trim().parse::<f32>().ok())
        .unwrap_or(fallback)
}

/// Read the shell style out of a `BusTheme` (the inverse of
/// [`bus_theme_with_shell`]). Missing or malformed tokens fall back
/// per-field to the compile-time defaults, so a stale preset (saved
/// before the shell tokens existed) renders exactly as before.
pub fn shell_style_from_bus_theme(bus: &BusTheme) -> ShellStyle {
    let d = ShellStyle::default();
    ShellStyle {
        menubar_bg: shell_color(bus, "shell-menubar-bg", d.menubar_bg),
        backdrop_dim: shell_color(bus, "shell-backdrop-dim", d.backdrop_dim),
        switcher_bg: shell_color(bus, "shell-switcher-bg", d.switcher_bg),
        switcher_border: shell_color(bus, "shell-switcher-border", d.switcher_border),
        switcher_pad: shell_space(bus, "shell-switcher-pad", d.switcher_pad),
        switcher_tile_pad: shell_space(bus, "shell-switcher-tile-pad", d.switcher_tile_pad),
        launcher_width: shell_space(bus, "shell-launcher-width", d.launcher_width),
        launcher_pad: shell_space(bus, "shell-launcher-pad", d.launcher_pad),
    }
}

/// Write `shell`'s eight values into `t`'s palette as `shell-*` tokens
/// (the inverse of [`shell_style_from_bus_theme`]). Mirrors
/// `bus_theme_from_atoms`'s upsert behavior — overwrite when present,
/// insert when the seed somehow lacks the token — preserving the
/// write-list ⊇ read-list invariant.
pub fn bus_theme_with_shell(mut t: BusTheme, shell: &ShellStyle) -> BusTheme {
    use sola_core::theme::{Token, TokenKind};
    let entries: [(&str, TokenKind, String); 8] = [
        ("shell-menubar-bg", TokenKind::Color, color_to_hex(shell.menubar_bg)),
        ("shell-backdrop-dim", TokenKind::Color, color_to_hex(shell.backdrop_dim)),
        ("shell-switcher-bg", TokenKind::Color, color_to_hex(shell.switcher_bg)),
        ("shell-switcher-border", TokenKind::Color, color_to_hex(shell.switcher_border)),
        ("shell-switcher-pad", TokenKind::Space, format!("{}px", shell.switcher_pad)),
        ("shell-switcher-tile-pad", TokenKind::Space, format!("{}px", shell.switcher_tile_pad)),
        ("shell-launcher-width", TokenKind::Space, format!("{}px", shell.launcher_width)),
        ("shell-launcher-pad", TokenKind::Space, format!("{}px", shell.launcher_pad)),
    ];
    for (name, kind, value) in entries {
        match t.palette.tokens.get_mut(name) {
            Some(tok) => tok.value = value,
            None => {
                t.palette
                    .tokens
                    .insert(name.to_string(), Token::new(kind, &value, &["shell"]));
            }
        }
    }
    t
}
```

Note: `BusTheme` is this module's existing alias for `sola_core::theme::Theme` — follow whatever the file already imports.

- [ ] **Step 4: Run tests, verify green**

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml`
Expected: ALL pass (4 new shell_style tests included).

- [ ] **Step 5: Commit**

```bash
git add crates/sola-kit/src/theme.rs
git commit -m "feat(sola-kit): ShellStyle — typed shell-* token extraction and write-back"
```

---

### Task 3: Parameterized backplate component

**Files:**
- Modify: `crates/sola-kit/src/components/card.rs` (`accent_backplate_style` ~line 64, `accent_backplate` ~line 90)
- Modify: `crates/sola-kit/src/components/mod.rs` (export line ~58: `pub use card::{accent_backplate, card, modal};`)
- Modify: `crates/sola-kit/src/storybook/pages/card.rs` (demo, below the accent backplate demo ~line 57)

- [ ] **Step 1: Add `backplate_style` / `backplate`, rewrite `accent_backplate_style` as a wrapper**

In `card.rs`, replace `accent_backplate_style` and add the two new fns directly above it:

```rust
/// Parameterized backplate style: caller supplies fill and border
/// colors (alpha included — e.g. the shell's `shell-switcher-bg` /
/// `shell-switcher-border` tokens). Radius, border width, and shadow
/// are the backplate constants.
///
/// Radius choice: `RADIUS_XL` (14px) is used for the modal; the
/// switcher backplate is a slightly softer 16px to visually distinguish
/// it as a secondary frame. Using a plain literal keeps the two values
/// intentionally independent — don't abstract them into the same const.
pub fn backplate_style(fill: Color, border: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: iced::Border {
            color: border,
            width: 1.0,
            radius: 16.0.into(), // switcher backplate: 2px softer than modal
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
        ..container::Style::default()
    }
}

/// [`accent_backplate`] with caller-supplied fill/border colors.
/// Returns a `Container` so the caller can chain sizing/padding.
pub fn backplate<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
    fill: Color,
    border: Color,
) -> Container<'a, Message, Theme> {
    container(content).style(backplate_style(fill, border))
}

/// Style for [`accent_backplate`]: primary-tinted translucent fill and
/// border at 16px radius, with a deep drop shadow. Thin wrapper over
/// [`backplate_style`] passing the palette-derived defaults.
pub fn accent_backplate_style(theme: &Theme) -> container::Style {
    let accent = theme.extended_palette().primary.base.color;
    backplate_style(Color { a: 0.18, ..accent }, Color { a: 0.35, ..accent })(theme)
}
```

(`accent_backplate` itself is unchanged.)

- [ ] **Step 2: Export and demo**

`components/mod.rs`:

```rust
pub use card::{accent_backplate, backplate, card, modal};
```

`storybook/pages/card.rs` — add `backplate` to the existing `sola_kit::components` import, then below the accent backplate demo block add:

```rust
let custom_backplate_demo = backplate(
    body("Parameterized backplate — caller-supplied fill/border (gold @ 0.20)."),
    iced::Color::from_rgba(1.0, 0.72, 0.0, 0.20),
    iced::Color::from_rgba(1.0, 0.72, 0.0, 0.40),
)
.padding(24);
```

and push it into the page column with a caption line, matching the page's existing pattern:

```rust
body("Parameterized backplate — shell switcher uses this with shell-* tokens.").style(muted),
custom_backplate_demo,
code("backplate(content, fill, border)").style(muted),
```

- [ ] **Step 3: Build + test, verify green**

Run: `cargo make build sola-kit` — expect clean.
Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml` — expect ALL pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sola-kit/src/components/card.rs crates/sola-kit/src/components/mod.rs crates/sola-kit/src/storybook/pages/card.rs
git commit -m "feat(sola-kit): parameterized backplate(fill, border); accent_backplate delegates"
```

---

### Task 4: Shell consumes `ShellStyle`

**Files:**
- Modify: `crates/sola-kit/src/theme.rs` — `menubar` (~line 100) + its test `menubar_chrome_theme` (~line 553)
- Modify: `crates/sola-shell/src/app.rs` — `Shell` struct (~line 80), `boot()` struct init, `theme()` (~line 425)
- Modify: `crates/sola-shell/src/app/bus.rs` — `on_theme` (~line 51)
- Modify: `crates/sola-shell/src/switcher/view.rs`, `crates/sola-shell/src/launcher/view.rs`

- [ ] **Step 1: kit — `menubar` gains the bg parameter**

```rust
/// Theme for the menubar. `bg` is the menubar background
/// (`shell-menubar-bg` — black by default).
///
/// Background tiers are generated from the bg base so hover/active fills
/// derive from it; foreground text and icons still follow the real palette.
pub fn menubar(base: &Theme, bg: Color) -> Theme {
    let mut palette = base.palette();
    palette.background = bg;
    Theme::custom_with_fn(
        "sola-menubar".to_string(),
        palette,
        Extended::generate,
    )
}
```

Update the test's first line: `let t = menubar(&default_theme(), Color::BLACK);` (rest unchanged).

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml` — expect ALL pass.

- [ ] **Step 2: shell state — `style` field**

`crates/sola-shell/src/app.rs`:

```rust
pub struct Shell {
    pub theme: iced::Theme,
    /// Shell-specific chrome (shell-* tokens) — colors with alpha +
    /// switcher/launcher spacing. Refreshed alongside `theme` on every
    /// Topic::Theme delivery.
    pub style: theme::ShellStyle,
    // ... existing fields unchanged
```

(`theme` here is the existing `sola_kit::theme` import the file already uses for `theme::default_theme()`.)

In `boot()`'s `Shell { ... }` initializer add `style: theme::ShellStyle::default(),` next to `theme`.

In `theme()`:

```rust
pub fn theme(&self, window: iced::window::Id) -> iced::Theme {
    if Some(window) == self.menubar_window_id {
        return theme::menubar(&self.theme, self.style.menubar_bg);
    }
    theme::overlay(&self.theme)
}
```

(Keep the existing body's exact structure — only the `menubar` call gains the second argument.)

`crates/sola-shell/src/app/bus.rs`:

```rust
/// Apply an updated bus theme to the iced renderer.
fn on_theme(&mut self, t: BusTheme) {
    self.theme = sola_kit::theme::theme_from_bus(&t);
    self.style = sola_kit::theme::shell_style_from_bus_theme(&t);
    sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(&t));
}
```

- [ ] **Step 3: switcher view**

`crates/sola-shell/src/switcher/view.rs` — tile padding (inside the per-app map):

```rust
// Tile padding knob: vertical = shell-switcher-tile-pad,
// horizontal = vertical + 4 (preserves the original 16/20).
let tp = shell.style.switcher_tile_pad;
let card_container: Element<'_, Msg> = container(card_content)
    .padding(Padding { top: tp, bottom: tp, left: tp + 4.0, right: tp + 4.0 })
    .style(sola_kit::components::card::list_tile_style(is_selected))
    .into();
```

Backplate (replaces the `accent_backplate` call):

```rust
// Backplate fill/border come from the shell-* tokens (alpha-capable);
// padding from shell-switcher-pad. Seed values match the old
// accent-derived look exactly.
let backplate: Element<'_, Msg> = sola_kit::components::backplate(
    row(cards)
        .spacing(12)
        .align_y(Alignment::Center),
    shell.style.switcher_bg,
    shell.style.switcher_border,
)
.padding(Padding::new(shell.style.switcher_pad))
.into();
```

Update the module doc comment (`~36px padding` → `shell-switcher-pad`) and the stale `accent_backplate` comment above it.

- [ ] **Step 4: launcher view**

`crates/sola-shell/src/launcher/view.rs`:

Row padding (in the per-app map, replacing the literal `Padding { top: 12.0, ... }`):

```rust
// Row padding knob: vertical = shell-launcher-pad,
// horizontal = vertical + 4 (preserves the original 12/16).
let lp = shell.style.launcher_pad;
let row_btn = iced::widget::button(row_content)
    .on_press(Msg::Launch)
    .padding(Padding { top: lp, bottom: lp, left: lp + 4.0, right: lp + 4.0 })
    .width(Length::Fill)
    .style(kit_btn::list_item(is_selected));
```

Card width (line ~132): `let card: Element<'_, Msg> = modal(card_body).width(Length::Fixed(shell.style.launcher_width)).into();`

Backdrop (replaces the hardcoded `from_rgba(0.0, 0.0, 0.0, 0.40)` closure):

```rust
// Backdrop dim comes from shell-backdrop-dim (alpha-capable).
let dim = shell.style.backdrop_dim;
let backdrop: Element<'_, Msg> = mouse_area(
    container(text(""))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(dim)),
            ..Default::default()
        }),
)
.on_press(Msg::CloseLauncher)
.into();
```

Update the stale "Backdrop: very light dim" comment to mention the token.

- [ ] **Step 5: Build + test, verify green**

Run: `cargo make build sola-kit && cargo make build sola-shell` — expect clean.
Run: `cargo test -p sola-shell` — expect 39/39 pass.
Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml` — expect ALL pass.

- [ ] **Step 6: Commit**

```bash
git add crates/sola-kit/src/theme.rs crates/sola-shell/src/app.rs crates/sola-shell/src/app/bus.rs crates/sola-shell/src/switcher/view.rs crates/sola-shell/src/launcher/view.rs
git commit -m "feat(sola-shell): drive chrome from ShellStyle (shell-* tokens); menubar bg parameterized"
```

---

### Task 5: Storybook Shell page + preset round-trip

**Files:**
- Modify: `crates/sola-kit/src/storybook/mod.rs` — `Page` enum/`ALL`/`label`/`section` (~lines 31–113), `Msg` (~line 115), `AtomField` vicinity (~line 175) for the new field enums, `ThemePreset` (~line 227), `Storybook` struct (~line 238), `default()` (~line 288), update arms (~lines 414–530), `broadcast_theme`/`persist_active_theme`/`retract_custom_theme`/`upsert_custom_theme`/`resync_active_theme` (~lines 563–701), `page_view` routing (~line 897)
- Create: `crates/sola-kit/src/storybook/pages/shell.rs`
- Modify: `crates/sola-kit/src/storybook/pages/mod.rs` (add `pub mod shell;`)
- Test: `mod tests` in `storybook/mod.rs`

- [ ] **Step 1: Write the failing round-trip test** (in storybook `mod tests`)

```rust
#[test]
fn preset_bus_theme_roundtrips_shell_style() {
    let mut preset = ThemePreset {
        name: "test".into(),
        atoms: theme::Atoms::default(),
        fonts: theme::FontSelection::default(),
        shell: theme::ShellStyle::default(),
    };
    preset.shell.switcher_bg = theme::parse("#ffb80080");
    preset.shell.launcher_width = 720.0;
    let bus = preset.bus_theme();
    assert_eq!(theme::shell_style_from_bus_theme(&bus), preset.shell);
}
```

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml preset_bus_theme` — expect compile error (no `shell` field, no `bus_theme`).

- [ ] **Step 2: `ThemePreset.shell` + `bus_theme()` helper; route every bus composition through it**

```rust
pub struct ThemePreset {
    pub name: String,
    pub atoms: theme::Atoms,
    pub fonts: theme::FontSelection,
    pub shell: theme::ShellStyle,
}

impl ThemePreset {
    /// The preset's complete bus form — atoms + fonts + shell tokens.
    /// The single source for broadcast, persist, retract, and resync
    /// matching, so the value-equality invariant can't drift between
    /// call sites.
    fn bus_theme(&self) -> sola_bus::topics::Theme {
        theme::bus_theme_with_shell(theme::bus_theme_from(&self.atoms, &self.fonts), &self.shell)
    }
}
```

Replace every `theme::bus_theme_from(&X.atoms, &X.fonts)` with `X.bus_theme()`. Five sites:
- `broadcast_theme`: `let bus_theme = active.bus_theme();`
- `persist_active_theme`: `theme: active.bus_theme(),`
- `retract_custom_theme`: `theme: removed.bus_theme(),`
- `resync_active_theme` (×2): `if &self.active().bus_theme() == live` and `.position(|p| &p.bus_theme() == live)`

In `Storybook::default()`: add `shell: theme::ShellStyle::default(),` to `default_preset`.

In `upsert_custom_theme`: add `shell: theme::shell_style_from_bus_theme(&named.theme),` to the `ThemePreset` construction.

Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml preset_bus_theme` — expect PASS.

- [ ] **Step 3: Field enums, state, messages, update arms**

Next to `AtomField` add (mirroring its get/set pattern):

```rust
/// Identifies which shell color knob an `EditShellColor` targets.
/// UI-shaped (the Shell page's swatches and color picker carry it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellColorField {
    MenubarBg,
    BackdropDim,
    SwitcherBg,
    SwitcherBorder,
}

impl ShellColorField {
    pub fn get(self, s: &theme::ShellStyle) -> iced::Color {
        match self {
            Self::MenubarBg => s.menubar_bg,
            Self::BackdropDim => s.backdrop_dim,
            Self::SwitcherBg => s.switcher_bg,
            Self::SwitcherBorder => s.switcher_border,
        }
    }
    pub fn set(self, s: &mut theme::ShellStyle, c: iced::Color) {
        match self {
            Self::MenubarBg => s.menubar_bg = c,
            Self::BackdropDim => s.backdrop_dim = c,
            Self::SwitcherBg => s.switcher_bg = c,
            Self::SwitcherBorder => s.switcher_border = c,
        }
    }
}

/// Identifies which shell spacing knob a `SetShellSpace` targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSpaceField {
    SwitcherPad,
    SwitcherTilePad,
    LauncherWidth,
    LauncherPad,
}

impl ShellSpaceField {
    pub fn set(self, s: &mut theme::ShellStyle, v: f32) {
        match self {
            Self::SwitcherPad => s.switcher_pad = v,
            Self::SwitcherTilePad => s.switcher_tile_pad = v,
            Self::LauncherWidth => s.launcher_width = v,
            Self::LauncherPad => s.launcher_pad = v,
        }
    }
}
```

`Storybook` struct — add below `editing_atom`:

```rust
/// Which shell color knob's inline picker is open on the Shell page,
/// if any. Mutually exclusive with `editing_atom` — both pair with
/// the single `picker`.
editing_shell: Option<ShellColorField>,
```

(and `editing_shell: None,` in `default()`).

`Msg` — two new variants:

```rust
/// Open the inline color picker for one shell color knob. No-op when
/// Default is active (read-only).
EditShellColor(ShellColorField),
/// Set one shell spacing knob from the Shell page's number inputs.
/// No-op when Default is active.
SetShellSpace(ShellSpaceField, f32),
```

New update arms (next to `EditAtom`):

```rust
Msg::EditShellColor(field) => {
    if self.is_default_active() {
        tracing::debug!("ignoring EditShellColor — Default theme is read-only");
        return;
    }
    self.editing_atom = None;
    // Toggle: clicking the open knob's swatch again closes it.
    if self.editing_shell == Some(field) {
        self.editing_shell = None;
        self.picker = None;
    } else {
        let color = field.get(&self.active().shell);
        self.editing_shell = Some(field);
        self.picker = Some(ColorPicker::new(color));
    }
}
Msg::SetShellSpace(field, value) => {
    if self.is_default_active() {
        tracing::debug!("ignoring SetShellSpace — Default theme is read-only");
        return;
    }
    field.set(&mut self.active_mut().shell, value);
    self.broadcast_theme();
    self.persist_active_theme();
}
```

Mutual-exclusion edits to existing arms:
- `Msg::EditAtom`: add `self.editing_shell = None;` right before the toggle `if`.
- `Msg::ClosePicker`: add `self.editing_shell = None;`
- `Msg::Picker`: extend the apply:

```rust
if let Some(field) = self.editing_atom {
    self.apply_atom(field, color);
} else if let Some(field) = self.editing_shell {
    self.apply_shell_color(field, color);
}
```

- `Msg::SelectTheme`: add `self.editing_shell = None;` next to `self.editing_atom = None;`
- `Msg::DeleteActiveTheme`: same addition.

New private method next to `apply_atom`:

```rust
/// Write one shell color onto the active preset and propagate it.
/// Unlike `apply_atom` there's no `refresh_active_theme` — shell
/// tokens don't feed the storybook's own iced theme, only the
/// broadcast bus value.
fn apply_shell_color(&mut self, field: ShellColorField, color: iced::Color) {
    if self.is_default_active() {
        tracing::debug!("ignoring shell color edit — Default theme is read-only");
        return;
    }
    field.set(&mut self.active_mut().shell, color);
    self.broadcast_theme();
    self.persist_active_theme();
}
```

- [ ] **Step 4: `Page::Shell` + routing**

- `Page` enum: add `Shell,` after `Theme`.
- `Page::ALL`: insert `Page::Shell,` after `Page::Theme`.
- `label()`: `Page::Shell => "Shell",`
- `section()`: change the Theme arm to `Page::Theme | Page::Shell => Some("Theme"),`
- `page_view()`:

```rust
Page::Shell => pages::shell::view(
    &self.active().shell,
    editable,
    self.editing_shell,
    self.picker.as_ref().map(|p| p.view().map(Msg::Picker)),
),
```

- `pages/mod.rs`: add `pub mod shell;`

- [ ] **Step 5: Create `pages/shell.rs`**

Mirror `pages/theme.rs`'s structure (swatch tile + anchored picker popover; `number_input` like the NumberInput page — check that page's import path for the component):

```rust
//! Shell page — editor for sola-shell's customizable chrome: the
//! shell-* color tokens (alpha-capable) and the switcher/launcher
//! spacing knobs. Edits route through the same preset machinery as the
//! Theme page (mutate active preset → broadcast Topic::Theme →
//! persist), so the running shell restyles as you drag.

use iced::widget::{column, container, mouse_area, row};
use iced::{Color, Element, Length, Padding};

use sola_kit::components::number_input::number_input;
use sola_kit::components::swatch::swatch_sized;
use sola_kit::components::text::{body, caption, code, heading, muted, subheading};
use sola_kit::components::{popover, popover_anchored};
use sola_kit::theme::{self, ShellStyle};

use crate::storybook::{Msg, ShellColorField, ShellSpaceField};

const SWATCH_SIZE: f32 = 56.0;
const GRID_GAP: f32 = 44.0;

pub fn view<'a>(
    shell: &'a ShellStyle,
    editable: bool,
    editing: Option<ShellColorField>,
    mut picker_view: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let intro: Element<'a, Msg> = if editable {
        body(
            "Live editor for sola-shell's chrome. Colors carry alpha — \
             the switcher backplate fill is translucent by design. Edits \
             re-emit Topic::Theme, so the running shell restyles \
             immediately.",
        )
        .style(muted)
        .into()
    } else {
        body(
            "Default theme — read-only. Click \"New Theme\" in the header \
             above to fork it under a new name; edits then route to that \
             copy.",
        )
        .style(muted)
        .into()
    };

    // (label, field, token-name caption)
    let colors: &[(&str, ShellColorField, &str)] = &[
        ("MENUBAR_BG", ShellColorField::MenubarBg, "shell-menubar-bg"),
        ("BACKDROP", ShellColorField::BackdropDim, "shell-backdrop-dim"),
        ("SWITCHER_BG", ShellColorField::SwitcherBg, "shell-switcher-bg"),
        ("SWITCHER_BORDER", ShellColorField::SwitcherBorder, "shell-switcher-border"),
    ];
    let mut color_row = row![].spacing(GRID_GAP);
    for (name, field, token) in colors {
        let picker = if editing == Some(*field) { picker_view.take() } else { None };
        color_row = color_row.push(swatch_tile(shell, name, *field, token, editable, editing, picker));
    }

    column![
        heading("Shell"),
        intro,

        subheading("Colors"),
        body("Click a swatch to edit. The picker's alpha rail is live — \
              e.g. drag SWITCHER_BG's alpha to retune the backplate \
              translucency.")
        .style(muted),
        color_row,

        subheading("Switcher"),
        space_row("Backplate padding", ShellSpaceField::SwitcherPad, shell.switcher_pad, 0.0..=64.0, 2.0, editable),
        space_row("Tile padding", ShellSpaceField::SwitcherTilePad, shell.switcher_tile_pad, 0.0..=48.0, 2.0, editable),

        subheading("Launcher"),
        space_row("Card width", ShellSpaceField::LauncherWidth, shell.launcher_width, 320.0..=1280.0, 20.0, editable),
        space_row("Row padding", ShellSpaceField::LauncherPad, shell.launcher_pad, 0.0..=32.0, 2.0, editable),
    ]
    .spacing(28)
    .into()
}

/// One color knob: swatch (click target + accent ring while editing,
/// anchored picker popover) over label / hex / token captions. Mirrors
/// the Theme page's `swatch_tile` with `ShellColorField` in place of
/// `AtomField`.
fn swatch_tile<'a>(
    shell: &ShellStyle,
    name: &'a str,
    field: ShellColorField,
    token: &'a str,
    editable: bool,
    editing: Option<ShellColorField>,
    picker: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let color = field.get(shell);
    let tile = swatch_sized::<Msg>(color, SWATCH_SIZE);
    let tile: Element<'a, Msg> = if editable {
        let selected = editing == Some(field);
        let ring = if selected { 2.0 } else { 0.0 };
        let framed = container(tile)
            .padding(Padding::from(ring))
            .style(move |theme: &iced::Theme| {
                let p = theme.extended_palette();
                iced::widget::container::Style {
                    border: iced::Border {
                        color: if selected { p.primary.base.color } else { Color::TRANSPARENT },
                        width: ring,
                        radius: 8.0.into(),
                    },
                    ..iced::widget::container::Style::default()
                }
            });
        let trigger = mouse_area(framed).on_press(Msg::EditShellColor(field));
        match picker {
            Some(view) => popover_anchored(trigger, popover(view), Msg::ClosePicker).into(),
            None => trigger.into(),
        }
    } else {
        tile
    };

    column![
        tile,
        body(name),
        code(theme::color_to_hex(color)).style(muted),
        caption(token).style(muted),
    ]
    .spacing(6)
    .width(Length::Fixed(SWATCH_SIZE + 36.0))
    .into()
}

/// One spacing knob: label + `[−] value px [+]` stepper (plain text on
/// the read-only Default preset).
fn space_row<'a>(
    label: &'a str,
    field: ShellSpaceField,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    editable: bool,
) -> Element<'a, Msg> {
    let control: Element<'a, Msg> = if editable {
        number_input(value, range, step, "px", move |v| Msg::SetShellSpace(field, v))
    } else {
        container(body(format!("{value:.0} px"))).into()
    };
    row![
        container(body(label)).width(Length::Fixed(160.0)),
        control,
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center)
    .into()
}
```

(If the `number_input`/`swatch_sized` import paths differ from the above, copy them from `pages/number_input.rs` / `pages/theme.rs` — those pages are the source of truth.)

- [ ] **Step 6: Build + full test run, verify green**

Run: `cargo make build sola-kit` — expect clean.
Run: `cargo test --manifest-path crates/sola-kit/Cargo.toml` — expect ALL pass (including `preset_bus_theme_roundtrips_shell_style`).
Run: `cargo make build` — expect clean (workspace unaffected but confirm).

- [ ] **Step 7: Commit**

```bash
git add crates/sola-kit/src/storybook/mod.rs crates/sola-kit/src/storybook/pages/mod.rs crates/sola-kit/src/storybook/pages/shell.rs
git commit -m "feat(sola-kit): storybook Shell page — shell-* color/spacing editor; presets round-trip shell tokens"
```

---

### Task 6: Docs

**Files:**
- Modify: `docs/specs/2026-06-06-shell-customization-design.md` (status line)
- Modify: `CLAUDE.md` (sola-kit theme protocol section)

- [ ] **Step 1: Update spec status**

Change `**Status:** approved design, pending implementation plan` to `**Status:** implemented (2026-06-06)`.

- [ ] **Step 2: Note ShellStyle in CLAUDE.md**

In the "Theme protocol (`src/theme.rs`)" section of the Sola `CLAUDE.md`, append a short paragraph:

```markdown
**Shell chrome** rides the same palette as `shell-*` tokens (4 alpha-capable
colors + 4 spacing values, group `"shell"`). `ShellStyle` is the typed view:
`shell_style_from_bus_theme` extracts (per-token fallback to compile-time
defaults), `bus_theme_with_shell` writes back. The shell refreshes it on every
`Topic::Theme`; the storybook's Shell page is the editor. Colors round-trip as
`#rrggbbaa` when translucent.
```

- [ ] **Step 3: Commit**

```bash
git add docs/specs/2026-06-06-shell-customization-design.md CLAUDE.md
git commit -m "docs: mark shell customization implemented; document ShellStyle in CLAUDE.md"
```

> Note: `CLAUDE.md` may carry an unrelated pre-existing modification — stage hunks carefully (`git add -p CLAUDE.md`) or ask the controller if its diff looks unrelated to this task.

---

## Acceptance criteria

1. `cargo make build` and `cargo make build sola-kit` clean; kit tests (60 + ~7 new) and shell tests (39) green.
2. Seed bus theme and `ShellStyle::default()` agree byte-for-byte (test-enforced) — untouched presets still resync-match.
3. A stale preset (no shell tokens) renders identically to before this change (fallback test-enforced).
4. Storybook Shell page: 4 swatches with alpha-capable pickers + 4 spacing steppers; read-only on Default; edits persist and broadcast.
5. Manual (user-run): `cargo make install`, fork a theme, drag SWITCHER_BG alpha → live switcher backplate fades; restart → values persist.
