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
    // Iosevka Fixed — manually placed by user. cosmic-text needs TTF/OTF;
    // the WOFF2 we ship for sola-terminal won't load here.
    // Install: cp assets/fonts/Iosevka-Fixed*.ttf /opt/sola/share/fonts/Iosevka/
    "Iosevka/Iosevka-Fixed.ttf",
    "Iosevka/Iosevka-Fixed-Bold.ttf",
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


/// Semantic font roles — what kit components actually reach for.
///
/// Each field is a `Font` (a family + weight + style). Defaults bias
/// toward SF Pro for everything UI-shaped, with JetBrains Mono for
/// code; apps that want a different family install a custom `Fonts`
/// at startup via [`install`]. Components never reach for a family
/// constant directly — they call the role accessors below
/// ([`ui`], [`ui_medium`], [`display`], [`chrome`], [`mono`]).
///
/// The role vocabulary mirrors how the legacy CEF kit named font
/// tokens (`--font-ui`, `--font-display`, …). Adding a new role is a
/// matter of adding a field, a default, and an accessor.
#[derive(Debug, Clone)]
pub struct Fonts {
    /// Default body / sidebar item / button text.
    pub ui: Font,
    /// 500-weight UI emphasis (focused-app title, active row text).
    pub ui_medium: Font,
    /// Page titles, large headings.
    pub display: Font,
    /// Dense desktop chrome — small uppercase labels, section headers.
    pub chrome: Font,
    /// Code, JSON, terminal-shaped output.
    pub mono: Font,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            ui: SF_PRO,
            ui_medium: SF_PRO_MEDIUM,
            display: SF_PRO_MEDIUM,
            chrome: SF_PRO,
            mono: MONO,
        }
    }
}

/// Process-wide font role table. Lazily initialised to
/// [`Fonts::default`]; [`install`] swaps the whole table so bus-driven
/// theme deliveries can re-pick fonts at runtime.
static FONTS: std::sync::LazyLock<std::sync::RwLock<Fonts>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(Fonts::default()));

/// Swap the kit's role→family mapping for this process. Re-callable —
/// bus-driven theme deliveries call this on every `Topic::Theme` so a
/// font edit in the storybook propagates everywhere on the next render.
pub fn install(fonts: Fonts) {
    if let Ok(mut guard) = FONTS.write() {
        *guard = fonts;
    }
}

/// Snapshot the currently-installed `Fonts` table by value. The role
/// accessors below all go through this; a momentary read lock per call
/// is cheap and keeps callers from holding the lock across rendering.
fn current() -> Fonts {
    FONTS.read().map(|g| g.clone()).unwrap_or_default()
}

pub fn ui() -> Font { current().ui }
pub fn ui_medium() -> Font { current().ui_medium }
pub fn display() -> Font { current().display }
pub fn chrome() -> Font { current().chrome }
pub fn mono() -> Font { current().mono }


/// Font family names the kit ships and loads at boot. These are the
/// values a settings UI offers in a font picker, and the strings the
/// bus theme's `FontFamily` tokens carry. Order is roughly UI-shaped
/// first, condensed/chrome middle, mono last.
pub const INSTALLED_FAMILIES: &[&str] = &[
    "SF Pro",
    "Inter",
    "Roboto Flex",
    "Roboto Condensed",
    "JetBrains Mono",
    "Iosevka Fixed",
];

/// Build a `Fonts` table from a per-role family-name selection.
/// Unknown family names round-trip back through `Font::with_name` and
/// cosmic-text picks a fallback at shape time. `medium_for` flips the
/// weight to Medium when the role calls for emphasis (`ui_medium`).
pub fn fonts_from_families(
    ui_family: &str,
    ui_medium_family: &str,
    display_family: &str,
    chrome_family: &str,
    mono_family: &str,
) -> Fonts {
    Fonts {
        ui: Font::with_name(static_family(ui_family)),
        ui_medium: medium(static_family(ui_medium_family)),
        display: medium(static_family(display_family)),
        chrome: Font::with_name(static_family(chrome_family)),
        mono: Font::with_name(static_family(mono_family)),
    }
}

fn medium(family: &'static str) -> Font {
    Font { weight: iced::font::Weight::Medium, ..Font::with_name(family) }
}

/// `iced::Font::with_name` needs a `&'static str`. Intern incoming
/// family names by matching against [`INSTALLED_FAMILIES`]; unknown
/// names leak via `Box::leak` so we never have to chase lifetimes
/// through the role accessors. Family-name churn is bounded (a handful
/// per session even with hot edits), so the leak is acceptable.
fn static_family(name: &str) -> &'static str {
    for f in INSTALLED_FAMILIES {
        if *f == name {
            return f;
        }
    }
    Box::leak(name.to_string().into_boxed_str())
}
