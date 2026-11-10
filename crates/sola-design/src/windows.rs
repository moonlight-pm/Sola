//! A window per project (Next): each project is its own window, a tab in
//! the shell's tab bar, with its canvases along the top; a navigator
//! window ([`crate::navigator`]) picks the project. Closing a window only
//! closes the window; the files stay. The window last focused or touched
//! is the *active* one: its canvas takes input, keys and menu actions.

use std::collections::HashMap;

use iced::{Size, Task, window};
use sola_bus::topics::Activity;
use sola_kit::tabs::TabWindows;

/// The project of a navigator window: none yet.
pub const NAVIGATOR: &str = "";

/// One open window.
pub struct Win {
    /// Its project's id ([`NAVIGATOR`] for a navigator).
    pub project: String,
    /// Its plane's size, once laid out.
    pub plane: Option<Size>,
    /// Its output scale (physical px per logical px).
    pub scale: f32,
}

pub struct DesignWindows {
    tabs: TabWindows,
    wins: HashMap<window::Id, Win>,
    /// Navigators opened, for their tab keys.
    navigators_opened: u32,
    /// The window keys, menu actions and agents' default go to.
    pub active: Option<window::Id>,
}

impl DesignWindows {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            tabs: TabWindows::new(app_id),
            wins: HashMap::new(),
            navigators_opened: 0,
            active: None,
        }
    }

    /// Open project `project`'s window. A `background` window joins its
    /// zone behind, unfocused.
    pub fn open(
        &mut self,
        app_id: &'static str,
        project: &str,
        background: bool,
    ) -> (window::Id, Task<window::Id>) {
        self.open_tab(app_id, project, project, background)
    }

    /// Open a navigator window, in front.
    pub fn open_navigator(&mut self, app_id: &'static str) -> (window::Id, Task<window::Id>) {
        self.navigators_opened += 1;
        let key = format!("navigator-{}", self.navigators_opened);
        self.open_tab(app_id, &key, NAVIGATOR, false)
    }

    fn open_tab(
        &mut self,
        app_id: &'static str,
        key: &str,
        project: &str,
        background: bool,
    ) -> (window::Id, Task<window::Id>) {
        let mut settings = sola_kit::app::window_settings_transparent(app_id);
        settings.size = Size::new(1280.0, 820.0);
        let (window, task) = self.tabs.open(key, settings, background);
        self.wins.insert(
            window,
            Win {
                project: project.to_string(),
                plane: None,
                scale: 1.0,
            },
        );
        tracing::info!(%key, background, "design window");
        (window, task)
    }

    /// Forget a window (closing, or closed under us). Returns its project.
    pub fn forget(&mut self, window: window::Id) -> Option<String> {
        let win = self.wins.remove(&window)?;
        self.tabs.close(window);
        if self.active == Some(window) {
            self.active = None;
        }
        Some(win.project)
    }

    pub fn get(&self, window: window::Id) -> Option<&Win> {
        self.wins.get(&window)
    }

    pub fn get_mut(&mut self, window: window::Id) -> Option<&mut Win> {
        self.wins.get_mut(&window)
    }

    pub fn project_of(&self, window: window::Id) -> Option<&str> {
        self.wins.get(&window).map(|w| w.project.as_str())
    }

    pub fn window_of(&self, project: &str) -> Option<window::Id> {
        self.wins
            .iter()
            .find(|(_, w)| w.project == project)
            .map(|(id, _)| *id)
    }

    /// The navigator windows.
    pub fn navigators(&self) -> impl Iterator<Item = window::Id> + '_ {
        self.wins
            .iter()
            .filter(|(_, w)| w.project == NAVIGATOR)
            .map(|(id, _)| *id)
    }

    /// Every open window.
    pub fn iter(&self) -> impl Iterator<Item = (window::Id, &Win)> {
        self.wins.iter().map(|(id, w)| (*id, w))
    }

    pub fn is_empty(&self) -> bool {
        self.wins.is_empty()
    }

    /// Fold a bus message (`TabWindow`: paired ids, shown changes).
    /// Returns true when a window's shown state changed.
    pub fn update(&mut self, msg: &sola_bus::Message) -> bool {
        self.tabs.update(msg)
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

    /// The shell has the window on screen.
    pub fn shown(&self, window: window::Id) -> bool {
        self.tabs.shown(window)
    }

    /// Ask the shell to show `window` and focus it.
    pub fn show(&self, window: window::Id) {
        self.tabs.show(window);
    }

    /// Mark the window's row in the shell's tab bar.
    pub fn set_activity(&mut self, window: window::Id, activity: Activity) {
        self.tabs.set_activity(window, activity);
    }
}
