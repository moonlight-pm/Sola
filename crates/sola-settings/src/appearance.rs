//! Appearance: the background picture, whether the theme's colors follow
//! it (and which of its schemes), and the idle fade. The picture is set
//! from Paint (File → Set as Background) or `solactl background`; here it
//! can be cleared. The settings are `Topic::Appearance`: the shell acts on
//! the colors, sola-river on the fade. The schemes come from the shell
//! (`Topic::ThemeVariations`).

use iced::widget::{Space, button, column, container, row, slider, text, toggler};
use iced::{Alignment, Background as Fill, Border, Color, Element, Length, Theme};
use sola_bus::topics::{Appearance, Background, ThemeVariation, ThemeVariations, Topic};
use sola_kit::app::bus;
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::{form_row, toggle_style};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, alpha};
use sola_kit::components::text as kit_text;

#[derive(Debug, Clone)]
pub enum AppearanceMsg {
    /// Adapt colors to the background on / off.
    Adapt(bool),
    /// The strength slider moved (shown, not yet sent).
    Strength(u8),
    /// The strength slider was let go: send it.
    StrengthDone,
    /// One of the picture's schemes, by index.
    Variation(u8),
    /// Fade the windows when idle on / off.
    Fade(bool),
    /// The fade delay slider moved (shown, not yet sent).
    FadeAfter(u16),
    /// The fade delay slider was let go: send it.
    FadeAfterDone,
    /// Back to the plain ground.
    ClearPicture,
}

/// The picture (`Topic::Background`), the settings (`Topic::Appearance`)
/// and the picture's schemes (`Topic::ThemeVariations`), as the bus last
/// said.
#[derive(Default)]
pub struct AppearanceState {
    pub picture: Option<Background>,
    pub look: Appearance,
    pub variations: Option<ThemeVariations>,
}

pub fn update(msg: AppearanceMsg, state: &mut AppearanceState) {
    let look = &mut state.look;
    match msg {
        AppearanceMsg::Adapt(on) => look.adapt_theme = on,
        AppearanceMsg::Strength(v) => {
            look.strength = v;
            return;
        }
        AppearanceMsg::StrengthDone => {}
        AppearanceMsg::Variation(i) => look.variation = i,
        AppearanceMsg::Fade(on) => look.fade_idle = on,
        AppearanceMsg::FadeAfter(m) => {
            look.fade_after_min = m;
            return;
        }
        AppearanceMsg::FadeAfterDone => {}
        AppearanceMsg::ClearPicture => {
            if let Some(picture) = state.picture.clone() {
                send(|b| b.retract(Topic::Background(picture)));
            }
            return;
        }
    }
    let topic = Topic::Appearance(state.look.clone());
    send(|b| b.emit(topic));
}

fn send(write: impl FnOnce(&mut sola_bus::BusClient) -> std::io::Result<()>) {
    let sent = bus()
        .lock()
        .map_err(|_| "bus unavailable".to_string())
        .and_then(|mut client| write(&mut client).map_err(|e| e.to_string()));
    if let Err(e) = sent {
        tracing::warn!(error = %e, "appearance: bus write failed");
    }
}

pub fn view(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    column![picture(state), colors(state), idle(state)]
        .spacing(SPACE_LG + SPACE_MD)
        .width(Length::Fill)
        .max_width(560)
        .into()
}

fn heading(label: &str) -> Element<'_, AppearanceMsg> {
    kit_text::caption(label).style(kit_text::muted).into()
}

fn picture(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let line: Element<'_, AppearanceMsg> = match &state.picture {
        Some(p) => form_row(
            file_name(&p.path),
            kit_btn::labeled_sm("Clear", kit_btn::secondary).on_press(AppearanceMsg::ClearPicture),
        )
        .into(),
        None => kit_text::body("None — the black ground.").into(),
    };
    column![
        heading("Background picture"),
        line,
        kit_text::caption("Set one from Paint: File → Set as Background.").style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .into()
}

fn colors(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let look = &state.look;
    let mut col = column![
        heading("Colors"),
        form_row(
            "Adapt colors to the background",
            toggler(look.adapt_theme)
                .on_toggle(AppearanceMsg::Adapt)
                .style(toggle_style),
        ),
        form_row(
            "Strength",
            slider_with(
                slider(0..=100u8, look.strength, AppearanceMsg::Strength)
                    .on_release(AppearanceMsg::StrengthDone),
                format!("{}%", look.strength),
            ),
        ),
    ]
    .spacing(SPACE_SM);
    let choices = state
        .variations
        .as_ref()
        .filter(|v| look.adapt_theme && Some(&v.path) == state.picture.as_ref().map(|p| &p.path))
        .map(|v| &v.choices);
    if let Some(choices) = choices.filter(|c| c.len() > 1) {
        let picked = (look.variation as usize).min(choices.len() - 1);
        let cards = choices
            .iter()
            .enumerate()
            .fold(row![].spacing(SPACE_MD), |r, (i, v)| r.push(card(i, v, i == picked)));
        col = col.push(cards);
    }
    col.push(
        kit_text::caption(
            "Windows, the tab bar and the top row take a tint and an accent from \
             the picture's colors; their lightness stays the same. Strength is \
             how much of the picture's color the tint takes; below it, one \
             scheme per major color in the picture. Off, or with no picture: \
             the greys.",
        )
        .style(kit_text::muted),
    )
    .into()
}

fn idle(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let look = &state.look;
    column![
        heading("Idle"),
        form_row(
            "Fade the windows when idle",
            toggler(look.fade_idle)
                .on_toggle(AppearanceMsg::Fade)
                .style(toggle_style),
        ),
        form_row(
            "Start after",
            slider_with(
                slider(1..=60u16, look.fade_after_min, AppearanceMsg::FadeAfter)
                    .on_release(AppearanceMsg::FadeAfterDone),
                format!("{} min", look.fade_after_min),
            ),
        ),
        kit_text::caption(format!(
            "With no input for that long, the windows fade into the background \
             over {} minutes. Any input brings them back. Not while a window is \
             fullscreen or an app keeps the screen awake (a video).",
            Appearance::FADE_MINUTES
        ))
        .style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .into()
}

fn slider_with<'a>(
    slider: impl Into<Element<'a, AppearanceMsg>>,
    value: String,
) -> Element<'a, AppearanceMsg> {
    let slider: Element<'a, AppearanceMsg> = container(slider.into())
        .width(Length::Fixed(200.0))
        .into();
    row![
        slider,
        text(value)
            .width(Length::Fixed(48.0))
            .style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center)
    .into()
}

/// A scheme as a small window: the raised titlebar over the base, an
/// accent pill; ringed in the accent when picked.
fn card(i: usize, v: &ThemeVariation, picked: bool) -> Element<'_, AppearanceMsg> {
    let color = |hex: &str| sola_kit::theme::try_parse(hex).unwrap_or(Color::BLACK);
    let (base, raised, accent) = (color(&v.base), color(&v.raised), color(&v.accent));
    let bar = container(Space::new().width(Length::Fill).height(12))
        .style(move |_: &Theme| container::Style {
            background: Some(Fill::Color(raised)),
            ..container::Style::default()
        });
    let pill = container(Space::new().width(22).height(6)).style(move |_: &Theme| {
        container::Style {
            background: Some(Fill::Color(accent)),
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    });
    let face = container(column![bar, container(pill).padding(SPACE_SM)])
        .width(Length::Fixed(96.0))
        .height(Length::Fixed(56.0))
        .clip(true)
        .style(move |theme: &Theme| container::Style {
            background: Some(Fill::Color(base)),
            border: Border {
                color: if picked {
                    theme.extended_palette().primary.base.color
                } else {
                    alpha(Color::WHITE, 0.12)
                },
                width: if picked { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            ..container::Style::default()
        });
    let label = text(v.name.as_str()).size(12);
    button(
        column![face, label]
            .spacing(SPACE_XS)
            .align_x(Alignment::Center),
    )
    .padding(0)
    .style(kit_btn::ghost)
    .on_press(AppearanceMsg::Variation(i as u8))
    .into()
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}
