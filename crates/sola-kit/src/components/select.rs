//! Compact identity select — trigger + hanging menu.
//!
//! Parent-controlled: the caller owns `open` and which option is
//! selected. The kit draws the chrome.
//!
//! Signature: a small **enamel plate** derived from a stable seed (profile
//! id, theme name, …) sits in the trigger and on every row. Selection is
//! the quiet selection wash + a lucide check — not a grey slab, not a
//! unicode tick. The menu hangs *below* the trigger at the trigger's
//! width (select grammar) — a raised popover, not a darker inset card.

use std::sync::OnceLock;

use iced::widget::{button, column, container, row, svg, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use crate::components::button as kit_button;
use crate::components::icon::{icon_handle, icon_svg};
use crate::components::popover::{self, popover, popover_anchored};
use crate::components::style::{hairline, mix_white, RADIUS_SM, SPACE_SM};
use crate::fonts;

/// Default hanging-menu width when the caller pins one via [`select_sized`].
pub const MENU_W_DEFAULT: f32 = 220.0;
const MARK: f32 = 10.0;

/// One row in the hanging menu.
pub struct SelectOption<Message> {
    pub label: String,
    pub selected: bool,
    pub message: Message,
    /// Stable seed for the enamel plate (id, slug). `None` omits the mark.
    pub mark_seed: Option<String>,
}

impl<Message> SelectOption<Message> {
    pub fn new(label: impl Into<String>, selected: bool, message: Message) -> Self {
        Self {
            label: label.into(),
            selected,
            message,
            mark_seed: None,
        }
    }

    pub fn mark(mut self, seed: impl Into<String>) -> Self {
        self.mark_seed = Some(seed.into());
        self
    }
}

/// Enamel plate colour from a stable seed. A small set of kiln-like
/// hues, mixed toward graphite so they read as file-tab enamel, not
/// avatar confetti.
pub fn enamel(seed: &str) -> Color {
    let mut h = 2_166_136_261u32;
    for b in seed.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    const KILN: [[f32; 3]; 8] = [
        [0.38, 0.68, 0.72], // patina teal
        [0.48, 0.54, 0.78], // slate iris
        [0.72, 0.52, 0.42], // kiln clay
        [0.46, 0.64, 0.50], // oxidized sage
        [0.68, 0.48, 0.58], // dusk enamel
        [0.58, 0.62, 0.40], // olive brass
        [0.40, 0.52, 0.70], // ink blue
        [0.74, 0.62, 0.40], // warm brass
    ];
    let [r, g, b] = KILN[(h as usize) % KILN.len()];
    Color { r, g, b, a: 1.0 }
}

/// Small rounded enamel plate. Rim is a soft white mix so the chip
/// reads as glazed, not a flat swatch.
pub fn identity_mark<'a, Message: 'a>(
    seed: &str,
    size: f32,
) -> Element<'a, Message, Theme> {
    let fill = enamel(seed);
    container(Space::new().width(size).height(size))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_theme: &Theme| {
            container::Style {
                background: Some(Background::Color(fill)),
                border: Border {
                    color: Color {
                        r: (fill.r * 0.55 + 0.45).min(1.0),
                        g: (fill.g * 0.55 + 0.45).min(1.0),
                        b: (fill.b * 0.55 + 0.45).min(1.0),
                        a: 1.0,
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// Select whose menu matches the trigger's laid-out width.
pub fn select<'a, Message: Clone + 'a>(
    label: impl Into<String>,
    options: impl IntoIterator<Item = SelectOption<Message>>,
    open: bool,
    on_toggle: Message,
    on_dismiss: Message,
) -> Element<'a, Message, Theme> {
    select_inner(label, options, open, on_toggle, on_dismiss, None)
}

/// Select whose hanging menu is pinned to `menu_width` (typically the
/// sidebar gutter so the panel aligns with the trigger). Prefer
/// [`select`] when the menu should match the trigger.
pub fn select_sized<'a, Message: Clone + 'a>(
    label: impl Into<String>,
    options: impl IntoIterator<Item = SelectOption<Message>>,
    open: bool,
    on_toggle: Message,
    on_dismiss: Message,
    menu_width: f32,
) -> Element<'a, Message, Theme> {
    select_inner(
        label,
        options,
        open,
        on_toggle,
        on_dismiss,
        Some(menu_width.max(140.0)),
    )
}

fn select_inner<'a, Message: Clone + 'a>(
    label: impl Into<String>,
    options: impl IntoIterator<Item = SelectOption<Message>>,
    open: bool,
    on_toggle: Message,
    on_dismiss: Message,
    menu_width: Option<f32>,
) -> Element<'a, Message, Theme> {
    let label = label.into();
    let options: Vec<SelectOption<Message>> = options.into_iter().collect();
    let trigger_seed = options
        .iter()
        .find(|o| o.selected)
        .and_then(|o| o.mark_seed.clone())
        .or_else(|| options.first().and_then(|o| o.mark_seed.clone()));

    let trigger = trigger_button(label, trigger_seed.as_deref(), open, on_toggle)
        .width(Length::Fill);

    if !open {
        return trigger.into();
    }

    let rows = column(options.into_iter().map(option_row)).spacing(2);
    let menu_w = match menu_width {
        Some(w) => Length::Fixed(w),
        None => Length::Fill,
    };
    let menu = popover(rows)
        .padding(SPACE_SM)
        .width(menu_w)
        .style(popover::style);

    let mut hanging = popover_anchored(trigger, menu, on_dismiss)
        .placement(popover::Placement::Below);
    if menu_width.is_none() {
        hanging = hanging.match_anchor_width();
    }
    hanging.into()
}

fn trigger_button<'a, Message: Clone + 'a>(
    label: String,
    seed: Option<&str>,
    open: bool,
    on_toggle: Message,
) -> iced::widget::Button<'a, Message> {
    let chevron = if open {
        icon_svg(chevron_up(), 12)
    } else {
        icon_svg(chevron_down(), 12)
    };

    let mut kids: Vec<Element<'a, Message, Theme>> = Vec::new();
    if let Some(seed) = seed {
        kids.push(identity_mark(seed, MARK));
    }
    kids.push(
        text(label)
            .size(12)
            .font(fonts::ui_medium())
            .width(Length::Fill)
            .into(),
    );
    kids.push(chevron);

    button(
        row(kids)
            .align_y(Alignment::Center)
            .spacing(SPACE_SM),
    )
    .padding(Padding::from([5, 8]))
    .style(move |theme, status| trigger_style(theme, status, open))
    .on_press(on_toggle)
}

fn option_row<'a, Message: Clone + 'a>(
    opt: SelectOption<Message>,
) -> Element<'a, Message, Theme> {
    let selected = opt.selected;
    let mut kids: Vec<Element<'a, Message, Theme>> = Vec::new();
    if let Some(seed) = opt.mark_seed.as_deref() {
        kids.push(identity_mark(seed, MARK));
    } else {
        kids.push(Space::new().width(MARK).height(MARK).into());
    }
    kids.push(
        text(opt.label)
            .size(12)
            .font(if selected {
                fonts::ui_medium()
            } else {
                fonts::ui()
            })
            .width(Length::Fill)
            .into(),
    );
    if selected {
        kids.push(icon_svg(check_icon(), 12));
    } else {
        kids.push(Space::new().width(12.0).height(12.0).into());
    }

    button(
        row(kids)
            .align_y(Alignment::Center)
            .spacing(SPACE_SM),
    )
    .padding(Padding::from([6, 8]))
    .width(Length::Fill)
    .style(kit_button::list_item(selected))
    .on_press(opt.message)
    .into()
}

fn trigger_style(theme: &Theme, status: button::Status, open: bool) -> button::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let rest = mix_white(raised, 0.03);
    let hover = mix_white(p.background.strong.color, 0.04);
    // Open = hover lift, not a darkened-cyan wash.
    let open_fill = mix_white(p.background.strong.color, 0.04);
    let (bg, edge) = if open {
        (open_fill, mix_white(open_fill, 0.10))
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => {
                (hover, mix_white(hover, 0.10))
            }
            _ => (rest, hairline(p, RADIUS_SM).color),
        }
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.background.base.text,
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

fn chevron_down() -> svg::Handle {
    static H: OnceLock<svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/chevron-down")).clone()
}

fn chevron_up() -> svg::Handle {
    static H: OnceLock<svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/chevron-up")).clone()
}

fn check_icon() -> svg::Handle {
    static H: OnceLock<svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/check")).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enamel_is_stable_for_a_seed() {
        let a = enamel("profile-aaa");
        let b = enamel("profile-aaa");
        assert_eq!(a, b);
    }

    #[test]
    fn enamel_differs_across_seeds() {
        // Eight kiln slots — two arbitrary ids should almost always differ.
        // If they collide, pick another pair rather than weakening the test.
        let a = enamel("primary");
        let b = enamel("alternate");
        assert_ne!(a, b);
    }
}
