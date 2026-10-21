//! A window per page (Next): every open page is its own window — a tab in
//! the shell's tab bar — painting through its own [`FrameSlot`]. The
//! window you are in is the active tab and carries the full chrome
//! (location bar, find, menus, dialogs, DevTools); the others show their
//! page live under a read-only bar.

use std::collections::HashMap;
use std::sync::Arc;

use iced::{Task, window};
use sola_kit::tabs::TabWindows;

use crate::engine::{Engine, FrameSlot, TabId};
use crate::run::SlotRegistry;

struct Page<E: Engine> {
    window: window::Id,
    slot: Arc<FrameSlot<E>>,
    /// The shown state last sent to the engine (`Cmd::ShowPage`).
    shown_sent: Option<bool>,
}

pub struct PageWindows<E: Engine> {
    tabs: TabWindows,
    pages: HashMap<TabId, Page<E>>,
    by_window: HashMap<window::Id, TabId>,
    /// Every page window's slot, read by the frame stream.
    slots: SlotRegistry<E>,
    /// Where new slots take their command channel, cursor and IME handle.
    template: Arc<FrameSlot<E>>,
    /// The page window that last had keyboard focus.
    pub focused: Option<window::Id>,
}

impl<E: Engine> PageWindows<E> {
    pub fn new(app_id: &'static str, template: Arc<FrameSlot<E>>) -> Self {
        Self {
            tabs: TabWindows::new(app_id),
            pages: HashMap::new(),
            by_window: HashMap::new(),
            slots: Arc::new(std::sync::Mutex::new(Vec::new())),
            template,
            focused: None,
        }
    }

    pub fn slots(&self) -> SlotRegistry<E> {
        self.slots.clone()
    }

    /// Open page `tab`'s window (`page_id` is its stable id, the shell tab
    /// key). A `background` page joins its zone behind, unfocused.
    pub fn open(
        &mut self,
        app_id: &'static str,
        tab: TabId,
        page_id: &str,
        background: bool,
    ) -> Task<window::Id> {
        let (w, h) = *self.template.last_size.lock().unwrap();
        let slot = Arc::new(FrameSlot::for_page(
            self.template.cmd_tx.clone(),
            self.template.cursor.clone(),
            self.template.ime.clone(),
            tab,
            w,
            h,
        ));
        let settings = sola_kit::app::window_settings_transparent(app_id);
        let (window, task) = self.tabs.open(page_id, settings, background);
        self.slots.lock().unwrap().push(slot.clone());
        self.pages.insert(
            tab,
            Page {
                window,
                slot,
                shown_sent: None,
            },
        );
        self.by_window.insert(window, tab);
        tracing::info!(tab = tab.0, %page_id, background, "page window");
        task
    }

    /// Close page `tab`'s window (the page itself is already closed).
    pub fn close(&mut self, tab: TabId) -> Task<window::Id> {
        let Some(page) = self.pages.remove(&tab) else {
            return Task::none();
        };
        self.forget(page.window, &page.slot);
        window::close(page.window)
    }

    /// The window closed under us: forget it.
    pub fn closed(&mut self, window: window::Id) -> Option<TabId> {
        let tab = self.by_window.get(&window).copied()?;
        if let Some(page) = self.pages.remove(&tab) {
            self.forget(page.window, &page.slot);
        }
        Some(tab)
    }

    fn forget(&mut self, window: window::Id, slot: &Arc<FrameSlot<E>>) {
        self.by_window.remove(&window);
        self.tabs.close(window);
        self.slots
            .lock()
            .unwrap()
            .retain(|s| !Arc::ptr_eq(s, slot));
        crate::cef::frame::retire_slot(slot.key);
        if self.focused == Some(window) {
            self.focused = None;
        }
    }

    pub fn tab_of(&self, window: window::Id) -> Option<TabId> {
        self.by_window.get(&window).copied()
    }

    pub fn window_of(&self, tab: TabId) -> Option<window::Id> {
        self.pages.get(&tab).map(|p| p.window)
    }

    pub fn slot_of(&self, tab: TabId) -> Option<Arc<FrameSlot<E>>> {
        self.pages.get(&tab).map(|p| p.slot.clone())
    }

    pub fn has(&self, tab: TabId) -> bool {
        self.pages.contains_key(&tab)
    }

    /// The pages that have a window.
    pub fn open_tabs(&self) -> Vec<TabId> {
        self.pages.keys().copied().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Fold a bus message (`TabWindow`: paired ids, shown changes).
    pub fn update(&mut self, msg: &sola_bus::Message) -> bool {
        self.tabs.update(msg)
    }

    /// sola-river's id for a page window, once the shell paired it.
    pub fn river_id(&self, window: window::Id) -> Option<u32> {
        self.tabs.river_id(window)
    }

    /// Ask the shell to show page `tab`'s window and focus it.
    pub fn show(&self, tab: TabId) {
        if let Some(window) = self.window_of(tab) {
            self.tabs.show(window);
        }
    }

    /// The pages whose window went on or off screen since last asked, and
    /// whether each is now shown: the engine paints shown pages and rests
    /// the others.
    pub fn take_shown_changes(&mut self) -> Vec<(TabId, bool)> {
        let mut out = Vec::new();
        for (tab, page) in &mut self.pages {
            let shown = self.tabs.shown(page.window);
            if page.shown_sent != Some(shown) {
                page.shown_sent = Some(shown);
                out.push((*tab, shown));
            }
        }
        out
    }
}
