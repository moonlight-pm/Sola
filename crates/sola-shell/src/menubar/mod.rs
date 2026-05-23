//! Menubar window — state, lifecycle, and view entry point.
//!
//! The menubar is the first of the four shell windows to open. It provides:
//! - Left cluster: system-menu button, focused-app title, app-menu labels.
//! - Right cluster: toast notification overlay, clock.
//!
//! Window state lives in [`MenubarState`]; the view is in [`view`].

pub mod view;

use chrono::Local;
use iced::window;
use sola_kit::app::window_settings;

/// Height of the menubar window in logical pixels.
pub const WINDOW_HEIGHT: u32 = 28;

/// Runtime state for the menubar window.
pub struct MenubarState {
    /// Active toast message, if any.
    pub toast: Option<String>,
    /// Monotonically-increasing generation counter used to cancel stale
    /// toast-expiry callbacks (see `Msg::ToastExpire`).
    pub toast_generation: u64,
    /// Current local time — updated by `Msg::ClockTick`.
    pub clock_now: chrono::DateTime<Local>,
    /// X-coordinates of each app-menu label button, in left-to-right order.
    /// Populated by Task 7 (menu anchor work); declared now so view can
    /// reference the field without a later structural change.
    pub label_positions: Vec<f32>,
}

impl MenubarState {
    pub fn new() -> Self {
        Self {
            toast: None,
            toast_generation: 0,
            clock_now: Local::now(),
            label_positions: Vec::new(),
        }
    }

    /// Push a new transient toast message and bump the generation counter.
    /// The caller should schedule a `Msg::ToastExpire(self.toast_generation)`
    /// task after calling this.
    pub fn push_toast(&mut self, message: impl Into<String>) {
        self.toast_generation = self.toast_generation.wrapping_add(1);
        self.toast = Some(message.into());
    }

    /// Clear the toast if `generation` matches the current generation.
    /// Stale expiry callbacks (from a superseded toast) are silently ignored.
    pub fn expire_toast(&mut self, generation: u64) {
        if generation == self.toast_generation {
            self.toast = None;
        }
    }
}

/// Open the menubar window and return `(id, Task<Id>)`.
/// The `id` is pre-allocated so `Shell` can store it immediately; the task
/// drives the actual OS-level window creation and resolves to the same id.
pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    settings.size = iced::Size::new(1920.0, WINDOW_HEIGHT as f32);
    settings.position = iced::window::Position::Specific(iced::Point::new(0.0, 0.0));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}
