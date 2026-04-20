use std::sync::{mpsc, Arc};

use crate::config::MailConfig;
use crate::idle::IdleHandle;
use crate::imap::ImapClient;
use crate::rules::MailRule;

pub struct MailState {
    pub client: tokio::sync::Mutex<Option<Arc<std::sync::Mutex<ImapClient>>>>,
    pub config: tokio::sync::RwLock<Option<MailConfig>>,
    pub idle_handle: tokio::sync::Mutex<Option<IdleHandle>>,
    pub idle_move_rules: Arc<std::sync::Mutex<Vec<MailRule>>>,
    pub keepalive_abort: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub event_tx: mpsc::Sender<String>,
}

impl MailState {
    pub fn new(event_tx: mpsc::Sender<String>) -> Self {
        Self {
            client: tokio::sync::Mutex::new(None),
            config: tokio::sync::RwLock::new(None),
            idle_handle: tokio::sync::Mutex::new(None),
            idle_move_rules: Arc::new(std::sync::Mutex::new(Vec::new())),
            keepalive_abort: tokio::sync::Mutex::new(None),
            event_tx,
        }
    }
}
