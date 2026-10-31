//! The Graphite dropdown sheet (DESIGN.md, Dropdown): a titlebar menu's
//! dropdown and the shell's desk menu. Raised, a 10% edge, 12 px corners,
//! 5 px pad, 26 px rows; the hot row is accent at 14%, key hints ink at
//! 55%, disabled rows ink at 38%.

use iced::{Color, Shadow, Theme, Vector};

use crate::components::style::{alpha, mix_white};

/// Inside the sheet, around the rows.
pub const PAD: f32 = 5.0;
pub const ROW_H: f32 = 26.0;
/// A row's text from its highlight's edge.
pub const ROW_PAD: f32 = 10.0;
pub const ROW_RADIUS: f32 = 7.0;
/// A separator's height (its 1 px rule centred).
pub const SEP_H: f32 = 11.0;
pub const MIN_W: f32 = 200.0;
pub const RADIUS: f32 = 12.0;
pub const LABEL_SIZE: f32 = 13.0;
pub const KEY_SIZE: f32 = 12.0;
/// Room between a label and its key hint.
pub const KEY_GAP: f32 = 28.0;

/// A sheet's colours, from the live theme.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub sheet: Color,
    pub edge: Color,
    pub hot: Color,
    pub sep: Color,
    pub ink: Color,
    /// Key hints.
    pub ink_dim: Color,
    /// Disabled rows.
    pub ink_faint: Color,
}

pub fn colors(theme: &Theme) -> Colors {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let ink = p.background.base.text;
    Colors {
        sheet: raised,
        edge: mix_white(raised, 0.1),
        hot: alpha(p.primary.base.color, 0.14),
        sep: alpha(Color::WHITE, 0.07),
        ink,
        ink_dim: alpha(ink, 0.55),
        ink_faint: alpha(ink, 0.38),
    }
}

/// The popover drop shadow (DESIGN.md, Elevation).
pub fn shadow() -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, 0.28),
        offset: Vector::new(0.0, 2.0),
        blur_radius: 10.0,
    }
}
