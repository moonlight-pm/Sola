//! What each request to the daemon was for, so its answer lands in the
//! right place; and which requests go on a connection of their own.

use std::path::PathBuf;

use iced::Task;
use sola_vault_proto::Request;

use super::{App, Msg};
use crate::daemon;

#[derive(Debug, Clone)]
pub enum Ask {
    Status,
    Settings,
    SetSettings,
    List,
    Folders,
    Organizations,
    Collections,
    Get(String),
    Draft(String),
    Login,
    TwoFactor,
    SendCode,
    Unlock,
    Lock,
    Logout,
    Sync,
    /// The master password for the item with this id.
    Verify(String),
    Create,
    Update,
    Trash(String),
    Restore(String),
    Delete(String),
    Favorite(String),
    Move(String),
    CreateFolder,
    RenameFolder,
    DeleteFolder(String),
    Generate,
    Export(PathBuf),
}

impl Ask {
    /// A login's KDF, a full sync, an export: on a connection of their own
    /// so nothing else waits behind them.
    fn slow(&self) -> bool {
        matches!(
            self,
            Ask::Login
                | Ask::TwoFactor
                | Ask::SendCode
                | Ask::Unlock
                | Ask::Verify(_)
                | Ask::Sync
                | Ask::Logout
                | Ask::Export(_)
        )
    }
}

impl App {
    pub(super) fn ask(&self, ask: Ask, req: Request) -> Task<Msg> {
        if ask.slow() {
            Task::perform(daemon::ask_alone(req), move |a| Msg::Answer(ask, a))
        } else {
            Task::perform(daemon::ask(req), move |a| Msg::Answer(ask, a))
        }
    }

    /// Everything an unlocked vault shows.
    pub(super) fn load_vault(&self) -> Task<Msg> {
        Task::batch([
            self.ask(Ask::List, Request::List { url: None }),
            self.ask(Ask::Folders, Request::Folders),
            self.ask(Ask::Organizations, Request::Organizations),
            self.ask(Ask::Collections, Request::Collections),
        ])
    }
}
