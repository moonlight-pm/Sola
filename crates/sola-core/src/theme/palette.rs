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
        // Colors — graphite surfaces (sola-kit-ds / sola-agent-ds).
        // Keep in sync with `sola_kit::theme::hex::*`.
        palette
            .tokens
            .insert("bg-primary".into(), Token::new(TokenKind::Color, "#0c0e12", &["surface"]));
        palette
            .tokens
            .insert("bg-secondary".into(), Token::new(TokenKind::Color, "#151922", &["surface"]));
        palette
            .tokens
            .insert("bg-tertiary".into(), Token::new(TokenKind::Color, "#1e2533", &["surface"]));
        palette
            .tokens
            .insert("bg-hover".into(), Token::new(TokenKind::Color, "#1e2533", &["surface"]));
        // Colors — borders
        palette
            .tokens
            .insert("border".into(), Token::new(TokenKind::Color, "#2a3344", &["border"]));
        palette
            .tokens
            .insert("border-subtle".into(), Token::new(TokenKind::Color, "#232a38", &["border"]));
        // Colors — text
        palette
            .tokens
            .insert("text-primary".into(), Token::new(TokenKind::Color, "#e9ecf2", &["text"]));
        palette
            .tokens
            .insert("text-secondary".into(), Token::new(TokenKind::Color, "#8b94a8", &["text"]));
        palette
            .tokens
            .insert("text-tertiary".into(), Token::new(TokenKind::Color, "#8b94a8", &["text"]));
        // text-muted doubles as a strong border (e.g. scrollbar thumb on
        // hover) which is why it's eligible for both `text` and `border`.
        palette
            .tokens
            .insert(
                "text-muted".into(),
                Token::new(TokenKind::Color, "#2a3344", &["text", "border"]),
            );
        palette.tokens.insert(
            "text-accent".into(),
            Token::new(TokenKind::Color, "#3dd6f5", &["text", "accent"]),
        );
        // Colors — accent + status
        palette
            .tokens
            .insert("accent".into(), Token::new(TokenKind::Color, "#3dd6f5", &["accent"]));
        palette.tokens.insert(
            "accent-dim".into(),
            Token::new(TokenKind::Color, "rgba(61, 214, 245, 0.10)", &["accent-tint"]),
        );
        // Quiet selection fill (kit `hex::SELECTION`).
        palette.tokens.insert(
            "selection".into(),
            Token::new(TokenKind::Color, "#163842", &["surface", "accent-tint"]),
        );
        palette
            .tokens
            .insert("danger".into(), Token::new(TokenKind::Color, "#f07178", &["status"]));
        palette
            .tokens
            .insert("success".into(), Token::new(TokenKind::Color, "#3ecf8e", &["status"]));
        palette
            .tokens
            .insert("warning".into(), Token::new(TokenKind::Color, "#e8b84a", &["status"]));
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
        // Shell — sola-shell's customizable chrome. Colors carry alpha
        // (#rrggbbaa). Cmd+Tab HUD materials (not cyan glass); see
        // docs/specs/2026-06-06-shell-customization-design.md and
        // docs/specs/2026-07-20-macos-look-and-feel-roadmap.md P6.
        palette
            .tokens
            .insert("shell-menubar-bg".into(), Token::new(TokenKind::Color, "#000000", &["shell"]));
        palette.tokens.insert(
            "shell-backdrop-dim".into(),
            Token::new(TokenKind::Color, "#00000099", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-bg".into(),
            Token::new(TokenKind::Color, "#1c1c1ee6", &["shell"]),
        );
        palette.tokens.insert(
            "shell-switcher-border".into(),
            Token::new(TokenKind::Color, "#ffffff1a", &["shell"]),
        );
        palette
            .tokens
            .insert("shell-switcher-pad".into(), Token::new(TokenKind::Space, "14px", &["shell"]));
        palette.tokens.insert(
            "shell-switcher-tile-pad".into(),
            Token::new(TokenKind::Space, "8px", &["shell"]),
        );
        palette.tokens.insert(
            "shell-launcher-width".into(),
            Token::new(TokenKind::Space, "560px", &["shell"]),
        );
        palette
            .tokens
            .insert("shell-launcher-pad".into(), Token::new(TokenKind::Space, "8px", &["shell"]));
        palette
    }
}
