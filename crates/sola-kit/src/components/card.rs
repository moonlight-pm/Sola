//! Card — an elevated container with kit chrome (raised bg, hairline
//! border, rounded corners, kit padding).
//!
//! `card(content)` returns an `iced::widget::Container` so the caller
//! can chain further iced methods (`.width(Fill)`, `.padding(...)`,
//! `.center_x()`, …). The style fn is exposed separately for callers
//! who already have a container and only want the kit chrome.

use iced::widget::{Container, container};
use iced::{Background, Color, Element, Shadow, Theme, Vector};

use crate::components::style::{hairline, RADIUS_LG, RADIUS_MD, RADIUS_XL, SPACE_XL};

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
/// `RADIUS_XL` (14px), and a heavy drop shadow to lift the panel off
/// the dimmed backdrop.
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
        border: hairline(p, RADIUS_XL),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.55),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 48.0,
        },
        ..container::Style::default()
    }
}

/// Deep-shadow modal card chrome (e.g. a centred launcher or command
/// palette panel). Opaque canvas background (`weaker` tier), hairline
/// border at `RADIUS_XL` (14px), and a heavy drop shadow. Returns a
/// `Container` so the caller can chain `.width(..)`, `.padding(..)`, etc.
pub fn modal<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(modal_style)
}

/// Parameterized backplate style: caller supplies fill and border
/// colors (alpha included — e.g. the shell's `shell-switcher-bg` /
/// `shell-switcher-border` tokens). Radius, border width, and shadow
/// are fixed to the backplate's values.
///
/// Radius choice: `RADIUS_XL` (14px) is used for the modal; the
/// switcher backplate is a slightly softer 16px to visually distinguish
/// it as a secondary frame. Using a plain literal keeps the two values
/// intentionally independent — don't abstract them into the same const.
pub fn backplate_style(fill: Color, border: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: iced::Border {
            color: border,
            width: 1.0,
            radius: 16.0.into(), // switcher backplate: 2px softer than modal
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
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
/// border at 16px radius, with a deep drop shadow. Thin wrapper over
/// [`backplate_style`] passing the palette-derived defaults.
pub fn accent_backplate_style(theme: &Theme) -> container::Style {
    let accent = theme.extended_palette().primary.base.color;
    backplate_style(Color { a: 0.18, ..accent }, Color { a: 0.35, ..accent })(theme)
}

/// Accent-tinted translucent backplate (e.g. the app switcher's frame):
/// primary colour at low alpha for fill and border, 16px radius, deep
/// shadow. Returns a `Container` so the caller can chain sizing/padding.
pub fn accent_backplate<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(accent_backplate_style)
}

/// Container style for a selectable tile (e.g. an app card in a switcher
/// grid). The container analog of `button::list_item` for tiles that need a
/// `mouse_area` wrapper (hover-driven selection) instead of a pressable
/// button: selected → filled accent pill with `primary.base.text` label
/// colour for legibility on the tinted fill; unselected → transparent with
/// no text-colour override (inherits the window default).
pub fn list_tile_style(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = theme.extended_palette();
        let background = if selected {
            Some(Background::Color(p.primary.base.color))
        } else {
            None
        };
        container::Style {
            background,
            text_color: selected.then_some(p.primary.base.text),
            border: iced::Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_MD.into(),
            },
            ..container::Style::default()
        }
    }
}
