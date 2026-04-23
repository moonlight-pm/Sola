use std::collections::HashMap;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub struct Session {
    pub name: Option<String>,
    pub working_dir: PathBuf,
    pub cancel_token: CancellationToken,
}

impl Session {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            name: None,
            working_dir,
            cancel_token: CancellationToken::new(),
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
        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);
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
}
