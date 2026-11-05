//! The pages' videos, told to sola-river (`Topic::WindowVideos`): each
//! page's latest from its watcher (`crate::videos`), placed in its window,
//! so the desk's opacity leaves them out and a watched one holds the idle
//! fade. None in the active page's window while the browser's own panels,
//! dialogs or menus are over the page: they would go opaque with it.

use std::collections::HashMap;

use sola_bus::topics::{Topic, WindowVideo, WindowVideos};

use super::App;
use crate::engine::{Engine, TabId};
use crate::videos::PageVideo;

#[derive(Default)]
pub(super) struct Videos {
    /// Each page's latest videos in view, its view pixels.
    pages: HashMap<TabId, Vec<PageVideo>>,
    /// What sola-river was last told, by its window id.
    told: HashMap<u32, Vec<WindowVideo>>,
}

impl<E: Engine> App<E> {
    /// The watchers' reports the engine sent.
    pub(super) fn drain_videos(&mut self) {
        let reports: Vec<_> = self.engine.videos_handle().lock().unwrap().drain(..).collect();
        for (tab, videos) in reports {
            if videos.is_empty() {
                self.videos.pages.remove(&tab);
            } else {
                self.videos.pages.insert(tab, videos);
            }
        }
    }

    /// Page `tab` closed: its videos go (its window's are taken back).
    pub(super) fn forget_videos(&mut self, tab: TabId) {
        self.videos.pages.remove(&tab);
    }

    /// The bus was (re)connected and holds none of ours: tell it again.
    pub(super) fn retell_videos(&mut self) {
        self.videos.told.clear();
    }

    /// Tell sola-river each page window's videos, once per change.
    pub(super) fn publish_videos(&mut self) {
        if self.videos.pages.is_empty() && self.videos.told.is_empty() {
            return;
        }
        let mut now: HashMap<u32, Vec<WindowVideo>> = HashMap::new();
        for (&tab, videos) in &self.videos.pages {
            let Some(window) = self.windows.window_of(tab) else {
                continue;
            };
            let (Some(river), Some(slot)) = (self.windows.river_id(window), self.windows.slot_of(tab))
            else {
                continue;
            };
            let Some(view) = *slot.page_view.lock().unwrap() else {
                continue;
            };
            if tab == self.cached_active && self.page_covered() {
                continue;
            }
            let placed = crate::videos::in_window(videos, view.origin, view.scale);
            if !placed.is_empty() {
                now.insert(river, placed);
            }
        }
        if now == self.videos.told {
            return;
        }
        let Ok(mut bus) = sola_kit::app::bus().lock() else {
            return;
        };
        for (&window_id, videos) in &now {
            if self.videos.told.get(&window_id) != Some(videos) {
                let videos = videos.clone();
                if let Err(e) = bus.emit(Topic::WindowVideos(WindowVideos { window_id, videos })) {
                    tracing::debug!(window_id, error = %e, "videos: not told");
                }
            }
        }
        for &window_id in self.videos.told.keys() {
            if !now.contains_key(&window_id) {
                let gone = WindowVideos { window_id, videos: Vec::new() };
                if let Err(e) = bus.retract(Topic::WindowVideos(gone)) {
                    tracing::debug!(window_id, error = %e, "videos: not taken back");
                }
            }
        }
        self.videos.told = now;
    }

    /// The browser's own panels, dialogs or menus are over the active page.
    fn page_covered(&self) -> bool {
        self.find_open
            || self.downloads_panel_open
            || self.vault_is_open()
            || self.webapps.wrap.is_some()
            || self.wallpaper.is_some()
            || self.context_menu.is_some()
            || self.file_dialog.is_some()
            || self.pending_http_auth.is_some()
            || self.pending_js_dialog.is_some()
            || self.pending_media.is_some()
            || self.pending_notify.is_some()
            || !self.omnibox_hits().is_empty()
    }
}
