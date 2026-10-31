//! The Edit menu on the sheet's fields: the shell takes ⌘C / ⌘V / ⌘X / ⌘A
//! at the compositor, so the app applies them to the focused field
//! itself and stores the new value as `on_input` would have.

use iced::Task;
use iced::widget::text_editor;
use sola_kit::components::text_input::{MenuEdit, MenuEdited};

use super::update::VaultMsg;
use super::view::id;
use crate::app::{App, Msg};
use crate::engine::{EditCmd, Engine};

impl<E: Engine> App<E> {
    /// An Edit action while the sheet is up: to its focused field.
    pub(crate) fn vault_edit_action(&mut self, cmd: EditCmd) -> Task<Msg> {
        tracing::debug!(?cmd, "edit → vault panel");
        match cmd {
            EditCmd::Paste => {
                iced::clipboard::read().map(|t| Msg::Vault(VaultMsg::ClipboardPaste(t)))
            }
            EditCmd::Copy => self.vault_menu_edit(MenuEdit::Copy),
            EditCmd::Cut => self.vault_menu_edit(MenuEdit::Cut),
            EditCmd::SelectAll => self.vault_menu_edit(MenuEdit::SelectAll),
            EditCmd::Undo | EditCmd::Redo => Task::none(),
        }
    }

    pub(super) fn vault_menu_edit(&self, edit: MenuEdit) -> Task<Msg> {
        sola_kit::components::text_input::edit_focused(edit.clone())
            .map(move |done| Msg::Vault(VaultMsg::MenuEdited(edit.clone(), done)))
    }

    pub(super) fn vault_menu_edited(&mut self, edit: MenuEdit, done: Option<MenuEdited>) -> Task<Msg> {
        let Some(done) = done else {
            // No line field has focus: the notes editor, if it does.
            if self.vault.draft.is_none() {
                return Task::none();
            }
            return iced::widget::operation::is_focused(id("notes")).map(move |focused| {
                if focused {
                    Msg::Vault(VaultMsg::NotesMenuEdit(edit.clone()))
                } else {
                    Msg::Noop
                }
            });
        };
        if let (Some(field), Some(value)) = (done.id.as_ref(), done.value) {
            self.vault_set_field(field, value);
        }
        match done.copied.filter(|t| !t.is_empty()) {
            Some(t) => iced::clipboard::write(t),
            None => Task::none(),
        }
    }

    /// Store an Edit-menu result in the field whose input is `field`.
    fn vault_set_field(&mut self, field: &iced::widget::Id, value: String) {
        let is = |name: &str| *field == id(name);
        let v = &mut self.vault;
        let slot = if is("email") {
            &mut v.forms.email
        } else if is("password") {
            &mut v.forms.password
        } else if is("server-url") {
            &mut v.forms.server_url
        } else if is("otp") {
            &mut v.forms.otp
        } else if is("unlock") {
            &mut v.forms.unlock
        } else if is("reprompt") {
            &mut v.reprompt_password
        } else if is("search") {
            v.list.chosen = 0;
            &mut v.list.query
        } else if let Some(form) = v.create.as_mut().filter(|_| {
            is("new-username") || is("new-password") || is("new-website")
        }) {
            if is("new-username") {
                &mut form.username
            } else if is("new-password") {
                &mut form.password
            } else {
                &mut form.website
            }
        } else if let Some(draft) = v.draft.as_mut() {
            if is("edit-name") {
                &mut draft.name
            } else if let Some(row) = draft.rows.iter_mut().find(|r| is(&format!("edit:{}", r.key))) {
                &mut row.value
            } else if let Some(uri) = draft
                .uris
                .iter_mut()
                .enumerate()
                .find(|(i, _)| is(&format!("uri:{i}")))
                .map(|(_, u)| u)
            {
                &mut uri.value
            } else if let Some(c) = draft
                .custom
                .iter_mut()
                .enumerate()
                .find(|(i, _)| is(&format!("custom-name:{i}")))
                .map(|(_, c)| c)
            {
                &mut c.name
            } else if let Some(c) = draft
                .custom
                .iter_mut()
                .enumerate()
                .find(|(i, _)| is(&format!("custom:{i}")))
                .map(|(_, c)| c)
            {
                &mut c.value
            } else {
                return;
            }
        } else {
            return;
        };
        *slot = value;
    }

    /// The Edit menu on the notes editor (paste keeps line breaks).
    pub(super) fn vault_notes_menu_edit(&mut self, edit: MenuEdit) -> Task<Msg> {
        let selection = self.vault.notes.selection().filter(|s| !s.is_empty());
        let (action, copied) = match edit {
            MenuEdit::Paste(t) => (
                Some(text_editor::Action::Edit(text_editor::Edit::Paste(
                    std::sync::Arc::new(t),
                ))),
                None,
            ),
            MenuEdit::SelectAll => (Some(text_editor::Action::SelectAll), None),
            MenuEdit::Copy => (None, selection),
            MenuEdit::Cut => (
                selection
                    .is_some()
                    .then_some(text_editor::Action::Edit(text_editor::Edit::Delete)),
                selection,
            ),
        };
        if let Some(action) = action.filter(|_| !self.vault.busy) {
            self.vault_edit_notes(action);
        }
        copied.map(iced::clipboard::write).unwrap_or_else(Task::none)
    }

    pub(super) fn vault_edit_notes(&mut self, action: text_editor::Action) {
        let v = &mut self.vault;
        let edits = action.is_edit();
        v.notes.perform(action);
        if let Some(d) = v.draft.as_mut().filter(|_| edits) {
            d.notes = v.notes.text();
        }
    }
}
