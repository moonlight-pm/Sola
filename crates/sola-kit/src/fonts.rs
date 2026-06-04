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
///
/// Also calls [`ensure_system_fonts`] so a single call at startup wires
/// up *both* the shipped fonts (via the returned bytes) and every
/// system-installed family (directly on iced's font db).
pub fn load_all() -> Vec<Vec<u8>> {
    ensure_system_fonts();
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

/// Register every system-installed font into iced's *global* font
/// database so `Font::with_name(family)` resolves for any family the
/// font picker offers.
///
/// iced 0.14 builds its `cosmic_text::FontSystem` via `new_with_fonts`,
/// which — unlike cosmic-text's own `FontSystem::new()` — does **not**
/// call `load_system_fonts()`. So out of the box iced can only render
/// fonts registered through the `.font(bytes)` builder; picking a
/// system family from the picker silently falls back to the default
/// face (this is why a font swap appeared to do nothing). We load the
/// system fonts straight onto iced's shared db, which is mmap-backed
/// and lazy, so the cost is a directory scan, not a full read of every
/// face. Idempotent — guarded to run once per process.
pub fn ensure_system_fonts() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| match iced_graphics::text::font_system().write() {
        Ok(mut fs) => {
            fs.raw().db_mut().load_system_fonts();
            tracing::info!("loaded system fonts into iced font db");
        }
        Err(err) => {
            tracing::warn!("iced font system lock poisoned, skipping system fonts: {err}");
        }
    });
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
#[derive(Debug, Clone, PartialEq)]
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

/// Real per-em metrics of a monospace font, read from its TTF tables.
///
/// Both ratios are normalised by the font's `units_per_em`, so they are
/// independent of point size: multiply by the glyph point size to get the
/// pixel advance / line box. Defaults to JetBrains Mono's measured values so
/// a consumer that can't resolve the active font still gets a sane cell box.
#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    /// Horizontal advance of one cell / em (monospace advance ÷ unitsPerEm).
    pub advance_per_em: f32,
    /// Line height / em = (ascent − descent + lineGap) ÷ unitsPerEm.
    pub line_per_em: f32,
}

impl Default for FontMetrics {
    fn default() -> Self {
        // JetBrains Mono: advance 600 / upm 1000 = 0.6; line box
        // (1020 − (−300) + 0) / 1000 = 1.32. Matches what the parser
        // reads from JetBrainsMono-Regular.ttf, so the fallback path and
        // the parsed path agree when the active mono IS JetBrains Mono.
        Self { advance_per_em: 0.6, line_per_em: 1.32 }
    }
}

/// Real metrics of whatever font [`mono`] currently resolves to.
///
/// Resolves the current mono family name, then measures it off the **same**
/// font database iced renders from (`iced_graphics::text::font_system()`'s
/// `fontdb::Database`). That db holds both the shipped fonts (registered via
/// the iced builder's `.font(bytes)`) and the system fonts (loaded by
/// [`ensure_system_fonts`]), so any family iced can actually render — including
/// `.ttc` collections like `Iosevka Term Slab` — gets correct metrics here.
///
/// Falls back to a direct scan of the shipped [`FONT_FILES`] on disk (so the
/// deterministic JetBrains-Mono path holds even before iced has populated its
/// db), then to [`FontMetrics::default`] (JetBrains Mono's ratios) when the
/// family can't be measured at all. Called rarely (startup + on font change),
/// so the db query + parse on each call is fine.
pub fn mono_metrics() -> FontMetrics {
    let font = mono();
    let family = match font.family {
        iced::font::Family::Name(name) => name,
        _ => {
            tracing::warn!("mono font has no named family; using default metrics");
            return FontMetrics::default();
        }
    };
    let want = family.trim();

    // 1. Measure off iced's shared font db (covers shipped + system, incl. .ttc).
    if let Some(m) = mono_metrics_from_db(want) {
        return m;
    }

    // 2. Fallback: scan the shipped TTFs on disk. Keeps the deterministic
    //    JetBrains-Mono path working in environments where iced hasn't
    //    populated its db yet (e.g. headless unit tests).
    for relative in FONT_FILES {
        let path = format!("{FONT_DIR}/{relative}");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue, // missing files already warned by load_all
        };
        let face = match ttf_parser::Face::parse(&bytes, 0) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if !face_family_matches(&face, want) {
            continue;
        }
        if let Some(m) = metrics_from_face(&face) {
            return m;
        }
    }

    tracing::warn!(
        family = %want,
        "mono family not found in iced font db or shipped fonts; using default metrics"
    );
    FontMetrics::default()
}

/// Query iced's shared font db for `want` and read its metrics off the matched
/// face. Returns `None` if the family isn't in the db, the lock is poisoned, or
/// the face can't be parsed. Handles `.ttc` collections by passing the face's
/// `face_index` to the parser.
fn mono_metrics_from_db(want: &str) -> Option<FontMetrics> {
    // The db must be populated before we read it; `ensure_system_fonts` is
    // idempotent, so calling it here is cheap on repeat.
    ensure_system_fonts();

    // cosmic-text 0.15 only exposes `db_mut()` (no `&Database` accessor), and
    // `FontSystem::raw()` takes `&mut self`, so we need the write lock even
    // though we only read.
    let mut fs = match iced_graphics::text::font_system().write() {
        Ok(fs) => fs,
        Err(err) => {
            tracing::warn!("iced font system lock poisoned, can't measure mono: {err}");
            return None;
        }
    };
    let db = fs.raw().db_mut();

    let query = fontdb::Query {
        families: &[fontdb::Family::Name(want)],
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    };
    let id = db.query(&query)?;

    db.with_face_data(id, |data, face_index| {
        ttf_parser::Face::parse(data, face_index)
            .ok()
            .and_then(|f| metrics_from_face(&f))
    })
    .flatten()
}

/// True when `face`'s embedded family name (English `Family` id 1 or
/// `TypographicFamily` id 16) matches `want`, case-insensitive and trimmed.
fn face_family_matches(face: &ttf_parser::Face, want: &str) -> bool {
    for name in face.names() {
        if name.name_id == ttf_parser::name_id::FAMILY
            || name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY
        {
            if let Some(s) = name.to_string() {
                if s.trim().eq_ignore_ascii_case(want) {
                    return true;
                }
            }
        }
    }
    false
}

/// Read advance/line ratios off a parsed face. `None` if `units_per_em` is 0
/// (malformed) or no representative glyph has an advance.
fn metrics_from_face(face: &ttf_parser::Face) -> Option<FontMetrics> {
    let upm = face.units_per_em();
    if upm == 0 {
        return None;
    }
    let upm = upm as f32;

    // Representative monospace advance. For a fixed-pitch font every glyph
    // shares one advance, so '0' (then 'M', then space) is safe. A non-mono
    // font selected as "mono" would only have *this* glyph's advance, which
    // won't represent every cell — acceptable since "mono" is meant to be a
    // monospace family.
    let advance = ['0', 'M', ' ']
        .iter()
        .filter_map(|c| face.glyph_index(*c))
        .find_map(|g| face.glyph_hor_advance(g))?;

    // descender is negative, so (ascender − descender) adds its magnitude.
    let line = face.ascender() as f32 - face.descender() as f32 + face.line_gap() as f32;

    Some(FontMetrics {
        advance_per_em: advance as f32 / upm,
        line_per_em: line / upm,
    })
}


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


/// Every family name an in-app font picker can offer: the shipped
/// [`INSTALLED_FAMILIES`] plus whatever fontdb finds installed on the
/// system, deduped and sorted. The shipped families are always present
/// even if the system scan misses them, so a picker built on this never
/// loses the kit's own fonts.
pub fn pickable_families() -> Vec<String> {
    let mut all: Vec<String> = INSTALLED_FAMILIES.iter().map(|s| s.to_string()).collect();
    all.extend(system_families());
    dedup_sorted(all)
}

/// Family names fontdb finds installed on the system, sorted and
/// deduped. Empty if the scan finds nothing (e.g. a headless box with
/// no font directories) — callers should fall back to
/// [`INSTALLED_FAMILIES`] or use [`pickable_families`], which folds
/// both.
pub fn system_families() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let names = db
        .faces()
        .filter_map(|f| f.families.first().map(|(name, _)| name.clone()));
    dedup_sorted(names)
}

/// Collect into a sorted, deduplicated `Vec<String>`.
fn dedup_sorted<I: IntoIterator<Item = String>>(iter: I) -> Vec<String> {
    let mut v: Vec<String> = iter.into_iter().collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_sorted_sorts_and_dedupes() {
        let out = dedup_sorted([
            "Inter".to_string(),
            "Arial".to_string(),
            "Inter".to_string(),
            "Zed".to_string(),
        ]);
        assert_eq!(
            out,
            vec!["Arial".to_string(), "Inter".to_string(), "Zed".to_string()]
        );
    }

    #[test]
    fn mono_metrics_default_is_jetbrains_mono_ratios() {
        // The fallback (and the value the parser should read for JetBrains
        // Mono) is advance 0.6 / line 1.32 per em.
        let d = FontMetrics::default();
        assert!((d.advance_per_em - 0.6).abs() < 1e-6);
        assert!((d.line_per_em - 1.32).abs() < 1e-6);
    }

    #[test]
    fn mono_metrics_parses_jetbrains_mono_from_disk() {
        // Default mono is JetBrains Mono. If the font pack is present, the
        // parser must read ≈0.6 / ≈1.32 off the real TTF — this exercises the
        // family match + advance/line reads. When the file isn't in the test
        // env, mono_metrics() falls back to the same default, so the assertion
        // holds either way (the values agree by construction).
        let m = mono_metrics();
        assert!(
            (m.advance_per_em - 0.6).abs() < 0.02,
            "JetBrains Mono advance_per_em ≈ 0.6, got {}",
            m.advance_per_em
        );
        assert!(
            (m.line_per_em - 1.32).abs() < 0.02,
            "JetBrains Mono line_per_em ≈ 1.32, got {}",
            m.line_per_em
        );
    }

    #[test]
    fn pickable_families_always_contains_shipped() {
        let all = pickable_families();
        for fam in INSTALLED_FAMILIES {
            assert!(
                all.contains(&fam.to_string()),
                "{fam} missing from pickable_families"
            );
        }
    }
}
