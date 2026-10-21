//! Windows opened as tabs (Next, slice 2): an app with a window per tab —
//! a terminal tab, a page, an agent session — opens each window through
//! [`TabWindows::open`]. That announces the tab (`Topic::Tab`) just before
//! the window opens, so the shell can tell which window it is and keep its
//! place in the tab bar; the shell answers with the window's sola-river id
//! (`Topic::TabWindow`). Subscribe to `TopicKind::TabWindow` and feed every
//! bus message to [`TabWindows::update`].

use std::collections::HashMap;

use iced::{Task, window};
use sola_bus::Message;
use sola_bus::topics::{Tab, TabWindow, Topic};

struct Entry {
    key: String,
    seq: u32,
    /// sola-river's id, once the shell has paired it.
    river: Option<u32>,
}

pub struct TabWindows {
    app_id: &'static str,
    /// Windows this process has opened as tabs.
    opened: u32,
    windows: HashMap<window::Id, Entry>,
}

impl TabWindows {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            app_id,
            opened: 0,
            windows: HashMap::new(),
        }
    }

    /// The bus key of the app's tab `id`.
    pub fn key(&self, id: &str) -> String {
        format!("{}/{id}", self.app_id)
    }

    /// Open the window of the app's tab `id`, announcing it first.
    pub fn open(
        &mut self,
        id: &str,
        settings: window::Settings,
    ) -> (window::Id, Task<window::Id>) {
        self.opened += 1;
        let key = self.key(id);
        self.announce(&key, self.opened, None);
        let (wid, task) = window::open(settings);
        self.windows.insert(
            wid,
            Entry {
                key,
                seq: self.opened,
                river: None,
            },
        );
        (wid, task)
    }

    fn tab(&self, key: &str, seq: u32, window_id: Option<u32>) -> Tab {
        Tab {
            key: key.to_string(),
            app_id: self.app_id.to_string(),
            pid: std::process::id(),
            seq,
            window_id,
        }
    }

    fn announce(&self, key: &str, seq: u32, window_id: Option<u32>) {
        if let Ok(mut bus) = crate::app::bus().lock() {
            if let Err(e) = bus.emit(Topic::Tab(self.tab(key, seq, window_id))) {
                tracing::warn!(%key, "announce tab failed: {e:?}");
            }
        }
    }

    /// The tab's window is gone (or going): retract its announcement.
    pub fn close(&mut self, window: window::Id) {
        let Some(entry) = self.windows.remove(&window) else {
            return;
        };
        if let Ok(mut bus) = crate::app::bus().lock() {
            let _ = bus.retract(Topic::Tab(self.tab(&entry.key, entry.seq, None)));
        }
    }

    /// Fold one bus message (`TabWindow` pairs and their retractions). A
    /// new pair is announced back with the window's id.
    pub fn update(&mut self, msg: &Message) {
        let Some(Topic::TabWindow(TabWindow { key, window_id })) = Topic::parse(msg) else {
            return;
        };
        let river = msg.sticky.then_some(window_id);
        let Some(entry) = self.windows.values_mut().find(|e| e.key == key) else {
            return;
        };
        if entry.river == river {
            return;
        }
        entry.river = river;
        let seq = entry.seq;
        if river.is_some() {
            self.announce(&key, seq, river);
        }
    }

    /// sola-river's id for this window, once paired.
    pub fn river_id(&self, window: window::Id) -> Option<u32> {
        self.windows.get(&window).and_then(|e| e.river)
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}
