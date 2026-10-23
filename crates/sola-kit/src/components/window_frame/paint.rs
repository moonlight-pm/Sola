//! Colours of the frame and the fills under and over its content.

use iced::advanced::Renderer as _;
use iced::advanced::renderer::Quad;
use iced::border::Radius;
use iced::{Background, Border, Color, Rectangle, Theme};

use super::{Chrome, GROUND, RADIUS};
use crate::components::float_clip::{self, Ears};
use crate::components::style::{alpha, mix_white};

/// Classic close light (`#FF5F57`), lit only on the focused window.
const CLOSE_RED: Color = Color::from_rgb(1.0, 0.372_5, 0.341_2);
const CLOSE_HOVER: Color = Color::from_rgb(1.0, 0.45, 0.42);

/// The frame's colours for one focus state, from the live theme.
pub(super) struct Colors {
    pub face: Color,
    pub bar: Color,
    pub bar_line: Color,
    pub ink: Color,
    pub ink_dim: Color,
    pub ink_faint: Color,
    pub pill: Color,
    pub rim: Color,
    pub close: Color,
    pub close_hover: Color,
    pub sheet: Color,
    pub sheet_edge: Color,
    pub hot: Color,
    pub sep: Color,
}

pub(super) fn colors(theme: &Theme, focused: bool) -> Colors {
    let p = theme.extended_palette();
    // `palette().background` is the app's fill even under the overlay
    // theme a float uses (which clears `background.base`).
    let face = theme.palette().background;
    let raised = p.background.weaker.color;
    let ink = p.background.base.text;
    let ink_dim = alpha(ink, 0.55);
    let ink_faint = alpha(ink, 0.38);
    Colors {
        face,
        bar: if focused {
            raised
        } else {
            mix(face, raised, 0.5)
        },
        bar_line: alpha(Color::BLACK, 0.5),
        ink: if focused { ink } else { ink_dim },
        ink_dim: if focused { ink_dim } else { ink_faint },
        ink_faint,
        pill: alpha(ink, 0.14),
        rim: alpha(Color::WHITE, if focused { 0.18 } else { 0.07 }),
        close: if focused {
            CLOSE_RED
        } else {
            alpha(Color::WHITE, 0.12)
        },
        close_hover: if focused {
            CLOSE_HOVER
        } else {
            alpha(Color::WHITE, 0.2)
        },
        sheet: raised,
        sheet_edge: mix_white(raised, 0.1),
        hot: alpha(p.primary.base.color, 0.14),
        sep: alpha(Color::WHITE, 0.07),
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Fill under the content: the ground square (tiles) and the rounded face.
pub(super) fn under(renderer: &mut iced::Renderer, bounds: Rectangle, chrome: Chrome, face: Color) {
    if chrome == Chrome::Tiled {
        fill(renderer, bounds, GROUND, Radius::default());
    }
    fill(renderer, bounds, face, RADIUS.into());
}

/// Over the content: the corners (painted or cleared), then the rim.
pub(super) fn over(renderer: &mut iced::Renderer, bounds: Rectangle, chrome: Chrome, rim: Color) {
    let ears = match chrome {
        Chrome::Floating => Ears::Clear,
        _ => Ears::Paint(GROUND),
    };
    float_clip::draw_ears(renderer, bounds, RADIUS, ears);
    renderer.with_layer(bounds, |renderer| {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: rim,
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..Quad::default()
            },
            Background::Color(Color::TRANSPARENT),
        );
    });
}

pub(super) fn fill(renderer: &mut iced::Renderer, bounds: Rectangle, color: Color, radius: Radius) {
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border {
                radius,
                ..Border::default()
            },
            ..Quad::default()
        },
        Background::Color(color),
    );
}
