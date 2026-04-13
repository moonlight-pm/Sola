use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::ChildStdin;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Idle,
    Running,
    Error(String),
}

pub struct Session {
    pub name: Option<String>,
    pub working_dir: PathBuf,
    pub cancel_token: CancellationToken,
    pub status: SessionStatus,
    /// Stdin handle for the running claude subprocess.
    /// Present only while a subprocess is active.
    pub stdin: Option<Arc<Mutex<ChildStdin>>>,
}

impl Session {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            name: None,
            working_dir,
            cancel_token: CancellationToken::new(),
            status: SessionStatus::Idle,
            stdin: None,
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
        let session = Session::new(working_dir);
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

    pub async fn set_stdin(&self, session_id: &str, stdin: Option<Arc<Mutex<ChildStdin>>>) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.stdin = stdin;
        }
    }

    pub async fn cancel_session(&self, session_id: &str) {
        if let Some(session) = self.sessions.read().await.get(session_id) {
            session.cancel_token.cancel();
        }
    }

    pub async fn is_running(&self, session_id: &str) -> bool {
        self.sessions.read().await
            .get(session_id)
            .map(|s| s.status == SessionStatus::Running)
            .unwrap_or(false)
    }

    pub async fn get_stdin(&self, session_id: &str) -> Option<Arc<Mutex<ChildStdin>>> {
        self.sessions.read().await
            .get(session_id)
            .and_then(|s| s.stdin.clone())
    }
}
