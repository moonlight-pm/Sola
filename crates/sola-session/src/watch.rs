//! Apps that aren't Sola's live as long as they have a window on the desk
//! (Joshua, 2026-10-31). Sola has no tray, so an app that hides to one —
//! Steam does when its window closes — would run on with nothing to show
//! for it. Once such an app has had a window and has had none for
//! [`GRACE`], or has shown none [`FIRST_WINDOW`] after it started, it
//! quits the way Super+Q quits it ([`crate::quit`]).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use sola_bus::topics::Window;

use crate::quit::Quit;
use crate::tag;

/// How long an app may be without a window before it quits. Steam swaps
/// its windows with about a second between them.
pub const GRACE: Duration = Duration::from_secs(5);

/// How long a new app may take to show its first window.
pub const FIRST_WINDOW: Duration = Duration::from_secs(120);

/// One app that isn't Sola's.
pub struct Foreign {
    /// What it was launched with (its quit command derives from it).
    pub command: String,
    /// Launched, or found running, at.
    pub since: Instant,
    pub seen_window: bool,
    /// Since when it has had no window (after having had one).
    pub missing_since: Option<Instant>,
    /// Set while it quits.
    pub quit: Option<Quit>,
}

impl Foreign {
    pub fn launched(command: &str, now: Instant) -> Self {
        Self {
            command: command.to_string(),
            since: now,
            seen_window: false,
            missing_since: None,
            quit: None,
        }
    }

    /// Found running with its windows (a restarted sola-session).
    pub fn adopted(command: &str, now: Instant) -> Self {
        Self {
            seen_window: true,
            ..Self::launched(command, now)
        }
    }

    /// A live window list: whether one of its windows is in it.
    pub fn note(&mut self, has_window: bool, now: Instant) {
        if has_window {
            self.seen_window = true;
            self.missing_since = None;
        } else if self.seen_window {
            self.missing_since.get_or_insert(now);
        }
    }

    /// Why it should quit now, if it should.
    pub fn due(&self, now: Instant) -> Option<&'static str> {
        if self.quit.is_some() {
            return None;
        }
        match self.missing_since {
            Some(t) if now.duration_since(t) >= GRACE => Some("its last window closed"),
            None if !self.seen_window && now.duration_since(self.since) >= FIRST_WINDOW => {
                Some("it showed no window")
            }
            _ => None,
        }
    }
}

/// Which app each window's process belongs to, remembered per pid while
/// the window is up.
#[derive(Default)]
pub struct Owners(HashMap<u32, Option<String>>);

impl Owners {
    /// The apps with a window in `windows`: each window's app id, and the
    /// app its process carries the tag of — a Steam game's window
    /// (`steam_app_<id>`) is Steam's.
    pub fn apps_with_windows(&mut self, windows: &[Window]) -> HashSet<String> {
        let live: HashSet<u32> = windows.iter().filter_map(|w| w.pid).collect();
        self.0.retain(|pid, _| live.contains(pid));
        windows.iter().flat_map(|w| self.apps_of(w)).collect()
    }

    /// The apps `window` is a window of: its app id, and the app its
    /// process carries the tag of.
    pub fn apps_of(&mut self, window: &Window) -> Vec<String> {
        let mut apps = vec![window.app_id.clone()];
        if let Some(pid) = window.pid {
            if let Some(owner) = self.0.entry(pid).or_insert_with(|| tag::owner(pid)) {
                if *owner != window.app_id {
                    apps.push(owner.clone());
                }
            }
        }
        apps
    }

    /// The apps whose last window is `window_id`: asked to close it, they
    /// quit (Steam, its window closed, starts its webhelper again and comes
    /// back with an empty window).
    pub fn last_window_of(&mut self, windows: &[Window], window_id: u32) -> Vec<String> {
        let Some(closing) = windows.iter().find(|w| w.window_id == window_id) else {
            return Vec::new();
        };
        let others: HashSet<String> = windows
            .iter()
            .filter(|w| w.window_id != window_id)
            .flat_map(|w| self.apps_of(w))
            .collect();
        self.apps_of(closing)
            .into_iter()
            .filter(|app| !others.contains(app))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quits_once_its_windows_stay_gone() {
        let t0 = Instant::now();
        let mut app = Foreign::launched("/bin/steam", t0);
        app.note(true, t0);
        app.note(false, t0 + Duration::from_secs(1));
        assert_eq!(app.due(t0 + Duration::from_secs(5)), None);
        // A window back within the grace keeps it.
        app.note(true, t0 + Duration::from_secs(2));
        assert_eq!(app.due(t0 + Duration::from_secs(60)), None);
        app.note(false, t0 + Duration::from_secs(61));
        assert_eq!(app.due(t0 + Duration::from_secs(66)), Some("its last window closed"));
    }

    #[test]
    fn quits_when_no_first_window_comes() {
        let t0 = Instant::now();
        let mut app = Foreign::launched("/bin/steam", t0);
        app.note(false, t0 + Duration::from_secs(10));
        assert_eq!(app.missing_since, None, "no window yet is not a closed one");
        assert_eq!(app.due(t0 + Duration::from_secs(119)), None);
        assert_eq!(app.due(t0 + FIRST_WINDOW), Some("it showed no window"));
    }

    #[test]
    fn an_adopted_app_has_had_its_window() {
        let t0 = Instant::now();
        let mut app = Foreign::adopted("/bin/steam", t0);
        assert_eq!(app.due(t0 + FIRST_WINDOW), None);
        app.note(false, t0);
        assert!(app.due(t0 + GRACE).is_some());
    }

    fn window(id: u32, app_id: &str) -> Window {
        Window {
            window_id: id,
            app_id: app_id.into(),
            title: app_id.into(),
            pid: None,
        }
    }

    #[test]
    fn closing_an_apps_last_window_is_closing_the_app() {
        let mut owners = Owners::default();
        let ws = [window(1, "steam"), window(2, "steam"), window(3, "sola-mail")];
        assert!(owners.last_window_of(&ws, 1).is_empty(), "Steam has another");
        assert_eq!(owners.last_window_of(&ws[1..], 2), ["steam"]);
        assert!(owners.last_window_of(&ws, 9).is_empty(), "no such window");
    }

    #[test]
    fn windows_count_by_app_id_without_a_pid() {
        let mut owners = Owners::default();
        let ws = [Window {
            window_id: 1,
            app_id: "steam".into(),
            title: "Steam".into(),
            pid: None,
        }];
        assert!(owners.apps_with_windows(&ws).contains("steam"));
    }
}
