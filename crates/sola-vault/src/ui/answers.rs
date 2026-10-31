//! The daemon's answers, each to where it was asked from. A locked or
//! signed-out answer means the status moved under us: ask for it again.

use iced::Task;
use sola_vault_proto::{ErrorCode, Reply, Request, VaultError};

use super::ask::Ask;
use super::{App, Msg, Pane, Section, sheets};
use crate::daemon::Answer;

impl App {
    pub(super) fn on_answer(&mut self, ask: Ask, answer: Answer) -> Task<Msg> {
        let refresh = match &answer {
            Err(e) if matches!(e.code, ErrorCode::Locked | ErrorCode::SignedOut) => {
                !matches!(ask, Ask::Status)
            }
            _ => false,
        };
        let task = match answer {
            Ok(reply) => self.on_reply(ask, reply),
            Err(e) => self.on_error(ask, e),
        };
        if refresh {
            return Task::batch([task, self.ask(Ask::Status, Request::Status)]);
        }
        task
    }

    fn on_reply(&mut self, ask: Ask, reply: Reply) -> Task<Msg> {
        match (ask, reply) {
            (_, Reply::Status(s)) => self.apply_status(s),
            (_, Reply::Settings(s)) => {
                self.settings = s;
                Task::none()
            }
            (_, Reply::Items { items, unreadable }) => {
                self.items = items;
                self.unreadable = unreadable;
                self.loaded = true;
                self.after_list()
            }
            (_, Reply::Folders(folders)) => {
                self.folders = folders;
                if let Section::Folder(id) = &self.section
                    && !self.folders.iter().any(|f| f.id == *id)
                {
                    self.section = Section::All;
                }
                Task::none()
            }
            (_, Reply::Organizations(orgs)) => {
                self.orgs = orgs;
                Task::none()
            }
            (_, Reply::Collections(c)) => {
                self.collections = c;
                if let Section::Collection(id) = &self.section
                    && !self.collections.iter().any(|c| c.id == *id)
                {
                    self.section = Section::All;
                }
                Task::none()
            }
            (Ask::Get(id), Reply::Item(record)) => {
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.record = Pane::Open(Box::new(record));
                }
                Task::none()
            }
            (Ask::Draft(id), Reply::Draft(draft)) => self.on_draft_loaded(id, draft),
            (_, Reply::Login(outcome)) => self.on_login(Ok(outcome)),
            (Ask::SendCode, _) => {
                self.on_code_sent(Ok(()));
                Task::none()
            }
            (Ask::Unlock, _) => self.on_unlocked(Ok(())),
            (Ask::Verify(id), Reply::Checked(ok)) => self.on_verified(id, ok),
            (Ask::Create | Ask::Update, Reply::Saved { id }) => self.on_saved(id),
            (Ask::Trash(id), _) => {
                self.toast_list_change("Moved to the trash");
                self.after_write(Some(id))
            }
            (Ask::Restore(id), _) => {
                self.toast_list_change("Restored");
                self.after_write(Some(id))
            }
            (Ask::Delete(id), _) => {
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.selected = None;
                    self.record = Pane::Empty;
                }
                self.toast_list_change("Deleted forever");
                self.after_write(None)
            }
            (Ask::Favorite(id) | Ask::Move(id), _) => self.after_write(Some(id)),
            (Ask::CreateFolder | Ask::RenameFolder, _) => {
                self.sheet = None;
                self.ask(Ask::Folders, Request::Folders)
            }
            (Ask::DeleteFolder(id), _) => {
                self.sheet = None;
                if self.section == Section::Folder(id) {
                    self.section = Section::All;
                }
                Task::batch([
                    self.ask(Ask::Folders, Request::Folders),
                    self.ask(Ask::List, Request::List { url: None }),
                ])
            }
            (Ask::Sync, _) => self.ask(Ask::Status, Request::Status),
            (Ask::Lock | Ask::Logout, _) => self.ask(Ask::Status, Request::Status),
            (Ask::SetSettings, _) => Task::none(),
            (Ask::Generate, Reply::Generated(value)) => {
                self.on_generated(Ok(value));
                Task::none()
            }
            (Ask::Export(path), Reply::Exported { data }) => self.on_exported(path, data),
            (ask, _) => {
                tracing::debug!(?ask, "an answer nobody waited for");
                Task::none()
            }
        }
    }

    fn on_error(&mut self, ask: Ask, e: VaultError) -> Task<Msg> {
        tracing::info!(?ask, code = ?e.code, "vault request failed");
        match ask {
            Ask::Status | Ask::Settings | Ask::Folders | Ask::Organizations | Ask::Collections => {
                Task::none()
            }
            Ask::List => {
                if e.code != ErrorCode::Locked && e.code != ErrorCode::SignedOut {
                    return self.show_toast(format!("Couldn't list the vault: {}", e.message));
                }
                Task::none()
            }
            Ask::Get(id) => {
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.record = Pane::Failed(e.message);
                }
                Task::none()
            }
            Ask::Draft(_) => self.show_toast(format!("Couldn't edit it: {}", e.message)),
            Ask::Login | Ask::TwoFactor => self.on_login(Err(e)),
            Ask::SendCode => {
                self.on_code_sent(Err(e));
                Task::none()
            }
            Ask::Unlock => self.on_unlocked(Err(e)),
            Ask::Verify(_) => {
                if let Some(r) = self.reprompt.as_mut() {
                    r.busy = false;
                    r.error = Some(e.message);
                }
                Task::none()
            }
            Ask::Create | Ask::Update => {
                if let Some(form) = self.edit.as_mut() {
                    form.busy = false;
                    form.error = Some(match e.code {
                        ErrorCode::Conflict => {
                            "This item changed elsewhere. Sync, then edit it again.".into()
                        }
                        _ => e.message,
                    });
                }
                Task::none()
            }
            Ask::CreateFolder | Ask::RenameFolder | Ask::DeleteFolder(_) => {
                if let Some(sheets::Sheet::Folder(f)) = self.sheet.as_mut() {
                    f.busy = false;
                    f.error = Some(e.message);
                    return Task::none();
                }
                self.sheet = None;
                self.show_toast(format!("Couldn't change the folder: {}", e.message))
            }
            Ask::Generate => {
                self.on_generated(Err(e));
                Task::none()
            }
            Ask::Export(_) => {
                self.picker = None;
                if let Some(sheets::Sheet::Export(x)) = self.sheet.as_mut() {
                    x.busy = false;
                    x.error = Some(e.message);
                    return Task::none();
                }
                self.show_toast(format!("Couldn't export: {}", e.message))
            }
            Ask::Sync => {
                let ask_status = self.ask(Ask::Status, Request::Status);
                Task::batch([ask_status, self.show_toast(format!("Couldn't sync: {}", e.message))])
            }
            Ask::Trash(_)
            | Ask::Restore(_)
            | Ask::Delete(_)
            | Ask::Favorite(_)
            | Ask::Move(_)
            | Ask::Lock
            | Ask::Logout
            | Ask::SetSettings => {
                if matches!(ask, Ask::Delete(_)) {
                    self.sheet = None;
                }
                self.show_toast(e.message)
            }
        }
    }

    /// After a write that changes the list: list again, read the open
    /// record again if it was the one written.
    fn after_write(&mut self, id: Option<String>) -> Task<Msg> {
        let mut tasks = vec![self.ask(Ask::List, Request::List { url: None })];
        if let Some(id) = id
            && self.selected.as_deref() == Some(id.as_str())
            && matches!(self.record, Pane::Open(_))
        {
            tasks.push(self.ask(Ask::Get(id.clone()), Request::Get { id }));
        }
        Task::batch(tasks)
    }

    fn toast_list_change(&mut self, text: &str) {
        self.sheet = None;
        let _ = self.show_toast(text);
    }
}
