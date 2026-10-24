//! Colors that follow the background picture ("adapt the theme to the
//! background", `Topic::Appearance`): [`Scheme::from_pixels`] samples the
//! picture's hues, and [`Theme::adapt`] rewrites the color tokens from it —
//! every surface, border and text color keeps the **lightness** of its
//! grey in the seed palette (so contrast is the grey theme's) and takes a
//! little of the picture's hue, and the accent takes the picture's most
//! vivid hue. [`Theme::unadapt`] puts the seed's greys back.
//!
//! Colors are worked in OKLab / OKLCH (Björn Ottosson), where lightness
//! and chroma are perceptual, so a tint reads the same strength in every
//! hue.

use std::f32::consts::PI;

use super::types::{Palette, Theme, TokenKind};

/// What a picture gives the theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheme {
    /// The surfaces' hue (degrees), the picture's dominant hue, and their
    /// chroma at full strength: as strong as the picture is colorful (0: a
    /// grey picture).
    pub tint_hue: f32,
    pub tint_chroma: f32,
    /// The accent's hue (degrees) and chroma.
    pub accent_hue: f32,
    pub accent_chroma: f32,
}

/// Hue bins of the histogram (10° each).
const BINS: usize = 36;
/// The most schemes a picture offers.
pub const MAX_VARIATIONS: usize = 4;
/// A major hue is at least this far from the others, and has at least this
/// share of the dominant one's weight.
const HUES_APART: f32 = 30.0;
const HUE_WEIGHT: f32 = 0.05;
/// Pixels below this chroma are grey: they vote for no hue.
const GREY: f32 = 0.02;
/// The strongest tint a surface takes at full strength (OKLCH chroma); up
/// to it, the picture's mean chroma.
const TINT_MAX: f32 = 0.08;
/// Text takes this much of the surfaces' tint.
const TEXT_SHARE: f32 = 0.3;
/// Surfaces darker than this take a thinner tint (a tint reads stronger
/// near black).
const TINT_FULL_L: f32 = 0.18;
/// The accent's lightness (the seed cyan's), the lowest it steps down to
/// for a hue that cannot be vivid that light (blue), and its strongest
/// chroma.
const ACCENT_L: f32 = 0.82;
const ACCENT_L_MIN: f32 = 0.68;
const ACCENT_C: f32 = 0.14;
/// An accent hue at least this far from the tint, when the picture has
/// one with enough weight; else the tint's own hue.
const ACCENT_APART: f32 = 60.0;
const ACCENT_WEIGHT: f32 = 0.2;

/// How each adapted token is made: the seed token whose lightness (and
/// alpha) it keeps, and what it takes from the picture.
const RECIPE: &[(&str, Role)] = &[
    ("bg-primary", Role::Surface),
    ("bg-secondary", Role::Surface),
    ("bg-tertiary", Role::Surface),
    ("bg-hover", Role::Surface),
    ("selection", Role::Surface),
    ("highlight", Role::Surface),
    ("border", Role::Surface),
    ("border-subtle", Role::Surface),
    ("text-muted", Role::Surface),
    ("text-primary", Role::Text),
    ("text-secondary", Role::Text),
    ("text-tertiary", Role::Text),
    ("accent", Role::Accent),
    ("text-accent", Role::Accent),
    ("accent-dim", Role::AccentDim),
    ("shell-menubar-bg", Role::Surface),
    ("shell-switcher-bg", Role::Surface),
];

#[derive(Clone, Copy)]
enum Role {
    Surface,
    Text,
    Accent,
    /// The accent at 10%.
    AccentDim,
}

impl Scheme {
    /// Sample `rgb` (8-bit sRGB triples, a downscaled picture: a few
    /// thousand pixels is plenty).
    pub fn from_pixels(rgb: &[u8]) -> Self {
        Self::variations(rgb)[0]
    }

    /// The picture's schemes, one per major hue (at most
    /// [`MAX_VARIATIONS`], the dominant first): each tints with its hue and
    /// takes the most vivid hue apart from it as the accent. A grey
    /// picture gives one, grey.
    pub fn variations(rgb: &[u8]) -> Vec<Self> {
        let mut tint_bins = [0.0f32; BINS];
        let mut vivid_bins = [0.0f32; BINS];
        let mut chroma_sum = 0.0f32;
        let mut count = 0usize;
        let pixels: Vec<Lch> = rgb
            .chunks_exact(3)
            .map(|p| Lch::from_srgb8(p[0], p[1], p[2]))
            .collect();
        for px in &pixels {
            count += 1;
            chroma_sum += px.c;
            if px.c < GREY {
                continue;
            }
            let bin = bin_of(px.h);
            // Area × colorfulness for the tint; vivid and bright for the
            // accent.
            tint_bins[bin] += px.c;
            vivid_bins[bin] += px.c * px.c * px.l;
        }
        let mean_chroma = if count == 0 { 0.0 } else { chroma_sum / count as f32 };
        let tint_bins = smooth(&tint_bins);
        let vivid_bins = smooth(&vivid_bins);
        let tints = major_hues(&tint_bins);
        if tints.is_empty() {
            return vec![Self::grey()];
        }
        tints
            .into_iter()
            .map(|bin| {
                let tint_hue = mean_hue(&pixels, centre(bin));
                let accent_hue = accent_for(&pixels, &vivid_bins, tint_hue);
                // The accent as vivid as the picture's own color there
                // allows.
                let accent_peak = pixels
                    .iter()
                    .filter(|p| hue_apart(p.h, accent_hue) < 15.0)
                    .map(|p| p.c)
                    .fold(0.0f32, f32::max);
                Self {
                    tint_hue,
                    tint_chroma: mean_chroma.min(TINT_MAX),
                    accent_hue,
                    accent_chroma: accent_peak.clamp(0.06, ACCENT_C),
                }
            })
            .collect()
    }

    /// The tint's hue in a word ("Amber", "Blue", …; "Grey" untinted).
    pub fn name(&self) -> &'static str {
        if self.tint_chroma <= 0.0 {
            return "Grey";
        }
        hue_name(self.tint_hue)
    }

    /// Three of the scheme's colors at `strength`, for a preview:
    /// the base, the raised fill and the accent (`#rrggbb`).
    pub fn preview(&self, strength: f32) -> [String; 3] {
        let seed = Palette::seed();
        let grey = |name: &str| {
            seed.tokens
                .get(name)
                .and_then(|t| Rgba::parse(&t.value))
                .unwrap_or(Rgba {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                })
        };
        [
            self.color(Role::Surface, grey("bg-primary"), strength).hex(),
            self.color(Role::Surface, grey("bg-secondary"), strength).hex(),
            self.color(Role::Accent, grey("accent"), strength).hex(),
        ]
    }

    /// A grey picture: no tint, the seed accent's hue.
    fn grey() -> Self {
        Self {
            tint_hue: 0.0,
            tint_chroma: 0.0,
            accent_hue: 210.0,
            accent_chroma: ACCENT_C,
        }
    }

    fn color(&self, role: Role, seed: Rgba, strength: f32) -> Rgba {
        let l = Lch::from_rgb(seed.rgb()).l;
        let tint = self.tint_chroma * strength.clamp(0.0, 1.0);
        match role {
            Role::Surface => {
                let c = tint * (l / TINT_FULL_L).min(1.0);
                Lch::new(l, c, self.tint_hue).to_rgba(seed.a)
            }
            Role::Text => Lch::new(l, tint * TEXT_SHARE, self.tint_hue).to_rgba(seed.a),
            Role::Accent => self.accent().to_rgba(1.0),
            Role::AccentDim => self.accent().to_rgba(0.10),
        }
    }

    /// The accent: as light as the seed's, stepping down (to
    /// [`ACCENT_L_MIN`]) until its chroma fits the sRGB gamut.
    fn accent(&self) -> Lch {
        let mut l = ACCENT_L;
        while l > ACCENT_L_MIN && !Lch::new(l, self.accent_chroma, self.accent_hue).fits() {
            l -= 0.01;
        }
        Lch::new(l, self.accent_chroma, self.accent_hue)
    }
}

/// The colors Settings lets you set (`Appearance.colors` keys), with their
/// labels: each sets a family of tokens ([`Theme::customize`]).
pub const CUSTOM_COLORS: &[(&str, &str)] = &[
    ("window", "Window"),
    ("panels", "Panels"),
    ("highlight", "Highlight"),
    ("accent", "Accent"),
];

/// The token a custom color is read back from (for showing it).
pub fn custom_color_token(key: &str) -> Option<&'static str> {
    match key {
        "window" => Some("bg-primary"),
        "panels" => Some("bg-secondary"),
        "highlight" => Some("highlight"),
        "accent" => Some("accent"),
        _ => None,
    }
}

impl Theme {
    /// Set the custom colors (`key → #rrggbb`, keys of [`CUSTOM_COLORS`];
    /// others and malformed values are skipped), each on its family:
    /// `window` the base; `panels` the raised fill and the switcher's
    /// (keeping its alpha); `highlight` a column's active row, and the
    /// selection takes its hue at the selection's own lightness; `accent`
    /// the accent, its text and its 10% wash. Returns true when a token
    /// changed.
    pub fn customize(&mut self, colors: &std::collections::BTreeMap<String, String>) -> bool {
        let seed = Palette::seed();
        let mut values: Vec<(&str, String)> = Vec::new();
        for (key, hex) in colors {
            let Some(c) = Rgba::parse(hex).filter(|c| c.a >= 0.999) else {
                continue;
            };
            match key.as_str() {
                "window" => values.push(("bg-primary", c.hex())),
                "panels" => {
                    values.push(("bg-secondary", c.hex()));
                    values.push(("shell-switcher-bg", Rgba { a: 0.9, ..c }.hex()));
                }
                "highlight" => {
                    values.push(("highlight", c.hex()));
                    if let Some(sel) = seed.tokens.get("selection").and_then(|t| Rgba::parse(&t.value)) {
                        let [r, g, b] = keep_hue(c.rgb(), sel.rgb());
                        values.push(("selection", Rgba { r, g, b, a: 1.0 }.hex()));
                    }
                }
                "accent" => {
                    values.push(("accent", c.hex()));
                    values.push(("text-accent", c.hex()));
                    values.push(("accent-dim", Rgba { a: 0.10, ..c }.hex()));
                }
                _ => {}
            }
        }
        self.rewrite(|name| {
            values
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.clone())
        })
    }

    /// Rewrite the adapted color tokens from `scheme`, the tint at
    /// `strength` (0–1) of the picture's own colorfulness (each token from
    /// its seed grey, so the result does not depend on what was there).
    /// Returns true when a token changed.
    pub fn adapt(&mut self, scheme: &Scheme, strength: f32) -> bool {
        let seed = Palette::seed();
        self.rewrite(|name| {
            let (_, role) = RECIPE.iter().find(|(n, _)| *n == name)?;
            // The accents need no seed value (`accent-dim` is `rgba(…)`).
            let base = Rgba::parse(&seed.tokens.get(name)?.value).unwrap_or(Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            });
            Some(scheme.color(*role, base, strength).hex())
        })
    }

    /// Put the seed's values back on the adapted tokens. Returns true when
    /// a token changed.
    pub fn unadapt(&mut self) -> bool {
        let seed = Palette::seed();
        self.rewrite(|name| {
            RECIPE.iter().find(|(n, _)| *n == name)?;
            Some(seed.tokens.get(name)?.value.clone())
        })
    }

    fn rewrite(&mut self, value_for: impl Fn(&str) -> Option<String>) -> bool {
        let mut changed = false;
        // A theme saved before a managed token existed (`highlight`) gets
        // it from the seed first.
        let seed = Palette::seed();
        for (name, _) in RECIPE {
            if !self.palette.tokens.contains_key(*name) {
                if let Some(token) = seed.tokens.get(*name) {
                    self.palette.tokens.insert((*name).to_string(), token.clone());
                    changed = true;
                }
            }
        }
        for (name, token) in self.palette.tokens.iter_mut() {
            if token.kind != TokenKind::Color {
                continue;
            }
            if let Some(value) = value_for(name) {
                if token.value != value {
                    token.value = value;
                    changed = true;
                }
            }
        }
        changed
    }
}

/// `color` at the OKLab lightness of `lightness_from`, keeping its own hue
/// and chroma (lowered to fit the gamut): a lift that does not wash a tint
/// out the way mixing with white does. sRGB channels, 0–1.
pub fn keep_hue(color: [f32; 3], lightness_from: [f32; 3]) -> [f32; 3] {
    let own = Lch::from_rgb(color);
    let l = Lch::from_rgb(lightness_from).l;
    Lch::new(l, own.c, own.h).to_rgba(1.0).rgb()
}

fn bin_of(h: f32) -> usize {
    ((h.rem_euclid(360.0) / (360.0 / BINS as f32)) as usize).min(BINS - 1)
}

fn centre(bin: usize) -> f32 {
    (bin as f32 + 0.5) * (360.0 / BINS as f32)
}

/// Circular [1, 2, 1] smoothing, so a hue on a bin edge still counts.
fn smooth(bins: &[f32; BINS]) -> [f32; BINS] {
    let mut out = [0.0; BINS];
    for i in 0..BINS {
        out[i] = bins[(i + BINS - 1) % BINS] + 2.0 * bins[i] + bins[(i + 1) % BINS];
    }
    out
}

/// The heaviest bins, each [`HUES_APART`] from those before it, down to
/// [`HUE_WEIGHT`] of the heaviest; at most [`MAX_VARIATIONS`].
fn major_hues(bins: &[f32; BINS]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..BINS).filter(|&b| bins[b] > 0.0).collect();
    order.sort_by(|&a, &b| bins[b].total_cmp(&bins[a]));
    let Some(&top) = order.first() else {
        return Vec::new();
    };
    let mut chosen: Vec<usize> = Vec::new();
    for b in order {
        if chosen.len() == MAX_VARIATIONS || bins[b] < HUE_WEIGHT * bins[top] {
            break;
        }
        if chosen
            .iter()
            .all(|&c| hue_apart(centre(b), centre(c)) >= HUES_APART)
        {
            chosen.push(b);
        }
    }
    chosen
}

/// The accent for a tint: the most vivid hue at least [`ACCENT_APART`]
/// from it with enough weight, else the tint's own hue.
fn accent_for(pixels: &[Lch], vivid_bins: &[f32; BINS], tint_hue: f32) -> f32 {
    let best = vivid_bins.iter().cloned().fold(0.0f32, f32::max);
    let bin = (0..BINS)
        .filter(|&b| hue_apart(centre(b), tint_hue) >= ACCENT_APART)
        .filter(|&b| vivid_bins[b] >= ACCENT_WEIGHT * best)
        .max_by(|&a, &b| vivid_bins[a].total_cmp(&vivid_bins[b]));
    match bin {
        Some(b) => mean_hue(pixels, centre(b)),
        None => tint_hue,
    }
}

/// A hue (OKLCH degrees) in a word.
fn hue_name(h: f32) -> &'static str {
    match h.rem_euclid(360.0) {
        h if h < 20.0 => "Rose",
        h if h < 45.0 => "Ember",
        h if h < 75.0 => "Amber",
        h if h < 110.0 => "Gold",
        h if h < 150.0 => "Olive",
        h if h < 180.0 => "Green",
        h if h < 215.0 => "Teal",
        h if h < 245.0 => "Sky",
        h if h < 280.0 => "Blue",
        h if h < 310.0 => "Violet",
        h if h < 345.0 => "Magenta",
        _ => "Rose",
    }
}

/// The angle between two hues, 0–180.
fn hue_apart(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// The chroma-weighted circular mean of the colored pixels within half of
/// [`HUES_APART`] of `around` (so neighbouring variations stay apart).
fn mean_hue(pixels: &[Lch], around: f32) -> f32 {
    let (mut x, mut y) = (0.0f32, 0.0f32);
    for p in pixels {
        if p.c >= GREY && hue_apart(p.h, around) <= HUES_APART / 2.0 {
            let r = p.h.to_radians();
            x += p.c * r.cos();
            y += p.c * r.sin();
        }
    }
    if x == 0.0 && y == 0.0 {
        return around;
    }
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// An sRGB color with alpha, 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rgba {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Rgba {
    /// `#rrggbb` or `#rrggbbaa`.
    fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        if s.len() != 6 && s.len() != 8 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        let a = if s.len() == 8 { byte(6)? } else { 255 };
        Some(Self {
            r: byte(0)? as f32 / 255.0,
            g: byte(2)? as f32 / 255.0,
            b: byte(4)? as f32 / 255.0,
            a: a as f32 / 255.0,
        })
    }

    fn rgb(self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }

    /// `#rrggbb`, or `#rrggbbaa` when translucent (the kit's form).
    fn hex(self) -> String {
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 0.999 {
            format!("#{:02x}{:02x}{:02x}", b(self.r), b(self.g), b(self.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                b(self.r),
                b(self.g),
                b(self.b),
                b(self.a)
            )
        }
    }
}

/// OKLCH: lightness 0–1, chroma, hue in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Lch {
    l: f32,
    c: f32,
    h: f32,
}

impl Lch {
    fn new(l: f32, c: f32, h: f32) -> Self {
        Self { l, c, h }
    }

    fn from_srgb8(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgb([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
    }

    fn from_rgb(rgb: [f32; 3]) -> Self {
        let [l, a, b] = oklab_from_linear(rgb.map(to_linear));
        Self {
            l,
            c: (a * a + b * b).sqrt(),
            h: b.atan2(a).to_degrees().rem_euclid(360.0),
        }
    }

    /// Inside the sRGB gamut as it is.
    fn fits(self) -> bool {
        let (s, co) = (self.h * PI / 180.0).sin_cos();
        linear_from_oklab([self.l, self.c * co, self.c * s])
            .iter()
            .all(|v| (-0.0005..=1.0005).contains(v))
    }

    /// Back to sRGB, lowering chroma until it fits the gamut.
    fn to_rgba(self, alpha: f32) -> Rgba {
        let mut c = self.c;
        loop {
            let (s, co) = (self.h * PI / 180.0).sin_cos();
            let lin = linear_from_oklab([self.l, c * co, c * s]);
            let inside = lin.iter().all(|v| (-0.0005..=1.0005).contains(v));
            if inside || c <= 0.0 {
                let [r, g, b] = lin.map(|v| from_linear(v.clamp(0.0, 1.0)));
                return Rgba { r, g, b, a: alpha };
            }
            c = (c - 0.002).max(0.0);
        }
    }
}

fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn oklab_from_linear([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn linear_from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(rgb: [u8; 3], n: usize) -> Vec<u8> {
        rgb.iter().copied().cycle().take(3 * n).collect()
    }

    fn token(theme: &Theme, name: &str) -> String {
        theme.palette.tokens[name].value.clone()
    }

    #[test]
    fn oklab_round_trips() {
        for rgb in [[0.1, 0.5, 0.9], [0.92, 0.92, 0.92], [0.04, 0.04, 0.04]] {
            let back = Lch::from_rgb(rgb).to_rgba(1.0);
            for (a, b) in rgb.iter().zip(back.rgb()) {
                assert!((a - b).abs() < 0.002, "{rgb:?} → {back:?}");
            }
        }
    }

    #[test]
    fn a_grey_picture_keeps_the_greys() {
        let scheme = Scheme::from_pixels(&fill([90, 90, 90], 100));
        assert_eq!(scheme.tint_chroma, 0.0);
        let mut theme = Theme::default();
        theme.adapt(&scheme, 0.6);
        assert_eq!(token(&theme, "bg-primary"), "#0b0b0b");
        assert_eq!(token(&theme, "text-primary"), "#ececec");
    }

    #[test]
    fn a_blue_picture_tints_the_surfaces_blue_at_the_same_lightness() {
        let scheme = Scheme::from_pixels(&fill([30, 60, 160], 100));
        assert!((200.0..290.0).contains(&scheme.tint_hue), "{scheme:?}");
        assert!(scheme.tint_chroma > 0.0);
        let mut theme = Theme::default();
        assert!(theme.adapt(&scheme, 0.6));
        let bg = Rgba::parse(&token(&theme, "bg-secondary")).unwrap();
        assert!(bg.b > bg.r, "blue tint: {bg:?}");
        let (seed, now) = (
            Lch::from_rgb(Rgba::parse("#151515").unwrap().rgb()).l,
            Lch::from_rgb(bg.rgb()).l,
        );
        assert!((seed - now).abs() < 0.01, "lightness kept: {seed} vs {now}");
        // Translucent shell tokens keep their alpha.
        assert!(token(&theme, "shell-switcher-bg").ends_with("e6"));
        // The dim accent is the accent at 10%.
        assert!(token(&theme, "accent-dim").ends_with("1a"));
        // Fonts and sizes are untouched.
        assert_eq!(token(&theme, "space-md"), "12px");
    }

    #[test]
    fn the_accent_takes_a_vivid_hue_apart_from_the_tint() {
        // Mostly dull orange, some vivid blue.
        let mut px = fill([120, 70, 40], 300);
        px.extend(fill([40, 120, 255], 60));
        let scheme = Scheme::from_pixels(&px);
        assert!(hue_apart(scheme.tint_hue, 50.0) < 30.0, "{scheme:?}");
        assert!(hue_apart(scheme.accent_hue, 260.0) < 30.0, "{scheme:?}");
    }

    #[test]
    fn strength_scales_the_tint() {
        let scheme = Scheme::from_pixels(&fill([30, 60, 160], 100));
        let chroma = |strength: f32| {
            let mut theme = Theme::default();
            theme.adapt(&scheme, strength);
            Lch::from_rgb(Rgba::parse(&token(&theme, "bg-tertiary")).unwrap().rgb()).c
        };
        assert!(chroma(0.0) < 0.002, "no strength: grey");
        assert!(chroma(1.0) > chroma(0.5) && chroma(0.5) > chroma(0.2));
    }

    #[test]
    fn variations_follow_the_major_hues() {
        // Orange most, blue next, a little green.
        let mut px = fill([140, 70, 30], 300);
        px.extend(fill([40, 90, 200], 150));
        px.extend(fill([40, 140, 60], 60));
        let v = Scheme::variations(&px);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(hue_apart(v[0].tint_hue, 50.0) < 30.0, "{v:?}");
        assert!(hue_apart(v[1].tint_hue, 260.0) < 30.0, "{v:?}");
        assert!(hue_apart(v[2].tint_hue, 145.0) < 30.0, "{v:?}");
        // The blue one's accent is apart from blue.
        assert!(hue_apart(v[1].accent_hue, v[1].tint_hue) >= ACCENT_APART);
        assert_eq!(Scheme::from_pixels(&px), v[0]);
        assert_eq!(v[1].name(), "Blue");
        let [base, raised, accent] = v[1].preview(0.6);
        assert!(base.starts_with('#') && raised.len() == 7 && accent.len() == 7);
    }

    #[test]
    fn keep_hue_lifts_a_grey_as_mixing_white_does_and_keeps_a_tint() {
        let grey = [20.0 / 255.0; 3];
        let mixed = [53.0 / 255.0; 3];
        for (a, b) in keep_hue(grey, mixed).iter().zip(mixed) {
            assert!((a - b).abs() < 1.0 / 255.0);
        }
        let navy = Rgba::parse("#091327").unwrap().rgb();
        let lifted = keep_hue(navy, Rgba::parse("#1f2838").unwrap().rgb());
        let (before, after) = (Lch::from_rgb(navy), Lch::from_rgb(lifted));
        assert!(after.l > before.l);
        assert!((after.c - before.c).abs() < 0.005, "chroma kept: {before:?} {after:?}");
    }

    #[test]
    fn custom_colors_set_their_families() {
        let mut theme = Theme::default();
        let colors: std::collections::BTreeMap<String, String> = [
            ("accent", "#ff8800"),
            ("panels", "#203040"),
            ("highlight", "#6040a0"),
            ("bogus", "#ffffff"),
            ("window", "not a color"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert!(theme.customize(&colors));
        assert_eq!(token(&theme, "accent"), "#ff8800");
        assert_eq!(token(&theme, "text-accent"), "#ff8800");
        assert_eq!(token(&theme, "accent-dim"), "#ff88001a");
        assert_eq!(token(&theme, "bg-secondary"), "#203040");
        assert_eq!(token(&theme, "shell-switcher-bg"), "#203040e6");
        assert_eq!(token(&theme, "highlight"), "#6040a0");
        // The selection takes the highlight's hue at its own lightness.
        let sel = Lch::from_rgb(Rgba::parse(&token(&theme, "selection")).unwrap().rgb());
        let seed = Lch::from_rgb(Rgba::parse("#292929").unwrap().rgb());
        assert!((sel.l - seed.l).abs() < 0.01 && sel.c > 0.02);
        assert_eq!(token(&theme, "bg-primary"), "#0b0b0b", "a malformed color is skipped");
        assert!(theme.unadapt());
        assert_eq!(theme, Theme::default());
    }

    #[test]
    fn a_theme_saved_without_a_managed_token_gets_it() {
        let mut theme = Theme::default();
        theme.palette.tokens.remove("highlight");
        assert!(theme.unadapt());
        assert_eq!(token(&theme, "highlight"), "#353535");
        let scheme = Scheme::from_pixels(&fill([30, 60, 160], 100));
        theme.palette.tokens.remove("highlight");
        theme.adapt(&scheme, 0.6);
        assert_ne!(token(&theme, "highlight"), "#353535", "adapted, not just added");
    }

    #[test]
    fn a_grey_picture_offers_grey() {
        let v = Scheme::variations(&fill([90, 90, 90], 100));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name(), "Grey");
    }

    #[test]
    fn adapting_is_idempotent_and_unadapt_restores_the_seed() {
        let scheme = Scheme::from_pixels(&fill([160, 60, 40], 50));
        let mut theme = Theme::default();
        assert!(theme.adapt(&scheme, 0.6));
        let once = theme.clone();
        assert!(!theme.adapt(&scheme, 0.6));
        assert_eq!(theme, once);
        assert!(theme.unadapt());
        assert_eq!(theme, Theme::default());
    }
}
