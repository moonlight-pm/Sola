use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::pty::PtyManager;

#[derive(Serialize, Deserialize, Clone)]
pub struct TabEntry {
    pub pty_id: String,
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RestoredTab {
    pub tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

pub struct TerminalState {
    pub tabs: RwLock<Vec<TabEntry>>,
    pub custom_titles: RwLock<HashMap<String, String>>,
    pub pty_manager: Mutex<PtyManager>,
}

impl TerminalState {
    pub fn new() -> Self {
        Self {
            tabs: RwLock::new(Vec::new()),
            custom_titles: RwLock::new(HashMap::new()),
            pty_manager: Mutex::new(PtyManager::new()),
        }
    }

    pub async fn persist_to_disk(&self) {
        let tabs = self.tabs.read().await;
        let titles = self.custom_titles.read().await;

        // Query live CWDs from tmux
        let live_paths: HashMap<String, String> =
            crate::tmux::list_session_paths().into_iter().collect();

        let serialized: Vec<serde_json::Value> = tabs
            .iter()
            .map(|tab| {
                // Live CWD takes priority over stored CWD
                let cwd = live_paths
                    .get(&tab.tmux_session)
                    .cloned()
                    .or_else(|| tab.cwd.clone());

                let custom_title = titles
                    .get(&tab.tmux_session)
                    .cloned()
                    .or_else(|| tab.custom_title.clone());

                let mut obj = serde_json::json!({
                    "tmuxSession": tab.tmux_session,
                });
                if let Some(t) = custom_title {
                    obj["customTitle"] = serde_json::Value::String(t);
                }
                if let Some(c) = cwd {
                    obj["cwd"] = serde_json::Value::String(c);
                }
                obj
            })
            .collect();

        let payload = serde_json::json!({ "tabs": serialized });

        let path = state_file_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!("Failed to create state dir {}: {e}", parent.display());
                return;
            }
        }

        let tmp_path = path.with_extension("json.tmp");
        let content = match serde_json::to_string_pretty(&payload) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to serialize terminal state: {e}");
                return;
            }
        };

        if let Err(e) = std::fs::write(&tmp_path, &content) {
            warn!("Failed to write state to {}: {e}", tmp_path.display());
            return;
        }

        if let Err(e) = std::fs::rename(&tmp_path, &path) {
            warn!("Failed to rename state file: {e}");
            return;
        }

        info!("Persisted terminal state ({} tabs)", tabs.len());
    }

    pub fn load_from_disk() -> Vec<RestoredTab> {
        let path = state_file_path();

        // Discover live tmux sessions
        let live_sessions: std::collections::HashSet<String> =
            crate::tmux::list_sessions().into_iter().collect();

        // Parse saved state if it exists
        let mut saved: Vec<RestoredTab> = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(value) => value
                        .get("tabs")
                        .and_then(|t| t.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|entry| {
                                    let tmux_session =
                                        entry.get("tmuxSession")?.as_str()?.to_string();
                                    let custom_title = entry
                                        .get("customTitle")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    let cwd = entry
                                        .get("cwd")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                    Some(RestoredTab {
                                        tmux_session,
                                        custom_title,
                                        cwd,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    Err(e) => {
                        warn!("Failed to parse terminal state file: {e}");
                        Vec::new()
                    }
                },
                Err(e) => {
                    warn!("Failed to read terminal state file: {e}");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Remove sessions that are no longer alive in tmux
        saved.retain(|tab| live_sessions.contains(&tab.tmux_session));

        // Add any live sessions not already in the saved state
        let new_sessions: Vec<String> = {
            let saved_sessions: std::collections::HashSet<&str> =
                saved.iter().map(|t| t.tmux_session.as_str()).collect();
            live_sessions
                .iter()
                .filter(|s| !saved_sessions.contains(s.as_str()))
                .cloned()
                .collect()
        };
        for session in new_sessions {
            saved.push(RestoredTab {
                tmux_session: session,
                custom_title: None,
                cwd: None,
            });
        }

        info!("Loaded {} tabs from state", saved.len());
        saved
    }
}

fn state_file_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("sola").join("terminal-state.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_file_path_under_sola() {
        let path = state_file_path();
        assert!(path.to_string_lossy().contains("sola/terminal-state.json"));
    }

    #[test]
    fn empty_state_serializes() {
        let state = TerminalState::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            state.persist_to_disk().await;
        });
        let path = state_file_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap();
            let value: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(value.get("tabs").unwrap().as_array().unwrap().is_empty());
            let _ = std::fs::remove_file(&path);
        }
    }
}
