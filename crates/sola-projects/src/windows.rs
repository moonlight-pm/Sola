//! A window per project (Next): each project is its own window, a tab in
//! the shell's tab bar, with its tabs along the top. Closing a window only
//! closes the window; the project and its tmux sessions stay, and
//! Project → Open brings it back.

use std::collections::HashMap;

use iced::{Task, window};
use sola_kit::tabs::TabWindows;

/// The key of the window shown while there is no project yet.
pub const EMPTY: &str = "";

pub struct ProjectWindows {
    tabs: TabWindows,
    by_project: HashMap<String, window::Id>,
    by_window: HashMap<window::Id, String>,
    sizes: HashMap<window::Id, iced::Size>,
    /// The window keys and menu actions go to: the last one focused.
    pub focused: Option<window::Id>,
}

impl ProjectWindows {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            tabs: TabWindows::new(app_id),
            by_project: HashMap::new(),
            by_window: HashMap::new(),
            sizes: HashMap::new(),
            focused: None,
        }
    }

    /// Open project `project_id`'s window ([`EMPTY`] for the no-project
    /// window). A `background` window joins its zone behind, unfocused.
    pub fn open(
        &mut self,
        app_id: &'static str,
        project_id: &str,
        background: bool,
    ) -> Task<window::Id> {
        let key = if project_id.is_empty() { "new" } else { project_id };
        let settings = sola_kit::app::window_settings_transparent(app_id);
        let (window, task) = self.tabs.open(key, settings, background);
        self.by_project.insert(project_id.to_string(), window);
        self.by_window.insert(window, project_id.to_string());
        if self.focused.is_none() {
            self.focused = Some(window);
        }
        tracing::info!(project = %project_id, background, "project window");
        task
    }

    /// Forget a window (closing, or closed under us). Returns its project.
    pub fn forget(&mut self, window: window::Id) -> Option<String> {
        let project = self.by_window.remove(&window)?;
        self.by_project.remove(&project);
        self.sizes.remove(&window);
        self.tabs.close(window);
        if self.focused == Some(window) {
            self.focused = self.by_window.keys().next().copied();
        }
        Some(project)
    }

    pub fn project_of(&self, window: window::Id) -> Option<&str> {
        self.by_window.get(&window).map(String::as_str)
    }

    pub fn window_of(&self, project_id: &str) -> Option<window::Id> {
        self.by_project.get(project_id).copied()
    }

    pub fn has(&self, project_id: &str) -> bool {
        self.by_project.contains_key(project_id)
    }

    /// Every open window and its project.
    pub fn iter(&self) -> impl Iterator<Item = (window::Id, &str)> {
        self.by_window.iter().map(|(w, p)| (*w, p.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.by_window.is_empty()
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

    /// sola-river's id for a window, once the shell paired it.
    pub fn river_id(&self, window: window::Id) -> Option<u32> {
        self.tabs.river_id(window)
    }

    /// Ask the shell to show project `project_id`'s window and focus it.
    pub fn show(&self, project_id: &str) {
        if let Some(window) = self.window_of(project_id) {
            self.tabs.show(window);
        }
    }
}
