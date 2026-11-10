//! The windows a web app had when it was quit, so that opening it again
//! brings them all back, each under its own page id (the shell keeps a
//! window's place by it). One file for every profile:
//! `<browser data>/quit-apps.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::session::SessionTab;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct QuitApps {
    /// App id → its pages, in window order.
    #[serde(default)]
    apps: BTreeMap<String, Vec<SessionTab>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl QuitApps {
    pub fn load() -> Self {
        Self::load_from(crate::profiles::browser_data_root().join("quit-apps.json"))
    }

    fn load_from(path: PathBuf) -> Self {
        let mut this = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                warn!(path = %path.display(), error = %e, "quit web apps unreadable; starting empty");
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        this.path = Some(path);
        this
    }

    /// Remember `pages` as the windows app `app_id` had.
    pub fn remember(&mut self, app_id: &str, pages: Vec<SessionTab>) {
        if pages.is_empty() {
            return self.forget(app_id);
        }
        self.apps.insert(app_id.to_string(), pages);
        self.save();
    }

    /// The windows app `app_id` had, now taken.
    pub fn take(&mut self, app_id: &str) -> Vec<SessionTab> {
        let pages = self.apps.remove(app_id).unwrap_or_default();
        if !pages.is_empty() {
            self.save();
        }
        pages
    }

    /// The app is gone (removed): nothing to bring back.
    pub fn forget(&mut self, app_id: &str) {
        if self.apps.remove(app_id).is_some() {
            self.save();
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Err(e) = write(path, self) {
            warn!(path = %path.display(), error = %e, "save quit web apps failed");
        }
    }
}

fn write(path: &Path, apps: &QuitApps) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_vec_pretty(apps)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str) -> SessionTab {
        SessionTab {
            id: id.into(),
            url: format!("https://example.com/{id}"),
            app: Some("web-x".into()),
            ..SessionTab::default()
        }
    }

    #[test]
    fn a_quit_app_s_windows_survive_a_restart_and_come_back_once() {
        let dir = std::env::temp_dir().join(format!("sola-quit-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("quit-apps.json");
        let mut apps = QuitApps::load_from(path.clone());
        apps.remember("web-x", vec![page("a"), page("b")]);
        let mut again = QuitApps::load_from(path.clone());
        let ids: Vec<String> = again.take("web-x").into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(QuitApps::load_from(path.clone()).take("web-x").is_empty(), "taken once");
        let mut gone = QuitApps::load_from(path.clone());
        gone.remember("web-y", vec![page("c")]);
        gone.forget("web-y");
        assert!(QuitApps::load_from(path).take("web-y").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
