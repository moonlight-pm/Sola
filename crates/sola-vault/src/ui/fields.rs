//! Every text field in Vault by name, so a typed value, a pasted one and an
//! Edit menu command (which arrives with the field's widget id) all land
//! in the same place.

use iced::Task;
use iced::widget::Id;
use sola_kit::components::text_input::{MenuEdit, MenuEdited, edit_focused};
use zeroize::Zeroizing;

use super::{App, Msg};
use crate::model::edit as draft_edit;
use crate::model::otp;

/// A field's text. Zeroized when dropped: many are passwords.
pub type Text = Zeroizing<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Search,
    Email,
    Password,
    ServerUrl,
    Unlock,
    Otp,
    EmailCode,
    Reprompt,
    Name,
    Row(String),
    Uri(usize),
    CustomName(usize),
    CustomValue(usize),
    FolderName,
    ExportPassword,
    ExportConfirm,
}

impl Field {
    pub fn id(&self) -> Id {
        Id::from(match self {
            Field::Search => "vault:search".to_string(),
            Field::Email => "vault:email".into(),
            Field::Password => "vault:password".into(),
            Field::ServerUrl => "vault:server".into(),
            Field::Unlock => "vault:unlock".into(),
            Field::Otp => "vault:otp".into(),
            Field::EmailCode => "vault:email-code".into(),
            Field::Reprompt => "vault:reprompt".into(),
            Field::Name => "vault:name".into(),
            Field::Row(key) => format!("vault:row:{key}"),
            Field::Uri(i) => format!("vault:uri:{i}"),
            Field::CustomName(i) => format!("vault:custom-name:{i}"),
            Field::CustomValue(i) => format!("vault:custom-value:{i}"),
            Field::FolderName => "vault:folder-name".into(),
            Field::ExportPassword => "vault:export-password".into(),
            Field::ExportConfirm => "vault:export-confirm".into(),
        })
    }

    pub fn focus<T>(&self) -> Task<T> {
        iced::widget::operation::focus(self.id())
    }
}

impl App {
    /// The fields on screen now (an Edit menu command finds its own).
    fn fields_shown(&self) -> Vec<Field> {
        let mut f = vec![
            Field::Search,
            Field::Email,
            Field::Password,
            Field::ServerUrl,
            Field::Unlock,
            Field::EmailCode,
            Field::Reprompt,
            Field::Name,
            Field::FolderName,
            Field::ExportPassword,
            Field::ExportConfirm,
            Field::Otp,
        ];
        if let Some(e) = &self.edit {
            f.extend(e.draft.rows.iter().map(|r| Field::Row(r.key.clone())));
            f.extend((0..e.draft.uris.len()).map(Field::Uri));
            f.extend((0..e.draft.custom.len()).map(Field::CustomName));
            f.extend((0..e.draft.custom.len()).map(Field::CustomValue));
        }
        f
    }

    /// A field's value changed (typed, pasted, cut).
    pub(super) fn set_field(&mut self, field: Field, value: Text) -> Task<Msg> {
        match field {
            Field::Search => self.query = value.to_string(),
            Field::Email => self.signin.email = value.to_string(),
            Field::Password => self.signin.password = value,
            Field::ServerUrl => self.signin.url = value.to_string(),
            Field::Unlock => {
                self.unlock.password = value;
                self.unlock.error = None;
            }
            Field::Otp => {
                if let Some(step) = self.signin.two_step.as_mut() {
                    step.code = Zeroizing::new(otp::accept(&value));
                    step.error = None;
                }
            }
            Field::EmailCode => {
                if let Some(step) = self.signin.two_step.as_mut() {
                    step.code = value;
                    step.error = None;
                }
            }
            Field::Reprompt => {
                if let Some(r) = self.reprompt.as_mut() {
                    r.password = value;
                    r.error = None;
                }
            }
            Field::Name => {
                if let Some(e) = self.edit.as_mut() {
                    e.draft.name = value.to_string();
                    e.name_error = None;
                }
            }
            Field::Row(key) => {
                if let Some(e) = self.edit.as_mut() {
                    e.draft.set_row(&key, value.to_string());
                }
            }
            Field::Uri(i) => {
                if let Some(e) = self.edit.as_mut() {
                    draft_edit::set_uri(&mut e.draft, i, value.to_string());
                }
            }
            Field::CustomName(i) => {
                if let Some(e) = self.edit.as_mut() {
                    draft_edit::set_custom_name(&mut e.draft, i, value.to_string());
                }
            }
            Field::CustomValue(i) => {
                if let Some(e) = self.edit.as_mut() {
                    draft_edit::set_custom_value(&mut e.draft, i, value.to_string());
                }
            }
            Field::FolderName => {
                if let Some(super::Sheet::Folder(f)) = self.sheet.as_mut() {
                    f.name = value.to_string();
                    f.error = None;
                }
            }
            Field::ExportPassword => {
                if let Some(super::Sheet::Export(x)) = self.sheet.as_mut() {
                    x.password = value;
                    x.error = None;
                }
            }
            Field::ExportConfirm => {
                if let Some(super::Sheet::Export(x)) = self.sheet.as_mut() {
                    x.confirm = value;
                    x.error = None;
                }
            }
        }
        Task::none()
    }

    // ── The Edit menu ────────────────────────────────────────────────

    /// Run `edit` on the focused field; [`Msg::MenuEdited`] comes back.
    pub(super) fn menu_edit(&self, edit: MenuEdit) -> Task<Msg> {
        edit_focused(edit.clone()).map(move |done| Msg::MenuEdited(edit.clone(), done))
    }

    pub(super) fn menu_paste(&self) -> Task<Msg> {
        iced::clipboard::read().map(|t| Msg::Pasted(t.map(Zeroizing::new)))
    }

    pub(super) fn on_menu_edited(&mut self, edit: MenuEdit, done: Option<MenuEdited>) -> Task<Msg> {
        let Some(done) = done else {
            // No field has focus: a paste goes to the notes being edited.
            if let (MenuEdit::Paste(t), Some(e)) = (edit, self.edit.as_mut()) {
                e.notes.perform(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Paste(t.into()),
                ));
            }
            return Task::none();
        };
        let field = done
            .id
            .as_ref()
            .and_then(|id| self.fields_shown().into_iter().find(|f| f.id() == *id));
        let stored = match (field, done.value) {
            (Some(field), Some(value)) => self.set_field(field, Zeroizing::new(value)),
            _ => Task::none(),
        };
        match done.copied {
            Some(t) if !t.is_empty() => Task::batch([stored, iced::clipboard::write(t)]),
            _ => stored,
        }
    }
}
