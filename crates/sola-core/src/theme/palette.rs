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
        // Colors — surfaces
        palette
            .tokens
            .insert("bg-primary".into(), Token::new(TokenKind::Color, "#0d1117", &["surface"]));
        palette
            .tokens
            .insert("bg-secondary".into(), Token::new(TokenKind::Color, "#161b22", &["surface"]));
        palette
            .tokens
            .insert("bg-tertiary".into(), Token::new(TokenKind::Color, "#1c2129", &["surface"]));
        palette
            .tokens
            .insert("bg-hover".into(), Token::new(TokenKind::Color, "#1a2030", &["surface"]));
        // Colors — borders
        palette
            .tokens
            .insert("border".into(), Token::new(TokenKind::Color, "#2d333b", &["border"]));
        palette
            .tokens
            .insert("border-subtle".into(), Token::new(TokenKind::Color, "#21262d", &["border"]));
        // Colors — text
        palette
            .tokens
            .insert("text-primary".into(), Token::new(TokenKind::Color, "#e6edf3", &["text"]));
        palette
            .tokens
            .insert("text-secondary".into(), Token::new(TokenKind::Color, "#8b949e", &["text"]));
        palette
            .tokens
            .insert("text-tertiary".into(), Token::new(TokenKind::Color, "#6e7681", &["text"]));
        // text-muted is the brightest of the gray atoms; doubles as a
        // strong border (e.g. scrollbar thumb on hover) which is why
        // it's eligible for both `text` and `border` groups.
        palette
            .tokens
            .insert(
                "text-muted".into(),
                Token::new(TokenKind::Color, "#484f58", &["text", "border"]),
            );
        palette.tokens.insert(
            "text-accent".into(),
            Token::new(TokenKind::Color, "#58a6ff", &["text", "accent"]),
        );
        // Colors — accent + status
        palette
            .tokens
            .insert("accent".into(), Token::new(TokenKind::Color, "#00d4ff", &["accent"]));
        palette.tokens.insert(
            "accent-dim".into(),
            Token::new(TokenKind::Color, "rgba(0, 212, 255, 0.12)", &["accent-tint"]),
        );
        palette
            .tokens
            .insert("danger".into(), Token::new(TokenKind::Color, "#f85149", &["status"]));
        palette
            .tokens
            .insert("success".into(), Token::new(TokenKind::Color, "#3fb950", &["status"]));
        palette
            .tokens
            .insert("warning".into(), Token::new(TokenKind::Color, "#d29922", &["status"]));
        // Fonts — the kit's semantic role vocabulary
        // (`font-ui` / `font-ui-medium` / `font-display` / `font-chrome` /
        // `font-mono`). Defaults are the kit's role defaults: Inter for
        // every UI-shaped role, JetBrains Mono for code. The kit's
        // `fonts_from_families` applies the Medium weight to the
        // `ui-medium` / `display` roles, so the *family* name is "Inter"
        // for all four sans roles. Picker selections overwrite these at
        // runtime. Both families must be installed system-wide (see
        // docs/manual/distribution.md).
        for (name, family) in [
            ("font-ui", "Inter"),
            ("font-ui-medium", "Inter"),
            ("font-display", "Inter"),
            ("font-chrome", "Inter"),
            ("font-mono", "JetBrains Mono"),
        ] {
            palette.tokens.insert(
                name.into(),
                Token::new(TokenKind::FontFamily, family, &["font-family"]),
            );
        }
        // Text sizes
        palette
            .tokens
            .insert("text-caption".into(), Token::new(TokenKind::TextSize, "11px", &["text-size"]));
        palette
            .tokens
            .insert("text-body".into(), Token::new(TokenKind::TextSize, "12px", &["text-size"]));
        palette
            .tokens
            .insert("text-body-lg".into(), Token::new(TokenKind::TextSize, "13px", &["text-size"]));
        palette
            .tokens
            .insert("text-heading".into(), Token::new(TokenKind::TextSize, "16px", &["text-size"]));
        palette
            .tokens
            .insert("text-display".into(), Token::new(TokenKind::TextSize, "20px", &["text-size"]));
        // Spacing
        palette
            .tokens
            .insert("space-xs".into(), Token::new(TokenKind::Space, "4px", &["space"]));
        palette
            .tokens
            .insert("space-sm".into(), Token::new(TokenKind::Space, "8px", &["space"]));
        palette
            .tokens
            .insert("space-md".into(), Token::new(TokenKind::Space, "12px", &["space"]));
        palette
            .tokens
            .insert("space-lg".into(), Token::new(TokenKind::Space, "16px", &["space"]));
        palette
            .tokens
            .insert("space-xl".into(), Token::new(TokenKind::Space, "20px", &["space"]));
        palette
            .tokens
            .insert("space-xxl".into(), Token::new(TokenKind::Space, "24px", &["space"]));
        // Radius
        palette
            .tokens
            .insert("radius-sm".into(), Token::new(TokenKind::Radius, "3px", &["radius"]));
        palette
            .tokens
            .insert("radius-md".into(), Token::new(TokenKind::Radius, "4px", &["radius"]));
        palette
            .tokens
            .insert("radius-lg".into(), Token::new(TokenKind::Radius, "6px", &["radius"]));
        palette
    }
}
