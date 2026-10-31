//! A window per session: each session is its own window, a tab in the
//! shell's tab bar (sessions anywhere, 2026-10-30). Closing a window only
//! stops this computer watching; the session keeps running where it is.
//! A navigator ([`crate::navigator`]) is a window with no session yet.

use std::collections::{HashMap, HashSet};

use iced::{Task, window};
use sola_bus::topics::Activity;
use sola_kit::tabs::TabWindows;

use crate::catalog::Tab;
use crate::status::AgentStatus;

pub struct SessionWindows {
    tabs: TabWindows,
    by_tab: HashMap<String, window::Id>,
    by_window: HashMap<window::Id, String>,
    sizes: HashMap<window::Id, iced::Size>,
    /// Windows with no session yet: where a project is picked.
    navigators: HashSet<window::Id>,
    /// Navigators opened, for their tab ids.
    navigators_opened: u32,
    /// The window keys and menu actions go to: the last one focused.
    pub focused: Option<window::Id>,
}

impl SessionWindows {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            tabs: TabWindows::new(app_id),
            by_tab: HashMap::new(),
            by_window: HashMap::new(),
            sizes: HashMap::new(),
            navigators: HashSet::new(),
            navigators_opened: 0,
            focused: None,
        }
    }

    /// Open a navigator window, in front.
    pub fn open_navigator(&mut self, app_id: &'static str) -> (window::Id, Task<window::Id>) {
        let settings = sola_kit::app::window_settings_transparent(app_id);
        self.navigators_opened += 1;
        let id = format!("navigator-{}", self.navigators_opened);
        let (window, task) = self.tabs.open(&id, settings, false);
        self.navigators.insert(window);
        self.focused = Some(window);
        tracing::info!(tab = %id, "navigator window");
        (window, task)
    }


    /// Open session `tab_id`'s window. A `background` window joins its
    /// zone behind, unfocused.
    pub fn open(
        &mut self,
        app_id: &'static str,
        tab_id: &str,
        background: bool,
    ) -> Task<window::Id> {
        let settings = sola_kit::app::window_settings_transparent(app_id);
        let (window, task) = self.tabs.open(tab_id, settings, background);
        self.by_tab.insert(tab_id.to_string(), window);
        self.by_window.insert(window, tab_id.to_string());
        if self.focused.is_none() {
            self.focused = Some(window);
        }
        tracing::info!(tab = %tab_id, background, "session window");
        task
    }

    /// Forget a window (closing, or closed under us). Returns its session
    /// (a navigator has none).
    pub fn forget(&mut self, window: window::Id) -> Option<String> {
        let tab = if self.navigators.remove(&window) {
            None
        } else {
            let tab = self.by_window.remove(&window)?;
            self.by_tab.remove(&tab);
            Some(tab)
        };
        self.sizes.remove(&window);
        self.tabs.close(window);
        if self.focused == Some(window) {
            self.focused = self
                .by_window
                .keys()
                .chain(self.navigators.iter())
                .next()
                .copied();
        }
        tab
    }

    pub fn tab_of(&self, window: window::Id) -> Option<&str> {
        self.by_window.get(&window).map(String::as_str)
    }

    /// The session the focused window shows.
    pub fn focused_tab(&self) -> Option<&str> {
        self.focused.and_then(|w| self.tab_of(w))
    }

    pub fn window_of(&self, tab_id: &str) -> Option<window::Id> {
        self.by_tab.get(tab_id).copied()
    }

    pub fn has(&self, tab_id: &str) -> bool {
        self.by_tab.contains_key(tab_id)
    }

    /// Every open window and its session.
    pub fn iter(&self) -> impl Iterator<Item = (window::Id, &str)> {
        self.by_window.iter().map(|(w, p)| (*w, p.as_str()))
    }

    /// No window at all: no session's, no navigator.
    pub fn is_empty(&self) -> bool {
        self.by_window.is_empty() && self.navigators.is_empty()
    }

    pub fn set_size(&mut self, window: window::Id, size: iced::Size) {
        self.sizes.insert(window, size);
    }

    pub fn size(&self, window: window::Id) -> Option<iced::Size> {
        self.sizes.get(&window).copied()
    }

    /// Fold a bus message (`TabWindow`: paired ids, shown changes).
    pub fn update(&mut self, msg: &sola_bus::Message) {
        self.tabs.update(msg);
    }

    /// The window the shell asks to close (`Topic::CloseTab`, the tab
    /// bar's stoplight), if it is one of ours.
    pub fn close_request(&self, msg: &sola_bus::Message) -> Option<window::Id> {
        self.tabs.close_request(msg)
    }

    /// sola-river's id for a window, once the shell paired it.
    pub fn river_id(&self, window: window::Id) -> Option<u32> {
        self.tabs.river_id(window)
    }

    /// Ask the shell to show session `tab_id`'s window and focus it.
    pub fn show(&self, tab_id: &str) {
        if let Some(window) = self.window_of(tab_id) {
            self.tabs.show(window);
        }
    }

    /// Mark each window's row in the shell's tab bar with its session's
    /// activity. Sends only what changed.
    pub fn publish_activity(&mut self, tabs: &[Tab]) {
        for (window, tab) in self.by_window.iter() {
            self.tabs.set_activity(*window, session_activity(tab, tabs));
        }
    }
}

/// A session's mark in the shell's bar.
pub fn session_activity(tab_id: &str, tabs: &[Tab]) -> Activity {
    tabs.iter()
        .find(|t| t.id == tab_id)
        .map_or(Activity::Idle, |t| AgentStatus::rollup([t.status]).activity())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::Kind;

    fn tab(id: &str, project_id: &str, status: AgentStatus) -> Tab {
        Tab {
            id: id.into(),
            project_id: project_id.into(),
            name: id.into(),
            title: None,
            path: PathBuf::from("/tmp"),
            host: None,
            kind: Kind::Worktree,
            parent: None,
            layout: None,
            active_pane: None,
            status,
            agent: None,
        }
    }

    #[test]
    fn each_session_shows_its_own_mark() {
        let tabs = [
            tab("a", "sola", AgentStatus::Working),
            tab("b", "sola", AgentStatus::Waiting),
            tab("c", "oath", AgentStatus::Done),
        ];
        assert_eq!(session_activity("a", &tabs), Activity::Working);
        assert_eq!(session_activity("b", &tabs), Activity::Waiting);
        assert_eq!(session_activity("c", &tabs), Activity::Idle);
        assert_eq!(session_activity("gone", &tabs), Activity::Idle);
    }
}
