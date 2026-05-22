//! Shared font constants + loading helper.
//!
//! Fonts ship under `/opt/sola/share/fonts/` (synced by
//! `cargo make assets sync`). Each pack is a single TTF whose
//! `name` field declares the family the constants below reference.
//! Missing files warn but don't kill the app — a binary built
//! against an out-of-date `/opt/sola/share` should still launch
//! with fallback fonts.

use iced::Font;

/// Font pack directory shared with every other sola process.
pub const FONT_DIR: &str = "/opt/sola/share/fonts";

/// Mono font for code, JSON, table rows. JetBrainsMono-Regular.ttf
/// declares itself as `JetBrains Mono`.
pub const MONO: Font = Font::with_name("JetBrains Mono");

/// Default sans for body / UI text — variable Roboto Flex,
/// family name `Roboto Flex`.
pub const NORMAL: Font = Font::with_name("Roboto Flex");

/// Condensed sans for buttons, headers, and other chrome widgets
/// that need to fit tightly. `Roboto Condensed`.
pub const CONDENSED: Font = Font::with_name("Roboto Condensed");

/// Bold variant for prominent labels — the regular weight reads
/// too thin at small sizes. cosmic-text falls back to faux-bold
/// synthesis without the matching TTF, so we ship the explicit
/// `RobotoCondensed-Bold.ttf`.
pub const CONDENSED_BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::with_name("Roboto Condensed")
};

/// Font files registered at startup, relative to [`FONT_DIR`].
/// Order matches the constants above — change in lockstep.
pub const FONT_FILES: &[&str] = &[
    "JetBrainsMono/JetBrainsMono-Regular.ttf",
    "RobotoFlex/RobotoFlex.ttf",
    "RobotoCondensed/RobotoCondensed-Regular.ttf",
    "RobotoCondensed/RobotoCondensed-Bold.ttf",
];

/// Read the kit's standard font files off disk. Caller passes the
/// returned bytes to `iced::application(...).font(bytes)` (or the
/// equivalent for whatever iced builder it has in hand). Missing
/// files log a warning and are skipped.
pub fn load_all() -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(FONT_FILES.len());
    for relative in FONT_FILES {
        let path = format!("{FONT_DIR}/{relative}");
        match std::fs::read(&path) {
            Ok(bytes) => {
                tracing::info!(path = %path, bytes = bytes.len(), "registering font");
                out.push(bytes);
            }
            Err(e) => {
                tracing::warn!(path = %path, "skipping font: {e}");
            }
        }
    }
    out
}
