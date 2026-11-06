//! The app's colors, from the theme: graphite, with one color per standing
//! so that anything that is not current shows. Shared by the widgets and
//! the canvases.

use iced::{Color, Theme};
use sola_kit::components::style::{alpha, mix_white};

use crate::model::Package;
use crate::standing::Standing;

#[derive(Debug, Clone, Copy)]
pub struct Ink {
    pub bg: Color,
    pub raised: Color,
    pub fg: Color,
    /// Secondary text.
    pub fg2: Color,
    /// Tertiary text: captions, quiet labels.
    pub fg3: Color,
    /// The quietest mark: a column that agrees with this machine.
    pub fg4: Color,
    pub rule: Color,
    pub rule2: Color,
    pub face2: Color,
    pub face3: Color,
    pub accent: Color,
    pub warning: Color,
    pub danger: Color,
    pub success: Color,
    /// Lines up to what uses a package.
    pub up: Color,
}

impl Ink {
    pub fn of(theme: &Theme) -> Self {
        let p = theme.extended_palette();
        let bg = p.background.base.color;
        let fg = p.background.base.text;
        Ink {
            bg,
            raised: p.background.weak.color,
            fg,
            fg2: alpha(fg, 0.62),
            fg3: alpha(fg, 0.46),
            fg4: alpha(fg, 0.30),
            rule: mix_white(bg, 0.07),
            rule2: mix_white(bg, 0.12),
            face2: mix_white(bg, 0.06),
            face3: mix_white(bg, 0.10),
            accent: p.primary.base.color,
            warning: p.warning.base.color,
            danger: p.danger.base.color,
            success: p.success.base.color,
            up: Color { a: 1.0, ..mix_white(bg, 0.84) },
        }
    }

    /// The LED for a package, `(fill, edge)`: red over everything when the
    /// audit has advisories on it; only an edge for off and the origin.
    pub fn led(&self, p: &Package) -> (Color, Option<Color>) {
        if p.advised() {
            return (self.danger, None);
        }
        match p.standing {
            Standing::NotPushed => (self.warning, None),
            Standing::Local => (alpha(self.warning, 0.62), None),
            Standing::Update => (self.accent, None),
            Standing::OriginOnly => (Color::TRANSPARENT, Some(alpha(self.accent, 0.28))),
            Standing::Off => (mix_white(self.bg, 0.03), Some(mix_white(self.bg, 0.13))),
            Standing::Current => (mix_white(self.bg, 0.13), None),
        }
    }
}
