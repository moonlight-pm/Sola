//! Export: Bitwarden JSON (plain, or encrypted with a password of its
//! own) or CSV. The file is named first in the kit's Save panel; then the
//! daemon writes the export and Vault saves it, readable by you alone.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length, Task};
use sola_kit::components::button as kit_btn;
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg, Outcome};
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{SPACE_LG, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_vault_proto::{ExportFormat, Request};
use zeroize::Zeroize;

use super::Sheet;
use crate::model::when;
use crate::ui::ask::Ask;
use crate::ui::fields::{Field, Text};
use crate::ui::signin::{head, labeled_input, lead_text, secret_input, sheet_sized};
use crate::ui::widgets::{self, PlateSize};
use crate::ui::{App, Msg, Phase, Popup};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    EncryptedJson,
    Json,
    Csv,
}

impl Format {
    fn label(self) -> &'static str {
        match self {
            Format::EncryptedJson => "JSON, encrypted",
            Format::Json => "JSON",
            Format::Csv => "CSV",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            _ => "json",
        }
    }
}

#[derive(Default)]
pub struct ExportSheet {
    pub format: Format,
    pub password: Text,
    pub confirm: Text,
    pub error: Option<String>,
    pub busy: bool,
    pub reveal: bool,
}

#[derive(Debug, Clone)]
pub enum ExportMsg {
    Format(Format),
    Reveal,
    /// Name the file, then export.
    Go,
}

impl App {
    pub(in crate::ui) fn open_export(&mut self) -> Task<Msg> {
        self.popup = None;
        if self.phase() != Phase::Unlocked {
            return Task::none();
        }
        self.sheet = Some(Sheet::Export(ExportSheet::default()));
        Field::ExportPassword.focus()
    }

    pub(in crate::ui) fn on_export(&mut self, m: ExportMsg) -> Task<Msg> {
        let Some(Sheet::Export(x)) = self.sheet.as_mut() else {
            return Task::none();
        };
        match m {
            ExportMsg::Format(f) => {
                x.format = f;
                x.error = None;
                self.popup = None;
                Task::none()
            }
            ExportMsg::Reveal => {
                x.reveal = !x.reveal;
                Task::none()
            }
            ExportMsg::Go => {
                if x.busy {
                    return Task::none();
                }
                if x.format == Format::EncryptedJson {
                    if x.password.is_empty() {
                        x.error = Some("Choose a password for the file.".into());
                        return Field::ExportPassword.focus();
                    }
                    if *x.password != *x.confirm {
                        x.error = Some("The two passwords differ.".into());
                        return Field::ExportConfirm.focus();
                    }
                }
                let name = format!(
                    "bitwarden_export_{}.{}",
                    when::today().format("%Y%m%d"),
                    x.format.extension()
                );
                self.picker = Some(
                    FilePicker::save()
                        .title("Export the vault")
                        .action("Export")
                        .start_dir(start_dir())
                        .suggested_name(name),
                );
                Task::none()
            }
        }
    }

    pub(in crate::ui) fn on_picker(&mut self, m: PickerMsg) -> Task<Msg> {
        let Some(picker) = self.picker.as_mut() else {
            return Task::none();
        };
        let outcome = picker.update(m);
        let follow = picker.scroll_task().map(Msg::Picker);
        match outcome {
            Some(Outcome::Cancelled) => {
                self.picker = None;
                Task::none()
            }
            Some(Outcome::Picked(path)) => {
                self.picker = None;
                let Some(Sheet::Export(x)) = self.sheet.as_mut() else {
                    return Task::none();
                };
                x.busy = true;
                x.error = None;
                let format = match x.format {
                    Format::EncryptedJson => ExportFormat::EncryptedJson {
                        password: x.password.to_string(),
                    },
                    Format::Json => ExportFormat::Json,
                    Format::Csv => ExportFormat::Csv,
                };
                self.ask(Ask::Export(path), Request::Export { format })
            }
            Some(Outcome::PickedMany(_)) => {
                self.picker = None;
                Task::none()
            }
            None => follow,
        }
    }

    pub(in crate::ui) fn on_exported(&mut self, path: PathBuf, mut data: String) -> Task<Msg> {
        let written = write_private(&path, data.as_bytes());
        data.zeroize();
        match written {
            Ok(()) => {
                tracing::info!(path = %path.display(), "vault exported");
                self.sheet = None;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.show_toast(format!("Exported to {name}"))
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not write the export");
                if let Some(Sheet::Export(x)) = self.sheet.as_mut() {
                    x.busy = false;
                    x.error = Some(format!("Couldn't write the file: {e}"));
                }
                Task::none()
            }
        }
    }

    pub(in crate::ui) fn view_export<'a>(&'a self, x: &'a ExportSheet) -> Element<'a, Msg> {
        let open = self.popup == Some(Popup::ExportFormat);
        let format = select(
            x.format.label(),
            [Format::EncryptedJson, Format::Json, Format::Csv]
                .into_iter()
                .map(|f| SelectOption::new(f.label(), x.format == f, Msg::Export(ExportMsg::Format(f)))),
            open,
            Msg::Popup(if open { None } else { Some(Popup::ExportFormat) }),
            Msg::Popup(None),
        );
        let mut body = column![row![
            kit_text::body("Format").width(Length::Fill),
            container(format).width(Length::Fixed(180.0)),
        ]
        .align_y(Alignment::Center)]
        .spacing(SPACE_LG);
        if x.format == Format::EncryptedJson {
            let error = x.error.as_deref();
            body = body
                .push(lead_text(
                    "The file is encrypted with a password of its own. Bitwarden asks for it \
                     when the file is imported."
                        .into(),
                ))
                .push(secret_input(
                    Field::ExportPassword,
                    "File password",
                    &x.password,
                    x.reveal,
                    error.is_some(),
                    Msg::Export(ExportMsg::Reveal),
                ))
                .push(labeled_input(
                    secret_input(
                        Field::ExportConfirm,
                        "The password again",
                        &x.confirm,
                        x.reveal,
                        error.is_some(),
                        Msg::Export(ExportMsg::Reveal),
                    ),
                    error,
                ));
        } else {
            body = body.push(lead_text(
                "The file holds every password in plain text. Keep it somewhere safe, and delete \
                 it once it has done its job."
                    .into(),
            ));
            if let Some(e) = &x.error {
                body = body.push(crate::ui::signin::error_caption(e));
            }
        }
        body = body.push(
            row![
                Space::new().width(Length::Fill),
                kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::CloseSheet),
                kit_btn::labeled(if x.busy { "Exporting…" } else { "Export…" }, kit_btn::primary)
                    .on_press_maybe((!x.busy).then_some(Msg::Export(ExportMsg::Go))),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        );
        sheet_sized(
            head(
                None,
                Some(widgets::glyph_plate("lucide/download", PlateSize::Small)),
                "Export the vault".into(),
                "Your own items, without attachments".into(),
            ),
            body.into(),
            400.0,
        )
    }
}

/// Downloads, or home.
fn start_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into());
    let downloads = home.join("Downloads");
    if downloads.is_dir() { downloads } else { home }
}

/// Write `bytes` to `path`, readable and writable by you alone.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // A file that was there keeps its mode unless told.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_is_yours_alone() {
        let dir = std::env::temp_dir().join(format!("sola-vault-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.json");
        write_private(&path, b"{}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
