//! Windows opened as tabs (Next, slice 2): an app with a window per tab —
//! a terminal tab, a page, an agent session — opens each window through
//! [`TabWindows::open`]. That announces the tab (`Topic::Tab`) just before
//! the window opens, so the shell can tell which window it is and keep its
//! place in the tab bar; the shell answers with the window's sola-river id
//! and whether it is on screen (`Topic::TabWindow`). Subscribe to
//! `TopicKind::TabWindow` and feed every bus message to
//! [`TabWindows::update`]. [`TabWindows::set_activity`] puts a mark on the
//! tab's row in the bar (`Topic::TabActivity`: working, or waiting on a
//! decision).

use std::collections::HashMap;

use iced::{Task, window};
use sola_bus::Message;
use sola_bus::topics::{Activity, ShowTab, Tab, TabActivity, TabWindow, Topic};

struct Entry {
    key: String,
    seq: u32,
    background: bool,
    /// sola-river's id, once the shell has paired it.
    river: Option<u32>,
    /// On screen, per the shell (a window starts shown until it says).
    shown: bool,
    /// Last [`TabActivity`] sent; `None` until the first, which is always
    /// sent so a restarted app clears what it left behind.
    activity: Option<Activity>,
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

    /// Open the window of the app's tab `id`, announcing it first. A
    /// `background` tab joins its zone behind the shown one, unfocused.
    pub fn open(
        &mut self,
        id: &str,
        settings: window::Settings,
        background: bool,
    ) -> (window::Id, Task<window::Id>) {
        self.opened += 1;
        let key = self.key(id);
        self.announce(&key, self.opened, None, background);
        let (wid, task) = window::open(settings);
        self.windows.insert(
            wid,
            Entry {
                key,
                seq: self.opened,
                background,
                river: None,
                shown: true,
                activity: None,
            },
        );
        (wid, task)
    }

    fn tab(&self, key: &str, seq: u32, window_id: Option<u32>, background: bool) -> Tab {
        Tab {
            key: key.to_string(),
            app_id: self.app_id.to_string(),
            pid: std::process::id(),
            seq,
            window_id,
            background,
        }
    }

    fn announce(&self, key: &str, seq: u32, window_id: Option<u32>, background: bool) {
        if let Ok(mut bus) = crate::app::bus().lock() {
            if let Err(e) = bus.emit(Topic::Tab(self.tab(key, seq, window_id, background))) {
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
            let _ = bus.retract(Topic::Tab(self.tab(&entry.key, entry.seq, None, false)));
            if entry.activity.is_some() {
                let _ = bus.retract(Topic::TabActivity(TabActivity {
                    key: entry.key.clone(),
                    activity: Activity::Idle,
                }));
            }
        }
    }

    /// Say what the tab is doing (its row's mark in the shell's tab bar).
    /// Sent when it changes; call it as often as is convenient.
    pub fn set_activity(&mut self, window: window::Id, activity: Activity) {
        let Some(entry) = self.windows.get_mut(&window) else {
            return;
        };
        if entry.activity == Some(activity) {
            return;
        }
        entry.activity = Some(activity);
        if let Ok(mut bus) = crate::app::bus().lock() {
            let key = entry.key.clone();
            if let Err(e) = bus.emit(Topic::TabActivity(TabActivity { key, activity })) {
                tracing::warn!(key = %entry.key, "tab activity failed: {e:?}");
            }
        }
    }

    /// Ask the shell to show this tab's window and focus it.
    pub fn show(&self, window: window::Id) {
        let Some(entry) = self.windows.get(&window) else {
            return;
        };
        if let Ok(mut bus) = crate::app::bus().lock() {
            let _ = bus.emit(Topic::ShowTab(ShowTab {
                key: entry.key.clone(),
            }));
        }
    }

    /// Fold one bus message (`TabWindow` pairs, shown changes, and their
    /// retractions). A new pair is announced back with the window's id.
    /// Returns true when a window's shown state changed.
    pub fn update(&mut self, msg: &Message) -> bool {
        let Some(Topic::TabWindow(TabWindow {
            key,
            window_id,
            shown,
        })) = Topic::parse(msg)
        else {
            return false;
        };
        let river = msg.sticky.then_some(window_id);
        let Some(entry) = self.windows.values_mut().find(|e| e.key == key) else {
            return false;
        };
        let shown_changed = msg.sticky && entry.shown != shown;
        if msg.sticky {
            entry.shown = shown;
        }
        if entry.river != river {
            entry.river = river;
            let (seq, background) = (entry.seq, entry.background);
            if river.is_some() {
                self.announce(&key, seq, river, background);
            }
        }
        shown_changed
    }

    /// sola-river's id for this window, once paired.
    pub fn river_id(&self, window: window::Id) -> Option<u32> {
        self.windows.get(&window).and_then(|e| e.river)
    }

    /// Is this window on screen? True until the shell says otherwise.
    pub fn shown(&self, window: window::Id) -> bool {
        self.windows.get(&window).is_none_or(|e| e.shown)
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}
