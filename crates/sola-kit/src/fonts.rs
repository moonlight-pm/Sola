//! Shared font constants + system-font loading helper.
//!
//! Sola no longer bundles or registers font files. Fonts are resolved
//! by family name through the system fontconfig database, loaded into
//! iced's font db by [`ensure_system_fonts`].
//!
//! Defaults prefer **SF Pro Text** (UI chrome) and **Iosevka Term Slab**
//! (mono) when installed; fall back to Inter / JetBrains Mono. Licensed
//! SF faces live in a **gitignored** stash at `.local/fonts/` (see that
//! README) and must be installed system-wide (`fc-cache`) to be used.
//! See `docs/manual/distribution.md`.

use iced::Font;

/// Mono font for code, JSON, table rows, terminals.
/// Preferred: `Iosevka Term Slab`. Fallback: `JetBrains Mono`.
pub const MONO: Font = Font::with_name("Iosevka Term Slab");

/// Inter — open-source UI fallback when SF Pro is not installed.
pub const INTER: Font = Font::with_name("Inter");

/// Inter Medium (weight 500) — fallback for medium UI roles.
pub const INTER_MEDIUM: Font = Font {
    weight: iced::font::Weight::Medium,
    ..Font::with_name("Inter")
};

/// SF Pro Text — preferred UI face for macOS-like chrome (when installed).
pub const SF_PRO_TEXT: Font = Font::with_name("SF Pro Text");

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
/// Each field is a `Font` (a family + weight + style). Defaults prefer
/// SF Pro Text for UI-shaped roles and Iosevka Term Slab for mono when
/// those families are installed; otherwise Inter / JetBrains Mono.
/// Components never reach for a family constant directly — they call
/// the role accessors below ([`ui`], [`ui_medium`], [`display`],
/// [`chrome`], [`mono`]).
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
    /// Dense desktop chrome — menubar labels, status extras, section headers.
    pub chrome: Font,
    /// Code, JSON, terminal-shaped output.
    pub mono: Font,
}

impl Default for Fonts {
    fn default() -> Self {
        let ui_fam = preferred_ui_family();
        let mono_fam = preferred_mono_family();
        Self {
            ui: Font::with_name(ui_fam),
            ui_medium: medium(ui_fam),
            display: medium(ui_fam),
            chrome: Font::with_name(ui_fam),
            mono: Font::with_name(mono_fam),
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

    // Measure off iced's shared font db (covers every system-installed
    // family, incl. `.ttc` collections like `Iosevka Term Slab`).
    if let Some(m) = mono_metrics_from_db(want) {
        return m;
    }

    tracing::warn!(
        family = %want,
        "mono family not found in iced font db; using default metrics"
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


/// Recommended families Sola defaults to (and common fallbacks). These seed
/// the font picker vocabulary; [`pickable_families`] folds in every
/// system-installed family on top. SF faces are **not** shipped in-repo —
/// see `.local/fonts/README.md`.
pub const INSTALLED_FAMILIES: &[&str] = &[
    "SF Pro Text",
    "SF Pro Display",
    "Inter",
    "Iosevka Term Slab",
    "JetBrains Mono",
];

/// Preferred UI family when installed (macOS chrome feel).
pub const DEFAULT_UI_FAMILY: &str = "SF Pro Text";
/// Fallback UI family (always expected via distro packages).
pub const FALLBACK_UI_FAMILY: &str = "Inter";
/// Preferred mono family (terminals, code).
pub const DEFAULT_MONO_FAMILY: &str = "Iosevka Term Slab";
/// Fallback mono family.
pub const FALLBACK_MONO_FAMILY: &str = "JetBrains Mono";

/// Resolve UI family: SF Pro Text if available, else Inter.
pub fn preferred_ui_family() -> &'static str {
    if family_available(DEFAULT_UI_FAMILY) {
        DEFAULT_UI_FAMILY
    } else {
        FALLBACK_UI_FAMILY
    }
}

/// Resolve mono family: Iosevka Term Slab if available, else JetBrains Mono.
pub fn preferred_mono_family() -> &'static str {
    if family_available(DEFAULT_MONO_FAMILY) {
        DEFAULT_MONO_FAMILY
    } else if family_available(FALLBACK_MONO_FAMILY) {
        FALLBACK_MONO_FAMILY
    } else {
        DEFAULT_MONO_FAMILY
    }
}

/// Build a `Fonts` table from a per-role family-name selection.
///
/// If a requested family is empty or not present in iced's font database,
/// that role falls back through preferred → distro fallback so we never
/// emit a `Font` that fontconfig remaps to an unrelated face.
pub fn fonts_from_families(
    ui_family: &str,
    ui_medium_family: &str,
    display_family: &str,
    chrome_family: &str,
    mono_family: &str,
) -> Fonts {
    let ui_default = preferred_ui_family();
    let mono_default = preferred_mono_family();
    let pick = |requested: &str, default: &'static str| -> &'static str {
        if !requested.is_empty() && family_available(requested) {
            static_family(requested)
        } else {
            default
        }
    };

    Fonts {
        ui: Font::with_name(pick(ui_family, ui_default)),
        ui_medium: medium(pick(ui_medium_family, ui_default)),
        display: medium(pick(display_family, ui_default)),
        chrome: Font::with_name(pick(chrome_family, ui_default)),
        mono: Font::with_name(pick(mono_family, mono_default)),
    }
}

fn medium(family: &'static str) -> Font {
    Font {
        weight: iced::font::Weight::Medium,
        ..Font::with_name(family)
    }
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
/// Families the picker offers = exactly what's installed system-wide
/// (fontconfig); falls back to [`INSTALLED_FAMILIES`] only when no system
/// fonts are found (e.g. a headless test environment with no font dirs).
pub fn pickable_families() -> Vec<String> {
    let sys = system_families();
    if sys.is_empty() {
        // Headless / no fontconfig: fall back to the recommended list so the
        // picker is never empty (and the invariant test holds).
        INSTALLED_FAMILIES.iter().map(|s| s.to_string()).collect()
    } else {
        sys
    }
}/// Family names fontdb finds installed on the system, sorted and

/// True when `family` is present in iced's font database (i.e. the renderer
/// can actually draw it). Mirrors the DB the renderer uses, so "available"
/// == "renders as itself" (not a fontconfig substitute).
///
/// Used by [`fonts_from_families`] to fall back to the role default when a
/// persisted selection names a family that isn't installed.
pub fn family_available(family: &str) -> bool {
    ensure_system_fonts();
    match iced_graphics::text::font_system().write() {
        Ok(mut fs) => {
            let db = fs.raw().db_mut();
            db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            })
            .is_some()
        }
        // Lock poisoned — can't check; assume available so we don't
        // silently override a font that might actually work.
        Err(_) => true,
    }
}
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
    fn mono_metrics_default_is_sane_cell_ratios() {
        // Compile-time fallback when no face can be measured (headless /
        // missing mono). Not tied to a specific family.
        let d = FontMetrics::default();
        assert!((d.advance_per_em - 0.6).abs() < 1e-6);
        assert!((d.line_per_em - 1.32).abs() < 1e-6);
    }

    #[test]
    fn mono_metrics_resolves_active_mono_from_system_db() {
        // Preferred mono is Iosevka Term Slab when installed. Metrics must
        // be positive and mono-like (advance roughly 0.45–0.65 em).
        ensure_system_fonts();
        let m = mono_metrics();
        assert!(
            m.advance_per_em > 0.4 && m.advance_per_em < 0.75,
            "mono advance_per_em out of range: {}",
            m.advance_per_em
        );
        assert!(
            m.line_per_em > 1.0 && m.line_per_em < 1.6,
            "mono line_per_em out of range: {}",
            m.line_per_em
        );
    }

    // pickable_families() returns system-installed families (or
    // INSTALLED_FAMILIES on headless). Only require that installed
    // fallbacks appear — SF Pro may be absent until the user installs
    // faces from `.local/fonts/`.
    #[test]
    fn pickable_families_always_contains_shipped() {
        let all = pickable_families();
        let sys = system_families();
        if sys.is_empty() {
            for fam in INSTALLED_FAMILIES {
                assert!(
                    all.contains(&fam.to_string()),
                    "{fam} missing from headless pickable_families"
                );
            }
            return;
        }
        for fam in [FALLBACK_UI_FAMILY, FALLBACK_MONO_FAMILY, DEFAULT_MONO_FAMILY] {
            if family_available(fam) {
                assert!(
                    all.contains(&fam.to_string()),
                    "{fam} installed but missing from pickable_families"
                );
            }
        }
    }

    // When system fonts are present the picker returns only families that
    // actually pass family_available(), plus dedup guarantees no duplicates.
    // This test checks structural invariants rather than exact membership so
    // it stays valid regardless of which families are installed on the host.
    #[test]
    fn pickable_families_only_installed() {
        let sys = system_families();
        // Only run structural assertions when there are real system fonts;
        // headless CI uses the INSTALLED_FAMILIES fallback path which is
        // covered by pickable_families_always_contains_shipped above.
        if sys.is_empty() {
            return;
        }
        let all = pickable_families();
        // No duplicates.
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(all.len(), sorted.len(), "pickable_families contains duplicates");
        // Every returned family either passes family_available() or is one of
        // the INSTALLED_FAMILIES fallback entries (which are valid system names
        // on this machine since sys was non-empty above).
        let fallbacks: Vec<String> = INSTALLED_FAMILIES.iter().map(|s| s.to_string()).collect();
        for fam in &all {
            assert!(
                family_available(fam) || fallbacks.contains(fam),
                "pickable_families contains '{}' which is not available and not a known fallback",
                fam
            );
        }
        // The guaranteed-absent sentinel must never appear.
        assert!(
            !all.contains(&"Definitely Not A Real Font Family 9c4e".to_string()),
            "bogus family must not appear in pickable_families"
        );
    }

    #[test]
    fn family_available_known_and_unknown() {
        let sys = system_families();
        // Positive assertion only makes sense when the system DB is populated.
        if !sys.is_empty() {
            // At least one of our mono defaults should be present on dev boxes.
            assert!(
                family_available(DEFAULT_MONO_FAMILY)
                    || family_available(FALLBACK_MONO_FAMILY)
                    || family_available(FALLBACK_UI_FAMILY),
                "expected a known Sola font family to be available"
            );
        }
        assert!(
            !family_available("Definitely Not A Real Font 12345"),
            "bogus family should report unavailable"
        );
    }

    #[test]
    fn fonts_from_families_falls_back_for_unavailable() {
        let sys = system_families();
        // Only meaningful when system fonts are present; headless has no DB to
        // query so family_available always returns true (lock-poisoned path),
        // making the fallback unreachable.
        if sys.is_empty() {
            return;
        }
        // Use a family name that can never exist on any real system so this
        // test is environment-independent — we're testing the fallback logic,
        // not whether a specific font is installed.
        let f = fonts_from_families(
            "Inter",
            "Inter",
            "Inter",
            "Inter",
            "Definitely Not A Real Font Family 9c4e", // guaranteed absent → must fall back
        );
        match f.mono.family {
            iced::font::Family::Name(name) => {
                assert_eq!(
                    name, DEFAULT_MONO_FAMILY,
                    "unavailable mono family should fall back to {DEFAULT_MONO_FAMILY}, got {name}"
                );
            }
            other => panic!("expected Family::Name, got {other:?}"),
        }
    }
}
