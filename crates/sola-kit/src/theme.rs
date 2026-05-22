//! Theme — iced `Theme` flavored with the canonical sola palette.
//!
//! The legacy CEF kit ships a richer token system (palette + per-component
//! bindings, broadcast as CSS over `Topic::Theme`). Iced apps can't consume
//! CSS, so for now we resolve the same color story to an
//! `iced::theme::Palette` at startup and ship it as a `Theme::custom`.
//!
//! Wiring this to the live `Topic::Theme` bus update is a v0.2 task — see
//! `docs/vault/sola-kit.md` for the migration plan. Until then, every kit
//! app calls [`default_theme`] once at startup and that's it.

use iced::{Color, Theme};

/// Theme name reported back to iced — appears in `Theme::name()` and is
/// what `pick_list(Theme::ALL, …)` would display if we ever exposed it.
pub const THEME_NAME: &str = "sola";

/// Canonical sola palette as raw hex strings — the source of truth that
/// the iced `Palette` below mirrors. Keep these in sync if you tweak
/// either; we deliberately don't share atoms with the legacy kit yet
/// because that protocol is CSS-oriented (see crate docs).
pub mod hex {
    pub const BG: &str = "#0d1117";
    pub const FG: &str = "#c9d1d9";
    pub const ACCENT: &str = "#58a6ff";
    pub const SUCCESS: &str = "#3fb950";
    pub const WARNING: &str = "#d29922";
    pub const DANGER: &str = "#f85149";
    /// Muted variant of the foreground — used for secondary text
    /// (timestamps, deemphasized cells).
    pub const FG_MUTED: &str = "#6e7681";
    /// Slightly lifted background — used for panels, sticky cards,
    /// and other surfaces that should read as "above" the canvas.
    pub const BG_RAISED: &str = "#161b22";
    /// Even more lifted — hover / selected rows.
    pub const BG_HOVER: &str = "#21262d";
    /// 1px hairline color used for dividers between cells.
    pub const BORDER: &str = "#30363d";
}

/// Build the canonical sola iced theme. Apps call this in their
/// `theme(&self) -> Theme` method. Returning a fresh `Theme::custom`
/// per call is cheap (palette is `Copy`).
pub fn default_theme() -> Theme {
    Theme::custom(
        THEME_NAME.to_string(),
        iced::theme::Palette {
            background: parse(hex::BG),
            text: parse(hex::FG),
            primary: parse(hex::ACCENT),
            success: parse(hex::SUCCESS),
            warning: parse(hex::WARNING),
            danger: parse(hex::DANGER),
        },
    )
}

/// Parse `#rrggbb` into an iced `Color`. Panics on malformed input —
/// the inputs are compile-time constants in this crate, so the panic
/// is a self-check rather than a runtime concern.
pub fn parse(s: &str) -> Color {
    let s = s.trim_start_matches('#');
    assert_eq!(s.len(), 6, "expected #rrggbb, got {s:?}");
    let r = u8::from_str_radix(&s[0..2], 16).expect("rr") as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).expect("gg") as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).expect("bb") as f32 / 255.0;
    Color::from_rgb(r, g, b)
}
