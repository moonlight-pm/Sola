//! ColorPicker showcase — the full spectrum picker, stateful so the
//! saturation/value field and the hue/alpha rails actually drag.

use iced::widget::column;
use iced::{Color, Element};

use sola_kit::components::card;
use sola_kit::components::color_picker;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::ColorPicker;

#[derive(Clone, Debug)]
pub enum Msg {
    Picker(color_picker::Message),
}

pub struct State {
    picker: ColorPicker,
}

impl Default for State {
    fn default() -> Self {
        Self { picker: ColorPicker::new(Color::from_rgb(0.35, 0.62, 0.95)) }
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
        heading("ColorPicker"),
        body(
            "A real picker control: drag the saturation/value field and the \
             hue / alpha rails, or type into Hex / RGB / HSL. HSV is the \
             canonical model, so hue and saturation survive value → 0."
        )
        .style(muted),
        card(state.picker.view().map(Msg::Picker)),
        code("ColorPicker::new(color) · view().map(Msg::Picker) · update(m)").style(muted),
    ]
    .spacing(16)
    .into()
}
