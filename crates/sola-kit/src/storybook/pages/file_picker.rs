//! FilePicker showcase — the kit's Open (several files), Save and Choose
//! Folder panels, live. In an app the panel also takes keys
//! (`FilePicker::keys`): arrows, Return, Escape, ⌘↑ / ⌘↓, ⌘⇧., typing a
//! name.

use iced::Element;
use iced::widget::{column, text};

use sola_kit::components::file_picker::{self, FilePicker, Outcome};
use sola_kit::components::text::muted;

use super::chrome;

#[derive(Clone, Debug)]
pub enum Msg {
    Open(file_picker::Message),
    Save(file_picker::Message),
    Folder(file_picker::Message),
}

pub struct State {
    open: FilePicker,
    save: FilePicker,
    folder: FilePicker,
    last: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: FilePicker::open()
                .multiple()
                .title("Open images")
                .filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"]),
            save: FilePicker::save()
                .title("Save image")
                .filter("Images", &["png", "jpg", "jpeg", "webp"])
                .suggested_name("untitled.png"),
            folder: FilePicker::folder().title("Choose a project folder"),
            last: None,
        }
    }
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        let outcome = match msg {
            Msg::Open(m) => self.open.update(m),
            Msg::Save(m) => self.save.update(m),
            Msg::Folder(m) => self.folder.update(m),
        };
        match outcome {
            Some(Outcome::Picked(path)) => {
                self.last = Some(format!("Picked {}", path.display()));
            }
            Some(Outcome::PickedMany(paths)) => {
                self.last = Some(format!("Picked {} files", paths.len()));
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
            "One panel for every app. Path is a trail of chips, not a typed string. \
             Places on the left, files in a quiet well, name + confirm along the bottom. \
             Shift / ⌘ choose several where an app allows it.",
        ),
        chrome::panel(state.open.view().map(Msg::Open)),
        chrome::scene("Save — asks before replacing; New Folder"),
        chrome::panel(state.save.view().map(Msg::Save)),
        chrome::scene("Choose Folder"),
        chrome::panel(state.folder.view().map(Msg::Folder)),
        text(last).size(12).style(muted),
    ]
    .spacing(20)
    .into()
}
