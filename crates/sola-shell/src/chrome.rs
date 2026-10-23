//! Each window's chrome (shell → sola-river and apps, `Topic::WindowChrome`):
//! whether it is tiled, whether it is in Cinema, and whether the compositor
//! rims it because its app draws no frame of its own (Next slice 6). The
//! shell recomputes the whole set whenever frames go out and sends only
//! what changed.

use std::collections::HashMap;

use sola_bus::topics::WindowChrome;

/// What was last published, per window.
#[derive(Debug, Default)]
pub struct ChromeState {
    sent: HashMap<u32, WindowChrome>,
}

/// What to emit (new or changed) and what to retract (windows gone).
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub emit: Vec<WindowChrome>,
    pub retract: Vec<WindowChrome>,
}

impl ChromeState {
    /// Take the full current set; return the difference from the last one.
    pub fn update(&mut self, next: impl IntoIterator<Item = WindowChrome>) -> Changes {
        let next: HashMap<u32, WindowChrome> =
            next.into_iter().map(|c| (c.window_id, c)).collect();
        let mut changes = Changes::default();
        for (id, chrome) in &next {
            if self.sent.get(id) != Some(chrome) {
                changes.emit.push(*chrome);
            }
        }
        for (id, chrome) in &self.sent {
            if !next.contains_key(id) {
                changes.retract.push(*chrome);
            }
        }
        changes.emit.sort_by_key(|c| c.window_id);
        changes.retract.sort_by_key(|c| c.window_id);
        self.sent = next;
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome(window_id: u32, tiled: bool, rim: bool) -> WindowChrome {
        WindowChrome {
            window_id,
            tiled,
            cinema: false,
            rim,
        }
    }

    #[test]
    fn sends_new_and_changed_windows_once() {
        let mut s = ChromeState::default();
        let first = s.update([chrome(1, true, false), chrome(2, true, true)]);
        assert_eq!(first.emit, vec![chrome(1, true, false), chrome(2, true, true)]);
        assert!(s.update([chrome(1, true, false), chrome(2, true, true)]) == Changes::default());
        let floated = s.update([chrome(1, false, false), chrome(2, true, true)]);
        assert_eq!(floated.emit, vec![chrome(1, false, false)]);
        assert!(floated.retract.is_empty());
    }

    #[test]
    fn retracts_windows_that_left() {
        let mut s = ChromeState::default();
        s.update([chrome(1, true, false), chrome(2, true, true)]);
        let gone = s.update([chrome(1, true, false)]);
        assert!(gone.emit.is_empty());
        assert_eq!(gone.retract, vec![chrome(2, true, true)]);
    }
}
