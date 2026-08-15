//! FilePicker showcase — live Open/Save panel on a desk.

use iced::widget::{column, text};
use iced::Element;

use sola_kit::components::file_picker::{self, FilePicker, Outcome};
use sola_kit::components::text::muted;

use super::chrome;

#[derive(Clone, Debug)]
pub enum Msg {
    Open(file_picker::Message),
    Save(file_picker::Message),
}

pub struct State {
    open: FilePicker,
    save: FilePicker,
    last: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: FilePicker::open()
                .title("Open image")
                .filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"]),
            save: FilePicker::save()
                .title("Save image")
                .filter("Images", &["png", "jpg", "jpeg", "webp"])
                .suggested_name("untitled.png"),
            last: None,
        }
    }
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        let outcome = match msg {
            Msg::Open(m) => self.open.update(m),
            Msg::Save(m) => self.save.update(m),
        };
        match outcome {
            Some(Outcome::Picked(path)) => {
                self.last = Some(format!("Picked {}", path.display()));
            }
            Some(Outcome::Cancelled) => {
                self.last = Some("Cancelled".into());
            }
            None => {}
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let last = state
        .last
        .as_deref()
        .unwrap_or("Pick a file — nothing chosen yet.");

    column![
        chrome::lede(
            "File picker",
            "Path is a trail of chips, not a typed string. Places on the left, \
             files in a quiet well, name + confirm along the bottom.",
        ),
        chrome::panel(state.open.view().map(Msg::Open)),
        chrome::scene("Save"),
        chrome::panel(state.save.view().map(Msg::Save)),
        text(last).size(12).style(muted),
    ]
    .spacing(20)
    .into()
}
