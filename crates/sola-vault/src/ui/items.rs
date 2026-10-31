//! Choosing a place and an item, and what can be done to an item:
//! favorite, clone, move, trash, restore, delete forever; folders made,
//! renamed and deleted. Re-prompt items ask for the master password
//! before they open.

use iced::Task;
use sola_vault_proto::{ItemSummary, Request};

use super::ask::Ask;
use super::fields::{Field, Text};
use super::sheets::{Confirm, FolderSheet, Sheet};
use super::{App, Msg, Pane, Reprompt, Section};
use crate::model::section::{self, visible};
use crate::model::sort;

impl App {
    /// The open place's items, filtered and sorted as the list shows them.
    pub(super) fn shown_items(&self) -> Vec<&ItemSummary> {
        let mut items = visible(&self.items, &self.section, &self.query);
        sort::sort(&mut items, self.sort);
        items
    }

    pub(super) fn summary(&self, id: &str) -> Option<&ItemSummary> {
        self.items.iter().find(|i| i.id == id)
    }

    pub(super) fn selected_summary(&self) -> Option<&ItemSummary> {
        self.selected.as_deref().and_then(|id| self.summary(id))
    }

    pub(super) fn section_title(&self) -> String {
        match &self.section {
            Section::All => "All items".into(),
            Section::Favorites => "Favorites".into(),
            Section::Kind(k) => Section::kind_label(*k).into(),
            Section::Folder(id) => self
                .folders
                .iter()
                .find(|f| f.id == *id)
                .map_or_else(|| "Folder".into(), |f| f.name.clone()),
            Section::NoFolder => "No folder".into(),
            Section::Collection(id) => self
                .collections
                .iter()
                .find(|c| c.id == *id)
                .map_or_else(|| "Collection".into(), |c| c.name.clone()),
            Section::Trash => "Trash".into(),
        }
    }

    pub(super) fn show(&mut self, section: Section) -> Task<Msg> {
        self.section = section;
        self.popup = None;
        Task::none()
    }

    /// A row was chosen. An item being edited keeps its form until saved
    /// or discarded.
    pub(super) fn select(&mut self, id: String) -> Task<Msg> {
        if let Some(form) = self.edit.as_mut() {
            if super::edit::form_dirty(form) {
                form.confirm_discard = true;
                return Task::none();
            }
            self.edit = None;
        }
        let same = self.selected.as_deref() == Some(id.as_str());
        let asking = self.reprompt.as_ref().is_some_and(|r| r.id == id);
        if same && (asking || matches!(self.record, Pane::Open(_) | Pane::Loading)) {
            return Task::none();
        }
        self.popup = None;
        self.revealed.clear();
        self.history_open = false;
        self.selected = Some(id.clone());
        self.reprompt = None;
        let asks = self.summary(&id).is_some_and(|s| s.reprompt);
        if asks && !self.verified.contains(&id) {
            self.record = Pane::Empty;
            self.reprompt = Some(Reprompt {
                id,
                password: Text::default(),
                reveal: false,
                busy: false,
                error: None,
            });
            return Field::Reprompt.focus();
        }
        self.record = Pane::Loading;
        self.ask(Ask::Get(id.clone()), Request::Get { id })
    }

    /// Open `id` from outside (`--item`, a handoff): now, or once the list
    /// has it.
    pub(super) fn open_item(&mut self, id: String) -> Task<Msg> {
        if self.loaded && self.summary(&id).is_some() {
            self.pending_open = None;
            self.query.clear();
            let deleted = self.summary(&id).is_some_and(|s| s.deleted);
            if deleted {
                self.section = Section::Trash;
            } else if !self.section.holds(self.summary(&id).expect("checked")) {
                self.section = Section::All;
            }
            return self.select(id);
        }
        self.pending_open = Some(id);
        Task::none()
    }

    /// The list arrived: the selection still there, an item waiting to open.
    pub(super) fn after_list(&mut self) -> Task<Msg> {
        if let Some(id) = self.pending_open.clone() {
            if self.summary(&id).is_some() {
                return self.open_item(id);
            }
            self.pending_open = None;
            return self.show_toast("That item isn't in the vault.");
        }
        if let Some(id) = &self.selected
            && self.summary(id).is_none()
            && self.edit.is_none()
        {
            self.selected = None;
            self.record = Pane::Empty;
            self.reprompt = None;
        }
        Task::none()
    }

    /// Up or down the list.
    pub(super) fn step(&mut self, down: bool) -> Task<Msg> {
        let next = {
            let items = self.shown_items();
            let at = self
                .selected
                .as_deref()
                .and_then(|id| items.iter().position(|i| i.id == id));
            let i = match (at, down) {
                (None, true) => Some(0),
                (None, false) => items.len().checked_sub(1),
                (Some(i), true) => (i + 1 < items.len()).then_some(i + 1),
                (Some(i), false) => i.checked_sub(1),
            };
            i.and_then(|i| items.get(i)).map(|s| s.id.clone())
        };
        match next {
            Some(id) => self.select(id),
            None => Task::none(),
        }
    }

    // ── Re-prompt ────────────────────────────────────────────────────

    pub(super) fn submit_reprompt(&mut self) -> Task<Msg> {
        let Some(r) = self.reprompt.as_mut() else {
            return Task::none();
        };
        if r.busy {
            return Task::none();
        }
        if r.password.is_empty() {
            r.error = Some("Enter your master password.".into());
            return Task::none();
        }
        r.busy = true;
        r.error = None;
        let id = r.id.clone();
        let password = r.password.to_string();
        self.ask(Ask::Verify(id), Request::VerifyMasterPassword { password })
    }

    pub(super) fn on_verified(&mut self, id: String, ok: bool) -> Task<Msg> {
        let Some(r) = self.reprompt.as_mut().filter(|r| r.id == id) else {
            return Task::none();
        };
        r.busy = false;
        r.password = Text::default();
        if !ok {
            r.error = Some("That isn't your master password.".into());
            return Field::Reprompt.focus();
        }
        self.reprompt = None;
        self.verified.insert(id.clone());
        if self.selected.as_deref() == Some(id.as_str()) {
            self.record = Pane::Loading;
            return self.ask(Ask::Get(id.clone()), Request::Get { id });
        }
        Task::none()
    }

    // ── What an item can have done to it ─────────────────────────────

    /// The open record's id, when it is readable (verified if it asks).
    fn open_id(&self) -> Option<String> {
        match &self.record {
            Pane::Open(r) => Some(r.id.clone()),
            _ => None,
        }
    }

    pub(super) fn toggle_favorite(&mut self) -> Task<Msg> {
        let Some(s) = self.selected_summary() else {
            return Task::none();
        };
        if s.deleted {
            return Task::none();
        }
        let (id, favorite) = (s.id.clone(), !s.favorite);
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.favorite = favorite;
        }
        if let Pane::Open(r) = &mut self.record
            && r.id == id
        {
            r.favorite = favorite;
        }
        self.ask(Ask::Favorite(id.clone()), Request::SetFavorite { id, favorite })
    }

    pub(super) fn trash(&mut self) -> Task<Msg> {
        self.popup = None;
        let Some(s) = self.selected_summary().filter(|s| !s.deleted) else {
            return Task::none();
        };
        let id = s.id.clone();
        self.ask(Ask::Trash(id.clone()), Request::Trash { id })
    }

    pub(super) fn restore(&mut self) -> Task<Msg> {
        self.popup = None;
        let Some(s) = self.selected_summary().filter(|s| s.deleted) else {
            return Task::none();
        };
        let id = s.id.clone();
        self.ask(Ask::Restore(id.clone()), Request::Restore { id })
    }

    pub(super) fn ask_delete_forever(&mut self) -> Task<Msg> {
        self.popup = None;
        if let Some(s) = self.selected_summary() {
            self.sheet = Some(Sheet::Confirm(Confirm::DeleteForever {
                id: s.id.clone(),
                name: s.name.clone(),
            }));
        }
        Task::none()
    }

    pub(super) fn clone_item(&mut self) -> Task<Msg> {
        self.popup = None;
        let Pane::Open(record) = &self.record else {
            return Task::none();
        };
        if record.deleted.is_some() {
            return Task::none();
        }
        let draft = crate::model::edit::clone_of(record);
        self.start_form(draft)
    }

    pub(super) fn ask_move(&mut self) -> Task<Msg> {
        self.popup = None;
        if self.selected_summary().is_some_and(|s| !s.deleted) {
            self.sheet = Some(Sheet::MoveTo);
        }
        Task::none()
    }

    pub(super) fn move_to(&mut self, folder: Option<String>) -> Task<Msg> {
        self.sheet = None;
        let Some(s) = self.selected_summary() else {
            return Task::none();
        };
        if s.folder_id == folder {
            return Task::none();
        }
        let id = s.id.clone();
        self.ask(Ask::Move(id.clone()), Request::MoveToFolder { id, folder_id: folder })
    }

    // ── Folders ──────────────────────────────────────────────────────

    pub(super) fn new_folder(&mut self) -> Task<Msg> {
        self.popup = None;
        self.sheet = Some(Sheet::Folder(FolderSheet::default()));
        Field::FolderName.focus()
    }

    pub(super) fn rename_folder(&mut self, id: String) -> Task<Msg> {
        self.popup = None;
        let name = self
            .folders
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        self.sheet = Some(Sheet::Folder(FolderSheet {
            id: Some(id),
            name,
            ..FolderSheet::default()
        }));
        Task::batch([
            Field::FolderName.focus(),
            iced::widget::operation::select_all(Field::FolderName.id()),
        ])
    }

    pub(super) fn ask_delete_folder(&mut self, id: String) -> Task<Msg> {
        self.popup = None;
        if let Some(f) = self.folders.iter().find(|f| f.id == id) {
            self.sheet = Some(Sheet::Confirm(Confirm::DeleteFolder {
                id,
                name: f.name.clone(),
            }));
        }
        Task::none()
    }

    pub(super) fn save_folder(&mut self) -> Task<Msg> {
        let Some(Sheet::Folder(f)) = self.sheet.as_mut() else {
            return Task::none();
        };
        if f.busy {
            return Task::none();
        }
        let name = f.name.trim().to_string();
        if name.is_empty() {
            f.error = Some("Give the folder a name.".into());
            return Task::none();
        }
        f.busy = true;
        match f.id.clone() {
            Some(id) => self.ask(Ask::RenameFolder, Request::RenameFolder { id, name }),
            None => self.ask(Ask::CreateFolder, Request::CreateFolder { name }),
        }
    }

    /// The question's danger button.
    pub(super) fn confirm(&mut self) -> Task<Msg> {
        let Some(Sheet::Confirm(c)) = self.sheet.take() else {
            return Task::none();
        };
        match c {
            Confirm::DeleteForever { id, .. } => {
                self.ask(Ask::Delete(id.clone()), Request::Delete { id })
            }
            Confirm::DeleteFolder { id, .. } => {
                self.ask(Ask::DeleteFolder(id.clone()), Request::DeleteFolder { id })
            }
            Confirm::SignOut => {
                self.signin = super::signin::SignIn::default();
                self.ask(Ask::Logout, Request::Logout)
            }
        }
    }

    pub(super) fn ask_sign_out(&mut self) -> Task<Msg> {
        self.popup = None;
        if self.status.as_ref().is_some_and(|s| s.signed_in) {
            self.sheet = Some(Sheet::Confirm(Confirm::SignOut));
        }
        Task::none()
    }

    pub(super) fn lock(&mut self) -> Task<Msg> {
        if self.status.as_ref().is_some_and(|s| s.unlocked) {
            return self.ask(Ask::Lock, Request::Lock);
        }
        Task::none()
    }

    /// Item → Copy Username / Password / Code, on the open record.
    pub(super) fn copy_from_menu(&mut self, what: super::copy::CopyWhat) -> Task<Msg> {
        if self.open_id().is_none() {
            return Task::none();
        }
        self.copy(what)
    }

    /// The kinds the sidebar lists for this vault.
    pub(super) fn listed_kinds(&self) -> Vec<sola_vault_proto::ItemKind> {
        let mut kinds = section::KINDS.to_vec();
        kinds.extend(section::present_more_kinds(&self.items));
        kinds
    }
}

/// A website as the browser opens it: a bare host gets `https://`.
pub fn launchable(uri: &str) -> String {
    let t = uri.trim();
    if t.contains("://") {
        t.to_string()
    } else {
        format!("https://{t}")
    }
}

#[cfg(test)]
mod tests {
    use super::launchable;

    #[test]
    fn bare_hosts_open_over_https() {
        assert_eq!(launchable("github.com"), "https://github.com");
        assert_eq!(launchable(" http://x.test/a "), "http://x.test/a");
    }
}
