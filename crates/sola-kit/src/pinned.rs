//! Windows that stay where they show (`Topic::PinnedTabs`): a window on the
//! Common workspace, seen from any other, is not closed from there. The kit
//! keeps the list for the app, whatever it subscribes to; a frame offers no
//! close for a pinned window ([`crate::window_frame`] for an app with one
//! window, [`crate::tabs::TabWindows::closable`] for one with a window per
//! tab).

use std::sync::RwLock;

use sola_bus::Message;
use sola_bus::topics::Topic;

static KEYS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Take the list from a bus delivery that carries it.
pub(crate) fn observe(msg: &Message) {
    // By name first: every message passes here.
    if msg.topic != "PinnedTabs" {
        return;
    }
    if let Some(Topic::PinnedTabs(pinned)) = Topic::parse(msg) {
        if let Ok(mut keys) = KEYS.write() {
            *keys = pinned.keys;
        }
    }
}

/// Whether the tab with this key stays where it shows.
pub fn is_pinned(key: &str) -> bool {
    KEYS.read().is_ok_and(|keys| keys.iter().any(|k| k == key))
}

/// Whether this app's window, a tab of no key of its own, stays.
pub(crate) fn this_app() -> bool {
    crate::app::app_id().is_some_and(is_pinned)
}
