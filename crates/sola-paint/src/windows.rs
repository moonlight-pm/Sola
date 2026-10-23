//! A window per image (Next): each open image is its own window, a tab in
//! the shell's tab bar named for the file. Closing the window closes the
//! image; the last one closing quits. With nothing open, one window offers
//! Open. Each window keeps its own stage: its drawing cache, its size, and
//! where the pointer last was.

use std::collections::HashMap;
use std::path::Path;

use iced::widget::canvas;
use iced::{Point, Size, Task, window};
use sola_kit::tabs::TabWindows;

/// The doc of the window shown while no image is open (doc ids start at 1).
pub const EMPTY: u64 = 0;

/// One open window.
pub struct Win {
    /// Its image ([`EMPTY`] for the no-image window).
    pub doc: u64,
    pub cache: canvas::Cache,
    /// The stage's last laid-out size.
    pub stage: Size,
    pub cursor: Option<Point>,
}

pub struct PaintWindows {
    tabs: TabWindows,
    wins: HashMap<window::Id, Win>,
    /// The window keys, menu actions and the tools act on: the last one
    /// focused or touched.
    pub focused: Option<window::Id>,
}

impl PaintWindows {
    pub fn new(app_id: &'static str) -> Self {
        Self {
            tabs: TabWindows::new(app_id),
            wins: HashMap::new(),
            focused: None,
        }
    }

    /// Open the window of image `doc` at `path` (`None` for the no-image
    /// window). Its tab is keyed by the path, so the shell puts it back in
    /// its place when the session restores it. A `background` window joins
    /// its zone behind, unfocused.
    pub fn open(
        &mut self,
        app_id: &'static str,
        doc: u64,
        path: Option<&Path>,
        background: bool,
    ) -> (window::Id, Task<window::Id>) {
        let key = match path {
            Some(p) => p.to_string_lossy().into_owned(),
            None if doc == EMPTY => "new".to_string(),
            None => format!("image-{doc}"),
        };
        let mut settings = sola_kit::app::window_settings_transparent(app_id);
        settings.size = Size::new(1180.0, 800.0);
        let (window, task) = self.tabs.open(&key, settings, background);
        self.wins.insert(
            window,
            Win {
                doc,
                cache: canvas::Cache::new(),
                stage: Size::new(800.0, 600.0),
                cursor: None,
            },
        );
        if self.focused.is_none() || !background {
            self.focused = Some(window);
        }
        tracing::info!(doc, %key, background, "paint window");
        (window, task)
    }

    /// Forget a window (closing, or closed under us). Returns its doc.
    pub fn forget(&mut self, window: window::Id) -> Option<u64> {
        let win = self.wins.remove(&window)?;
        self.tabs.close(window);
        if self.focused == Some(window) {
            self.focused = self.wins.keys().next().copied();
        }
        Some(win.doc)
    }

    pub fn get(&self, window: window::Id) -> Option<&Win> {
        self.wins.get(&window)
    }

    pub fn get_mut(&mut self, window: window::Id) -> Option<&mut Win> {
        self.wins.get_mut(&window)
    }

    /// The window showing image `doc`.
    pub fn of_doc(&self, doc: u64) -> Option<window::Id> {
        self.wins
            .iter()
            .find(|(_, w)| w.doc == doc)
            .map(|(id, _)| *id)
    }

    pub fn doc_of(&self, window: window::Id) -> Option<u64> {
        self.wins.get(&window).map(|w| w.doc)
    }

    /// Every open window.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Win> {
        self.wins.values_mut()
    }

    pub fn is_empty(&self) -> bool {
        self.wins.is_empty()
    }

    /// Fold a bus message (`TabWindow`: paired ids, shown changes).
    pub fn update(&mut self, msg: &sola_bus::Message) {
        self.tabs.update(msg);
    }

    /// sola-river's id for a window, once the shell paired it.
    pub fn river_id(&self, window: window::Id) -> Option<u32> {
        self.tabs.river_id(window)
    }

    /// Ask the shell to show `window` and focus it.
    pub fn show(&self, window: window::Id) {
        self.tabs.show(window);
    }
}
