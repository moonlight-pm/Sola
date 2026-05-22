//! Bus topic dispatch for the iced shell.
//!
//! `Shell::handle_bus` parses an incoming `sola_bus::Message` into a typed
//! `Topic` and routes it to a per-topic method. Only `on_theme` and
//! `on_output_geometry` have real bodies in this task; all others are stubs
//! that will be filled in as each window comes online (Tasks 5–10).

use sola_bus::topics::{
    AppMenuPayload, Application, ChordEvent, LaunchResultPayload, MouseClickedPayload,
    MouseEnteredPayload, OutputGeometry, Topic, UserAppExitedPayload, Window,
};
use sola_core::theme::Theme as BusTheme;

use super::Shell;

impl Shell {
    /// Parse a raw bus message and dispatch to the matching handler.
    /// Unknown topics are silently ignored — the shell only subscribes to
    /// the set it cares about, but the bus may deliver others during a
    /// reconnect replay.
    pub fn handle_bus(&mut self, message: &sola_bus::Message) {
        let Some(topic) = Topic::parse(message) else {
            return;
        };
        match topic {
            Topic::Theme(t) => self.on_theme(t),
            Topic::OutputGeometry(g) => self.on_output_geometry(g),
            Topic::Windows(w) => self.on_windows(w),
            Topic::SetAppMenu(m) => self.on_set_app_menu(m),
            Topic::Application(a) => self.on_application(a),
            Topic::Chord(c) => self.on_chord(c),
            Topic::ChordReleased(c) => self.on_chord_released(c),
            Topic::MouseEntered(e) => self.on_mouse_entered(e),
            Topic::MouseClicked(e) => self.on_mouse_clicked(e),
            Topic::MouseLeft => self.on_mouse_left(),
            Topic::LaunchResult(r) => self.on_launch_result(r),
            Topic::UserAppExited(e) => self.on_user_app_exited(e),
            Topic::Zones(z) => self.on_zones(z),
            // All other topics (mail, terminal, monitor, etc.) are not consumed
            // by sola-shell; ignore them quietly.
            _ => {}
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
    ///
    /// TODO Task 5+: emit `Topic::Frame` to reposition windows after geometry
    /// changes (e.g. on monitor hotplug). For now just record the size.
    fn on_output_geometry(&mut self, g: OutputGeometry) {
        // OutputGeometry fields are i32 (matching the Wayland wire format).
        self.output_size = Some((g.width, g.height));
    }

    // -------------------------------------------------------------------------
    // Stub handlers — bodies filled in by later tasks
    // -------------------------------------------------------------------------

    /// Receive the full window list from sola-river.
    /// TODO Task 5: wire menubar update; Task 7: dismiss menu on focus change.
    fn on_windows(&mut self, _w: Vec<Window>) {
        // TODO Task 5 (menubar): update focused-app display.
        // TODO Task 7 (menu):    close menu on focus change.
    }

    /// Receive an app's menu definition (keyed sticky per app_id).
    /// TODO Task 5: insert into MenuCache and refresh menubar.
    fn on_set_app_menu(&mut self, _m: AppMenuPayload) {
        // TODO Task 5 (menubar): self.menus.insert(...); push to menubar window.
    }

    /// Receive a user-defined application entry from the bus.
    /// TODO Task 10: append/update self.applications and refresh launcher.
    fn on_application(&mut self, _a: Application) {
        // TODO Task 10 (chord/bus): update self.applications catalog.
    }

    /// Receive a chord event (key press).
    /// TODO Task 10: dispatch to launcher toggle, switcher cycle, etc.
    fn on_chord(&mut self, _c: ChordEvent) {
        // TODO Task 10 (chord wiring): Meta+Space → launcher; Super → switcher.
    }

    /// Receive a chord-released event (key release).
    /// TODO Task 10: confirm switcher selection on Super_L release.
    fn on_chord_released(&mut self, _c: ChordEvent) {
        // TODO Task 10 (chord wiring): Super_L release → switcher confirm.
    }

    /// Cursor entered a window surface.
    /// TODO Task 10: start focus-hover timer (pending_focus_generation).
    fn on_mouse_entered(&mut self, _e: MouseEnteredPayload) {
        // TODO Task 10 (chord/bus): schedule focus-follows-mouse timer.
    }

    /// Mouse button pressed on a window surface.
    /// TODO Task 7: dismiss open menu on click outside shell.
    fn on_mouse_clicked(&mut self, _e: MouseClickedPayload) {
        // TODO Task 7 (menu): close menu on outside click.
    }

    /// Cursor left all tracked surfaces.
    /// TODO Task 10: cancel pending focus-hover timer.
    fn on_mouse_left(&mut self) {
        // TODO Task 10 (chord/bus): cancel focus-hover timer (increment generation).
    }

    /// Receive the result of a Topic::LaunchApp request.
    /// TODO Task 8: on failure, surface error in launcher.
    fn on_launch_result(&mut self, _r: LaunchResultPayload) {
        // TODO Task 8 (launcher): surface launch errors.
    }

    /// A user app process exited.
    /// TODO Task 10: remove from known_windows / MRU if still present.
    fn on_user_app_exited(&mut self, _e: UserAppExitedPayload) {
        // TODO Task 10 (chord/bus): clean up window state on exit.
    }

    /// Receive the current zone-assignment map.
    /// TODO Task 10: update self.zoning and re-emit Topic::Composition.
    fn on_zones(&mut self, _z: std::collections::HashMap<String, sola_bus::topics::Zone>) {
        // TODO Task 10 (chord/bus): apply zone assignments, recompose.
    }
}
