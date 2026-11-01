//! Page favicons on the shell's bar. Each page's latest favicon is written
//! as a picture file — named by its content, so a changed icon is a new
//! file the bar loads afresh — and named on the page's tab
//! (`TabWindows::set_icon`): the bar shows it in place of the browser's
//! globe. A web app saved without an icon (its page had none yet when it
//! was made) takes its page's favicon as the app's icon.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use sola_bus::topics::Topic;

use super::App;
use crate::engine::{Engine, TabId};

/// Where the page favicons are kept while the browser runs.
fn dir() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    cache.join("sola/browser/favicons")
}

/// Drop the last run's favicons: every page sends its own again as it loads.
pub(super) fn clear() {
    let _ = std::fs::remove_dir_all(dir());
}

/// Write page `page_id`'s favicon, removing the page's older one; its path.
fn write(page_id: &str, png: &[u8]) -> Option<String> {
    let mut hasher = DefaultHasher::new();
    png.hash(&mut hasher);
    let dir = dir();
    let path = dir.join(format!("{page_id}-{:016x}.png", hasher.finish()));
    if !path.exists() {
        forget(page_id);
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png)) {
            tracing::warn!(path = %path.display(), error = %e, "page favicon: could not write");
            return None;
        }
    }
    Some(path.display().to_string())
}

/// Page `page_id` closed: its favicon goes.
pub(super) fn forget(page_id: &str) {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return;
    };
    let prefix = format!("{page_id}-");
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

impl<E: Engine> App<E> {
    /// The favicons the engine sent: kept for wrapping a page, written and
    /// named on a browser page's tab, and a web app's when it has none.
    pub(super) fn drain_favicons(&mut self) {
        let icons: Vec<crate::engine::FaviconIpc> =
            self.engine.favicons_handle().lock().unwrap().drain(..).collect();
        for icon in icons {
            let tab = icon.tab_id;
            if let Some(app_id) = self.webapps.tabs.get(&tab).cloned() {
                self.webapps.favicons.insert(tab, icon.png);
                self.adopt_app_icon(&app_id, tab);
                continue;
            }
            let page_id = self.page_id(tab);
            if let Some(path) = write(&page_id, &icon.png) {
                self.page_icons.insert(tab, path.clone());
                self.windows.set_icon(tab, Some(path));
            }
            self.webapps.favicons.insert(tab, icon.png);
        }
    }

    /// Name each page's favicon on its window's tab (a window opened after
    /// its favicon came).
    pub(super) fn sync_page_icons(&mut self) {
        for (tab, path) in &self.page_icons {
            self.windows.set_icon(*tab, Some(path.clone()));
        }
    }

    /// A web app saved with the globe takes page `tab`'s favicon as its icon.
    fn adopt_app_icon(&mut self, app_id: &str, tab: TabId) {
        let Some(app) = self.webapps.apps.get(app_id) else {
            return;
        };
        if !app.icon.is_empty() && app.icon != "lucide/globe" {
            return;
        }
        let icon = self.save_icon(app_id, tab);
        if icon == "lucide/globe" {
            return;
        }
        let mut app = app.clone();
        app.icon = icon;
        tracing::info!(app = %app_id, icon = %app.icon, "web app takes its page's favicon");
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            if let Err(e) = bus.emit(Topic::Application(app.clone())) {
                tracing::warn!(error = %e, "web app icon failed");
            }
        }
        self.webapps.apps.insert(app_id.to_string(), app);
    }

    /// Page `tab` closed: its favicon goes with it.
    pub(super) fn forget_page_icon(&mut self, tab: TabId) {
        if self.page_icons.remove(&tab).is_some() {
            if let Some(page_id) = self.page_ids.get(&tab) {
                forget(page_id);
            }
        }
    }
}
