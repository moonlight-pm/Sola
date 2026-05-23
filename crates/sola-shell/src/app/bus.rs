//! Bus topic dispatch for the iced shell.
//!
//! `Shell::handle_bus` parses an incoming `sola_bus::Message` into a typed
//! `Topic` and routes it to a per-topic method. Only `on_theme` and
//! `on_output_geometry` have real bodies in this task; all others are stubs
//! that will be filled in as each window comes online (Tasks 5–10).

use std::time::Duration;

use iced::Task;
use sola_bus::topics::{
    AppMenuPayload, Application, ChordEvent, LaunchResultPayload, MouseClickedPayload,
    MouseEnteredPayload, OutputGeometry, Topic, UserAppExitedPayload, Window,
};
use sola_core::theme::Theme as BusTheme;

use super::{Msg, Shell};

impl Shell {
    /// Parse a raw bus message and dispatch to the matching handler.
    /// Returns a `Task` if any handler schedules async work (e.g. toast expiry).
    /// Unknown topics are silently ignored.
    pub fn handle_bus(&mut self, message: &sola_bus::Message) -> Task<Msg> {
        let Some(topic) = Topic::parse(message) else {
            return Task::none();
        };
        match topic {
            Topic::Theme(t) => { self.on_theme(t); Task::none() }
            Topic::OutputGeometry(g) => { self.on_output_geometry(g); Task::none() }
            Topic::Windows(w) => { self.on_windows(w); Task::none() }
            Topic::SetAppMenu(m) => { self.on_set_app_menu(m); Task::none() }
            Topic::Application(a) => { self.on_application(a); Task::none() }
            Topic::Chord(c) => { self.on_chord(c); Task::none() }
            Topic::ChordReleased(c) => { self.on_chord_released(c); Task::none() }
            Topic::MouseEntered(e) => { self.on_mouse_entered(e); Task::none() }
            Topic::MouseClicked(e) => { self.on_mouse_clicked(e); Task::none() }
            Topic::MouseLeft => { self.on_mouse_left(); Task::none() }
            Topic::LaunchResult(r) => self.on_launch_result(r),
            Topic::UserAppExited(e) => self.on_user_app_exited(e),
            Topic::Zones(z) => { self.on_zones(z); Task::none() }
            // All other topics (mail, terminal, monitor, etc.) are not consumed
            // by sola-shell; ignore them quietly.
            _ => Task::none(),
        }
    }

    // -------------------------------------------------------------------------
    // Real handlers
    // -------------------------------------------------------------------------

    /// Apply an updated bus theme to the iced renderer.
    fn on_theme(&mut self, t: BusTheme) {
        self.theme = sola_kit::theme::from_bus_theme(&t);
    }

    /// Store output geometry so windows can be positioned.
    fn on_output_geometry(&mut self, g: OutputGeometry) {
        self.output_size = Some((g.width, g.height));
    }

    /// Receive the full window list from sola-river.
    /// Rebuilds the window registry, derives focus changes, and updates the
    /// MRU list. The menubar reflects the new focused app implicitly via
    /// `view()` re-rendering after state mutation.
    fn on_windows(&mut self, windows: Vec<Window>) {
        // Collect old and new app_id sets to detect additions and removals.
        let old_app_ids: std::collections::HashSet<String> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != "sola-shell")
            .map(|w| w.app_id.clone())
            .collect();
        let new_app_ids: std::collections::HashSet<String> = windows
            .iter()
            .filter(|w| w.app_id != "sola-shell")
            .map(|w| w.app_id.clone())
            .collect();

        let added: Vec<String> = new_app_ids
            .iter()
            .filter(|id| !old_app_ids.contains(id.as_str()))
            .cloned()
            .collect();
        let removed: Vec<String> = old_app_ids
            .iter()
            .filter(|id| !new_app_ids.contains(id.as_str()))
            .cloned()
            .collect();

        // Rebuild lookup map.
        self.window_id_by_key.clear();
        for w in &windows {
            self.window_id_by_key
                .insert((w.app_id.clone(), w.title.clone()), w.window_id);
        }
        self.known_windows = windows;

        // Track removed apps: clean up MRU and focused state.
        let focused_app_was_removed = self
            .focused_app_id
            .as_deref()
            .map(|f| removed.iter().any(|r| r == f))
            .unwrap_or(false);
        for id in &removed {
            self.mru_apps.retain(|m| m != id);
            self.mru_window_by_app.remove(id);
            if self.focused_app_id.as_deref() == Some(id.as_str()) {
                self.focused_app_id = None;
                self.focused_window_id = None;
            }
        }

        // Focus the newest app if one appeared.
        let prev_focused = self.focused_app_id.clone();
        if let Some(id) = added.first() {
            self.set_focus(id);
            if let Some(wid) = self.lookup_any_window_id(id) {
                self.focused_window_id = Some(wid);
                self.mru_window_by_app.insert(id.clone(), wid);
            }
        } else if focused_app_was_removed {
            // Focused app closed — fall back to next MRU, or clear.
            if let Some(next) = self.mru_apps.first().cloned() {
                let wid = self
                    .mru_window_by_app
                    .get(&next)
                    .copied()
                    .or_else(|| self.lookup_any_window_id(&next));
                self.set_focus(&next);
                if let Some(wid) = wid {
                    self.focused_window_id = Some(wid);
                    self.mru_window_by_app.insert(next.clone(), wid);
                }
            }
            // If mru_apps is empty, focused_app_id is already None — the
            // menubar view will render an empty title.
        }

        // Dismiss open menu if the focused app changed.
        if self.menu_open && self.focused_app_id != prev_focused {
            self.menu_open = false;
            self.current_open_index = None;
            // TODO Task 10: emit Topic::Composition to hide the menu surface.
        }
    }

    /// Receive an app's menu definition (keyed sticky per app_id).
    fn on_set_app_menu(&mut self, m: AppMenuPayload) {
        self.menus.set_menu(m);
    }

    /// Update focus state and MRU ordering for the given app_id.
    /// Does not emit bus events — in the iced port, the menubar view re-renders
    /// automatically when `focused_app_id` changes.
    fn set_focus(&mut self, app_id: &str) {
        self.focused_app_id = Some(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());
    }

    /// Look up any window_id for an app_id (first match in known_windows).
    fn lookup_any_window_id(&self, app_id: &str) -> Option<u32> {
        self.known_windows
            .iter()
            .find(|w| w.app_id == app_id)
            .map(|w| w.window_id)
    }

    // -------------------------------------------------------------------------
    // Handlers with task return (toast scheduling)
    // -------------------------------------------------------------------------

    /// Receive the result of a Topic::LaunchApp request.
    /// On failure: surface a toast in the menubar and schedule its expiry.
    fn on_launch_result(&mut self, r: LaunchResultPayload) -> Task<Msg> {
        if r.ok {
            return Task::none();
        }
        let msg = format!(
            "Failed to launch {}: {}",
            r.app_id,
            r.error.as_deref().unwrap_or("unknown error")
        );
        self.menubar.push_toast(msg);
        let toast_gen = self.menubar.toast_generation;
        Task::perform(
            tokio::time::sleep(Duration::from_secs(5)),
            move |_| Msg::ToastExpire(toast_gen),
        )
    }

    /// A user app process exited.
    fn on_user_app_exited(&mut self, e: UserAppExitedPayload) -> Task<Msg> {
        let msg = if let Some(sig) = e.signal {
            format!("{} killed (signal {})", e.app_id, sig)
        } else {
            let code = e.code.unwrap_or(0);
            if code != 0 {
                format!("{} exited (code {})", e.app_id, code)
            } else {
                return Task::none();
            }
        };
        self.menubar.push_toast(msg);
        let toast_gen = self.menubar.toast_generation;
        Task::perform(
            tokio::time::sleep(Duration::from_secs(5)),
            move |_| Msg::ToastExpire(toast_gen),
        )
    }

    // -------------------------------------------------------------------------
    // Stub handlers — bodies filled in by later tasks
    // -------------------------------------------------------------------------

    /// Receive a user-defined application entry from the bus.
    /// Extends the application catalog; if the launcher is active, re-runs
    /// the filter so new entries appear immediately.
    fn on_application(&mut self, a: Application) {
        // add() is a no-op if the app_id already exists; use update() to
        // replace an existing entry (e.g. second Topic::Application replay).
        if self.applications.get(&a.app_id).is_some() {
            let _ = self.applications.update(&a.app_id.clone(), a);
        } else {
            let _ = self.applications.add(a);
        }
        // Re-filter if the launcher is currently visible.
        if self.launcher.active {
            let apps = self.applications.clone();
            let query = self.launcher.query.clone();
            self.launcher.apply_query(&apps, &query);
        }
    }

    /// Receive a chord event (key press).
    /// TODO Task 10: dispatch to launcher toggle, switcher cycle, etc.
    fn on_chord(&mut self, _c: ChordEvent) {}

    /// Receive a chord-released event (key release).
    /// TODO Task 10: confirm switcher selection on Super_L release.
    fn on_chord_released(&mut self, _c: ChordEvent) {}

    /// Cursor entered a window surface.
    /// TODO Task 10: start focus-hover timer (pending_focus_generation).
    fn on_mouse_entered(&mut self, _e: MouseEnteredPayload) {}

    /// Mouse button pressed on a window surface.
    /// If a menu is open and the click lands on a non-shell window, close it.
    fn on_mouse_clicked(&mut self, e: MouseClickedPayload) {
        if !self.menu_open {
            return;
        }
        // known_windows contains only non-sola-shell windows (sola-river omits
        // the shell's own surfaces from Topic::Windows).  Any click on a tracked
        // window_id means the user clicked outside the shell — dismiss the menu.
        let is_app_window = self
            .known_windows
            .iter()
            .any(|w| w.window_id == e.window_id);
        if is_app_window {
            self.menu_open = false;
            self.current_open_index = None;
            // TODO Task 10: emit Topic::Composition to hide the menu surface.
        }
    }

    /// Cursor left all tracked surfaces.
    /// TODO Task 10: cancel pending focus-hover timer.
    fn on_mouse_left(&mut self) {}

    /// Receive the current zone-assignment map.
    /// TODO Task 10: update self.zoning and re-emit Topic::Composition.
    fn on_zones(&mut self, _z: std::collections::HashMap<String, sola_bus::topics::Zone>) {}
}
