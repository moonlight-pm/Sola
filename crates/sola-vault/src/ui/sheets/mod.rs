//! Sheets over the window: the generator, settings, export, a folder's
//! name, moving an item, and the questions asked before something can't be
//! undone. One at a time, on a dim; Escape or Cancel closes it.

mod export;
mod generator;
mod settings;
mod share;

use iced::widget::{Space, button, column, container, mouse_area, opaque, row, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_SM};
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;
use zeroize::Zeroizing;

pub use export::{ExportMsg, ExportSheet};
pub use generator::{GenMsg, GenSheet, GenTarget};
pub use settings::SetMsg;
pub use share::{ShareMsg, ShareSheet};

use super::fields::Field;
use super::signin::{field_style, head, labeled_input, lead_text, sheet};
use super::widgets::{self, PlateSize};
use super::{App, Msg, style};

pub enum Sheet {
    Generator(GenSheet),
    Settings,
    Export(ExportSheet),
    Folder(FolderSheet),
    MoveTo,
    Share(ShareSheet),
    Confirm(Confirm),
}

#[derive(Default)]
pub struct FolderSheet {
    /// Renaming this folder; `None` makes a new one.
    pub id: Option<String>,
    pub name: String,
    pub error: Option<String>,
    pub busy: bool,
}

/// A question before something can't be undone.
pub enum Confirm {
    DeleteForever { id: String, name: String },
    DeleteFolder { id: String, name: String },
    SignOut,
}

impl App {
    pub(super) fn view_sheet<'a>(&'a self, sheet: &'a Sheet) -> Element<'a, Msg> {
        let card = match sheet {
            Sheet::Generator(g) => self.view_generator(g),
            Sheet::Settings => self.view_settings(),
            Sheet::Export(x) => self.view_export(x),
            Sheet::Folder(f) => view_folder(f),
            Sheet::MoveTo => self.view_move(),
            Sheet::Share(x) => self.view_share(x),
            Sheet::Confirm(c) => self.view_confirm(c),
        };
        // A click on the dim closes the sheet; one on the card does not.
        let dim = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::dim),
        )
        .on_press(Msg::CloseSheet);
        // Opaque: nothing under the dim takes a click or a hover.
        opaque(iced::widget::stack![
            dim,
            container(opaque(card))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        ])
    }

    fn view_move(&self) -> Element<'_, Msg> {
        let current = self.selected_summary().and_then(|s| s.folder_id.clone());
        let name = self.selected_summary().map(|s| s.name.clone()).unwrap_or_default();
        let row_for = |label: String, folder: Option<String>| -> Element<'_, Msg> {
            let on = current == folder;
            button(
                row![
                    widgets::muted("lucide/folder", 14.0),
                    text(label).font(fonts::ui()).size(13).width(Length::Fill),
                    if on {
                        widgets::muted("lucide/check", 14.0)
                    } else {
                        Space::new().width(14).into()
                    },
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([7.0, 10.0]))
            .width(Length::Fill)
            .style(style::list_row(on))
            .on_press(Msg::MoveTo(folder))
            .into()
        };
        let mut rows = column![row_for("No folder".into(), None)].spacing(2);
        for f in &self.folders {
            rows = rows.push(row_for(f.name.clone(), Some(f.id.clone())));
        }
        let body = column![
            rows,
            row![
                Space::new().width(Length::Fill),
                kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::CloseSheet),
            ],
        ]
        .spacing(SPACE_LG);
        sheet(
            head(
                None,
                Some(widgets::glyph_plate("lucide/folder", PlateSize::Small)),
                "Move to folder".into(),
                name,
            ),
            body.into(),
        )
    }

    fn view_confirm(&self, c: &Confirm) -> Element<'_, Msg> {
        let (title, line, action): (String, String, &str) = match c {
            Confirm::DeleteForever { name, .. } => (
                format!("Delete “{name}” forever?"),
                "It leaves the trash and can't come back.".into(),
                "Delete Forever",
            ),
            Confirm::DeleteFolder { name, .. } => (
                format!("Delete the folder “{name}”?"),
                "Its items stay in the vault, in no folder.".into(),
                "Delete Folder",
            ),
            Confirm::SignOut => {
                let email = self
                    .status
                    .as_ref()
                    .map(|s| s.email.clone())
                    .unwrap_or_default();
                (
                    "Sign out?".into(),
                    format!(
                        "Vault forgets {email} on this computer. Signing in again asks for \
                         the master password and the second step."
                    ),
                    "Sign Out",
                )
            }
        };
        let body = column![
            lead_text(line),
            row![
                Space::new().width(Length::Fill),
                kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::CloseSheet),
                kit_btn::labeled(action, kit_btn::danger).on_press(Msg::Confirm),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        ]
        .spacing(SPACE_LG);
        sheet(
            head(
                None,
                Some(widgets::glyph_plate("lucide/triangle-alert", PlateSize::Small)),
                title,
                String::new(),
            ),
            body.into(),
        )
    }
}

fn view_folder(f: &FolderSheet) -> Element<'_, Msg> {
    let title = if f.id.is_some() { "Rename folder" } else { "New folder" };
    let field = text_input("Name", &f.name).size(13)
        .id(Field::FolderName.id())
        .on_input(|v| Msg::Input(Field::FolderName, Zeroizing::new(v)))
        .on_submit(Msg::Submit(Field::FolderName))
        .style(field_style(f.error.is_some()));
    let action = match (f.id.is_some(), f.busy) {
        (_, true) => "Saving…",
        (true, false) => "Rename",
        (false, false) => "Create",
    };
    let body = column![
        labeled_input(field, f.error.as_deref()),
        row![
            Space::new().width(Length::Fill),
            kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::CloseSheet),
            kit_btn::labeled(action, kit_btn::primary)
                .on_press_maybe((!f.busy).then_some(Msg::SaveFolder)),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center),
    ]
    .spacing(SPACE_LG);
    sheet(
        head(
            None,
            Some(widgets::glyph_plate("lucide/folder", PlateSize::Small)),
            title.into(),
            String::new(),
        ),
        body.into(),
    )
}
