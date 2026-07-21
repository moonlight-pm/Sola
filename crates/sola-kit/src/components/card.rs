//! Card — an elevated container with kit chrome (raised bg, hairline
//! border, rounded corners, kit padding).
//!
//! `card(content)` returns an `iced::widget::Container` so the caller
//! can chain further iced methods (`.width(Fill)`, `.padding(...)`,
//! `.center_x()`, …). The style fn is exposed separately for callers
//! who already have a container and only want the kit chrome.

use iced::widget::{Container, container};
use iced::{Background, Color, Element, Shadow, Theme, Vector};

use crate::components::style::{hairline, RADIUS_LG, RADIUS_MD, SPACE_XL};

/// Wrap `content` in a card-styled container. Default padding is 16px;
/// override with `.padding(...)` on the returned container if needed.
pub fn card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style).padding(SPACE_XL)
}

pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        ..container::Style::default()
    }
}

/// Style for [`modal`]: opaque raised background, hairline border at
/// `RADIUS_LG` (8px), and a soft drop shadow — Spotlight/command-palette
/// restraint, not a marketing card lift.
///
/// **Background is `weaker`, not `base`**: overlay windows set
/// `base`'s alpha to zero for the see-through window fill; the modal
/// card must remain opaque, so we use the card-raised tier instead.
///
/// Shadow is the kit's standard escape hatch — iced's palette
/// vocabulary carries no shadow token (same rationale as `popover::style`).
pub fn modal_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.38),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 28.0,
        },
        ..container::Style::default()
    }
}

/// Centred overlay panel chrome (launcher / command palette). Opaque
/// `weaker` bg, hairline at `RADIUS_LG`, soft shadow. Returns a
/// `Container` so the caller can chain `.width(..)`, `.padding(..)`, etc.
pub fn modal<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(modal_style)
}

/// Parameterized backplate style: caller supplies fill and border
/// colors (alpha included — e.g. the shell's `shell-switcher-bg` /
/// `shell-switcher-border` tokens). Radius and border width are fixed
/// to the backplate's values.
///
/// Radius matches modal (`RADIUS_LG` / 8px) — Mission Control restraint,
/// not a marketing HUD card.
///
/// No drop shadow: the shell's renderer doesn't blur shadow quads, so a
/// shadow renders as a hard offset rectangle that pokes out past the
/// rounded border (most visibly below it). The 1px border + translucent
/// fill already separate the backplate from what's behind it.
pub fn backplate_style(fill: Color, border: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: iced::Border {
            color: border,
            width: 1.0,
            radius: RADIUS_LG.into(),
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
/// border at `RADIUS_LG`, with a deep drop shadow. Thin wrapper over
/// [`backplate_style`] passing the palette-derived defaults.
pub fn accent_backplate_style(theme: &Theme) -> container::Style {
    let accent = theme.extended_palette().primary.base.color;
    backplate_style(Color { a: 0.18, ..accent }, Color { a: 0.35, ..accent })(theme)
}

/// Accent-tinted translucent backplate (storybook demo of primary glass):
/// primary colour at low alpha for fill and border, `RADIUS_LG`. Returns a
/// `Container` so the caller can chain sizing/padding.
pub fn accent_backplate<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(accent_backplate_style)
}

/// Container style for a selectable tile (e.g. an app card in a switcher
/// grid). The container analog of `button::list_item` for tiles that need a
/// `mouse_area` wrapper (hover-driven selection) instead of a pressable
/// button: selected → quiet [`crate::theme::selection`] fill (not a full
/// accent pill); unselected → transparent with no text-colour override
/// (inherits the window default).
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
            border: iced::Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_MD.into(),
            },
            ..container::Style::default()
        }
    }
}

/// Like [`list_tile_style`] but with caller-supplied colors: `fill` is the
/// selected-tile background and `fg` the tile's text/foreground (applied
/// in both states so SVG glyphs and labels tint consistently). Lets shell
/// chrome drive switcher tiles from its own `shell-switcher-icon-*` tokens
/// instead of the palette's selection / fg. Same radius (`RADIUS_MD`) and
/// transparent-when-unselected behaviour as [`list_tile_style`].
pub fn list_tile_style_colored(
    selected: bool,
    fill: Color,
    fg: Color,
) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: selected.then_some(Background::Color(fill)),
        text_color: Some(fg),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}
