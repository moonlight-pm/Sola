//! Shared component style primitives — the bits that recurred across
//! the per-component style fns before they were factored out here.
//!
//! - [`filled`] builds the four-state `button::Style` shared by the
//!   `primary` and `danger` buttons (they differ only by palette tier).
//! - [`card_fill`] / [`primary_fill`] / [`hero_fill`] / [`stage_fill`] /
//!   [`canvas_ambient`] are **linear** material fills — iced has no
//!   radial gradients yet, so dual-radial OD ambients become multi-stop
//!   linear approximations.
//! - [`hairline`] / [`hairline_strong`] build **thin** edges (card /
//!   popover / swatch / text_input). See the hairline note below.
//! - [`bevel_ring`] / [`bevel_frame`] give panels a **dual-tone** edge
//!   (TL darker → BR lighter) — iced borders are single-colour, so the
//!   raised look is a 1px gradient frame around the face.
//! - [`dim`] is the shared disabled treatment (halve every alpha).
//! - the `RADIUS_*` / `SPACE_*` constants name the radii and spacing
//!   steps the kit uses, so component code stops sprinkling bare
//!   `6.0.into()` / `padding(16)` literals.
//!
//! ## Why hairlines are opaque sRGB mixes (not white@α)
//!
//! Open Design uses CSS `color-mix(in srgb, #fff 7%, transparent)`. Iced
//! packs border colours into **linear RGB** and premultiplies alpha; a
//! translucent white then reads ~3× brighter after the linear→sRGB
//! display path (measured ~#3e3f40 instead of ~#262931). That looks
//! like a heavy "chonky" outline even at `width: 1.0`.
//!
//! So we bake the CSS mix as an **opaque** colour on the intended
//! surface: `mix_white(surface, 0.07)`. Same intent, correct weight.

use iced::gradient::Linear;
use iced::theme::palette::{Extended, Pair};
use iced::widget::button;
use iced::{Background, Border, Color, Degrees, Shadow, Vector};

/// Corner radii (sola-kit-ds graphite pass). `SM` = inputs / ghost chrome,
/// `MD` = buttons / swatches, `LG` = cards / popovers, `XL` = large
/// floating panels, `PILL` = fully-rounded badges.
pub const RADIUS_SM: f32 = 5.0;
pub const RADIUS_MD: f32 = 7.0;
pub const RADIUS_LG: f32 = 10.0;
/// `XL` = large floating panels (modal launcher, accent switcher frame).
pub const RADIUS_XL: f32 = 14.0;
pub const RADIUS_PILL: f32 = 999.0;

/// Spacing / padding steps. Kit layouts step through these rather than
/// scattering raw pixel counts; asymmetric paddings still pass explicit
/// `Padding::from([..])` since they don't fall on a single scale.
pub const SPACE_XS: f32 = 2.0;
pub const SPACE_SM: f32 = 4.0;
pub const SPACE_MD: f32 = 8.0;
pub const SPACE_LG: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;

/// Regular control content padding `[vertical, horizontal]` — buttons,
/// default actions. Prefer this over inventing pad literals in apps.
pub const PAD_CONTROL: [u16; 2] = [7, 14];
/// Compact control padding — toolbar, steppers, dense chrome.
pub const PAD_CONTROL_SM: [u16; 2] = [5, 11];

/// Soft hairline weight — OD `--hairline` (white 7% in sRGB over surface).
pub const HAIRLINE_A: f32 = 0.07;
/// Stronger hairline — OD `--hairline-strong` (white 12% in sRGB).
pub const HAIRLINE_STRONG_A: f32 = 0.12;

/// CSS `color-mix(in srgb, #fff amount, surface)` as an **opaque** colour.
/// Use this for edges instead of `Color { a: amount, ..WHITE }`.
pub fn mix_white(surface: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    let k = 1.0 - t;
    Color {
        r: surface.r * k + t,
        g: surface.g * k + t,
        b: surface.b * k + t,
        a: 1.0,
    }
}

/// CSS `color-mix(in srgb, a amount, b)` — opaque sRGB lerp, `amount` of `a`.
pub fn mix(a: Color, b: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    let u = 1.0 - t;
    Color {
        r: a.r * t + b.r * u,
        g: a.g * t + b.g * u,
        b: a.b * t + b.b * u,
        a: a.a * t + b.a * u,
    }
}

/// Linear gradient background. Angle is CSS-like degrees via iced
/// (`0°` = up, clockwise). Stops are `(offset 0..=1, color)`; max 8.
///
/// Iced ships **linear only** (radial/conic TBD). Use this for card lift,
/// hero washes, primary fills, and ambient canvas approximations.
pub fn linear_bg(angle_deg: f32, stops: &[(f32, Color)]) -> Background {
    let mut g = Linear::new(Degrees(angle_deg));
    for &(offset, color) in stops {
        g = g.add_stop(offset, color);
    }
    Background::Gradient(g.into())
}

/// OD `.card` fill — soft top highlight into raised graphite
/// (`linear-gradient(180deg, raised+white4%, raised)`).
pub fn card_fill(raised: Color) -> Background {
    linear_bg(180.0, &[(0.0, mix_white(raised, 0.04)), (1.0, raised)])
}

/// OD primary / filled accent — top highlight into solid
/// (`linear-gradient(180deg, accent+white8%, accent)`).
pub fn primary_fill(accent: Color) -> Background {
    linear_bg(180.0, &[(0.0, mix_white(accent, 0.08)), (1.0, accent)])
}

/// North-star hero panel — selection-tinted diagonal into canvas
/// (`linear-gradient(160deg, selection@55%+raised, raised 55%, bg)`).
pub fn hero_fill(bg: Color, raised: Color, selection: Color) -> Background {
    let top = mix(selection, raised, 0.55);
    linear_bg(160.0, &[(0.0, top), (0.55, raised), (1.0, bg)])
}

/// Control-stage product panel — cool raised → canvas with a slight
/// cool lift at the top (radial glow approximated by a lighter start).
pub fn stage_fill(bg: Color, raised: Color, accent: Color) -> Background {
    let cool = mix(Color::from_rgb(0.102, 0.133, 0.188), raised, 0.20); // #1a2230-ish into raised
    let top = mix(cool, raised, 0.80);
    let glow_start = mix(accent, top, 0.12);
    linear_bg(
        165.0,
        &[(0.0, glow_start), (0.45, raised), (1.0, bg)],
    )
}

/// Optional soft canvas wash — **not** used as the storybook page fill.
///
/// OD layers dual *radials* that fade to transparent over solid `--bg`
/// (`#0c0e12`). A full-pane linear with a strong selection stop reads as a
/// grey/teal gradient. Prefer solid `background.base` for app canvas; keep
/// this helper only for deliberate accent panels (hero / stage).
pub fn canvas_ambient(bg: Color, accent: Color, selection: Color) -> Background {
    // Very light edge tints only — safe if someone layers this under chrome.
    let sel_wash = mix(selection, bg, 0.08);
    let accent_wash = mix(accent, bg, 0.03);
    linear_bg(
        118.0,
        &[(0.0, sel_wash), (0.35, bg), (0.75, bg), (1.0, accent_wash)],
    )
}

/// Soft hairline on the raised surface (cards, popovers, swatches).
pub fn hairline(palette: &Extended, radius: f32) -> Border {
    Border {
        color: mix_white(palette.background.weaker.color, HAIRLINE_A),
        width: 1.0,
        radius: radius.into(),
    }
}

/// Stronger hairline mixed over the raised surface (secondary buttons,
/// field wells at rest). Prefer [`hairline_on`] when the fill differs.
pub fn hairline_strong(palette: &Extended, radius: f32) -> Border {
    Border {
        color: mix_white(palette.background.weaker.color, HAIRLINE_STRONG_A),
        width: 1.0,
        radius: radius.into(),
    }
}

/// Hairline mixed over an arbitrary fill (inset fields, secondary btn).
pub fn hairline_on(surface: Color, amount: f32, radius: f32) -> Border {
    Border {
        color: mix_white(surface, amount),
        width: 1.0,
        radius: radius.into(),
    }
}

// ── Dual-tone bevel (raised edge) ─────────────────────────────────
//
// Iced `Border` is one colour on all sides. OD panels read slightly
// raised because the top-left edge is darker than the bottom-right
// (light from BR / catch-shadow on TL). We approximate that with a
// **1px gradient ring**: outer container paints [`bevel_ring`], content
// sits inside with `padding(1)`.

/// TL-dark → BR-light 1px frame fill (use with outer `padding(1)`).
pub fn bevel_ring(surface: Color) -> Background {
    // Darker than the mid hairline — sinks the TL edge (near-black).
    let dark = Color {
        r: surface.r * 0.45,
        g: surface.g * 0.45,
        b: surface.b * 0.45,
        a: 1.0,
    };
    let mid = mix_white(surface, HAIRLINE_A);
    // Brighter catch on the BR edge.
    let light = mix_white(surface, 0.20);
    // 135°: first stop sits at the top-left, last at the bottom-right.
    linear_bg(135.0, &[(0.0, dark), (0.40, mid), (1.0, light)])
}

/// Stronger dual-tone ring (hero / stage / hairline-strong panels).
pub fn bevel_ring_strong(surface: Color) -> Background {
    let dark = Color {
        r: surface.r * 0.35,
        g: surface.g * 0.35,
        b: surface.b * 0.35,
        a: 1.0,
    };
    let mid = mix_white(surface, HAIRLINE_STRONG_A);
    let light = mix_white(surface, 0.24);
    linear_bg(135.0, &[(0.0, dark), (0.40, mid), (1.0, light)])
}

/// Outer frame style for a dual-tone bevel. Pair with `padding(1)` and an
/// inner face that carries the panel fill (no border).
pub fn bevel_frame(surface: Color, radius: f32) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(bevel_ring(surface)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius.into(),
        },
        ..iced::widget::container::Style::default()
    }
}

/// Like [`bevel_frame`] with the stronger dual-tone ring.
pub fn bevel_frame_strong(surface: Color, radius: f32) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(bevel_ring_strong(surface)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius.into(),
        },
        ..iced::widget::container::Style::default()
    }
}

/// Mix `color` toward transparent at `alpha` (0..1). Used for soft
/// badge / secondary fills that tint without opaque slabs.
///
/// Prefer [`mix_white`] / opaque mixes for **borders** — see module docs.
pub fn alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

/// Darken `color` toward black by `amount` (0..1) — stands in for
/// `color-mix(in srgb, #000 amount%, color)` (inset field wells).
pub fn inset_surface(color: Color, amount: f32) -> Color {
    let keep = 1.0 - amount;
    Color {
        r: color.r * keep,
        g: color.g * keep,
        b: color.b * keep,
        a: color.a,
    }
}

/// Dark label on bright filled primaries / dangers (`#041018` / near-black).
pub const ON_FILL_DARK: Color = Color {
    r: 0.016,
    g: 0.063,
    b: 0.094,
    a: 1.0,
};

/// Soft accent glow under primary actions (OD `--glow-accent` approx).
pub fn accent_glow(accent: Color) -> Shadow {
    Shadow {
        color: alpha(accent, 0.35),
        offset: Vector::new(0.0, 4.0),
        blur_radius: 14.0,
    }
}

pub fn filled(
    base: Pair,
    strong: Pair,
    weak: Pair,
    status: button::Status,
) -> button::Style {
    filled_with(base, strong, weak, status, base.text, None)
}

/// Like [`filled`], with explicit label colour and optional resting glow.
///
/// When `glow` is set (primary actions), the fill is a vertical accent
/// gradient matching OD `.btn-primary` rather than a flat slab.
pub fn filled_with(
    base: Pair,
    strong: Pair,
    weak: Pair,
    status: button::Status,
    text: Color,
    glow: Option<Shadow>,
) -> button::Style {
    let fill = |c: Color| {
        if glow.is_some() {
            primary_fill(c)
        } else {
            Background::Color(c)
        }
    };
    let resting = button::Style {
        background: Some(fill(base.color)),
        text_color: text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        shadow: glow.unwrap_or_default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            // OD primary: brightness(1.05) — only lift the gradient path.
            background: Some(fill(if glow.is_some() {
                mix_white(strong.color, 0.06)
            } else {
                strong.color
            })),
            shadow: glow.unwrap_or_default(),
            ..resting
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(weak.color)),
            shadow: Default::default(),
            ..resting
        },
        button::Status::Disabled => dim(button::Style {
            shadow: Default::default(),
            ..resting
        }),
        button::Status::Active => resting,
    }
}

pub fn dim(base: button::Style) -> button::Style {
    button::Style {
        background: base.background.map(|bg| match bg {
            Background::Color(c) => Background::Color(Color { a: c.a * 0.5, ..c }),
            Background::Gradient(g) => Background::Gradient(g.scale_alpha(0.5)),
        }),
        text_color: Color {
            a: base.text_color.a * 0.5,
            ..base.text_color
        },
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn pair(bg: f32, text: f32) -> Pair {
        Pair::new(Color::from_rgb(bg, bg, bg), Color::from_rgb(text, text, text))
    }

    #[test]
    fn filled_active_uses_base_color_and_text() {
        let base = pair(0.1, 0.9);
        let s = filled(base, pair(0.2, 0.9), pair(0.05, 0.9), button::Status::Active);
        assert_eq!(s.background, Some(Background::Color(base.color)));
        assert_eq!(s.text_color, base.text);
    }

    #[test]
    fn filled_with_glow_uses_gradient_fill() {
        let base = pair(0.1, 0.9);
        let s = filled_with(
            base,
            pair(0.2, 0.9),
            pair(0.05, 0.9),
            button::Status::Active,
            ON_FILL_DARK,
            Some(accent_glow(base.color)),
        );
        assert!(
            matches!(s.background, Some(Background::Gradient(_))),
            "primary path must use a gradient fill"
        );
        assert_eq!(s.text_color, ON_FILL_DARK);
    }

    #[test]
    fn filled_hover_uses_strong_bg_but_keeps_base_text() {
        let base = pair(0.1, 0.9);
        let strong = pair(0.2, 0.3);
        let s = filled(base, strong, pair(0.05, 0.9), button::Status::Hovered);
        // No glow → flat strong colour (with slight white lift only when glowed).
        assert_eq!(s.background, Some(Background::Color(strong.color)));
        // Text stays the base pair's text — hover only lifts the fill.
        assert_eq!(s.text_color, base.text);
    }

    #[test]
    fn filled_pressed_uses_weak_bg() {
        let weak = pair(0.05, 0.9);
        let s = filled(pair(0.1, 0.9), pair(0.2, 0.9), weak, button::Status::Pressed);
        assert_eq!(s.background, Some(Background::Color(weak.color)));
    }

    #[test]
    fn filled_disabled_halves_alpha() {
        let base = Pair::new(
            Color::from_rgba(0.1, 0.1, 0.1, 1.0),
            Color::from_rgba(0.9, 0.9, 0.9, 1.0),
        );
        let s = filled(base, base, base, button::Status::Disabled);
        let Some(Background::Color(bg)) = s.background else {
            panic!("disabled style lost its background colour");
        };
        assert!((bg.a - 0.5).abs() < 1e-6, "bg alpha not halved: {}", bg.a);
        assert!(
            (s.text_color.a - 0.5).abs() < 1e-6,
            "text alpha not halved: {}",
            s.text_color.a
        );
    }

    #[test]
    fn hairline_is_opaque_srgb_mix_not_translucent_white() {
        let t = theme::default_theme();
        let ext = t.extended_palette();
        let b = hairline(ext, RADIUS_LG);
        assert!((b.color.a - 1.0).abs() < 1e-6, "must be opaque for iced linear path");
        // Lighter than the raised surface, darker than pure mid-grey.
        let raised = ext.background.weaker.color;
        assert!(b.color.r > raised.r);
        assert!(b.color.r < 0.25, "must stay subtle, got r={}", b.color.r);
        assert_eq!(b.width, 1.0);
        assert_eq!(b.radius, RADIUS_LG.into());
    }

    #[test]
    fn mix_white_matches_css_srgb_mix() {
        let surface = Color::from_rgb(0.082, 0.098, 0.133); // ~#151922
        let c = mix_white(surface, 0.07);
        // 0.07*1 + 0.93*channel
        assert!((c.r - (surface.r * 0.93 + 0.07)).abs() < 1e-5);
        assert!((c.a - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bevel_ring_is_gradient_tl_darker_than_br() {
        let surface = Color::from_rgb(0.082, 0.098, 0.133);
        let Background::Gradient(g) = bevel_ring(surface) else {
            panic!("bevel_ring must be a gradient");
        };
        let iced::gradient::Gradient::Linear(lin) = g;
        let stops: Vec<_> = lin.stops.iter().flatten().collect();
        assert!(stops.len() >= 2);
        let first = stops[0].color;
        let last = stops[stops.len() - 1].color;
        // TL (first) is darker → lower luma than BR (last).
        let luma = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        assert!(
            luma(first) < luma(last),
            "TL should be darker than BR: {first:?} vs {last:?}"
        );
    }

    #[test]
    fn radii_match_graphite_ds() {
        assert_eq!(RADIUS_SM, 5.0);
        assert_eq!(RADIUS_MD, 7.0);
        assert_eq!(RADIUS_LG, 10.0);
    }
}
