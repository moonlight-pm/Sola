//! Colors that follow the background picture ("adapt the theme to the
//! background", `Topic::Appearance`): [`Scheme::variations`] finds the
//! picture's own colors, and [`Theme::adapt`] rewrites the color tokens
//! from one — every surface, border and text color keeps the **lightness**
//! of its grey in the seed palette (so contrast is the grey theme's) and
//! takes the color's hue and some of its chroma, and the accent takes
//! another of the picture's colors. [`Theme::unadapt`] puts the seed's
//! greys back.
//!
//! The colors are modes, not averages: the pixels are counted in narrow
//! cells (a color each), and a cell is scored as Material Color
//! Utilities' `Score` does — the share of the picture its hue family
//! covers, and how vivid it is. A mean over a whole picture drifts to grey
//! (the dark sky of a space picture thins every color in it); a cell's
//! mean is the color its pixels share.
//!
//! Colors are worked in OKLab / OKLCH (Björn Ottosson), where lightness
//! and chroma are perceptual, so a tint reads the same strength in every
//! hue.

use std::f32::consts::PI;

use super::types::{Palette, Theme, TokenKind};

/// What a picture gives the theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheme {
    /// The surfaces' hue (degrees), the picture's color's, and their
    /// chroma at full strength: the color's own, up to [`TINT_MAX`] (0: a
    /// grey picture).
    pub tint_hue: f32,
    pub tint_chroma: f32,
    /// The accent's hue (degrees) and chroma.
    pub accent_hue: f32,
    pub accent_chroma: f32,
    /// The picture's color itself (sRGB 0–1), as it is there.
    pub source: [f32; 3],
}

/// The most schemes a picture offers.
pub const MAX_VARIATIONS: usize = 4;
/// Pixels below this chroma are grey: they count for no color.
const GREY: f32 = 0.02;
/// The cells the pixels are counted in (hue degrees, chroma, lightness):
/// narrow enough that a cell's mean is the color its pixels share.
const CELL_HUE: f32 = 10.0;
const CELL_CHROMA: f32 = 0.02;
const CELL_L: f32 = 0.1;
/// A cell is one of the picture's colors with at least this share of its
/// pixels (and two of them), and at least this chroma.
const CELL_SHARE: f32 = 0.002;
const SOURCE_CHROMA: f32 = 0.03;
/// A color's hue family: the colored pixels within this of its hue. A
/// color counts only when its family covers more than [`FAMILY_SHARE`] of
/// the picture.
const FAMILY: f32 = 15.0;
const FAMILY_SHARE: f32 = 0.01;
/// The score (Material Color Utilities' `Score`, its CAM16 chroma scaled to
/// OKLCH's): the family's share of the picture, plus chroma against a
/// target — a dull color loses a little, a vivid one gains more.
const WEIGHT_SHARE: f32 = 70.0;
const TARGET_CHROMA: f32 = 0.11;
const WEIGHT_DULL: f32 = 44.0;
const WEIGHT_VIVID: f32 = 132.0;
/// The schemes' colors are at first this far apart in hue, closer (down to
/// [`HUES_APART`]) only until there are [`MAX_VARIATIONS`].
const HUES_APART_FIRST: f32 = 90.0;
const HUES_APART: f32 = 30.0;
/// The strongest tint a surface takes at full strength (OKLCH chroma); up
/// to it, the picture's color's own chroma.
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
/// The accent is the best of the picture's colors at least this far
/// from the tint; with none, the tint's own hue.
const ACCENT_APART: f32 = 60.0;

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
    ("shell-flower", Role::Flower),
];

#[derive(Clone, Copy)]
enum Role {
    Surface,
    Text,
    Accent,
    /// The accent at 10%.
    AccentDim,
    /// The accent, but a grey picture (no color of its own) keeps the
    /// seed's.
    Flower,
}

impl Scheme {
    /// Sample `rgb` (8-bit sRGB triples, a downscaled picture: a few
    /// thousand pixels is plenty).
    pub fn from_pixels(rgb: &[u8]) -> Self {
        Self::variations(rgb)[0]
    }

    /// The picture's schemes, one per color it is made of (at most
    /// [`MAX_VARIATIONS`], the best first, as far apart in hue as they
    /// can be): each tints with its color and takes the best color apart
    /// from it as the accent. A grey picture gives one, grey.
    pub fn variations(rgb: &[u8]) -> Vec<Self> {
        let pixels: Vec<Lch> = rgb
            .chunks_exact(3)
            .map(|p| Lch::from_srgb8(p[0], p[1], p[2]))
            .collect();
        let sources = sources(&pixels);
        let chosen = apart(&sources);
        if chosen.is_empty() {
            return vec![Self::grey()];
        }
        chosen
            .into_iter()
            .map(|tint| {
                let accent_hue = sources
                    .iter()
                    .find(|s| hue_apart(s.color.h, tint.color.h) >= ACCENT_APART)
                    .map_or(tint.color.h, |s| s.color.h);
                Self {
                    tint_hue: tint.color.h,
                    tint_chroma: tint.color.c.min(TINT_MAX),
                    accent_hue,
                    // As vivid as the picture's own color there allows.
                    accent_chroma: peak_chroma(&pixels, accent_hue).clamp(0.06, ACCENT_C),
                    source: tint.color.to_rgba(1.0).rgb(),
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

    /// [`Self::source`] as `#rrggbb`.
    pub fn source_hex(&self) -> String {
        let [r, g, b] = self.source;
        Rgba { r, g, b, a: 1.0 }.hex()
    }

    /// A grey picture: no tint, the seed accent's hue.
    fn grey() -> Self {
        Self {
            tint_hue: 0.0,
            tint_chroma: 0.0,
            accent_hue: 210.0,
            accent_chroma: ACCENT_C,
            source: [0.5; 3],
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
            Role::Flower if self.tint_chroma == 0.0 => seed,
            Role::Flower => self.accent().to_rgba(1.0),
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
    /// the accent, its text, its 10% wash and the shell's flower. Returns
    /// true when a token changed.
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
                    values.push(("shell-flower", c.hex()));
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

/// A focused window's 1 px rim from the theme's `highlight` (sRGB 0–1):
/// the highlight a step lighter (4% white), in its own hue — on the grey
/// theme `#3d3d3d`, the lit edge white-at-18% gave; on a tinted or custom
/// highlight, that tint.
pub fn focus_rim(highlight: [f32; 3]) -> [f32; 3] {
    lifted(highlight, 0.04)
}

/// An unfocused window's 1 px rim from the theme's base (`bg-primary`):
/// 7% lighter in its hue, opaque — what white at 7% gave over the window,
/// without letting what is under the window's edge show (`rest_rim`).
pub fn rest_rim(base: [f32; 3]) -> [f32; 3] {
    lifted(base, 0.07)
}

/// `color` as lightened by mixing `amount` of white in, keeping its hue
/// and chroma ([`keep_hue`]). sRGB channels, 0–1.
pub fn lifted(color: [f32; 3], amount: f32) -> [f32; 3] {
    let lift = color.map(|c| c + (1.0 - c) * amount);
    keep_hue(color, lift)
}

/// The color token `name` of `theme` as sRGB 0–1 (the seed's when absent).
pub fn token_rgb(theme: &Theme, name: &str) -> [f32; 3] {
    let seed = Palette::seed();
    theme
        .palette
        .tokens
        .get(name)
        .and_then(|t| Rgba::parse(&t.value))
        .or_else(|| seed.tokens.get(name).and_then(|t| Rgba::parse(&t.value)))
        .map_or([0.0; 3], Rgba::rgb)
}

/// The `highlight` token of `theme` as sRGB 0–1 (the seed's when absent).
pub fn highlight_rgb(theme: &Theme) -> [f32; 3] {
    token_rgb(theme, "highlight")
}

/// One of the picture's colors, and its score.
#[derive(Debug, Clone, Copy)]
struct Source {
    color: Lch,
    score: f32,
}

/// A cell's pixels: how many, and their sums (hue as a unit vector).
#[derive(Default)]
struct Cell {
    count: u32,
    l: f32,
    c: f32,
    x: f32,
    y: f32,
}

impl Cell {
    fn add(&mut self, p: &Lch) {
        let (y, x) = p.h.to_radians().sin_cos();
        self.count += 1;
        self.l += p.l;
        self.c += p.c;
        self.x += x;
        self.y += y;
    }

    fn mean(&self) -> Lch {
        let n = self.count as f32;
        let h = self.y.atan2(self.x).to_degrees().rem_euclid(360.0);
        Lch::new(self.l / n, self.c / n, h)
    }
}

/// The picture's colors, the best scored first.
fn sources(pixels: &[Lch]) -> Vec<Source> {
    let n = pixels.len() as f32;
    // Colored pixels per degree of hue, for the families.
    let mut by_degree = [0u32; 360];
    let mut cells: std::collections::BTreeMap<(u16, u16, u16), Cell> = Default::default();
    for p in pixels.iter().filter(|p| p.c >= GREY) {
        by_degree[(p.h as usize) % 360] += 1;
        let key = (
            (p.h / CELL_HUE) as u16,
            (p.c / CELL_CHROMA) as u16,
            (p.l.max(0.0) / CELL_L) as u16,
        );
        cells.entry(key).or_default().add(p);
    }
    let family = |h: f32| {
        let at = h.round() as i32;
        let reach = FAMILY as i32;
        let count: u32 = (-reach..=reach)
            .map(|d| by_degree[(at + d).rem_euclid(360) as usize])
            .sum();
        count as f32 / n
    };
    let least = (CELL_SHARE * n).max(2.0);
    let mut out: Vec<Source> = cells
        .values()
        .filter(|cell| cell.count as f32 >= least)
        .map(Cell::mean)
        .filter(|color| color.c >= SOURCE_CHROMA)
        .filter_map(|color| {
            let share = family(color.h);
            (share > FAMILY_SHARE).then(|| Source {
                color,
                score: score(share, color.c),
            })
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

fn score(share: f32, chroma: f32) -> f32 {
    let weight = if chroma < TARGET_CHROMA {
        WEIGHT_DULL
    } else {
        WEIGHT_VIVID
    };
    share * WEIGHT_SHARE + (chroma - TARGET_CHROMA) * weight
}

/// Up to [`MAX_VARIATIONS`] of `sources` (best first), each at least
/// `apart` in hue from those before it — starting far apart, closing in
/// only as far as it takes to find enough.
fn apart(sources: &[Source]) -> Vec<Source> {
    let mut apart = HUES_APART_FIRST;
    loop {
        let mut chosen: Vec<Source> = Vec::new();
        for s in sources {
            if chosen.len() == MAX_VARIATIONS {
                break;
            }
            if chosen.iter().all(|c| hue_apart(c.color.h, s.color.h) >= apart) {
                chosen.push(*s);
            }
        }
        if chosen.len() == MAX_VARIATIONS || apart <= HUES_APART {
            return chosen;
        }
        apart -= 1.0;
    }
}

/// The most chroma among the picture's pixels in `hue`'s family.
fn peak_chroma(pixels: &[Lch], hue: f32) -> f32 {
    pixels
        .iter()
        .filter(|p| p.c >= GREY && hue_apart(p.h, hue) <= FAMILY)
        .map(|p| p.c)
        .fold(0.0, f32::max)
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
        assert_eq!(token(&theme, "shell-flower"), "#ffb800");
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
        // The dim accent is the accent at 10%; the flower is the accent.
        assert!(token(&theme, "accent-dim").ends_with("1a"));
        assert_eq!(token(&theme, "shell-flower"), token(&theme, "accent"));
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
    fn a_dark_picture_keeps_its_colors_vivid() {
        // A space picture: mostly black sky, a little vivid nebula. A mean
        // over the picture would thin the orange to almost grey.
        let mut px = fill([5, 5, 8], 900);
        px.extend(fill([200, 90, 40], 100));
        let scheme = Scheme::from_pixels(&px);
        assert!(hue_apart(scheme.tint_hue, 45.0) < 20.0, "{scheme:?}");
        assert_eq!(scheme.tint_chroma, TINT_MAX, "{scheme:?}");
        // The source is the nebula's own color.
        assert_eq!(scheme.source_hex(), "#c85a28");
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
        assert_eq!(token(&theme, "shell-flower"), "#ff8800");
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
    fn the_focus_rim_is_the_highlight_a_step_lighter_in_its_hue() {
        let grey = focus_rim(highlight_rgb(&Theme::default()));
        assert_eq!(Rgba { r: grey[0], g: grey[1], b: grey[2], a: 1.0 }.hex(), "#3d3d3d");
        let magenta = focus_rim(Rgba::parse("#8831a1").unwrap().rgb());
        let (m, h) = (Lch::from_rgb(magenta), Lch::from_rgb(Rgba::parse("#8831a1").unwrap().rgb()));
        assert!(m.l > h.l && hue_apart(m.h, h.h) < 2.0);
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
