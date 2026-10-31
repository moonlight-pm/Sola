//! Export: Bitwarden JSON (plain, or encrypted with a password of its
//! own) or CSV. Your master password first, as Bitwarden's own clients
//! ask it; then the file is named in the kit's Save panel; then the daemon
//! writes the export and Vault saves it, readable by you alone.

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
    /// Your master password: checked before anything is written.
    pub master: Text,
    pub master_error: Option<String>,
    pub busy: bool,
    pub reveal: bool,
}

impl ExportSheet {
    /// What stops an export, and the field it goes under: the file's
    /// password (encrypted JSON), then the master password.
    fn problem(&self) -> Option<(Field, &'static str)> {
        if self.format == Format::EncryptedJson {
            if self.password.is_empty() {
                return Some((Field::ExportPassword, "Choose a password for the file."));
            }
            if *self.password != *self.confirm {
                return Some((Field::ExportConfirm, "The two passwords differ."));
            }
        }
        if self.master.is_empty() {
            return Some((Field::ExportMaster, "Enter your master password to export."));
        }
        None
    }
}

#[derive(Debug, Clone)]
pub enum ExportMsg {
    Format(Format),
    Reveal,
    /// Check the master password, name the file, then export.
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
                if let Some((field, message)) = x.problem() {
                    if field == Field::ExportMaster {
                        x.master_error = Some(message.into());
                    } else {
                        x.error = Some(message.into());
                    }
                    return field.focus();
                }
                x.busy = true;
                x.error = None;
                x.master_error = None;
                let password = x.master.to_string();
                self.ask(Ask::ExportVerify, Request::VerifyMasterPassword { password })
            }
        }
    }

    /// The master password checked: on, to name the file; or say so.
    pub(in crate::ui) fn on_export_verified(&mut self, ok: bool) -> Task<Msg> {
        let Some(Sheet::Export(x)) = self.sheet.as_mut() else {
            return Task::none();
        };
        x.busy = false;
        x.master = Text::default();
        if !ok {
            x.master_error = Some("That isn't your master password.".into());
            return Field::ExportMaster.focus();
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
        body = body.push(lead_text("Enter your master password to export.".into())).push(labeled_input(
            secret_input(
                Field::ExportMaster,
                "Master password",
                &x.master,
                x.reveal,
                x.master_error.is_some(),
                Msg::Export(ExportMsg::Reveal),
            ),
            x.master_error.as_deref(),
        ));
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
    fn the_master_password_comes_before_any_file() {
        let text = |s: &str| Text::new(s.to_string());
        let mut x = ExportSheet::default();
        assert_eq!(x.problem().map(|p| p.0), Some(Field::ExportPassword));
        x.password = text("file");
        x.confirm = text("other");
        assert_eq!(x.problem().map(|p| p.0), Some(Field::ExportConfirm));
        x.confirm = text("file");
        assert_eq!(x.problem().map(|p| p.0), Some(Field::ExportMaster));
        x.master = text("master");
        assert_eq!(x.problem(), None);
        // A plain export needs no file password, but still the master one.
        let mut plain = ExportSheet {
            format: Format::Csv,
            ..ExportSheet::default()
        };
        assert_eq!(plain.problem().map(|p| p.0), Some(Field::ExportMaster));
        plain.master = text("master");
        assert_eq!(plain.problem(), None);
    }

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
