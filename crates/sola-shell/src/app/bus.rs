//! Bus topic dispatch for the iced shell.
//!
//! `Shell::handle_bus` parses an incoming `sola_bus::Message` into a typed
//! `Topic` and routes it to a per-topic method.

use std::collections::HashSet;
use std::time::Duration;

use iced::Task;
use sola_bus::topics::{
    AppHidden, AppMenuPayload, AppNotification, AppToast, Application, ChordEvent, FloatGeometry,
    FocusTarget, LaunchResultPayload, MailStatus, MenuActionPayload, MouseClickedPayload,
    MouseEnteredPayload,
    OutputGeometry, TabLayout, Topic, UserAppExitedPayload, Window, WindowChrome, WindowFloating,
    WindowGeometry, WindowParent, Zone,
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
            Topic::WindowParent(p) => {
                self.on_window_parent(p, message.sticky);
                Task::none()
            }
            Topic::TabLayout(l) => {
                self.on_tab_layout(l);
                Task::none()
            }
            Topic::Tab(t) => {
                self.on_tab(t, message.sticky);
                Task::none()
            }
            Topic::TabActivity(a) => {
                if message.sticky {
                    self.tab_activity.insert(a.key, a.activity);
                } else {
                    self.tab_activity.remove(&a.key);
                }
                Task::none()
            }
            Topic::ShowTab(s) => {
                match self.pairing.window_of(&s.key) {
                    Some(wid) => self.activate_window(wid),
                    None => tracing::debug!(key = %s.key, "ShowTab: no window"),
                }
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
            // the live CEF helpers). Each page is a window (Next): the new
            // page's window opens in front on its own, so there is nothing
            // to raise (raising the app would pull an older page forward).
            Topic::OpenUrl(req) => {
                if sola_core::sola_browser_is_running() {
                    tracing::debug!(url = %req.url, "OpenUrl: chrome live, it opens a window");
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
            Topic::MenuAction(a) => self.on_titlebar_menu_action(a),
            // All other topics are not consumed by sola-shell; ignore quietly.
            _ => Task::none(),
        }
    }

    // -------------------------------------------------------------------------
    // Real handlers
    // -------------------------------------------------------------------------

    /// A Window-menu choice from an app's titlebar (kit `menu::choose`). The
    /// click focused that app's window, so act on the focused window as the
    /// shortcut would. Other actions are the app's own and never reach here.
    fn on_titlebar_menu_action(&mut self, a: MenuActionPayload) -> Task<Msg> {
        let Some(action) = sola_kit::parse_window_action(&a.action_id) else {
            return Task::none();
        };
        if self.focused_app_id.as_deref() != Some(a.app_id.as_str()) {
            tracing::warn!(
                app_id = %a.app_id,
                action_id = %a.action_id,
                focused = ?self.focused_app_id,
                "titlebar window action from an app without focus; ignored"
            );
            return Task::none();
        }
        self.handle_window_action(action)
    }

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

        let now = std::time::Instant::now();
        // A window is the same window only with the same id *and* app: a
        // restarted sola-river numbers windows from 1 again.
        let old: std::collections::HashMap<u32, &str> = self
            .known_windows
            .iter()
            .map(|w| (w.window_id, w.app_id.as_str()))
            .collect();
        let new: std::collections::HashMap<u32, &str> = windows
            .iter()
            .map(|w| (w.window_id, w.app_id.as_str()))
            .collect();
        let removed_wids: Vec<u32> = old
            .iter()
            .filter(|(id, app)| new.get(id) != Some(app))
            .map(|(id, _)| *id)
            .collect();
        let added: Vec<(u32, String)> = windows
            .iter()
            .filter(|w| {
                w.app_id != Self::APP_ID && old.get(&w.window_id) != Some(&w.app_id.as_str())
            })
            .map(|w| (w.window_id, w.app_id.clone()))
            .collect();

        // Apps whose last window closed (MRU / hidden cleanup).
        let new_app_ids: HashSet<&str> = windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| w.app_id.as_str())
            .collect();
        let removed_apps: Vec<String> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID && !new_app_ids.contains(w.app_id.as_str()))
            .map(|w| w.app_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Rebuild lookup map (includes shell surfaces so emit_composition works).
        self.window_id_by_key.clear();
        for w in &windows {
            self.window_id_by_key
                .insert((w.app_id.clone(), w.title.clone()), w.window_id);
        }
        self.known_windows = windows;

        // Closed tabs leave their zones; each zone whose shown tab closed
        // shows its next one. A place waits a moment for a restarting app.
        let focused_closed = self
            .focused_window_id
            .is_some_and(|w| removed_wids.contains(&w));
        let focused_zone = self.focused_window_id.and_then(|w| self.tabs.zone_of(w));
        for &wid in &removed_wids {
            self.tabs.remove(wid, now);
            self.pending_parents.remove(&wid);
            if let Some(key) = self.pairing.window_closed(wid) {
                retract_tab_window(key);
            }
            self.tab_shown_sent.remove(&wid);
        }
        self.zoning.forget_windows(&removed_wids);

        for id in &removed_apps {
            self.mru_apps.retain(|m| m != id);
            self.mru_window_by_app.remove(id);
            if self.focused_app_id.as_deref() == Some(id.as_str()) {
                self.focused_app_id = None;
            }
            // Last surface gone — drop AppHidden so a later map of this
            // app_id is not stuck hidden (Super+H / Arcade).
            if self.is_app_hidden(id) {
                self.retract_app_hidden(id);
            }
        }
        if focused_closed {
            self.focused_window_id = None;
        }

        // Place new windows. A dialog rides its parent; a window that was
        // up before this shell started, or whose app just restarted, takes
        // its remembered place; anything else joins the focused zone and
        // takes focus.
        let first_list = !self.windows_seen;
        self.windows_seen = true;
        // Tabs announced for these windows: placed windows may move to their
        // tab's place; new ones are placed by their tab's key below.
        if self.pair_tabs(now) {
            self.sync_floats();
        }
        let mut float_frames = Vec::new();
        let mut take_focus: Option<(u32, String)> = None;
        let mut first_shown: Option<(u32, String)> = None;
        let mut placed_any = false;
        for (wid, app_id) in &added {
            if let Some(parent) = self.pending_parents.remove(wid) {
                self.tabs.set_parent(*wid, parent);
                self.zoning.float(app_id, *wid, false);
                continue;
            }
            let floats = crate::zoning::floats_by_default(app_id) || self.zoning.remembers(app_id);
            let key = self.pairing.key_of(*wid).unwrap_or(app_id).to_string();
            // A background tab joins behind the shown one, like a returning one.
            let behind = first_list || self.pairing.background(*wid);
            let placed = self.tabs.place(*wid, app_id, &key, floats, behind, now);
            placed_any = true;
            tracing::info!(window_id = wid, %app_id, ?placed, "placed window");
            if placed.zone == Zone::Float {
                let remembers = placed.returning || self.zoning.remembers(app_id);
                float_frames.extend(self.zoning.float(app_id, *wid, remembers));
            }
            if !placed.returning {
                take_focus = Some((*wid, app_id.clone()));
            } else if placed.shown && placed.zone != Zone::Float && first_shown.is_none() {
                first_shown = Some((*wid, app_id.clone()));
            }
        }
        // Restored windows do not take focus — unless nothing has it.
        if take_focus.is_none() && self.focused_window_id.is_none() {
            take_focus = first_shown;
        }
        Self::emit_frames(float_frames);
        if placed_any {
            self.persist_tabs();
        }

        // Every known app is in MRU at least at the least-recent end, so
        // Super+Tab is complete.
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if !self.mru_apps.iter().any(|m| m == &w.app_id) {
                self.mru_apps.push(w.app_id.clone());
            }
        }

        // Focus: a new window takes it. Screenshot cold-launch of
        // sola-preview sets `suppress_map_focus_for` so the window shows
        // without taking the seat (keyboard stays on the app that was
        // focused when the chord fired). When the focused window closed,
        // its zone's next tab takes focus, else the most recent app.
        let prev_focused = self.focused_window_id;
        if let Some((wid, app_id)) = take_focus.clone() {
            let suppress = self
                .suppress_map_focus_for
                .as_deref()
                .is_some_and(|s| s == app_id);
            if suppress {
                self.suppress_map_focus_for = None;
                self.mru_apps.retain(|m| m != &app_id);
                self.mru_apps.insert(0, app_id.clone());
                self.mru_window_by_app.insert(app_id, wid);
                let keep = self.screenshot_return_focus;
                self.restore_app_focus(keep);
            } else {
                self.focus_window(wid, &app_id);
            }
        } else if focused_closed {
            let hidden = |app: &str| self.is_app_hidden(app);
            let next = focused_zone
                .filter(|z| *z != Zone::Float)
                .and_then(|z| self.tabs.visible_shown(z, &hidden))
                .or_else(|| self.mru_window_on_showing(None));
            let next_app = next.and_then(|w| self.app_id_for_window(w));
            match (next, next_app) {
                (Some(wid), Some(app_id)) => self.focus_window(wid, &app_id),
                _ => {
                    self.focused_app_id = None;
                    self.emit_registered_chords();
                }
            }
        }

        // A restored window can take its zone's shown place from under
        // the focused tab.
        self.keep_focus_on_screen();

        // Drop hover if that window closed; then re-apply FFM over any
        // programmatic focus steal above.
        if let Some(wid) = self.pointer_window_id {
            if !self.known_windows.iter().any(|w| w.window_id == wid) {
                self.pointer_window_id = None;
            }
        }
        if take_focus.is_some() || focused_closed {
            self.sync_keyboard_focus_to_pointer();
        }

        // Dismiss open menu if the focused window changed.
        if self.focused_window_id != prev_focused && self.dismiss_open_menu() {
            self.emit_overlay_frames();
        }

        // Tell sola-river / kit apps which windows are floating (move/resize + CSD).
        self.sync_window_floating();
        self.emit_all_frames();

        // Re-derive the switcher app list whenever windows change while active.
        if self.switcher.active {
            crate::switcher::state::rebuild_apps(
                &mut self.switcher,
                &self.mru_apps.clone(),
                &self.known_windows.clone(),
            );
        }

        self.emit_composition();

        // Always re-emit registered chords. At fresh boot, the shell sees
        // only the menubar, so no other path has emitted them yet — and
        // sola-river never learns about Meta+Space / Meta+Tab / Meta+Q /
        // Meta+H / Meta+Grave / Meta+Numpad{…}, so no keyboard chord ever
        // reaches the shell.
        self.emit_registered_chords();
    }

    /// Give `wid` keyboard focus and bump its app in MRU (no stack change).
    fn focus_window(&mut self, wid: u32, app_id: &str) {
        self.bus_set_focus(app_id);
        self.focused_window_id = Some(wid);
        self.tabs.focus(wid);
        self.mru_window_by_app.insert(app_id.to_string(), wid);
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::Focus(FocusTarget { window_id: wid }));
        }
    }

    /// River named a dialog's parent (or took it back). A dialog leaves the
    /// bar and rides above its parent.
    fn on_window_parent(&mut self, p: WindowParent, sticky: bool) {
        let app_id = self.app_id_for_window(p.window_id);
        if !sticky {
            // Closing, usually (sola-river retracts on close, before the new
            // Windows list). One left without a parent stays a float with
            // no row.
            self.pending_parents.remove(&p.window_id);
            if self.tabs.clear_parent(p.window_id) {
                self.emit_composition();
            }
            return;
        }
        let Some(app_id) = app_id else {
            // Before its Windows entry: placed as a dialog when it comes.
            self.pending_parents.insert(p.window_id, p.parent);
            return;
        };
        tracing::info!(window_id = p.window_id, parent = p.parent, "dialog");
        let was_tab = self.tabs.set_parent(p.window_id, p.parent);
        self.zoning.float(&app_id, p.window_id, false);
        self.sync_window_floating();
        if was_tab {
            self.emit_all_frames();
            self.persist_tabs();
        }
        self.emit_composition();
    }

    /// The saved tab layout (persistent replay). Taken once; later
    /// deliveries are our own echo. Windows already up are placed again so
    /// they take their saved places.
    fn on_tab_layout(&mut self, layout: TabLayout) {
        if self.tab_layout_loaded {
            return;
        }
        self.tab_layout_loaded = true;
        self.tab_layout_ready = true;
        let now = std::time::Instant::now();
        tracing::info!(zones = layout.zones.len(), "tab layout loaded");
        self.tabs.load(&layout, now);
        self.last_tab_layout = Some(layout);
        self.tabs.clear();
        let windows: Vec<(u32, String)> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID && self.tabs.parent_of(w.window_id).is_none())
            .map(|w| (w.window_id, w.app_id.clone()))
            .collect();
        for (wid, app_id) in windows {
            let floats = crate::zoning::floats_by_default(&app_id);
            let key = self.pairing.key_of(wid).unwrap_or(&app_id).to_string();
            let placed = self.tabs.place(wid, &app_id, &key, floats, true, now);
            if placed.zone == Zone::Float {
                Self::emit_frames(self.zoning.float(&app_id, wid, true).into_iter().collect());
            } else {
                self.zoning.unfloat(wid);
            }
        }
        self.keep_focus_on_screen();
        self.sync_window_floating();
        self.emit_all_frames();
        self.emit_composition();
    }

    /// An app announced a tab it is opening as a window (or retracted one
    /// it closed). Pair it with its window; a window that already has a
    /// place may move to the tab's saved place.
    fn on_tab(&mut self, tab: sola_bus::topics::Tab, sticky: bool) {
        if !sticky {
            if let Some(wid) = self.pairing.retract(&tab.key) {
                tracing::info!(window_id = wid, key = %tab.key, "tab closed");
                retract_tab_window(tab.key);
            }
            return;
        }
        self.pairing.announce(crate::pairing::Announced {
            key: tab.key,
            app_id: tab.app_id,
            pid: tab.pid,
            seq: tab.seq,
            window_id: tab.window_id,
            background: tab.background,
        });
        if self.pair_tabs(std::time::Instant::now()) {
            self.sync_floats();
            self.keep_focus_on_screen();
            self.emit_all_frames();
            self.emit_composition();
        }
        self.persist_tabs();
    }

    /// Pair announced tabs with their windows (see `pairing.rs`), tell the
    /// apps which window each tab is (`TabWindow`), and remember placed
    /// windows by their tab's key. True when one moved to a saved place.
    fn pair_tabs(&mut self, now: std::time::Instant) -> bool {
        let candidates: Vec<(u32, String, u32)> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID && self.tabs.parent_of(w.window_id).is_none())
            .filter_map(|w| Some((w.window_id, w.app_id.clone(), w.pid?)))
            .collect();
        let refs: Vec<crate::pairing::Candidate<'_>> = candidates
            .iter()
            .map(|(w, a, p)| (*w, a.as_str(), *p))
            .collect();
        let mut moved = false;
        for (window_id, key) in self.pairing.pair(&refs) {
            tracing::info!(window_id, %key, "tab paired");
            if self.tabs.set_key(window_id, &key, now).is_some() {
                moved = true;
            }
            let shown = self.last_composition.contains(&window_id);
            self.tab_shown_sent.insert(window_id, shown);
            super::with_bus(|bus| {
                let _ = bus.emit(Topic::TabWindow(sola_bus::topics::TabWindow {
                    key,
                    window_id,
                    shown,
                }));
            });
        }
        moved
    }

    /// Tell apps which of their tab windows went on or off screen (after a
    /// Composition): a browser rests the pages behind.
    pub(crate) fn sync_tab_shown(&mut self) {
        let changes: Vec<(u32, String, bool)> = self
            .pairing
            .pairs()
            .filter_map(|(wid, key)| {
                let shown = self.last_composition.contains(&wid);
                (self.tab_shown_sent.get(&wid) != Some(&shown)).then(|| (wid, key.to_string(), shown))
            })
            .collect();
        for (window_id, key, shown) in changes {
            self.tab_shown_sent.insert(window_id, shown);
            super::with_bus(|bus| {
                let _ = bus.emit(Topic::TabWindow(sola_bus::topics::TabWindow {
                    key,
                    window_id,
                    shown,
                }));
            });
        }
    }

    /// Bring floating in line with the bar after windows moved zones
    /// outside `move_tab` (a tab taking its saved place).
    fn sync_floats(&mut self) {
        let mut frames = Vec::new();
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID || self.tabs.parent_of(w.window_id).is_some() {
                continue;
            }
            let float = self.tabs.zone_of(w.window_id) == Some(Zone::Float);
            if float && !self.zoning.is_floating(w.window_id) {
                frames.extend(self.zoning.float(&w.app_id, w.window_id, true));
            } else if !float && self.zoning.is_floating(w.window_id) {
                self.zoning.unfloat(w.window_id);
            }
        }
        Self::emit_frames(frames);
        self.sync_window_floating();
    }

    /// Keyboard focus stays on something on screen: a focused tab that is
    /// now behind another hands focus to its zone's shown tab.
    fn keep_focus_on_screen(&mut self) {
        let Some(wid) = self.focused_window_id else {
            return;
        };
        let tab = self.tabs.parent_of(wid).unwrap_or(wid);
        let Some(zone) = self.tabs.zone_of(tab).filter(|z| *z != Zone::Float) else {
            return;
        };
        let hidden = |app: &str| self.is_app_hidden(app);
        let Some(shown) = self.tabs.visible_shown(zone, &hidden) else {
            return;
        };
        if shown == tab {
            return;
        }
        if let Some(app_id) = self.app_id_for_window(shown) {
            self.focus_window(shown, &app_id);
        }
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
        // An app that publishes menus draws its own frame: no compositor rim.
        self.sync_window_chrome();
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
        if bump_mru {
            self.mru_apps.retain(|m| m != app_id);
            self.mru_apps.insert(0, app_id.to_string());
            self.ack_notify_badge(app_id);
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
    pub(crate) fn app_id_for_window(&self, window_id: u32) -> Option<String> {
        self.known_windows
            .iter()
            .find(|w| w.window_id == window_id && w.app_id != Self::APP_ID)
            .map(|w| w.app_id.clone())
    }

    /// Emit `Topic::Focus` so sola-river routes keyboard/pointer to `window_id`.
    pub(crate) fn emit_focus(&self, window_id: u32, prev_focused: Option<u32>) {
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
        self.tabs.focus(window_id);
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
        let recent = self
            .mru_window_by_app
            .get(app_id)
            .copied()
            .filter(|w| self.known_windows.iter().any(|k| k.window_id == *w));
        let Some(window_id) = recent.or_else(|| self.lookup_any_window_id(app_id)).or_else(|| {
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

    /// Show `window_id` in its zone and focus it: a tab bar row, Super+`,
    /// the switcher. Unhides a Super+H'd app first.
    pub(crate) fn activate_window(&mut self, window_id: u32) {
        let Some(app_id) = self.app_id_for_window(window_id) else {
            return;
        };
        if self.is_app_hidden(&app_id) {
            self.retract_app_hidden(&app_id);
            self.emit_all_frames();
        }
        self.raise_window_from_click(window_id);
    }

    /// Click activation: show the window in its zone (raise, for a float),
    /// focus it, and re-stack.
    pub(crate) fn raise_window_from_click(&mut self, window_id: u32) {
        let Some(app_id) = self.app_id_for_window(window_id) else {
            return;
        };
        // Showing a window shows its workspace; remember where focus was
        // on the one being left.
        self.remember_workspace_focus();
        self.tabs.show(window_id);
        self.tabs.focus(window_id);
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
        self.persist_tabs();
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
        // A zone whose tabs are all hidden gives its width away.
        self.emit_all_frames();
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

        // Meta+` / Meta+Shift+`: next / previous tab in the focused zone.
        if chord.meta && chord.keycode == sola_core::KeyCode::GRAVE {
            return Task::done(Msg::CycleTabs { back: chord.shift });
        }

        // Workspaces: Meta+1…9 shows one, Meta+Ctrl+1…9 sends the focused
        // window there, Meta+Ctrl+(Shift+)Tab steps.
        if chord.meta && !chord.alt {
            if let Some(index) = crate::keys::workspace_index(chord.keycode) {
                if chord.ctrl && !chord.shift {
                    self.send_focused_to_workspace(index);
                    return Task::none();
                }
                if !chord.ctrl && !chord.shift {
                    self.show_workspace(index);
                    return Task::none();
                }
            }
            if chord.ctrl && chord.keycode == sola_core::KeyCode::TAB {
                self.step_workspace(chord.shift);
                return Task::none();
            }
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
            if let Some(id) = self.focused_app_id.clone() {
                self.ack_notify_badge(&id);
            }
            crate::switcher::state::rebuild_apps(
                &mut self.switcher,
                &self.mru_apps.clone(),
                &self.known_windows.clone(),
            );
            self.switcher.active = true;
            // Start at index 1 so the second (next) app is pre-selected on
            // first press — mirrors the legacy macOS-style Alt+Tab feel.
            // A pointer already on an icon must not replace this until it
            // moves (`prepare_open` drops the previous hover gate).
            self.switcher.selected = if self.switcher.apps.len() > 1 { 1 } else { 0 };
            self.switcher.prepare_open();
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

impl Shell {
    /// Publish `Topic::WindowChrome` for every app window whose chrome
    /// changed (tiled, Cinema, compositor rim), and retract departed ones.
    /// Runs with every frame emission and when an app publishes menus.
    pub(super) fn sync_window_chrome(&mut self) {
        let next: Vec<WindowChrome> = self
            .known_windows
            .iter()
            .filter(|w| w.app_id != Self::APP_ID)
            .map(|w| WindowChrome {
                window_id: w.window_id,
                tiled: matches!(self.tabs.zone_of(w.window_id), Some(Zone::Left | Zone::Right))
                    && !self.zoning.is_floating(w.window_id),
                cinema: self
                    .tabs
                    .desk_of(w.window_id)
                    .and_then(|d| self.tabs.present_on(d))
                    == Some((w.window_id, crate::stacks::Present::Cinema)),
                rim: self.menus.get_menu(&w.app_id).is_none(),
            })
            .collect();
        let changes = self.window_chrome.update(next);
        if changes.emit.is_empty() && changes.retract.is_empty() {
            return;
        }
        if let Some(Ok(mut bus)) = sola_kit::app::try_bus().map(|b| b.lock()) {
            for chrome in changes.emit {
                let _ = bus.emit(Topic::WindowChrome(chrome));
            }
            for chrome in changes.retract {
                let _ = bus.retract(Topic::WindowChrome(chrome));
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

/// Drop the sticky `TabWindow` for a tab whose window is gone.
fn retract_tab_window(key: String) {
    super::with_bus(|bus| {
        let _ = bus.retract(Topic::TabWindow(sola_bus::topics::TabWindow {
            key,
            window_id: 0,
            shown: false,
        }));
    });
}
