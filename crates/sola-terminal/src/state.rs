use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::pty::PtyManager;

#[derive(Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub pty_id: String,
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub ordinal: u32,
}

pub struct TerminalState {
    pub tabs: RwLock<Vec<TabEntry>>,
    pub pty_manager: Mutex<PtyManager>,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            tabs: RwLock::new(Vec::new()),
            pty_manager: Mutex::new(PtyManager::new()),
        }
    }
}
