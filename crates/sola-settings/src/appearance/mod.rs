//! Appearance: the background picture, whether the theme's colors follow
//! it (and which of its schemes), the looks you make with your own colors,
//! and the idle fade. The picture is set from Paint (File → Set as
//! Background) or `solactl background`; here it can be cleared. The
//! settings are `Topic::Appearance`: the shell acts on the colors,
//! sola-river on the fade. The schemes come from the shell
//! (`Topic::ThemeVariations`).

mod looks;

use iced::widget::{container, row, slider, text, toggler};
use iced::{Alignment, Element, Length};
use sola_bus::topics::{Appearance, Background, ThemeVariations, Topic};
use sola_kit::app::bus;
use sola_kit::components::button as kit_btn;
use sola_kit::components::color_picker::{ColorPicker, Message as PickerMsg};
use sola_kit::components::form::toggle_style;
use sola_kit::components::style::SPACE_SM;
use sola_kit::components::text as kit_text;

use crate::page;

pub use looks::settle;

#[derive(Debug, Clone)]
pub enum AppearanceMsg {
    /// Adapt colors to the background on / off.
    Adapt(bool),
    /// The strength slider moved (shown, not yet sent).
    Strength(u8),
    /// The strength slider was let go: send it.
    StrengthDone,
    /// One of the picture's schemes, by index, as it is.
    Variation(u8),
    /// One of your looks (`Appearance.customs`), by index.
    Custom(u8),
    /// Forget the look of yours in use.
    RemoveCustom,
    /// Fade the windows when idle on / off.
    Fade(bool),
    /// The fade delay and length sliders moved (shown, not yet sent).
    FadeAfter(u16),
    FadeOver(u16),
    /// A fade slider was let go: send it.
    FadeDone,
    /// Back to the plain ground.
    ClearPicture,
    /// Open the picker for a custom color (`CUSTOM_COLORS` key); again
    /// closes it.
    Pick(&'static str),
    Picker(PickerMsg),
    /// Use the picked color.
    UsePicked,
    /// The look in use drops one of your colors.
    ResetColor(&'static str),
}

/// The picture (`Topic::Background`), the settings (`Topic::Appearance`)
/// and the picture's schemes (`Topic::ThemeVariations`), as the bus last
/// said.
#[derive(Default)]
pub struct AppearanceState {
    pub picture: Option<Background>,
    pub look: Appearance,
    pub variations: Option<ThemeVariations>,
    /// The live theme (`Topic::Theme`), for showing the colors as they are.
    pub theme: Option<sola_core::theme::Theme>,
    /// The custom color being picked, and its picker.
    picking: Option<(&'static str, ColorPicker)>,
}

pub fn update(msg: AppearanceMsg, state: &mut AppearanceState) {
    let look = &mut state.look;
    match msg {
        AppearanceMsg::Adapt(on) => look.adapt_theme = on,
        AppearanceMsg::Strength(v) => {
            look.strength = v;
            return;
        }
        AppearanceMsg::StrengthDone | AppearanceMsg::FadeDone => {}
        AppearanceMsg::Variation(i) => {
            looks::pick_variation(look, i);
            state.picking = None;
        }
        AppearanceMsg::Custom(i) => {
            looks::pick_custom(look, i);
            state.picking = None;
        }
        AppearanceMsg::RemoveCustom => {
            looks::remove_custom(look);
            state.picking = None;
        }
        AppearanceMsg::Fade(on) => look.fade_idle = on,
        AppearanceMsg::FadeAfter(m) => {
            look.fade_after_min = m;
            return;
        }
        AppearanceMsg::FadeOver(m) => {
            look.fade_over_min = m;
            return;
        }
        AppearanceMsg::ClearPicture => {
            if let Some(picture) = state.picture.clone() {
                send(|b| b.retract(Topic::Background(picture)));
            }
            return;
        }
        AppearanceMsg::Pick(key) => {
            let open = state.picking.as_ref().is_some_and(|(k, _)| *k == key);
            state.picking = (!open).then(|| (key, ColorPicker::new(looks::current(state, key))));
            return;
        }
        AppearanceMsg::Picker(m) => {
            if let Some((_, picker)) = &mut state.picking {
                picker.update(m);
            }
            return;
        }
        AppearanceMsg::UsePicked => {
            let Some((key, picker)) = state.picking.take() else {
                return;
            };
            let c = picker.color();
            let hex = sola_kit::theme::color_to_hex(iced::Color { a: 1.0, ..c });
            looks::set_color(look, key, hex);
        }
        AppearanceMsg::ResetColor(key) => {
            looks::reset_color(look, key);
            state.picking = None;
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
    page::page(vec![
        picture(state),
        looks::colors(state),
        looks::yours(state),
        idle(state),
    ])
}

fn picture(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let line = match &state.picture {
        Some(p) => page::setting(
            file_name(&p.path),
            kit_btn::labeled_sm("Clear", kit_btn::secondary).on_press(AppearanceMsg::ClearPicture),
        ),
        None => page::empty("None — the black ground."),
    };
    page::section(
        "Background picture",
        None,
        page::group(vec![line]),
        Some("Set one from Paint: File → Set as Background.".into()),
    )
}

fn idle(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let look = &state.look;
    page::section(
        "Idle",
        None,
        page::group(vec![
            page::setting(
                "Fade the windows when idle",
                toggler(look.fade_idle)
                    .on_toggle(AppearanceMsg::Fade)
                    .style(toggle_style),
            ),
            page::setting(
                "Start after",
                slider_with(
                    slider(1..=60u16, look.fade_after_min, AppearanceMsg::FadeAfter)
                        .on_release(AppearanceMsg::FadeDone),
                    format!("{} min", look.fade_after_min),
                ),
            ),
            page::setting(
                "Fade over",
                slider_with(
                    slider(1..=120u16, look.fade_over_min, AppearanceMsg::FadeOver)
                        .on_release(AppearanceMsg::FadeDone),
                    format!("{} min", look.fade_over_min),
                ),
            ),
        ]),
        Some(
            "After the first time with no input, the windows fade into the \
             background over the second. Any input brings them back. Not while \
             a window is fullscreen or an app keeps the screen awake (a video)."
                .into(),
        ),
    )
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
            .width(Length::Fixed(56.0))
            .style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .align_y(Alignment::Center)
    .into()
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}
