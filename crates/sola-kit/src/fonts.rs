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

/// Inter — open-source UI font that closely matches Apple's San
/// Francisco. The desktop variant ships as `InterVariable.ttf` and
/// declares family name `Inter`. Used by sola-shell for the
/// menubar / launcher / switcher / menu so the desktop chrome
/// reads the way it did under the legacy CEF shell.
pub const INTER: Font = Font::with_name("Inter");

/// Inter Medium (weight 500) — used for menubar chrome labels and the clock
/// so they read at the same visual weight as the legacy CEF shell's 500-weight
/// Inter text at 13 px.
pub const INTER_MEDIUM: Font = Font {
    weight: iced::font::Weight::Medium,
    ..Font::with_name("Inter")
};

/// SF Pro — Apple's system font (Regular weight). Used for sola-shell menubar
/// labels and the clock.  The TTF is placed manually by the user (Apple
/// license) at `/opt/sola/share/fonts/SFPro/`; it is not redistributed via
/// the asset sync.  See `crates/sola-assets/upstream.toml` for the note.
pub const SF_PRO: Font = Font::with_name("SF Pro");

/// SF Pro Medium (weight 500) — used for the focused-app title in the menubar
/// so it reads slightly heavier than the menu-label text.  The single-weight
/// TTF means cosmic-text may synthesise faux-medium.
pub const SF_PRO_MEDIUM: Font = Font {
    weight: iced::font::Weight::Medium,
    ..Font::with_name("SF Pro")
};

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
    "Inter/InterVariable.ttf",
    "Inter/InterVariable-Italic.ttf",
    // SF Pro — manually placed by user (Apple license, not synced).
    // Install: cp assets/fonts/SF-Pro*.ttf assets/fonts/SF-Compact*.ttf /opt/sola/share/fonts/SFPro/
    "SFPro/SF-Pro.ttf",
    "SFPro/SF-Pro-Italic.ttf",
    "SFPro/SF-Compact.ttf",
    "SFPro/SF-Compact-Italic.ttf",
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
