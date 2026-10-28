//! The kit's scrollbar: an accent pill inset in a dark channel.
//!
//! iced's default (a 10 px bar with 2 px corners that turns the accent on
//! hover) is what a bare `iced::widget::scrollable` draws, and iced's
//! `Theme` fixes that default — so kit apps take this one through
//! [`scrollable`], or [`style`] with [`bar`] where they build their own.
//! Both directions take the same geometry. The channel shows only while
//! the content overflows (iced draws no rail otherwise).

use iced::widget::scrollable::{
    AutoScroll, Direction, Rail, Scrollable, Scrollbar, Scroller, Status, Style,
};
use iced::{Border, Color, Element, Shadow, Theme, Vector};

/// The channel's width.
pub const RAIL_W: f32 = 8.0;
/// The pill's gap to the channel's edge, on every side.
pub const INSET: f32 = 2.0;
/// The pill's drawn width.
pub const THUMB_W: f32 = RAIL_W - 2.0 * INSET;
/// Space between the channel and the content's edge, either side.
pub const MARGIN: f32 = 2.0;
/// How far in from the scrollable's edge the bar reaches: pad scrolled
/// content by this much, plus the gap it wants from the channel.
pub const CLEARANCE: f32 = RAIL_W + 2.0 * MARGIN;

/// The channel: black this strong over whatever surface it sits on, in a
/// rim of the text colour this faint — a groove even on a black face.
const CHANNEL_A: f32 = 0.55;
const RIM_A: f32 = 0.1;
/// The pill — the theme's accent — at rest, under the pointer, and held.
const THUMB_A: f32 = 0.5;
const THUMB_HOVER_A: f32 = 0.7;
const THUMB_DRAG_A: f32 = 0.85;

/// The kit's scrollbar geometry, for `Direction::Vertical` /
/// `Direction::Horizontal`.
pub fn bar() -> Scrollbar {
    Scrollbar::new()
        .width(RAIL_W)
        .scroller_width(RAIL_W)
        .margin(MARGIN)
}

/// A vertical `scrollable` with the kit's scrollbar. Callers may still set
/// a horizontal direction (`Direction::Horizontal(bar())`).
pub fn scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(Direction::Vertical(bar()))
        .style(style)
}

/// A dark channel (black over the surface, so it reads on the face, a
/// raised list or a sheet) with a faint rim, and the accent pill in it. The pill's box is
/// the channel's full width — the whole channel grabs — and its
/// [`INSET`] is a transparent border: iced's wgpu quad draws the border
/// in place of the fill, so the pill shows [`INSET`] in from the channel
/// on every side, the ends too.
pub fn style(theme: &Theme, status: Status) -> Style {
    let p = theme.extended_palette();
    let ink = p.background.base.text;
    let accent = p.primary.base.color;
    let rail = |thumb_a: f32| Rail {
        background: Some(Color { a: CHANNEL_A, ..Color::BLACK }.into()),
        border: Border {
            color: Color { a: RIM_A, ..ink },
            width: 1.0,
            radius: (RAIL_W / 2.0).into(),
        },
        scroller: Scroller {
            background: Color { a: thumb_a, ..accent }.into(),
            border: Border {
                color: Color::TRANSPARENT,
                width: INSET,
                radius: (RAIL_W / 2.0).into(),
            },
        },
    };
    let (vertical, horizontal) = match status {
        Status::Active { .. } => (THUMB_A, THUMB_A),
        Status::Hovered {
            is_vertical_scrollbar_hovered: v,
            is_horizontal_scrollbar_hovered: h,
            ..
        } => (
            if v { THUMB_HOVER_A } else { THUMB_A },
            if h { THUMB_HOVER_A } else { THUMB_A },
        ),
        Status::Dragged {
            is_vertical_scrollbar_dragged: v,
            is_horizontal_scrollbar_dragged: h,
            ..
        } => (
            if v { THUMB_DRAG_A } else { THUMB_A },
            if h { THUMB_DRAG_A } else { THUMB_A },
        ),
    };
    Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail(vertical),
        horizontal_rail: rail(horizontal),
        gap: None,
        auto_scroll: AutoScroll {
            background: p.background.weaker.color.scale_alpha(0.9).into(),
            border: Border {
                color: Color { a: 0.8, ..ink },
                width: 1.0,
                radius: f32::MAX.into(),
            },
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.7),
                offset: Vector::ZERO,
                blur_radius: 2.0,
            },
            icon: Color { a: 0.8, ..ink },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_pill_sits_inset_in_the_channel_and_brightens_when_held() {
        assert!(THUMB_W > 0.0 && THUMB_W < RAIL_W);
        let theme = crate::theme::default_theme();
        let rest = style(
            &theme,
            Status::Active {
                is_horizontal_scrollbar_disabled: false,
                is_vertical_scrollbar_disabled: false,
            },
        );
        let held = style(
            &theme,
            Status::Dragged {
                is_horizontal_scrollbar_dragged: false,
                is_vertical_scrollbar_dragged: true,
                is_horizontal_scrollbar_disabled: false,
                is_vertical_scrollbar_disabled: false,
            },
        );
        let alpha = |s: &Style| match s.vertical_rail.scroller.background {
            iced::Background::Color(c) => c.a,
            _ => 0.0,
        };
        assert!(alpha(&held) > alpha(&rest));
        let accent = theme.extended_palette().primary.base.color;
        assert_eq!(
            rest.vertical_rail.scroller.background,
            Color { a: THUMB_A, ..accent }.into()
        );
        assert_eq!(rest.vertical_rail.scroller.border.width, INSET);
        assert_eq!(rest.vertical_rail.scroller.border.color, Color::TRANSPARENT);
        assert!(rest.vertical_rail.border.width < INSET, "the rim clears the pill");
        assert_eq!(
            held.horizontal_rail.scroller.background,
            rest.horizontal_rail.scroller.background
        );
    }
}
