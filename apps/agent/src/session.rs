use std::collections::HashMap;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Idle,
    Running,
    Error(String),
}

pub struct Session {
    pub session_id: String,
    pub name: Option<String>,
    pub working_dir: PathBuf,
    pub messages: Vec<claurst_core::types::Message>,
    pub cancel_token: CancellationToken,
    pub status: SessionStatus,
}

impl Session {
    pub fn new(session_id: String, working_dir: PathBuf) -> Self {
        Self {
            session_id,
            name: None,
            working_dir,
            messages: Vec::new(),
            cancel_token: CancellationToken::new(),
            status: SessionStatus::Idle,
        }
    }
}

pub struct SessionManager {
    pub sessions: tokio::sync::RwLock<HashMap<String, Session>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_session(&self, working_dir: PathBuf) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = Session::new(session_id.clone(), working_dir);
        self.sessions.write().await.insert(session_id.clone(), session);
        session_id
    }

    pub async fn close_session(&self, session_id: &str) {
        if let Some(session) = self.sessions.write().await.remove(session_id) {
            session.cancel_token.cancel();
        }
    }

    pub async fn rename_session(&self, session_id: &str, name: String) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.name = Some(name);
        }
    }

    pub async fn set_status(&self, session_id: &str, status: SessionStatus) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.status = status;
        }
    }

    pub async fn cancel_session(&self, session_id: &str) {
        if let Some(session) = self.sessions.read().await.get(session_id) {
            session.cancel_token.cancel();
        }
    }
}
