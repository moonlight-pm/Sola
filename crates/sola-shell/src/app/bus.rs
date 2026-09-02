//! Bus topic dispatch for the iced shell.
//!
//! `Shell::handle_bus` parses an incoming `sola_bus::Message` into a typed
//! `Topic` and routes it to a per-topic method.

use std::collections::HashSet;
use std::time::Duration;

use iced::Task;
use sola_bus::topics::{
    AppHidden, AppMenuPayload, AppNotification, AppToast, Application, ChordEvent, FloatGeometry,
    FocusTarget, LaunchResultPayload, MailStatus, MouseClickedPayload, MouseEnteredPayload,
    OutputGeometry, Topic, UserAppExitedPayload, Window, WindowFloating, WindowGeometry,
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
            Topic::Theme(t) => {
                self.on_theme(t);
                Task::none()
            }
            Topic::OutputGeometry(g) => {
                self.on_output_geometry(g);
                Task::none()
            }
            Topic::Windows(w) => {
                self.on_windows(w);
                self.resolve_pending_launch_if_window();
                Task::none()
            }
            Topic::SetAppMenu(m) => {
                self.on_set_app_menu(m);
                Task::none()
            }
            Topic::Application(a) => {
                self.on_application(a, message.sticky);
                Task::none()
            }
            Topic::AppHidden(h) => {
                self.on_app_hidden(h, message.sticky);
                Task::none()
            }
            Topic::Chord(c) => self.on_chord(c),
            Topic::ChordReleased(c) => self.on_chord_released(c),
            Topic::MouseEntered(e) => self.on_mouse_entered(e),
            Topic::MouseClicked(e) => {
                self.on_mouse_clicked(e);
                Task::none()
            }
            Topic::MouseLeft => self.on_mouse_left(),
            Topic::LaunchResult(r) => self.on_launch_result(r),
            Topic::UserAppExited(e) => self.on_user_app_exited(e),
            Topic::Zones(z) => {
                self.on_zones(z);
                Task::none()
            }
            Topic::WindowGeometry(g) => {
                self.on_window_geometry(g);
                Task::none()
            }
            Topic::FloatGeometry(f) => {
                self.on_float_geometry(f);
                Task::none()
            }
            // External links. Live sola-browser already subscribed to
            // OpenUrl and will open a tab. Only spawn if chrome is down
            // (otherwise we used to start a second process that reaped
            // the live CEF helpers). When chrome is up and the request
            // wants activation, raise the existing window to the top.
            Topic::OpenUrl(req) => {
                if sola_core::sola_browser_is_running() {
                    if req.activate {
                        tracing::info!(url = %req.url, "OpenUrl: chrome live, raising");
                        self.raise_app("sola-browser");
                    } else {
                        tracing::debug!(url = %req.url, "OpenUrl: chrome live, not spawning");
                    }
                } else {
                    sola_core::open_url_logged(&req.url);
                }
                Task::none()
            }
            Topic::AppToast(t) => self.on_app_toast(t),
            Topic::AppNotification(n) => self.on_app_notification(n),
            Topic::MailStatus(s) => {
                self.on_mail_status(s, message.sticky);
                Task::none()
            }
            // All other topics are not consumed by sola-shell; ignore quietly.
            _ => Task::none(),
        }
    }

    // -------------------------------------------------------------------------
    // Real handlers
    // -------------------------------------------------------------------------

    /// Menubar whisper (Opening… / screenshot). Not a notification.
    fn on_app_toast(&mut self, t: AppToast) -> Task<Msg> {
        let text = t.text.trim();
        if text.is_empty() {
            return Task::none();
        }
        self.menubar.push_toast(text.to_string());
        let toast_gen = self.menubar.toast_generation;
        Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
            Msg::ToastExpire(toast_gen)
        })
    }

    fn on_app_notification(&mut self, n: AppNotification) -> Task<Msg> {
        if n.title.trim().is_empty() && n.body.trim().is_empty() {
            return Task::none();
        }
        self.push_notification(n)
    }

    /// Apply an updated bus theme to the iced renderer.
    fn on_theme(&mut self, t: BusTheme) {
        self.theme = sola_kit::theme::theme_from_bus(&t);
        self.style = sola_kit::theme::shell_style_from_bus_theme(&t);
        sola_kit::fonts::install(sola_kit::theme::fonts_from_bus_theme(&t));
        sola_kit::theme::install_selection(sola_kit::theme::atoms_from_bus_theme(&t).selection);
    }

    /// Store output geometry, emit frames for all windows, and emit composition.
    ///
    /// Also re-emits registered chords. After a sola-bus restart the sticky
    /// chord map is empty; river re-publishes `OutputGeometry` on reconnect
    /// (and sticky replay hits this path on shell re-subscribe), so this is
    /// the natural place to restore shell grabs without a dedicated
    /// reconnect topic.
    fn on_output_geometry(&mut self, g: OutputGeometry) {
        self.output_size = Some((g.width, g.height));
        self.zoning.set_output_size(&g);
        self.emit_all_frames();
        self.emit_composition();
        self.emit_registered_chords();
    }

    /// Receive the full window list from sola-river.
    /// Rebuilds the window registry, derives focus changes, emits composition
    /// and focus updates, applies zone config to newly-appeared windows.
    ///
    /// Title-only updates (same `(window_id, app_id)` set) only refresh the
    /// lookup map — they used to re-emit every zone Frame, restack composition,
    /// and re-register chords, which made focus-follows-mouse feel like a
    /// double re-paint on chatty clients (e.g. Electron titles).
    fn on_windows(&mut self, windows: Vec<Window>) {
        // River can keep `closed`-less entries after a hard kill. Drop
        // those here so composition does not target a dead menubar id.
        let windows: Vec<Window> = windows
            .into_iter()
            .filter(|w| match w.pid {
                None => true,
                Some(pid) => std::path::Path::new("/proc").join(pid.to_string()).is_dir(),
            })
            .collect();
        tracing::info!(count = windows.len(), "shell received Windows");

        // Same surfaces/apps, possibly new titles — skip the heavy path.
        if Self::windows_identity_eq(&self.known_windows, &windows) {
            self.window_id_by_key.clear();
            for w in &windows {
                self.window_id_by_key
                    .insert((w.app_id.clone(), w.title.clone()), w.window_id);
            }
            self.known_windows = windows;
            tracing::debug!(
                count = self.known_windows.len(),
                "Windows title-only — skip frames/composition/chords"
            );
            return;
        }

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
            // Last surface gone — drop AppHidden so a later map of this
            // app_id is not stuck hidden (Super+H / Arcade).
            if self.is_app_hidden(id) {
                self.retract_app_hidden(id);
            }
        }

        // Clean up window-level zone tracking for windows that no longer exist.
        let current_wids: HashSet<u32> = self.known_windows.iter().map(|w| w.window_id).collect();
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

        // Per non-shell window: restore a saved zone if any; otherwise
        // default-float (client-requested size + WindowFloating for CSD).
        // gamescope is framed like any other app (zone/float host size).
        let mut zone_frames = Vec::new();
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if let Some(frame) = self.zoning.apply_config_zone(&w.app_id, w.window_id) {
                zone_frames.push(frame);
            } else if let Some(frame) = self.zoning.ensure_default_float(&w.app_id, w.window_id) {
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

        // Tell sola-river / kit apps which windows are floating (move/resize + CSD).
        self.sync_window_floating();

        // Emit frames for menubar overlays and explicitly zoned (non-float) windows.
        self.emit_all_frames();

        // Re-derive the switcher app list whenever windows change while active.
        if self.switcher.active {
            crate::switcher::state::rebuild_apps(
                &mut self.switcher,
                &self.mru_apps.clone(),
                &self.known_windows.clone(),
            );
        }

        // Ensure every known non-shell app is tracked in mru_apps at least at
        // the least-recent end. Without this, apps only pointer-focused (never
        // click-raised) used to live outside MRU and were composition-stacked
        // *above* every raised window — Helium/external apps looked "stuck"
        // on top. New maps still raise via bus_set_focus below.
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if !self.mru_apps.iter().any(|m| m == &w.app_id) {
                self.mru_apps.push(w.app_id.clone());
            }
        }

        // Raise + provisionally focus the newest app so it appears on top and
        // is usable when the pointer is already over it (or nowhere). Then
        // re-sync keyboard focus to the window under the pointer — if the
        // cursor is still over another app, FFM wins and the new map keeps
        // its raise without stealing input.
        //
        // If no new app appeared but the focused app was just closed, fall
        // back to the next MRU app — or clear the menubar if none remain —
        // then the same pointer resync applies.
        //
        // Screenshot cold-launch of sola-preview sets `suppress_map_focus_for`
        // so we raise the window in MRU/composition without taking the seat
        // (keyboard stays on the app that was focused when the chord fired).
        let prev_focused = self.focused_window_id;
        if let Some(id) = added.first() {
            let suppress = self
                .suppress_map_focus_for
                .as_deref()
                .is_some_and(|s| s == id.as_str());
            if suppress {
                self.suppress_map_focus_for = None;
                // Still put the new app at the front of MRU so it is raised,
                // but leave keyboard focus alone (and re-assert return focus).
                self.mru_apps.retain(|m| m != id);
                self.mru_apps.insert(0, id.clone());
                if let Some(wid) = self.lookup_any_window_id(id) {
                    self.mru_window_by_app.insert(id.clone(), wid);
                }
                let keep = self.screenshot_return_focus;
                self.restore_app_focus(keep);
            } else {
                self.bus_set_focus(id);
                if let Some(wid) = self.lookup_any_window_id(id) {
                    self.focused_window_id = Some(wid);
                    self.mru_window_by_app.insert(id.clone(), wid);
                    if let Ok(mut bus) = sola_kit::app::bus().lock() {
                        let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
                    }
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

        // Drop hover if that window closed; then re-apply FFM over any
        // programmatic focus steal above.
        if let Some(wid) = self.pointer_window_id {
            if !self.known_windows.iter().any(|w| w.window_id == wid) {
                self.pointer_window_id = None;
            }
        }
        if added.first().is_some() || focused_app_was_removed {
            self.sync_keyboard_focus_to_pointer();
        }

        // Dismiss open menu if the focused window changed.
        if self.focused_window_id != prev_focused && self.dismiss_open_menu() {
            self.emit_overlay_frames();
        }

        self.emit_composition();

        // Always re-emit registered chords. At fresh boot, the shell sees
        // only the menubar (added/removed are empty after the
        // app_id filter), so the early-return paths that normally call
        // `emit_registered_chords` never fire — and sola-river never
        // learns about Meta+Space / Meta+Tab / Meta+Q / Meta+H / Meta+Grave /
        // Meta+Numpad{…}, so no keyboard chord ever reaches the shell.
        self.emit_registered_chords();
    }

    /// True when both lists describe the same set of surfaces (window_id +
    /// app_id), ignoring title and order. Used to short-circuit title-only
    /// `Topic::Windows` storms.
    fn windows_identity_eq(a: &[Window], b: &[Window]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut ka: Vec<(u32, &str)> = a.iter().map(|w| (w.window_id, w.app_id.as_str())).collect();
        let mut kb: Vec<(u32, &str)> = b.iter().map(|w| (w.window_id, w.app_id.as_str())).collect();
        ka.sort_unstable();
        kb.sort_unstable();
        ka == kb
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

    /// Update keyboard/menubar focus for `app_id` and bump it to the front of
    /// the app MRU list (used by composition stacking + Super+Tab).
    ///
    /// Prefer [`Self::set_pointer_focus`] for focus-follows-mouse — that path
    /// must not raise windows.
    pub fn bus_set_focus(&mut self, app_id: &str) {
        self.apply_focus(app_id, true);
    }

    /// Focus-follows-mouse: keyboard/menubar focus only — no MRU bump, no raise.
    pub fn set_pointer_focus(&mut self, app_id: &str) {
        self.apply_focus(app_id, false);
    }

    /// Shared focus bookkeeping. When `bump_mru` is true the app moves to the
    /// front of the stack (raise on next `emit_composition`); when false only
    /// input focus / menubar / chords follow.
    ///
    /// Pointer focus still **registers** the app at the least-recent end of
    /// `mru_apps` if it is missing — that keeps Super+Tab complete and stops
    /// never-raised external windows from living in the "not in MRU" bucket.
    /// It never moves an already-listed app forward (that is raise-only).
    fn apply_focus(&mut self, app_id: &str, bump_mru: bool) {
        // Composition-hidden apps are not on screen; focusing them would
        // route keys to a River-hidden surface. Unhide first (raise_app).
        if self.is_app_hidden(app_id) {
            tracing::debug!(%app_id, "apply_focus: skip hidden app");
            return;
        }
        let app_changed = self.focused_app_id.as_deref() != Some(app_id);
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        if bump_mru {
            self.mru_apps.retain(|m| m != app_id);
            self.mru_apps.insert(0, app_id.to_string());
        } else if !self.mru_apps.iter().any(|m| m == app_id) {
            // Track without raising — least-recent = bottom of stack.
            self.mru_apps.push(app_id.to_string());
        }

        // Close any open menu on focus change (clears chip highlight too).
        if app_changed && self.dismiss_open_menu() {
            self.emit_overlay_frames();
            self.emit_composition();
        }

        // Per-app chord registrations change when the focused app changes.
        if app_changed {
            self.emit_registered_chords();
        }
    }

    /// Resolve a non-shell window_id → app_id, if known.
    fn app_id_for_window(&self, window_id: u32) -> Option<String> {
        self.known_windows
            .iter()
            .find(|w| w.window_id == window_id && w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
    }

    /// Emit `Topic::Focus` so sola-river routes keyboard/pointer to `window_id`.
    fn emit_focus(&self, window_id: u32, prev_focused: Option<u32>) {
        tracing::debug!(window_id, ?prev_focused, "emit Focus");
        super::with_bus(|bus| {
            let _ = bus.emit(Topic::Focus(FocusTarget { window_id }));
        });
    }

    /// Pointer focus (no raise, no stack change). Used after the FFM dwell
    /// timer and by map/close resync.
    pub(crate) fn focus_window_from_pointer(&mut self, window_id: u32) {
        if self.focused_window_id == Some(window_id) {
            tracing::debug!(window_id, "pointer focus no-op (already focused)");
            return;
        }
        let Some(app_id) = self.app_id_for_window(window_id) else {
            tracing::debug!(window_id, "pointer focus skipped (unknown window)");
            return;
        };
        let prev_window = self.focused_window_id;
        tracing::debug!(
            window_id,
            %app_id,
            ?prev_window,
            prev_app = ?self.focused_app_id,
            "pointer focus (FFM)"
        );
        self.set_pointer_focus(&app_id);
        self.focused_window_id = Some(window_id);
        self.emit_focus(window_id, prev_window);
    }

    /// Re-apply keyboard focus to whatever is under the pointer.
    ///
    /// General fix for focus-follows-mouse after any non-pointer focus change:
    /// a newly mapped window (or MRU fallback after close) steals focus, but
    /// if the cursor never left another app, River will not re-fire
    /// `MouseEntered`. Restore input focus to the hovered window without
    /// undoing the raise (stack order stays).
    fn sync_keyboard_focus_to_pointer(&mut self) {
        let Some(wid) = self.pointer_window_id else {
            return;
        };
        // Drop stale hover if the window vanished.
        if !self.known_windows.iter().any(|w| w.window_id == wid) {
            self.pointer_window_id = None;
            return;
        }
        // Menubar / shell overlays never take keyboard focus via FFM.
        if self
            .known_windows
            .iter()
            .any(|w| w.window_id == wid && w.app_id == Self::APP_ID)
        {
            return;
        }
        self.focus_window_from_pointer(wid);
    }

    /// Raise `app_id` as if the user clicked it (MRU + composition + seat).
    /// Unhides a Super+H / AppHidden app first. No-op when that app has no
    /// mapped window yet.
    pub(crate) fn raise_app(&mut self, app_id: &str) {
        if self.is_app_hidden(app_id) {
            self.unhide_app(app_id);
            return;
        }
        let Some(window_id) = self.lookup_any_window_id(app_id).or_else(|| {
            self.known_windows
                .iter()
                .find(|w| w.app_id.eq_ignore_ascii_case(app_id))
                .map(|w| w.window_id)
        }) else {
            tracing::debug!(%app_id, "raise_app: no mapped window");
            return;
        };
        self.raise_window_from_click(window_id);
    }

    /// Click activation: focus + raise to front of the composition stack.
    fn raise_window_from_click(&mut self, window_id: u32) {
        let Some(app_id) = self.app_id_for_window(window_id) else {
            return;
        };
        // Cancel any dwell timer — click is authoritative for focus.
        self.pending_focus_generation = self.pending_focus_generation.wrapping_add(1);
        let prev_focused = self.focused_window_id;
        self.bus_set_focus(&app_id);
        self.focused_window_id = Some(window_id);
        self.mru_window_by_app.insert(app_id, window_id);
        self.emit_focus(window_id, prev_focused);
        // Always re-`place_top`. If the app was already MRU-front the
        // order is unchanged and `emit_composition` would skip — River
        // can still have another surface (same zone) visually on top.
        self.last_composition.clear();
        self.emit_composition();
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
    /// On failure: drop any opening toast and surface a failure toast.
    /// On success: leave the opening toast until a matching window appears
    /// (or the opening timeout fires).
    fn on_launch_result(&mut self, r: LaunchResultPayload) -> Task<Msg> {
        if r.ok {
            return Task::none();
        }
        let _ = self.take_pending_for_app(&r.app_id);
        let msg = format!(
            "Failed to launch {}: {}",
            r.app_id,
            r.error.as_deref().unwrap_or("unknown error")
        );
        self.menubar.push_toast(msg);
        let toast_gen = self.menubar.toast_generation;
        Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
            Msg::ToastExpire(toast_gen)
        })
    }

    /// A user app process exited.
    ///
    /// Toasts on signal kills or non-zero exit codes. Clean exits (code 0)
    /// are silent — the legacy shell toasted on all exits but that was noisy
    /// for apps that self-close (e.g. a settings dialog that writes its config
    /// and exits). This divergence is intentional.
    ///
    /// If we were still showing "Opening …" for this app, clear that pending
    /// state either way so a hung toast does not outlive a dead process.
    fn on_mail_status(&mut self, s: MailStatus, sticky: bool) {
        if sticky {
            self.inbox_unread = Some(s.inbox_unread);
        } else {
            self.inbox_unread = None;
        }
    }

    fn on_user_app_exited(&mut self, e: UserAppExitedPayload) -> Task<Msg> {
        if e.app_id.eq_ignore_ascii_case("sola-mail") {
            self.inbox_unread = None;
        }
        let pending = self.take_pending_for_app(&e.app_id);
        let msg = if let Some(sig) = e.signal {
            format!("{} killed (signal {})", e.app_id, sig)
        } else {
            let code = e.code.unwrap_or(0);
            if code != 0 {
                format!("{} exited (code {})", e.app_id, code)
            } else {
                // Clean exit: if we were mid-open, drop the opening toast so
                // it does not sit until the 20s timeout with no window.
                if let Some(p) = pending {
                    self.menubar.expire_toast(p.toast_generation);
                }
                return Task::none();
            }
        };
        self.menubar.push_toast(msg);
        let toast_gen = self.menubar.toast_generation;
        Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
            Msg::ToastExpire(toast_gen)
        })
    }

    /// Screenshot capture finished: Fast PNG is on the compositor clipboard
    /// (or the call failed). No file, no Preview.
    pub(crate) fn on_screenshot_done(&mut self, result: Result<(), String>) -> Task<Msg> {
        let return_focus = self.screenshot_return_focus.take();
        let msg = match result {
            Ok(()) => "Screenshot copied".to_string(),
            Err(e) => format!("Screenshot failed: {e}"),
        };
        self.menubar.push_toast(msg);
        let toast_gen = self.menubar.toast_generation;
        if let Some(wid) = return_focus {
            self.restore_app_focus(Some(wid));
        }
        Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
            Msg::ToastExpire(toast_gen)
        })
    }

    // -------------------------------------------------------------------------
    // Application catalog
    // -------------------------------------------------------------------------

    /// Receive a user-defined application entry from the bus.
    /// Extends the application catalog; if the launcher is active, re-runs
    /// the filter so new entries appear immediately.
    ///
    /// `sticky=false` is a retract (settings remove, Arcade nest label
    /// clear) — drop the catalog entry rather than re-adding an empty one.
    fn on_application(&mut self, a: Application, sticky: bool) {
        if !sticky {
            tracing::info!(app_id = %a.app_id, "Application retract");
            self.applications.remove(&a.app_id);
        } else if self.applications.get(&a.app_id).is_some() {
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

    /// Hide or show an app's surfaces in composition (River hide/show).
    /// Sticky emit → hide; sticky=false retract → show. Case-insensitive
    /// match on window app_id when filtering composition.
    fn on_app_hidden(&mut self, h: AppHidden, sticky: bool) {
        let key = h.app_id.to_ascii_lowercase();
        if sticky {
            tracing::info!(app_id = %h.app_id, "AppHidden: hide");
            self.hidden_apps.insert(key, h.app_id);
        } else {
            tracing::info!(app_id = %h.app_id, "AppHidden: show (retract)");
            self.hidden_apps.remove(&key);
        }
        self.emit_composition();
    }

    // -------------------------------------------------------------------------
    // Chord dispatch
    // -------------------------------------------------------------------------

    /// Dispatch a chord event through the shell's action table.
    fn on_chord(&mut self, evt: ChordEvent) -> Task<Msg> {
        // Global media keys never map to a shell KeyChord — they're handled
        // out-of-process (MPRIS / wpctl via `solactl media`). Recognise them
        // first, before the KeyChord decode below rejects them as
        // "unrecognized", and run them regardless of overlay/focus state.
        if let Some(action) = keys::media_action(evt.keysym) {
            crate::media::trigger(action);
            return Task::none();
        }

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
            if self.selection.active || self.selection.pending {
                return Task::done(Msg::CloseSelection);
            }
            if self.launcher.active {
                return Task::done(Msg::CloseLauncher);
            }
            if self.shortcuts.active {
                return Task::done(Msg::CloseShortcuts);
            }
            if self.menu_open {
                return Task::done(Msg::CloseMenu);
            }
            if self.switcher.active {
                return Task::done(Msg::SwitcherCancel);
            }
        }

        // Selection marquee is modal — Escape cancels; ignore other chords.
        // Pending freeze is not modal (overlay is not up yet) but it owns the
        // screenshot slot, so ignore competing Super+Shift+3/4/5.
        if self.selection.active {
            return Task::none();
        }
        if self.selection.pending
            && chord.meta
            && chord.shift
            && matches!(
                chord.keycode,
                sola_core::KeyCode::KEY_3 | sola_core::KeyCode::KEY_4 | sola_core::KeyCode::KEY_5
            )
        {
            return Task::none();
        }

        // Super+K: keyboard-shortcuts overlay (Omarchy). Toggle even while
        // the launcher is up so the cheatsheet is always one chord away.
        if chord.meta
            && !chord.shift
            && !chord.ctrl
            && !chord.alt
            && chord.keycode == sola_core::KeyCode::K
        {
            if self.shortcuts.active {
                return Task::done(Msg::CloseShortcuts);
            }
            return Task::done(Msg::OpenShortcuts);
        }

        // Launcher / shortcuts are modal — they own the keyboard while
        // active, so eat every other chord. (Switcher has its own
        // navigation branch below.)
        if self.launcher.active || self.shortcuts.active {
            return Task::none();
        }

        // A dropdown menu is transient — any non-Escape chord should
        // dismiss it and then proceed normally (so Meta+Space still
        // opens the launcher, Meta+Tab still opens the switcher, etc.
        // even if the user left a menu hanging open).
        // Screenshot chords are the exception: they must copy the live
        // scene (open notifications panel, text selections) before chrome
        // moves.
        let screenshot = chord.meta
            && chord.shift
            && !chord.ctrl
            && !chord.alt
            && matches!(
                chord.keycode,
                sola_core::KeyCode::KEY_3 | sola_core::KeyCode::KEY_4 | sola_core::KeyCode::KEY_5
            );
        if self.menu_open && !screenshot {
            let _ = self.dismiss_open_menu();
            self.emit_overlay_frames();
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
            // Super+H while the switcher is up would hide the focused app
            // then Super-release would confirm and unhide it. Eat the chord.
            if chord.meta && chord.keycode == KeyCode::H {
                return Task::none();
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

        // Meta+H: hide focused app (omit from composition; River hide).
        if chord.meta
            && !chord.shift
            && !chord.ctrl
            && !chord.alt
            && chord.keycode == sola_core::KeyCode::H
        {
            self.hide_focused_app();
            return Task::none();
        }

        // Super+Shift+3: full-output screenshot (auto path) → toast + preview.
        if chord.meta
            && chord.shift
            && !chord.ctrl
            && !chord.alt
            && chord.keycode == sola_core::KeyCode::KEY_3
        {
            tracing::info!("Super+Shift+3 — full-output screenshot");
            self.arm_screenshot_handoff();
            return crate::screenshot::full();
        }

        // Super+Shift+4: freeze live output (menus still composed), then
        // selection marquee on that still.
        if chord.meta
            && chord.shift
            && !chord.ctrl
            && !chord.alt
            && chord.keycode == sola_core::KeyCode::KEY_4
        {
            tracing::info!("Super+Shift+4 — selection freeze");
            return Task::done(Msg::OpenSelection);
        }

        // Super+Shift+5: focused-window region screenshot → toast + preview.
        if chord.meta
            && chord.shift
            && !chord.ctrl
            && !chord.alt
            && chord.keycode == sola_core::KeyCode::KEY_5
        {
            tracing::info!("Super+Shift+5 — focused-window screenshot");
            let Some(app_id) = self.focused_app_id.clone() else {
                self.menubar
                    .push_toast("Screenshot failed: no focused window");
                let toast_gen = self.menubar.toast_generation;
                return Task::perform(tokio::time::sleep(Duration::from_secs(5)), move |_| {
                    Msg::ToastExpire(toast_gen)
                });
            };
            let title = self.focused_window_id.and_then(|wid| {
                self.known_windows
                    .iter()
                    .find(|w| w.window_id == wid)
                    .map(|w| w.title.clone())
            });
            self.arm_screenshot_handoff();
            return crate::screenshot::window(app_id, title);
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
            // Focus is applied in `CommitOverlayShow` once the iced swapchain
            // is live — focusing the parked 2×2 can unhide it.
            self.emit_registered_chords();
            self.emit_composition();
            return Task::none();
        }

        // Zone snapping (Meta+Numpad).
        if let Some(zone) = crate::zoning::zone_for_keycode(chord.keycode.raw()) {
            return self.snap_focused_zone(zone);
        }

        // Shell system menu shortcuts (Quit Sola has none — Super+Q is CloseApp).
        if let Some(action) = self.menus.lookup_shortcut(&chord, Self::APP_ID) {
            tracing::info!(action_id = %action.action_id, "shell shortcut");
            let flash = self.flash_menu_action(&action.app_id, &action.action_id);
            return Task::batch([
                flash,
                Task::done(Msg::MenuAction {
                    app_id: action.app_id,
                    action_id: action.action_id,
                }),
            ]);
        }

        // Focused app menu shortcut lookup.
        if let Some(focused) = self.focused_app_id.clone() {
            if let Some(action) = self.menus.lookup_shortcut(&chord, &focused) {
                tracing::info!(
                    app_id = %action.app_id,
                    action_id = %action.action_id,
                    "menu shortcut matched"
                );
                let flash = self.flash_menu_action(&action.app_id, &action.action_id);
                return Task::batch([
                    flash,
                    Task::done(Msg::MenuAction {
                        app_id: action.app_id,
                        action_id: action.action_id,
                    }),
                ]);
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

    /// Focus-follows-mouse dwell before keyboard focus moves (no raise).
    /// Long enough to cross a background app toward the menubar; short enough
    /// that intentional hover still feels responsive.
    const FOCUS_HOVER_DELAY: Duration = Duration::from_millis(200);

    /// Cursor entered a window surface.
    ///
    /// Focus-follows-mouse: after [`Self::FOCUS_HOVER_DELAY`], keyboard focus
    /// moves to the window under the pointer — **without** raising. Raising is
    /// click-only (see [`Self::on_mouse_clicked`]). The delay is a grace period
    /// for the path from a floating app up to the menubar across another window.
    fn on_mouse_entered(&mut self, e: MouseEnteredPayload) -> Task<Msg> {
        // Always remember hover (including shell) so map/close resync knows
        // whether the pointer is on an app vs chrome vs nowhere.
        self.pointer_window_id = Some(e.window_id);

        // Cancel any in-flight dwell (new target or shell chrome).
        self.pending_focus_generation = self.pending_focus_generation.wrapping_add(1);

        // Shell surfaces never take FFM focus — and entering the menubar
        // cancels a pending steal from an intermediate window, so the floater
        // you left keeps its menus.
        let is_shell = self
            .known_windows
            .iter()
            .any(|w| w.window_id == e.window_id && w.app_id == Self::APP_ID);
        if is_shell {
            tracing::debug!(window_id = e.window_id, "FFM enter shell — cancel dwell");
            return Task::none();
        }

        // Composition-hidden surfaces are River-hidden; ignore stray enter.
        if self
            .app_id_for_window(e.window_id)
            .is_some_and(|id| self.is_app_hidden(&id))
        {
            return Task::none();
        }

        // Already focused here — nothing to schedule.
        if self.focused_window_id == Some(e.window_id) {
            tracing::debug!(window_id = e.window_id, "FFM enter already focused");
            return Task::none();
        }

        let focus_gen = self.pending_focus_generation;
        let wid = e.window_id;
        let app = self.app_id_for_window(wid);
        tracing::debug!(
            window_id = wid,
            app_id = app.as_deref().unwrap_or("?"),
            generation = focus_gen,
            delay_ms = Self::FOCUS_HOVER_DELAY.as_millis() as u64,
            "FFM enter — schedule dwell"
        );
        Task::perform(tokio::time::sleep(Self::FOCUS_HOVER_DELAY), move |_| {
            Msg::FocusHoverFire {
                window_id: wid,
                generation: focus_gen,
            }
        })
    }

    /// Mouse button pressed on a window surface.
    ///
    /// Any click on an app window raises it to the front (and focuses it).
    /// Also dismisses an open menubar dropdown when the click is outside shell.
    fn on_mouse_clicked(&mut self, e: MouseClickedPayload) {
        // known_windows includes shell surfaces (sola-river reports them in
        // Topic::Windows), so we must exclude clicks on our own surfaces —
        // otherwise clicking a menubar label races OpenMenu with dismiss/raise.
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
        if !is_app_window {
            return;
        }

        // Outside-click dismiss for the menubar dropdown (before raise so
        // composition includes both the closed menu and the raised app).
        // Re-emit chords so Escape is unregistered once the overlay is gone.
        let dismissed_menu = self.dismiss_open_menu();

        self.raise_window_from_click(e.window_id);

        if dismissed_menu {
            // raise may already have re-emitted chords on app change; always
            // re-emit here so Escape drops even when focus stays put.
            self.emit_overlay_frames();
            self.emit_registered_chords();
        }
    }

    /// Cursor left all tracked surfaces.
    /// Leave keyboard focus where it is (classic sloppy-focus leave policy)
    /// and cancel any pending dwell so a transit gap does not fire late.
    fn on_mouse_left(&mut self) -> Task<Msg> {
        self.pointer_window_id = None;
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
        let mut zone_frames = Vec::new();
        for (app_id, wid) in windows {
            if let Some(frame) = self.zoning.apply_config_zone(&app_id, wid) {
                zone_frames.push(frame);
            } else if let Some(frame) = self.zoning.ensure_default_float(&app_id, wid) {
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
        self.sync_window_floating();
    }

    /// A window moved or resized (Topic::WindowGeometry from sola-river). If the
    /// window is currently floating, persist its rectangle per app_id so the
    /// float restores there on relaunch. Non-floating windows are ignored.
    fn on_window_geometry(&mut self, g: WindowGeometry) {
        // Track every window's live rect so an explicit float can inset from
        // where the window currently sits (see ZoningState::current_rect).
        self.zoning.live_geometry.insert(g.window_id, g.clone());
        let Some(app_id) = self
            .known_windows
            .iter()
            .find(|w| w.window_id == g.window_id)
            .map(|w| w.app_id.clone())
        else {
            return;
        };
        if self
            .zoning
            .note_window_geometry(&app_id, g.window_id, g.x, g.y, g.width, g.height)
        {
            if let Ok(mut bus) = sola_kit::app::bus().lock() {
                let _ = bus.emit(Topic::FloatGeometry(
                    self.zoning.float_geometry[&app_id].clone(),
                ));
            }
        }
    }

    /// Cache a floating app's remembered rectangle from Topic::FloatGeometry
    /// (persistent replay at startup, or our own echo after recording).
    fn on_float_geometry(&mut self, f: FloatGeometry) {
        self.zoning.float_geometry.insert(f.app_id.clone(), f);
    }

    /// Publish `Topic::WindowFloating` for any window whose float state changed
    /// since the last call, so sola-river can gate interactive move/resize on
    /// the window under the pointer. Called after every handler that can change
    /// a window's zone (float key, window appearance, zone-map replay).
    pub(super) fn sync_window_floating(&mut self) {
        let ids: Vec<u32> = self.known_windows.iter().map(|w| w.window_id).collect();
        let changes = self.zoning.take_floating_changes(&ids);
        if changes.is_empty() {
            return;
        }
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            for (window_id, floating) in changes {
                let _ = bus.emit(Topic::WindowFloating(WindowFloating {
                    window_id,
                    floating,
                }));
            }
        }
    }
}

#[cfg(test)]
mod windows_identity_tests {
    use super::Shell;
    use sola_bus::topics::Window;

    fn win(id: u32, app: &str, title: &str) -> Window {
        Window {
            window_id: id,
            app_id: app.into(),
            title: title.into(),
            pid: None,
        }
    }

    #[test]
    fn identity_eq_ignores_title_and_order() {
        let a = vec![win(1, "orca", "A"), win(2, "Helium", "x")];
        let b = vec![win(2, "Helium", "y"), win(1, "orca", "B")];
        assert!(Shell::windows_identity_eq(&a, &b));
    }

    #[test]
    fn identity_neq_on_new_window_or_app_id() {
        let a = vec![win(1, "orca", "A")];
        assert!(!Shell::windows_identity_eq(
            &a,
            &[win(1, "orca", "A"), win(2, "zed", "")]
        ));
        assert!(!Shell::windows_identity_eq(&a, &[win(1, "gamescope", "A")]));
        assert!(!Shell::windows_identity_eq(&a, &[]));
    }
}
