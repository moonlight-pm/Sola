//! Palette seed — the brand-level atom catalog. `Palette::seed`
//! produces every v1 atom (colors, fonts, sizes, spacing, radii) with
//! its selection groups attached. `Theme::default` uses these atoms
//! with no per-component bindings; sola-kit composes its bindings on
//! top via `kit_default_theme()`.

use super::types::{Palette, Token, TokenKind};

impl Palette {
    /// Build the seed palette — the v1 atom catalog. Never panics; pure
    /// data construction. Add new atoms here when the theme protocol
    /// grows.
    pub fn seed() -> Self {
        let mut palette = Palette::default();
        // Colors — graphite tool UI on neutral greys (Next: no blue in
        // the base; see `slate.rs` for the slate it replaced). Keep in sync
        // with `sola_kit::theme::hex::*`.
        palette.tokens.insert(
            "bg-primary".into(),
            Token::new(TokenKind::Color, "#0b0b0b", &["surface"]),
        );
        palette.tokens.insert(
            "bg-secondary".into(),
            Token::new(TokenKind::Color, "#151515", &["surface"]),
        );
        palette.tokens.insert(
            "bg-tertiary".into(),
            Token::new(TokenKind::Color, "#1f1f1f", &["surface"]),
        );
        palette.tokens.insert(
            "bg-hover".into(),
            Token::new(TokenKind::Color, "#1f1f1f", &["surface"]),
        );
        // Colors — borders
        palette.tokens.insert(
            "border".into(),
            Token::new(TokenKind::Color, "#2a2a2a", &["border"]),
        );
        palette.tokens.insert(
            "border-subtle".into(),
            Token::new(TokenKind::Color, "#1f1f1f", &["border"]),
        );
        // Colors — text
        palette.tokens.insert(
            "text-primary".into(),
            Token::new(TokenKind::Color, "#ececec", &["text"]),
        );
        palette.tokens.insert(
            "text-secondary".into(),
            Token::new(TokenKind::Color, "#949494", &["text"]),
        );
        palette.tokens.insert(
            "text-tertiary".into(),
            Token::new(TokenKind::Color, "#737373", &["text"]),
        );
        // text-muted doubles as a strong border (e.g. scrollbar thumb on
        // hover) which is why it's eligible for both `text` and `border`.
        palette.tokens.insert(
            "text-muted".into(),
            Token::new(TokenKind::Color, "#2a2a2a", &["text", "border"]),
        );
        palette.tokens.insert(
            "text-accent".into(),
            Token::new(TokenKind::Color, "#3dd6f5", &["text", "accent"]),
        );
        // Colors — accent + status (soft cyan, used sparsely)
        palette.tokens.insert(
            "accent".into(),
            Token::new(TokenKind::Color, "#3dd6f5", &["accent"]),
        );
        palette.tokens.insert(
            "accent-dim".into(),
            Token::new(
                TokenKind::Color,
                "rgba(61, 214, 245, 0.10)",
                &["accent-tint"],
            ),
        );
        // Quiet selection fill (kit `hex::SELECTION`) — a grey lift, not a
        // darkened neon (that mix reads as muddy teal).
        palette.tokens.insert(
            "selection".into(),
            Token::new(TokenKind::Color, "#292929", &["surface"]),
        );
        // A sidebar column's active row (the tab bar's focused window): a
        // step above `selection` (kit `hex::HIGHLIGHT`).
        palette.tokens.insert(
            "highlight".into(),
            Token::new(TokenKind::Color, "#353535", &["surface"]),
        );
        palette.tokens.insert(
            "danger".into(),
            Token::new(TokenKind::Color, "#f07178", &["status"]),
        );
        palette.tokens.insert(
            "success".into(),
            Token::new(TokenKind::Color, "#3ecf8e", &["status"]),
        );
        palette.tokens.insert(
            "warning".into(),
            Token::new(TokenKind::Color, "#e8b84a", &["status"]),
        );
        // Fonts — the kit's semantic role vocabulary
        // (`font-ui` / `font-ui-medium` / `font-display` / `font-chrome` /
        // `font-mono`). Prefer SF Pro Text + Iosevka Term Slab; kit falls
        // back to Inter / JetBrains Mono when a family is missing.
        // SF faces are not in-repo — see `.local/fonts/README.md`.
        for (name, family) in [
            ("font-ui", "SF Pro Text"),
            ("font-ui-medium", "SF Pro Text"),
            ("font-display", "SF Pro Text"),
            ("font-chrome", "SF Pro Text"),
            ("font-mono", "Iosevka Term Slab"),
        ] {
            palette.tokens.insert(
                name.into(),
                Token::new(TokenKind::FontFamily, family, &["font-family"]),
            );
        }
        // Text sizes
        palette.tokens.insert(
            "text-caption".into(),
            Token::new(TokenKind::TextSize, "11px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-body".into(),
            Token::new(TokenKind::TextSize, "12px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-body-lg".into(),
            Token::new(TokenKind::TextSize, "13px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-heading".into(),
            Token::new(TokenKind::TextSize, "16px", &["text-size"]),
        );
        palette.tokens.insert(
            "text-display".into(),
            Token::new(TokenKind::TextSize, "20px", &["text-size"]),
        );
        // Spacing
        palette.tokens.insert(
            "space-xs".into(),
            Token::new(TokenKind::Space, "4px", &["space"]),
        );
        palette.tokens.insert(
            "space-sm".into(),
            Token::new(TokenKind::Space, "8px", &["space"]),
        );
        palette.tokens.insert(
            "space-md".into(),
            Token::new(TokenKind::Space, "12px", &["space"]),
        );
        palette.tokens.insert(
            "space-lg".into(),
            Token::new(TokenKind::Space, "16px", &["space"]),
        );
        palette.tokens.insert(
            "space-xl".into(),
            Token::new(TokenKind::Space, "20px", &["space"]),
        );
        palette.tokens.insert(
            "space-xxl".into(),
            Token::new(TokenKind::Space, "24px", &["space"]),
        );
        // Radius — kit control scale (sola-kit-ds)
        palette.tokens.insert(
            "radius-sm".into(),
            Token::new(TokenKind::Radius, "5px", &["radius"]),
        );
        palette.tokens.insert(
            "radius-md".into(),
            Token::new(TokenKind::Radius, "7px", &["radius"]),
        );
        palette.tokens.insert(
            "radius-lg".into(),
            Token::new(TokenKind::Radius, "10px", &["radius"]),
        );
        // Shell — sola-shell's customizable chrome. Colors carry alpha
        // (#rrggbbaa). Cmd+Tab HUD materials (not cyan glass); see
        // docs/specs/2026-06-06-shell-customization-design.md and
        // docs/specs/2026-07-20-macos-look-and-feel-roadmap.md P6.
        palette.tokens.insert(
            "shell-menubar-bg".into(),
            Token::new(TokenKind::Color, "#050505", &["shell"]),
        );
        // The flower in the top row: Sola's amber until a look recolors
        // it (a picture's scheme or your accent, as the accent).
        palette.tokens.insert(
            "shell-flower".into(),
            Token::new(TokenKind::Color, "#ffb800", &["shell"]),
        );
        palette.tokens.insert(
            "shell-backdrop-dim".into(),
            Token::new(TokenKind::Color, "#00000099", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-bg".into(),
            Token::new(TokenKind::Color, "#151515e6", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-border".into(),
            Token::new(TokenKind::Color, "#ffffff1a", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-pad".into(),
            Token::new(TokenKind::Space, "14px", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-tile-pad".into(),
            Token::new(TokenKind::Space, "8px", &["shell"]),
        );
        palette.tokens.insert(
            "shell-launcher-width".into(),
            Token::new(TokenKind::Space, "560px", &["shell"]),
        );
        palette.tokens.insert(
            "shell-launcher-pad".into(),
            Token::new(TokenKind::Space, "8px", &["shell"]),
        );
        palette
    }

    /// The light seed: [`Palette::seed`] with every color in its light
    /// form (Settings → Appearance → Light, `Appearance.light`). Surfaces
    /// mirror the dark ladder — the canvas lightest, panels and hovers a
    /// step darker, lines darker still — the text is near black, and the
    /// accent and status colors are deepened to read on white. Sizes,
    /// fonts and spacing are the dark seed's.
    pub fn seed_light() -> Self {
        let mut palette = Self::seed();
        for (name, value) in LIGHT {
            if let Some(token) = palette.tokens.get_mut(*name) {
                token.value = (*value).to_string();
            }
        }
        palette
    }

    /// [`Palette::seed_light`] when `light`, else [`Palette::seed`].
    pub fn seed_for(light: bool) -> Self {
        if light { Self::seed_light() } else { Self::seed() }
    }
}

/// The light seed's colors (OKLCH lightness in the comments): every color
/// token of the dark seed has one.
const LIGHT: &[(&str, &str)] = &[
    ("bg-primary", "#f8f8f8"),    // 0.98
    ("bg-secondary", "#ededed"),  // 0.945
    ("bg-tertiary", "#e4e4e4"),   // 0.92
    ("bg-hover", "#e4e4e4"),      // 0.92
    ("border", "#d1d1d1"),        // 0.86
    ("border-subtle", "#e4e4e4"), // 0.92
    ("text-primary", "#1d1d1d"),  // 0.23
    ("text-secondary", "#5b5b5b"), // 0.47
    ("text-tertiary", "#7a7a7a"), // 0.58
    ("text-muted", "#c8c8c8"),
    // The seed cyan's hue at 0.58.
    ("text-accent", "#0289a0"),
    ("accent", "#0289a0"),
    ("accent-dim", "rgba(2, 137, 160, 0.12)"),
    ("selection", "#dedede"),     // 0.90
    ("highlight", "#d7d7d7"),     // 0.88
    // The dark seed's hues at 0.58.
    ("danger", "#c64b55"),
    ("success", "#01915d"),
    ("warning", "#9a7302"),
    ("shell-menubar-bg", "#ffffff"),
    ("shell-flower", "#f0a500"),
    ("shell-backdrop-dim", "#00000040"),
    ("shell-switcher-bg", "#ededede6"),
    ("shell-switcher-border", "#0000001a"),
];
