use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sola_app::config::JsonConfig;
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::pty::PtyManager;

#[derive(Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub pty_id: String,
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RestoredTab {
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedTerminalState {
    #[serde(default)]
    tabs: Vec<RestoredTab>,
}

impl JsonConfig for PersistedTerminalState {
    const FILE_NAME: &'static str = "terminal-state.json";
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

    pub async fn persist_to_disk(&self) {
        let tabs = self.tabs.read().await;

        let live_paths: HashMap<String, String> =
            crate::tmux::list_session_paths().into_iter().collect();

        let serialized: Vec<RestoredTab> = tabs
            .iter()
            .map(|tab| RestoredTab {
                tmux_session: tab.tmux_session.clone(),
                cwd: live_paths
                    .get(&tab.tmux_session)
                    .cloned()
                    .or_else(|| tab.cwd.clone()),
            })
            .collect();

        let state = PersistedTerminalState { tabs: serialized };
        state.save();
        info!("Persisted terminal state ({} tabs)", tabs.len());
    }

    pub fn load_from_disk() -> Vec<RestoredTab> {
        let mut saved = PersistedTerminalState::load().tabs;

        let Some(live) = crate::tmux::list_sessions() else {
            info!(
                "Loaded {} tabs from state (tmux query failed, keeping all)",
                saved.len()
            );
            return saved;
        };

        let live_sessions: std::collections::HashSet<String> = live.into_iter().collect();
        saved.retain(|tab| live_sessions.contains(&tab.tmux_session));

        let known: std::collections::HashSet<&str> =
            saved.iter().map(|t| t.tmux_session.as_str()).collect();
        let orphaned_live: Vec<String> = live_sessions
            .iter()
            .filter(|s| !known.contains(s.as_str()))
            .cloned()
            .collect();
        for session in orphaned_live {
            saved.push(RestoredTab {
                tmux_session: session,
                cwd: None,
            });
        }

        info!("Loaded {} tabs from state", saved.len());
        saved
    }
}
