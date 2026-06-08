//! Bus topic dispatch for the iced shell.
//!
//! `Shell::handle_bus` parses an incoming `sola_bus::Message` into a typed
//! `Topic` and routes it to a per-topic method.

use std::collections::HashSet;
use std::time::Duration;

use iced::Task;
use sola_bus::topics::{
    AppMenuPayload, Application, ChordEvent, FocusTarget, LaunchResultPayload,
    MouseClickedPayload, MouseEnteredPayload, OutputGeometry, Topic,
    UserAppExitedPayload, Window,
};
use sola_core::theme::Theme as BusTheme;

use crate::keys;

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
            Topic::Chord(c) => self.on_chord(c),
            Topic::ChordReleased(c) => self.on_chord_released(c),
            Topic::MouseEntered(e) => self.on_mouse_entered(e),
            Topic::MouseClicked(e) => { self.on_mouse_clicked(e); Task::none() }
            Topic::MouseLeft => self.on_mouse_left(),
            Topic::LaunchResult(r) => self.on_launch_result(r),
            Topic::UserAppExited(e) => self.on_user_app_exited(e),
            Topic::Zones(z) => { self.on_zones(z); Task::none() }
            // All other topics are not consumed by sola-shell; ignore quietly.
            _ => Task::none(),
        }
    }

    // -------------------------------------------------------------------------
    // Real handlers
    // -------------------------------------------------------------------------

    /// Apply an updated bus theme to the iced renderer.
    fn on_theme(&mut self, t: BusTheme) {
        self.theme = sola_kit::theme::theme_from_bus(&t);
        self.style = sola_kit::theme::shell_style_from_bus_theme(&t);
        sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(&t));
    }

    /// Store output geometry, emit frames for all windows, and emit composition.
    fn on_output_geometry(&mut self, g: OutputGeometry) {
        self.output_size = Some((g.width, g.height));
        self.zoning.set_output_size(&g);
        self.emit_all_frames();
        self.emit_composition();
    }

    /// Receive the full window list from sola-river.
    /// Rebuilds the window registry, derives focus changes, emits composition
    /// and focus updates, applies zone config to newly-appeared windows.
    fn on_windows(&mut self, windows: Vec<Window>) {
        tracing::info!(count = windows.len(), "shell received Windows");

        // Collect old and new app_id sets (excluding the shell itself) to
        // detect additions and removals.
        let old_app_ids: HashSet<String> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
            .collect();
        let new_app_ids: HashSet<String> = windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
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

        // Collect removed window IDs from the OLD known_windows before we overwrite it.
        // Used to purge per-window config_applied entries from ZoningState.
        let removed_wids: Vec<u32> = self
            .known_windows
            .iter()
            .filter(|w| removed.iter().any(|r| r == &w.app_id))
            .map(|w| w.window_id)
            .collect();

        // Rebuild lookup map (includes shell surfaces so emit_composition works).
        self.window_id_by_key.clear();
        for w in &windows {
            self.window_id_by_key
                .insert((w.app_id.clone(), w.title.clone()), w.window_id);
        }
        self.known_windows = windows;

        // Track removed apps: clean up MRU, zoning, and focused state.
        let focused_app_was_removed = self
            .focused_app_id
            .as_deref()
            .map(|f| removed.iter().any(|r| r == f))
            .unwrap_or(false);
        self.zoning.forget_windows(&removed_wids);

        for id in &removed {
            self.mru_apps.retain(|m| m != id);
            self.mru_window_by_app.remove(id);
            if self.focused_app_id.as_deref() == Some(id.as_str()) {
                self.focused_app_id = None;
                self.focused_window_id = None;
            }
        }

        // Clean up window-level zone tracking for windows that no longer exist.
        let current_wids: HashSet<u32> =
            self.known_windows.iter().map(|w| w.window_id).collect();
        let orphaned_wids: Vec<u32> = self
            .zoning
            .window_zones
            .keys()
            .copied()
            .filter(|wid| !current_wids.contains(wid))
            .collect();
        self.zoning.forget_windows(&orphaned_wids);
        self.zoning
            .window_zones
            .retain(|wid, _| current_wids.contains(wid));

        // For newly appeared apps, apply their saved zone config to the first
        // window. Subsequent windows of the same app are left unzoned.
        let mut zone_frames = Vec::new();
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if let Some(frame) = self.zoning.apply_config_zone(&w.app_id, w.window_id) {
                zone_frames.push(frame);
            }
        }
        if !zone_frames.is_empty() {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                for f in zone_frames {
                    let _ = bus.emit(Topic::Frame(f));
                }
            }
        }

        // Emit frames for menubar and all explicitly-zoned windows.
        self.emit_all_frames();

        // Re-derive the switcher app list whenever windows change while active.
        if self.switcher.active {
            crate::switcher::state::rebuild_apps(
                &mut self.switcher,
                &self.mru_apps.clone(),
                &self.known_windows.clone(),
            );
        }

        // Focus the newest app so the user can start using it immediately.
        // If no new app appeared but the focused app was just closed, fall
        // back to the next MRU app — or clear the menubar if none remain.
        let prev_focused = self.focused_window_id;
        if let Some(id) = added.first() {
            self.bus_set_focus(id);
            if let Some(wid) = self.lookup_any_window_id(id) {
                self.focused_window_id = Some(wid);
                self.mru_window_by_app.insert(id.clone(), wid);
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                }
            }
        } else if focused_app_was_removed {
            if let Some(next) = self.mru_apps.first().cloned() {
                self.bus_set_focus(&next);
                let wid = self
                    .mru_window_by_app
                    .get(&next)
                    .copied()
                    .or_else(|| self.lookup_any_window_id(&next));
                if let Some(wid) = wid {
                    self.focused_window_id = Some(wid);
                    self.mru_window_by_app.insert(next.clone(), wid);
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                    }
                }
            } else {
                // No apps left — no focused app, re-emit chords without per-app bindings.
                self.emit_registered_chords();
            }
        }

        // Dismiss open menu if the focused window changed.
        if self.menu_open && self.focused_window_id != prev_focused {
            self.menu_open = false;
            self.current_open_index = None;
        }

        self.emit_composition();

        // Always re-emit registered chords. At fresh boot, the shell sees
        // only its own four windows (added/removed are empty after the
        // app_id filter), so the early-return paths that normally call
        // `emit_registered_chords` never fire — and sola-river never
        // learns about Meta+Space / Meta+Tab / Meta+Q / Meta+Grave /
        // Meta+Numpad{…}, so no keyboard chord ever reaches the shell.
        self.emit_registered_chords();
    }

    /// Receive an app's menu definition. Re-emit registered chords since the
    /// chord set may have changed (new shortcuts added by the focused app).
    fn on_set_app_menu(&mut self, m: AppMenuPayload) {
        let focused_is_this = self.focused_app_id.as_deref() == Some(&m.app_id);
        self.menus.set_menu(m);
        // Always re-emit in case this is the focused app (per-app shortcuts changed).
        self.emit_registered_chords();
        let _ = focused_is_this; // used implicitly via emit_registered_chords
    }

    /// Update focus state, zoning, and MRU ordering for the given app_id.
    /// Also emits registered chords if the focused app changed (per-app chord
    /// set may have changed).
    pub fn bus_set_focus(&mut self, app_id: &str) {
        let app_changed = self.focused_app_id.as_deref() != Some(app_id);
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());

        // Close any open menu on focus change.
        if self.menu_open && app_changed {
            self.menu_open = false;
            self.current_open_index = None;
        }

        // Per-app chord registrations change when the focused app changes.
        if app_changed {
            self.emit_registered_chords();
        }
    }

    /// Look up any window_id for an app_id (first match in known_windows).
    pub fn lookup_any_window_id(&self, app_id: &str) -> Option<u32> {
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
    ///
    /// Toasts on signal kills or non-zero exit codes. Clean exits (code 0)
    /// are silent — the legacy shell toasted on all exits but that was noisy
    /// for apps that self-close (e.g. a settings dialog that writes its config
    /// and exits). This divergence is intentional.
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
    // Application catalog
    // -------------------------------------------------------------------------

    /// Receive a user-defined application entry from the bus.
    /// Extends the application catalog; if the launcher is active, re-runs
    /// the filter so new entries appear immediately.
    fn on_application(&mut self, a: Application) {
        if self.applications.get(&a.app_id).is_some() {
            let _ = self.applications.update(&a.app_id.clone(), a);
        } else {
            let _ = self.applications.add(a);
        }
        if self.launcher.active {
            let apps = self.applications.clone();
            let query = self.launcher.query.clone();
            self.launcher.apply_query(&apps, &query);
        }
    }

    // -------------------------------------------------------------------------
    // Chord dispatch
    // -------------------------------------------------------------------------

    /// Dispatch a chord event through the shell's action table.
    fn on_chord(&mut self, evt: ChordEvent) -> Task<Msg> {
        let Some(chord) = keys::from_chord_event(&evt) else {
            tracing::debug!(
                keysym = evt.keysym,
                modifiers = evt.modifiers,
                "unrecognized chord"
            );
            return Task::none();
        };

        tracing::debug!(
            keycode = chord.keycode.raw(),
            meta = chord.meta,
            ctrl = chord.ctrl,
            alt = chord.alt,
            shift = chord.shift,
            "chord fired"
        );

        let bare = !chord.meta && !chord.ctrl && !chord.alt && !chord.shift;

        // Escape dismisses whichever shell overlay is up. Only registered
        // while one is active (see `emit_registered_chords`), so we don't
        // steal Escape from terminal apps otherwise.
        if chord.keycode == sola_core::KeyCode::ESCAPE && bare {
            if self.launcher.active {
                return Task::done(Msg::CloseLauncher);
            }
            if self.menu_open {
                return Task::done(Msg::CloseMenu);
            }
            if self.switcher.active {
                return Task::done(Msg::SwitcherCancel);
            }
        }

        // Launcher is modal — it owns the keyboard while active, so eat
        // every other chord. (Switcher has its own navigation branch below.)
        if self.launcher.active {
            return Task::none();
        }

        // A dropdown menu is transient — any non-Escape chord should
        // dismiss it and then proceed normally (so Meta+Space still
        // opens the launcher, Meta+Tab still opens the switcher, etc.
        // even if the user left a menu hanging open).
        if self.menu_open {
            self.menu_open = false;
            self.current_open_index = None;
            self.emit_composition();
            self.emit_registered_chords();
        }

        // Switcher active: Tab/Right cycles forward; Left cycles backward;
        // Meta+Q closes the *selected* app (not the front one); Meta
        // release (handled in on_chord_released) confirms.
        if self.switcher.active {
            use sola_core::KeyCode;
            if chord.keycode == KeyCode::TAB || chord.keycode == KeyCode::RIGHT {
                return Task::done(Msg::SwitcherNav { next: true });
            }
            if chord.keycode == KeyCode::LEFT {
                return Task::done(Msg::SwitcherNav { next: false });
            }
            if chord.meta && chord.keycode == KeyCode::Q {
                if let Some(target) = self.switcher.apps.get(self.switcher.selected).cloned() {
                    tracing::info!(
                        app_id = %target.app_id,
                        "Meta+Q in switcher — close selected app"
                    );
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::CloseApp(target.app_id.clone()));
                    }
                    // Optimistic remove — Topic::Windows from sola-river
                    // will reconcile, but pulling the entry now keeps the
                    // switcher visually in step with the keypress. Clamp
                    // the selection so it still points at a real app;
                    // dismiss the switcher entirely if we just killed the
                    // last one.
                    self.switcher.apps.retain(|a| a.app_id != target.app_id);
                    if self.switcher.apps.is_empty() {
                        return Task::done(Msg::SwitcherCancel);
                    }
                    if self.switcher.selected >= self.switcher.apps.len() {
                        self.switcher.selected = self.switcher.apps.len() - 1;
                    }
                    self.emit_composition();
                }
                return Task::none();
            }
        }

        // Meta+Space: toggle launcher.
        if chord.meta && chord.keycode == sola_core::KeyCode::SPACE {
            tracing::info!(
                launcher_active = self.launcher.active,
                "Meta+Space — toggling launcher"
            );
            if self.launcher.active {
                return Task::done(Msg::CloseLauncher);
            } else {
                return Task::done(Msg::OpenLauncher);
            }
        }

        // Meta+Q: close focused app.
        if chord.meta && chord.keycode == sola_core::KeyCode::Q {
            tracing::info!("Meta+Q — close focused app");
            if let Some(ref focused) = self.focused_app_id.clone() {
                if let Ok(mut bus) = sola_kit::app::bus().lock() {
                    let _ = bus.emit(Topic::CloseApp(focused.clone()));
                }
            }
            return Task::none();
        }

        // Meta+`: cycle windows of the focused app.
        if chord.meta && chord.keycode == sola_core::KeyCode::GRAVE {
            tracing::info!("Meta+` — cycle app windows");
            return Task::done(Msg::CycleAppWindows);
        }

        // Meta+Tab: activate or cycle switcher.
        if chord.meta && chord.keycode == sola_core::KeyCode::TAB {
            if self.launcher.active {
                // Close launcher first, then open switcher.
                self.launcher.active = false;
                self.emit_composition();
                self.emit_registered_chords();
            }
            if self.switcher.active {
                // Already active: cycle forward.
                return Task::done(Msg::SwitcherNav { next: true });
            }
            tracing::info!("Meta+Tab — activating switcher");
            crate::switcher::state::rebuild_apps(
                &mut self.switcher,
                &self.mru_apps.clone(),
                &self.known_windows.clone(),
            );
            self.switcher.active = true;
            // Start at index 1 so the second (next) app is pre-selected on
            // first press — mirrors the legacy macOS-style Alt+Tab feel.
            self.switcher.selected = if self.switcher.apps.len() > 1 { 1 } else { 0 };
            self.emit_registered_chords();
            self.emit_composition();
            return Task::none();
        }

        // Zone snapping (Meta+Numpad).
        if let Some(frame) = self.zoning.handle_key(chord.keycode.raw(), self.focused_window_id) {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                let _ = bus.emit(Topic::Frame(frame));
                if let Some(zones) = self.zoning.take_zones_update() {
                    let _ = bus.emit(Topic::Zones(zones));
                }
            }
            return Task::none();
        }

        // Shell system shortcuts (e.g. Exit Sola from the shell's own menu).
        if let Some(action) = self.menus.lookup_shortcut(&chord, Self::APP_ID) {
            tracing::info!(action_id = %action.action_id, "shell shortcut");
            return Task::done(Msg::MenuAction {
                app_id: action.app_id,
                action_id: action.action_id,
            });
        }

        // Focused app menu shortcut lookup.
        if let Some(focused) = self.focused_app_id.clone() {
            if let Some(action) = self.menus.lookup_shortcut(&chord, &focused) {
                tracing::info!(
                    app_id = %action.app_id,
                    action_id = %action.action_id,
                    "menu shortcut matched"
                );
                return Task::done(Msg::MenuAction {
                    app_id: action.app_id,
                    action_id: action.action_id,
                });
            }
        }

        Task::none()
    }

    /// Handle a chord-released event.
    /// Super_L release while the switcher is active confirms the selection.
    fn on_chord_released(&mut self, evt: ChordEvent) -> Task<Msg> {
        if evt.keysym == keys::KEYSYM_SUPER_L && evt.modifiers == 0 && self.switcher.active {
            return Task::done(Msg::SwitcherConfirm);
        }
        Task::none()
    }

    // -------------------------------------------------------------------------
    // Mouse handlers
    // -------------------------------------------------------------------------

    /// Cursor entered a window surface.
    /// Starts a 500 ms focus-hover timer; if the cursor stays, raise that window.
    fn on_mouse_entered(&mut self, e: MouseEnteredPayload) -> Task<Msg> {
        // Skip shell surfaces — hovering the menubar must not steal focus.
        let is_shell = self
            .known_windows
            .iter()
            .any(|w| w.window_id == e.window_id && w.app_id == Self::APP_ID);
        if is_shell {
            return Task::none();
        }

        // Bump generation to cancel any previous pending fire.
        self.pending_focus_generation = self.pending_focus_generation.wrapping_add(1);
        let focus_gen = self.pending_focus_generation;
        let wid = e.window_id;

        Task::perform(
            tokio::time::sleep(Duration::from_millis(500)),
            move |_| Msg::FocusHoverFire { window_id: wid, generation: focus_gen },
        )
    }

    /// Mouse button pressed on a window surface.
    /// If a menu is open and the click lands on a non-shell window, close it.
    fn on_mouse_clicked(&mut self, e: MouseClickedPayload) {
        if !self.menu_open {
            return;
        }
        // Dismiss the menu only when the user clicks a non-shell window.
        // known_windows includes shell surfaces (sola-river reports them in
        // Topic::Windows), so we must exclude clicks on our own surfaces —
        // otherwise clicking a menubar label fires both OpenMenu AND this
        // dismiss handler, racing the open.
        let clicked_shell = self
            .known_windows
            .iter()
            .any(|w| w.window_id == e.window_id && w.app_id == Self::APP_ID);
        if clicked_shell {
            // Click is on a shell surface (menubar, menu overlay, etc.) —
            // let the normal OpenMenu / CloseMenu messages handle it.
            return;
        }
        let is_app_window = self
            .known_windows
            .iter()
            .any(|w| w.window_id == e.window_id);
        if is_app_window {
            self.menu_open = false;
            self.current_open_index = None;
            self.emit_composition();
            self.emit_registered_chords();
        }
    }

    /// Cursor left all tracked surfaces.
    /// Cancels any pending focus-hover timer by bumping the generation counter.
    fn on_mouse_left(&mut self) -> Task<Msg> {
        self.pending_focus_generation = self.pending_focus_generation.wrapping_add(1);
        Task::none()
    }

    // -------------------------------------------------------------------------
    // Zone snapshot
    // -------------------------------------------------------------------------

    /// Receive the current zone-assignment map from sola-session sticky replay.
    /// Seeds `zoning.app_zone_config` so that apps launched after this have
    /// their saved zones applied on first appearance.
    fn on_zones(&mut self, zones: std::collections::HashMap<String, sola_bus::topics::Zone>) {
        tracing::info!(count = zones.len(), "zones updated");
        self.zoning.set_zones(zones.clone());
        // Re-apply config zones to any windows already known.
        let windows: Vec<(String, u32)> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| (w.app_id.clone(), w.window_id))
            .collect();
        if !windows.is_empty() {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                for (app_id, wid) in windows {
                    if let Some(frame) = self.zoning.apply_config_zone(&app_id, wid) {
                        let _ = bus.emit(Topic::Frame(frame));
                    }
                }
            }
        }
    }
}
