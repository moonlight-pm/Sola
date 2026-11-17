//! What this device keeps: one sqlite file under the state directory.
//!
//! The file holds the account's private keys in the clear, as Signal-Desktop's
//! holds them beside its database key; it is made readable by its owner only.

mod account;
pub mod chats;
mod protocol;
mod schema;

pub use protocol::ProtocolStore;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context, Result};
use rusqlite::Connection;

#[derive(Clone)]
pub struct Store {
    db: Arc<Mutex<Connection>>,
}

impl Store {
    /// `$XDG_STATE_HOME/sola/signal/signal.db`.
    pub fn path() -> PathBuf {
        sola_core::paths::state_dir().join("signal").join("signal.db")
    }

    pub fn open(path: &Path) -> Result<Self> {
        let dir = path.parent().context("the store's directory")?;
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let db = Connection::open(path).with_context(|| path.display().to_string())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&db)?;
        Ok(Self { db: Arc::new(Mutex::new(db)) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().expect("the store's lock")
    }

    /// Where attachments' files are kept, beside the store.
    pub fn files_dir() -> PathBuf {
        Self::path().with_file_name("files")
    }
}

/// Now, as Signal counts time: milliseconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
