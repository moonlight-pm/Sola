//! ColorPicker — the editor, not a code sample.

use iced::widget::column;
use iced::{Color, Element};

use sola_kit::components::color_picker;
use sola_kit::components::ColorPicker;

use crate::storybook::pages::chrome::{lede, panel};

#[derive(Clone, Debug)]
pub enum Msg {
    Picker(color_picker::Message),
}

pub struct State {
    picker: ColorPicker,
}

impl Default for State {
    fn default() -> Self {
        Self {
            picker: ColorPicker::new(Color::from_rgb(0.239, 0.839, 0.961)),
        }
    }
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Picker(m) => self.picker.update(m),
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    column![
        lede(
            "ColorPicker",
            "Drag the field and rails, or type Hex / RGB / HSL. Hue survives value → 0.",
        ),
        panel(state.picker.view().map(Msg::Picker)),
    ]
    .spacing(16)
    .into()
}
