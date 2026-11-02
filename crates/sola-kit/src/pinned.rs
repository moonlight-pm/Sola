//! Whether the focused window is pinned to every workspace
//! (`Topic::FocusPinned`), for the check on the Window menu's Pin item in the
//! titlebars an app draws ([`crate::menu::prepare`]). The kit keeps it for
//! the app, whatever it subscribes to.

use std::sync::atomic::{AtomicBool, Ordering};

use sola_bus::Message;
use sola_bus::topics::Topic;

static FOCUS_PINNED: AtomicBool = AtomicBool::new(false);

/// Take it from a bus delivery that carries it. True when it changed.
pub(crate) fn observe(msg: &Message) -> bool {
    // By name first: every message passes here.
    if msg.topic != "FocusPinned" {
        return false;
    }
    let pinned = matches!(Topic::parse(msg), Some(Topic::FocusPinned(p)) if p.pinned && msg.sticky);
    FOCUS_PINNED.swap(pinned, Ordering::Relaxed) != pinned
}

/// Whether the focused window is pinned.
pub(crate) fn focus_is_pinned() -> bool {
    FOCUS_PINNED.load(Ordering::Relaxed)
}
