//! Per-app float-state tracking for kit apps that draw their own titlebar.
//!
//! An app doesn't know its own sola-river `window_id`, so we learn it by
//! matching `(app_id, title)` from `Topic::Windows`, then track the float bit
//! from the sticky `Topic::WindowFloating`. Feed [`update`] every bus message
//! (from the app's `bus_subscription` fold); read [`is_floating`] /
//! [`is_floating_any`] in `view`.
//!
//! [`update`]: FloatState::update
//! [`is_floating`]: FloatState::is_floating
//! [`is_floating_any`]: FloatState::is_floating_any

use std::collections::{HashMap, HashSet};

use sola_bus::Message;
use sola_bus::topics::Topic;

#[derive(Debug, Default)]
pub struct FloatState {
    app_id: String,
    /// This app's surfaces: sola-river `window_id` keyed by window title.
    ids_by_title: HashMap<String, u32>,
    /// Currently-floating `window_id`s (all apps; filtered on read).
    floating: HashSet<u32>,
}

impl FloatState {
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            ..Default::default()
        }
    }

    /// Fold one bus message. Call from the app's bus-message update arm.
    pub fn update(&mut self, msg: &Message) {
        match Topic::parse(msg) {
            // Windows is the full list each time — rebuild so closed windows drop.
            Some(Topic::Windows(windows)) => {
                self.ids_by_title.clear();
                for w in windows {
                    if w.app_id == self.app_id {
                        self.ids_by_title.insert(w.title, w.window_id);
                    }
                }
            }
            Some(Topic::WindowFloating(wf)) => {
                if wf.floating {
                    self.floating.insert(wf.window_id);
                } else {
                    self.floating.remove(&wf.window_id);
                }
            }
            _ => {}
        }
    }

    /// Is this app's surface with `title` currently floating?
    pub fn is_floating(&self, title: &str) -> bool {
        self.ids_by_title
            .get(title)
            .is_some_and(|id| self.floating.contains(id))
    }

    /// Is any of this app's surfaces floating? Convenient for single-window apps.
    pub fn is_floating_any(&self) -> bool {
        self.ids_by_title.values().any(|id| self.floating.contains(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::{Window, WindowFloating};

    fn win(window_id: u32, app_id: &str, title: &str) -> Window {
        Window {
            window_id,
            app_id: app_id.into(),
            title: title.into(),
            pid: None,
        }
    }

    #[test]
    fn tracks_own_float_by_app_id_and_title() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(
            &Topic::Windows(vec![
                win(7, "sola-monitor", "Monitor"),
                win(9, "other-app", "Other"),
            ])
            .to_message(),
        );
        assert!(!fs.is_floating_any());

        // our window floats
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 7, floating: true }).to_message());
        assert!(fs.is_floating_any());
        assert!(fs.is_floating("Monitor"));

        // another app's float does not count as ours
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 9, floating: true }).to_message());
        assert!(fs.is_floating("Monitor"));
        assert!(!fs.is_floating("Other")); // "Other" isn't ours

        // unfloat clears it
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 7, floating: false }).to_message());
        assert!(!fs.is_floating_any());
        assert!(!fs.is_floating("Monitor"));
    }

    #[test]
    fn closed_window_drops_from_tracking() {
        let mut fs = FloatState::new("sola-monitor");
        fs.update(&Topic::Windows(vec![win(7, "sola-monitor", "Monitor")]).to_message());
        fs.update(&Topic::WindowFloating(WindowFloating { window_id: 7, floating: true }).to_message());
        assert!(fs.is_floating_any());
        // window closes → Windows no longer lists it
        fs.update(&Topic::Windows(vec![]).to_message());
        assert!(!fs.is_floating_any());
    }
}
