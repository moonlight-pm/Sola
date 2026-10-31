//! The looks: the picture's schemes (cards from `Topic::ThemeVariations`)
//! and the ones you make. Changing one of your colors while a scheme is in
//! use makes a new look of yours from it (`Appearance.customs`), so the
//! scheme stays as it was; picking the scheme again goes back to it.
//! `Appearance.variation` and `.colors` are always the look in use's.

use iced::border::Radius;
use iced::widget::{Space, button, column, container, row, slider, text, toggler};
use iced::{Alignment, Background as Fill, Border, Color, Element, Length, Theme};
use sola_bus::topics::{Appearance, CustomLook, ThemeVariation};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::toggle_style;
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XS, alpha};

use super::{AppearanceMsg, AppearanceState, slider_with};
use crate::page;

/// A saved look as it came in: one in use from before looks were kept
/// (`colors` set, no `custom`) becomes one; an index past the end, none.
pub fn settle(look: &mut Appearance) {
    if look.custom.is_some_and(|i| usize::from(i) >= look.customs.len()) {
        look.custom = None;
    }
    let room = look.customs.len() < Appearance::MAX_CUSTOMS;
    if look.custom.is_none() && !look.colors.is_empty() && room {
        look.customs.push(CustomLook {
            variation: look.variation,
            colors: look.colors.clone(),
        });
        look.custom = Some((look.customs.len() - 1) as u8);
    }
}

pub(super) fn pick_variation(look: &mut Appearance, i: u8) {
    look.variation = i;
    look.custom = None;
    look.colors.clear();
}

pub(super) fn pick_custom(look: &mut Appearance, i: u8) {
    let Some(mine) = look.customs.get(usize::from(i)) else {
        return;
    };
    look.variation = mine.variation;
    look.colors = mine.colors.clone();
    look.custom = Some(i);
}

/// Forget the look of yours in use; its scheme stays in use.
pub(super) fn remove_custom(look: &mut Appearance) {
    if let Some(i) = look.custom.take().map(usize::from)
        && i < look.customs.len()
    {
        look.customs.remove(i);
    }
    look.colors.clear();
}

/// Set one of your colors on the look of yours in use — or, with a scheme
/// in use, on a new look made from it (none past [`Appearance::MAX_CUSTOMS`]).
pub(super) fn set_color(look: &mut Appearance, key: &str, hex: String) {
    let i = match look.custom.map(usize::from) {
        Some(i) if i < look.customs.len() => i,
        _ => {
            if look.customs.len() >= Appearance::MAX_CUSTOMS {
                return;
            }
            look.customs.push(CustomLook {
                variation: look.variation,
                colors: Default::default(),
            });
            look.customs.len() - 1
        }
    };
    look.customs[i].colors.insert(key.to_string(), hex);
    look.colors = look.customs[i].colors.clone();
    look.custom = Some(i as u8);
}

/// The look of yours in use takes its scheme's color back for `key`.
pub(super) fn reset_color(look: &mut Appearance, key: &str) {
    let Some(i) = look.custom.map(usize::from).filter(|&i| i < look.customs.len()) else {
        return;
    };
    look.customs[i].colors.remove(key);
    look.colors = look.customs[i].colors.clone();
}

/// Your look `i`'s name: "Custom", "Custom 2", …
fn custom_name(i: usize) -> String {
    match i {
        0 => "Custom".into(),
        _ => format!("Custom {}", i + 1),
    }
}

/// The picture's schemes, when they are for this picture and in use.
fn shown(state: &AppearanceState) -> Option<&[ThemeVariation]> {
    state
        .variations
        .as_ref()
        .filter(|v| {
            state.look.adapt_theme && Some(&v.path) == state.picture.as_ref().map(|p| &p.path)
        })
        .map(|v| v.choices.as_slice())
}

/// A look as a card shows it.
struct Face {
    name: String,
    base: Color,
    raised: Color,
    accent: Color,
    /// The picture's color the scheme comes from.
    source: Option<Color>,
}

fn hex(s: &str) -> Option<Color> {
    sola_kit::theme::try_parse(s)
}

/// The greys of the seed palette, as a face.
fn grey_face() -> Face {
    let seed = sola_core::theme::Theme::default();
    let token = |name: &str| {
        seed.palette
            .tokens
            .get(name)
            .and_then(|t| hex(&t.value))
            .unwrap_or(Color::BLACK)
    };
    Face {
        name: "Grey".into(),
        base: token("bg-primary"),
        raised: token("bg-secondary"),
        accent: token("accent"),
        source: None,
    }
}

fn scheme_face(v: &ThemeVariation) -> Face {
    Face {
        name: v.name.clone(),
        base: hex(&v.base).unwrap_or(Color::BLACK),
        raised: hex(&v.raised).unwrap_or(Color::BLACK),
        accent: hex(&v.accent).unwrap_or(Color::BLACK),
        source: hex(&v.source),
    }
}

/// The scheme `variation` (past the end, the last) — the greys while the
/// colors do not follow a picture.
fn base_face(state: &AppearanceState, variation: u8) -> Face {
    match shown(state) {
        Some(choices) if !choices.is_empty() => {
            scheme_face(&choices[usize::from(variation).min(choices.len() - 1)])
        }
        _ => grey_face(),
    }
}

fn custom_face(state: &AppearanceState, i: usize, mine: &CustomLook) -> Face {
    let mut face = base_face(state, mine.variation);
    let color = |key: &str| mine.colors.get(key).and_then(|h| hex(h));
    face.base = color("window").unwrap_or(face.base);
    face.raised = color("panels").unwrap_or(face.raised);
    face.accent = color("accent").unwrap_or(face.accent);
    face.name = custom_name(i);
    face.source = None;
    face
}

pub(super) fn colors(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let look = &state.look;
    let mut rows = vec![
        page::setting(
            "Adapt colors to the background",
            toggler(look.adapt_theme)
                .on_toggle(AppearanceMsg::Adapt)
                .style(toggle_style),
        ),
        page::setting(
            "Strength",
            slider_with(
                slider(0..=100u8, look.strength, AppearanceMsg::Strength)
                    .on_release(AppearanceMsg::StrengthDone),
                format!("{}%", look.strength),
            ),
        ),
    ];
    let mut cards: Vec<Element<'_, AppearanceMsg>> = Vec::new();
    let plain = look.custom.is_none();
    match shown(state) {
        Some(choices) if choices.len() > 1 => {
            let picked = usize::from(look.variation).min(choices.len() - 1);
            for (i, v) in choices.iter().enumerate() {
                cards.push(card(
                    scheme_face(v),
                    plain && i == picked,
                    AppearanceMsg::Variation(i as u8),
                ));
            }
        }
        // One base look: a card only to go back to from yours.
        _ if !look.customs.is_empty() => cards.push(card(
            base_face(state, look.variation),
            plain,
            AppearanceMsg::Variation(look.variation),
        )),
        _ => {}
    }
    for (i, mine) in look.customs.iter().enumerate() {
        cards.push(card(
            custom_face(state, i, mine),
            look.custom == Some(i as u8),
            AppearanceMsg::Custom(i as u8),
        ));
    }
    if !cards.is_empty() {
        rows.push(page::plain(
            iced::widget::Row::from_vec(cards)
                .spacing(SPACE_MD)
                .wrap()
                .vertical_spacing(SPACE_MD),
        ));
    }
    page::section(
        "Colors",
        None,
        page::group(rows),
        Some(
            "Windows, the tab bar and the top row take a tint and an accent from \
             one of the picture's colors (its swatch is beside the name); their \
             lightness stays the same. Strength is how much of that color the \
             tint takes. Off, or with no picture: the greys."
                .into(),
        ),
    )
}

/// The color a custom key shows now: the live theme's, else the seed's.
pub(super) fn current(state: &AppearanceState, key: &str) -> Color {
    let token = sola_core::theme::custom_color_token(key).unwrap_or("bg-primary");
    let from = |t: &sola_core::theme::Theme| {
        t.palette.tokens.get(token).and_then(|t| hex(&t.value))
    };
    state
        .theme
        .as_ref()
        .and_then(from)
        .or_else(|| from(&sola_core::theme::Theme::default()))
        .unwrap_or(Color::BLACK)
}

/// Your colors: the look in use's, each a swatch to change.
pub(super) fn yours(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let look = &state.look;
    let editing = look
        .custom
        .map(usize::from)
        .filter(|&i| i < look.customs.len());
    let full = editing.is_none() && look.customs.len() >= Appearance::MAX_CUSTOMS;
    let remove = editing.map(|i| {
        kit_btn::labeled_sm(format!("Remove {}", custom_name(i)), kit_btn::secondary)
            .on_press(AppearanceMsg::RemoveCustom)
            .into()
    });
    let mut rows: Vec<Element<'_, AppearanceMsg>> = Vec::new();
    for &(key, label) in sola_core::theme::CUSTOM_COLORS {
        let color = current(state, key);
        let swatch = button(container(Space::new().width(40).height(18)).style(move |_: &Theme| {
            container::Style {
                background: Some(Fill::Color(color)),
                border: Border {
                    color: alpha(Color::WHITE, 0.2),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            }
        }))
        .padding(0)
        .style(kit_btn::ghost)
        .on_press_maybe((!full).then_some(AppearanceMsg::Pick(key)));
        let mut controls = row![].spacing(SPACE_SM).align_y(Alignment::Center);
        if look.colors.contains_key(key) {
            controls = controls.push(
                page::action("Reset", kit_btn::ghost, AppearanceMsg::ResetColor(key)),
            );
        }
        controls = controls.push(swatch);
        rows.push(page::setting(label, controls));
        if let Some((_, picker)) = state.picking.as_ref().filter(|(k, _)| *k == key) {
            rows.push(page::editor(
                column![
                    picker.view().map(AppearanceMsg::Picker),
                    row![
                        page::action("Use", kit_btn::primary, AppearanceMsg::UsePicked),
                        page::action("Cancel", kit_btn::ghost, AppearanceMsg::Pick(key)),
                    ]
                    .spacing(SPACE_SM),
                ]
                .spacing(SPACE_SM),
            ));
        }
    }
    let base = base_face(state, look.variation).name;
    let what = match editing {
        Some(i) => format!(
            "Changes go to {} (made from {base}); {base} itself stays as it is — pick it \
             above to go back.",
            custom_name(i)
        ),
        None if full => format!(
            "You keep up to {} looks of your own: remove one to make another.",
            Appearance::MAX_CUSTOMS
        ),
        None => format!(
            "Changing one makes a look of your own from {base}, beside the picture's \
             schemes; {base} itself stays as it is."
        ),
    };
    page::section(
        "Your colors",
        remove,
        page::group(rows),
        Some(format!(
            "{what} The window, the panels (bars, titlebars, sidebars), the highlight \
             (the tab bar's focused row; selections take its hue) and the accent."
        )),
    )
}

/// A look as a small window: the raised titlebar over the base, an accent
/// pill; ringed in the accent when picked. Below, its name, after the
/// picture's color it comes from.
fn card(face: Face, picked: bool, on_press: AppearanceMsg) -> Element<'static, AppearanceMsg> {
    const RADIUS: f32 = 8.0;
    let Face {
        name,
        base,
        raised,
        accent,
        source,
    } = face;
    let ring = if picked { 2.0 } else { 1.0 };
    // Inside the ring, rounded to match it: `clip` cuts square, so a
    // square bar would cover the window's top corners.
    let bar = container(Space::new().width(Length::Fill).height(12)).style(move |_: &Theme| {
        container::Style {
            background: Some(Fill::Color(raised)),
            border: Border {
                radius: Radius {
                    top_left: RADIUS - ring,
                    top_right: RADIUS - ring,
                    bottom_right: 0.0,
                    bottom_left: 0.0,
                },
                ..Border::default()
            },
            ..container::Style::default()
        }
    });
    let pill = container(Space::new().width(22).height(6)).style(move |_: &Theme| container::Style {
        background: Some(Fill::Color(accent)),
        border: Border {
            radius: 3.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    });
    let window = container(column![bar, container(pill).padding(SPACE_SM)])
        .width(Length::Fixed(96.0))
        .height(Length::Fixed(56.0))
        .padding(ring)
        .clip(true)
        .style(move |theme: &Theme| container::Style {
            background: Some(Fill::Color(base)),
            border: Border {
                color: if picked {
                    theme.extended_palette().primary.base.color
                } else {
                    alpha(Color::WHITE, 0.12)
                },
                width: ring,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        });
    let mut label = row![].spacing(SPACE_XS).align_y(Alignment::Center);
    if let Some(source) = source {
        label = label.push(container(Space::new().width(8).height(8)).style(move |_: &Theme| {
            container::Style {
                background: Some(Fill::Color(source)),
                border: Border {
                    color: alpha(Color::WHITE, 0.25),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            }
        }));
    }
    label = label.push(text(name).size(12));
    button(column![window, label].spacing(SPACE_XS).align_x(Alignment::Center))
        .padding(0)
        .style(kit_btn::ghost)
        .on_press(on_press)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_scheme(variation: u8) -> Appearance {
        Appearance {
            adapt_theme: true,
            variation,
            ..Appearance::default()
        }
    }

    #[test]
    fn changing_a_color_on_a_scheme_makes_a_look_and_keeps_the_scheme() {
        let mut look = with_scheme(1);
        set_color(&mut look, "accent", "#ff8800".into());
        assert_eq!(look.customs.len(), 1);
        assert_eq!(look.custom, Some(0));
        assert_eq!(look.customs[0].variation, 1);
        assert_eq!(look.colors["accent"], "#ff8800");
        // More changes go to the same look.
        set_color(&mut look, "window", "#101820".into());
        assert_eq!(look.customs.len(), 1);
        assert_eq!(look.customs[0].colors.len(), 2);
        // The scheme again: as it was, the look kept.
        pick_variation(&mut look, 1);
        assert!(look.colors.is_empty() && look.custom.is_none());
        assert_eq!(look.customs.len(), 1);
        // Back to the look: its scheme and colors.
        pick_variation(&mut look, 3);
        pick_custom(&mut look, 0);
        assert_eq!(look.variation, 1);
        assert_eq!(look.colors.len(), 2);
    }

    #[test]
    fn a_change_from_another_scheme_makes_a_second_look() {
        let mut look = with_scheme(0);
        set_color(&mut look, "accent", "#ff8800".into());
        pick_variation(&mut look, 2);
        set_color(&mut look, "accent", "#00ff88".into());
        assert_eq!(look.customs.len(), 2);
        assert_eq!(look.custom, Some(1));
        assert_eq!(look.customs[0].colors["accent"], "#ff8800", "the first kept");
    }

    #[test]
    fn reset_and_remove() {
        let mut look = with_scheme(0);
        set_color(&mut look, "accent", "#ff8800".into());
        set_color(&mut look, "panels", "#202020".into());
        reset_color(&mut look, "accent");
        assert!(!look.colors.contains_key("accent"));
        assert_eq!(look.customs[0].colors.len(), 1);
        remove_custom(&mut look);
        assert!(look.customs.is_empty() && look.custom.is_none() && look.colors.is_empty());
        // No look of yours in use: a reset does nothing.
        reset_color(&mut look, "panels");
        assert!(look.customs.is_empty());
    }

    #[test]
    fn no_more_looks_past_the_most() {
        let mut look = with_scheme(0);
        for i in 0..Appearance::MAX_CUSTOMS + 2 {
            pick_variation(&mut look, 0);
            set_color(&mut look, "accent", format!("#0000{i:02x}"));
        }
        assert_eq!(look.customs.len(), Appearance::MAX_CUSTOMS);
        assert!(look.custom.is_none(), "the last change made nothing");
    }

    #[test]
    fn colors_from_before_looks_become_one() {
        let mut look = with_scheme(2);
        look.colors.insert("accent".into(), "#ff8800".into());
        settle(&mut look);
        assert_eq!(look.custom, Some(0));
        assert_eq!(look.customs[0].variation, 2);
        let mut stale = with_scheme(0);
        stale.custom = Some(4);
        settle(&mut stale);
        assert!(stale.custom.is_none());
    }
}
