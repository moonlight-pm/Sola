//! Card — an elevated container with kit chrome (raised bg, dual-tone
//! bevel edge, rounded corners, kit padding).
//!
//! `card(content)` returns an `iced::widget::Container` so the caller
//! can chain further iced methods (`.width(Fill)`, `.padding(...)`,
//! `.center_x()`, …). The style fn is exposed separately for callers
//! who already have a container and only want the kit chrome.
//!
//! ## Dual-tone edge
//!
//! Iced borders are a single colour. Cards use a **1px gradient frame**
//! (TL darker → BR lighter) via [`style::bevel_ring`] so panels read
//! slightly raised, matching the Open Design edge light.

use iced::widget::{Container, container};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme, Vector};

use crate::components::style::{
    bevel_frame, card_fill, hairline, RADIUS_LG, RADIUS_XL, SPACE_XL,
};

/// Wrap `content` in a card-styled container. Default padding is 16px
/// on the **face** (inside the 1px bevel); override with `.padding(...)`
/// only if you accept replacing the outer ring pad — prefer wrapping
/// content yourself when you need a different inner pad.
pub fn card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    let face = container(content)
        .padding(18)
        .width(Length::Fill)
        .style(style_face);
    container(face).padding(1).style(style_frame)
}

/// Outer 1px dual-tone bevel + soft drop shadow.
pub fn style_frame(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let mut s = bevel_frame(raised, RADIUS_XL);
    s.shadow = Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, 0.28),
        offset: Vector::new(0.0, 6.0),
        blur_radius: 16.0,
    };
    s
}

/// Inner face — soft vertical lift fill, no border (the frame is the edge).
pub fn style_face(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    container::Style {
        background: Some(card_fill(raised)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_XL.into(),
        },
        ..container::Style::default()
    }
}

/// Flat single-colour hairline style (no dual-tone). Kept for callers that
/// apply `.style(card::style)` to a bare container without the nest.
/// Prefer [`card`] / [`style_frame`] + [`style_face`] for new UI.
pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    container::Style {
        background: Some(card_fill(raised)),
        border: hairline(p, RADIUS_XL),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.28),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

/// Raised background, no border — quieter elevation for dense stacks
/// where hairlines add noise. Default [`style`] keeps the outline for
/// existing consumers.
pub fn style_plain(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(card_fill(p.background.weaker.color)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_XL.into(),
        },
        ..container::Style::default()
    }
}

/// Borderless raised card (see [`style_plain`]). Same default padding as
/// [`card`]; chain `.padding(...)` to override.
pub fn plain<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style_plain).padding(SPACE_XL)
}

/// Style for [`modal`]: dual-tone frame + opaque raised face, soft shadow.
///
/// **Face is `weaker`, not `base`**: overlay windows set `base`'s alpha to
/// zero for the see-through window fill; the modal card must remain opaque.
pub fn modal_style(theme: &Theme) -> container::Style {
    // Legacy single-container path — prefer [`modal`] which nests the bevel.
    style(theme)
}

/// Centred overlay panel chrome (launcher / command palette). Dual-tone
/// bevel + soft shadow. Returns a `Container` so the caller can chain
/// `.width(..)`, `.padding(..)`, etc. on the **outer** frame.
pub fn modal<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    let face = container(content).width(Length::Fill).style(style_face);
    container(face).padding(1).style(|theme: &Theme| {
        let p = theme.extended_palette();
        let mut s = bevel_frame(p.background.weaker.color, RADIUS_XL);
        s.shadow = Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.38),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 28.0,
        };
        s
    })
}

/// Parameterized backplate style: caller supplies fill and border
/// colors (alpha included — e.g. the shell's `shell-switcher-bg` /
/// `shell-switcher-border` tokens). Radius and border width are fixed
/// to the backplate's values.
///
/// Radius is `RADIUS_XL` (14px) — Cmd+Tab HUD pill, rounder than modal
/// (`RADIUS_LG`) so the strip reads as system chrome, not a form card.
///
/// No drop shadow: the shell's renderer doesn't blur shadow quads, so a
/// shadow renders as a hard offset rectangle that pokes out past the
/// rounded border (most visibly below it). The 1px border + translucent
/// fill already separate the backplate from what's behind it.
pub fn backplate_style(fill: Color, border: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: border,
            width: 1.0,
            radius: RADIUS_XL.into(),
        },
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// Backplate chrome with caller-supplied fill/border; `accent_backplate`
/// is the palette-derived specialization. Returns a `Container` so the
/// caller can chain sizing/padding.
pub fn backplate<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
    fill: Color,
    border: Color,
) -> Container<'a, Message, Theme> {
    container(content).style(backplate_style(fill, border))
}

/// Style for [`accent_backplate`]: primary-tinted translucent fill and
/// border at `RADIUS_XL`. Thin wrapper over [`backplate_style`] passing
/// the palette-derived defaults.
pub fn accent_backplate_style(theme: &Theme) -> container::Style {
    let accent = theme.extended_palette().primary.base.color;
    backplate_style(Color { a: 0.18, ..accent }, Color { a: 0.35, ..accent })(theme)
}

/// Accent-tinted translucent backplate (storybook demo of primary glass):
/// primary colour at low alpha for fill and border, `RADIUS_XL`. Returns a
/// `Container` so the caller can chain sizing/padding.
pub fn accent_backplate<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(accent_backplate_style)
}

/// Container style for a selectable tile (e.g. an icon cell in the app
/// switcher strip). The container analog of `button::list_item` for tiles
/// that need a `mouse_area` wrapper (hover-driven selection) instead of a
/// pressable button: selected → quiet fill (default: selection atom);
/// unselected → transparent. Radius is `RADIUS_LG` so the plate reads as
/// a soft HUD highlight under a large icon, not a list-row chip.
pub fn list_tile_style(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = theme.extended_palette();
        let background = if selected {
            Some(Background::Color(crate::theme::selection()))
        } else {
            None
        };
        container::Style {
            background,
            text_color: selected.then_some(p.background.base.text),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_LG.into(),
            },
            ..container::Style::default()
        }
    }
}

/// Like [`list_tile_style`] but with caller-supplied colors: `fill` is the
/// selected-cell background and `fg` the text/foreground (applied in both
/// states so SVG glyphs tint consistently). Shell chrome drives switcher
/// cells from `shell-switcher-icon-*` tokens. Same radius (`RADIUS_LG`)
/// and transparent-when-unselected behaviour as [`list_tile_style`].
pub fn list_tile_style_colored(
    selected: bool,
    fill: Color,
    fg: Color,
) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: selected.then_some(Background::Color(fill)),
        text_color: Some(fg),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_LG.into(),
        },
        ..container::Style::default()
    }
}
