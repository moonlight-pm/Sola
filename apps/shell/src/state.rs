use std::cell::RefCell;
use std::rc::Rc;

use sola_bus::topics::{App, CompositionEntry, FocusTarget, Topic};

use crate::menu::MenuCache;
use crate::switcher::SwitcherState;
use crate::util::eval_js;
use crate::zoning::ZoningState;

pub struct ShellState {
    pub focused_app_id: Option<String>,
    pub mru_apps: Vec<String>,
    pub known_apps: Vec<App>,
    pub menus: MenuCache,
    pub zoning: ZoningState,
    pub switcher: SwitcherState,
    pub switcher_webview: Option<webkit6::WebView>,
    pub menubar_webview: Option<webkit6::WebView>,
    pub menu_webview: Option<webkit6::WebView>,
    pub menu_open: bool,
    pub bus: Option<Rc<RefCell<sola_bus::BusClient>>>,
}

impl ShellState {
    pub fn new() -> Self {
        Self {
            focused_app_id: None,
            mru_apps: Vec::new(),
            known_apps: Vec::new(),
            menus: MenuCache::new(),
            zoning: ZoningState::new(),
            switcher: SwitcherState::default(),
            switcher_webview: None,
            menubar_webview: None,
            menu_webview: None,
            menu_open: false,
            bus: None,
        }
    }

    pub fn set_focus(&mut self, app_id: &str) {
        self.focused_app_id = Some(app_id.to_string());
        self.zoning.set_focused(app_id.to_string());
        self.mru_apps.retain(|m| m != app_id);
        self.mru_apps.insert(0, app_id.to_string());

        // Close any open menu — the focus event to JS handles the menubar UI.
        if self.menu_open {
            self.menu_open = false;
            if let Some(ref wv) = self.menu_webview {
                eval_js(wv, "clearMenu()");
            }
        }

        let menu = self.menus.get_menu(app_id);
        let app_name = menu
            .and_then(|m| m.menus.first())
            .map(|d| d.label.as_str())
            .unwrap_or(app_id);
        let menu_labels: Vec<String> = menu
            .map(|m| m.menus.iter().map(|d| d.label.clone()).collect())
            .unwrap_or_default();

        if let Some(ref wv) = self.menubar_webview {
            let msg = serde_json::json!({
                "event": "focus",
                "app_name": app_name,
                "menu_labels": menu_labels,
            })
            .to_string();
            let js_str = serde_json::to_string(&msg).unwrap_or_default();
            eval_js(wv, &format!("window.__solaRecv({js_str})"));
        }
    }

    pub fn rebuild_switcher_apps(&self) -> Vec<App> {
        let mut apps: Vec<App> = self
            .mru_apps
            .iter()
            .filter_map(|id| self.known_apps.iter().find(|a| &a.app_id == id))
            .cloned()
            .collect();
        // Append any known apps not yet in MRU.
        for a in &self.known_apps {
            if a.app_id != "sola-shell" && !self.mru_apps.contains(&a.app_id) {
                apps.push(a.clone());
            }
        }
        apps
    }

    /// Build the composition list (bottom to top) and emit it.
    pub fn emit_composition(&self, emit: &dyn Fn(Topic)) {
        let mut entries = Vec::new();

        // 1. Shell menubar — always present at the bottom.
        entries.push(CompositionEntry {
            app_id: "sola-shell".into(),
            title: Some("menubar".into()),
        });

        // 2. App windows ordered by MRU (least recent first = bottom of stack).
        for app_id in self.mru_apps.iter().rev() {
            if app_id == "sola-shell" {
                continue;
            }
            entries.push(CompositionEntry {
                app_id: app_id.clone(),
                title: None,
            });
        }

        // Apps not yet in MRU (just appeared).
        for app in &self.known_apps {
            if app.app_id == "sola-shell" {
                continue;
            }
            if !self.mru_apps.contains(&app.app_id) {
                entries.push(CompositionEntry {
                    app_id: app.app_id.clone(),
                    title: None,
                });
            }
        }

        // 3. Shell panels on top when active.
        if self.menu_open {
            entries.push(CompositionEntry {
                app_id: "sola-shell".into(),
                title: Some("menu".into()),
            });
        }
        if self.switcher.active {
            entries.push(CompositionEntry {
                app_id: "sola-shell".into(),
                title: Some("switcher".into()),
            });
        }

        emit(Topic::Composition(entries));
    }

    /// Emit Frame updates for all known apps.
    pub fn emit_all_frames(&self, emit: &dyn Fn(Topic)) {
        if let Some(frame) = self.zoning.menubar_frame() {
            emit(Topic::Frame(frame));
        }

        for app in &self.known_apps {
            if app.app_id == "sola-shell" {
                continue;
            }
            if let Some(frame) = self.zoning.app_frame(&app.app_id) {
                emit(Topic::Frame(frame));
            }
        }
    }

    /// Handle new/removed apps from the compositor's Apps list.
    pub fn handle_apps_update(&mut self, apps: Vec<App>, emit: &dyn Fn(Topic)) {
        let old_ids: std::collections::HashSet<&str> = self
            .known_apps
            .iter()
            .map(|a| a.app_id.as_str())
            .collect();
        let new_ids: std::collections::HashSet<&str> =
            apps.iter().map(|a| a.app_id.as_str()).collect();

        let added: Vec<String> = apps
            .iter()
            .filter(|a| !old_ids.contains(a.app_id.as_str()) && a.app_id != "sola-shell")
            .map(|a| a.app_id.clone())
            .collect();

        let removed: Vec<String> = self
            .known_apps
            .iter()
            .filter(|a| !new_ids.contains(a.app_id.as_str()) && a.app_id != "sola-shell")
            .map(|a| a.app_id.clone())
            .collect();

        self.known_apps = apps.clone();
        self.switcher.apps = apps
            .into_iter()
            .filter(|a| a.app_id != "sola-shell")
            .collect();

        for id in &removed {
            self.mru_apps.retain(|m| m != id);
        }

        // Emit Frames for new apps.
        for id in &added {
            if let Some(frame) = self.zoning.app_frame(id) {
                emit(Topic::Frame(frame));
            }
        }

        self.emit_composition(emit);

        // Focus the newest app.
        if let Some(id) = added.first() {
            self.set_focus(id);
            emit(Topic::Focus(FocusTarget {
                app_id: id.clone(),
                title: None,
            }));
            self.emit_composition(emit);
        }
    }
}
