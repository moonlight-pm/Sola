//! Swatch — color preview tile. Used by the theme storybook page to
//! catalog every palette atom; also useful for in-app color pickers,
//! token editors, syntax-highlighting indicators.
//!
//! Default tile is 56×56 with the kit hairline border and 6px corner
//! radius. Override either via the explicit `swatch_sized` form.

use iced::widget::{Space, container};
use iced::{Background, Color, Element, Length, Theme};

use crate::components::style::{RADIUS_MD, hairline};

const DEFAULT_SIZE: f32 = 56.0;

/// Standard 56×56 swatch. Border is the kit hairline so adjacent
/// swatches read as a contact sheet rather than floating tiles.
pub fn swatch<'a, Message: 'a>(color: Color) -> Element<'a, Message, Theme> {
    swatch_sized(color, DEFAULT_SIZE)
}

pub fn swatch_sized<'a, Message: 'a>(color: Color, size: f32) -> Element<'a, Message, Theme> {
    container(Space::new())
        .style(move |t| style(t, color))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

pub fn style(theme: &Theme, color: Color) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(color)),
        border: hairline(p, RADIUS_MD),
        ..container::Style::default()
    }
}
