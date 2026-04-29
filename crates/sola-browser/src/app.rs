use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Weak;

use base64::Engine;
use gtk4::prelude::*;
use serde_json::{Value, json};
use webkit6::prelude::*;

use sola_app::{AppCtx, AppRuntime, BusRegistry, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{
    BrowserConfig, BrowserHistory, BrowserTab, MenuActionPayload, OpenUrlRequest, Topic, TopicKind,
};

use crate::chrome;
use crate::state::HistoryOps;
use crate::tabs::{Tab, build_web_page_view};

pub struct BrowserApp {
    pub(crate) chrome: WindowHandle,
    pub(crate) container: gtk4::Overlay,
    pub(crate) web_context: webkit6::WebContext,
    pub(crate) network_session: webkit6::NetworkSession,
    /// Live WebViews for each tab; ordered roughly by creation. The
    /// canonical tab-strip order is `tabs_by_id[*].ordinal`, which JS
    /// sorts on. This Vec is just for WebView ownership and lookup.
    pub(crate) tabs: Vec<Tab>,
    /// Per-tab metadata mirror of the bus state. Authoritative for the
    /// tab-strip view; updated on every `BrowserTab` delivery.
    pub(crate) tabs_by_id: HashMap<String, BrowserTab>,
    /// Singleton browser-wide config, tracked from the bus.
    pub(crate) config: BrowserConfig,
    /// Whichever tab WebView is currently visible, or `None` for an
    /// empty browser. Updated only by `realize_active`.
    pub(crate) realized_active_tab_id: Option<String>,
    /// Visit-history aggregate, tracked from the bus.
    pub(crate) history: BrowserHistory,
}

impl SolaApp for BrowserApp {
    const APP_ID: &'static str = "sola-browser";

    fn new(ctx: &mut AppCtx) -> Self {
        // Remote Web Inspector (mirrors pre-refactor env setup).
        if std::env::var("WEBKIT_INSPECTOR_HTTP_SERVER").is_err() {
            unsafe { std::env::set_var("WEBKIT_INSPECTOR_HTTP_SERVER", "0.0.0.0:9224") };
            tracing::info!("remote inspector enabled at http://0.0.0.0:9224");
        }

        let chrome = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (1920, 1080),
            position: None,
            decorated: false,
            transparent: true,
            assets: crate::APP_ASSETS,
            initial_state: None,
            zoned: true,
            keyboard_target: true,
        });

        // Reparent the chrome WebView into an Overlay so we can add tab
        // WebView siblings later. The chrome is the Overlay's main child —
        // GTK auto-fills it to the Overlay's allocation every resize cycle,
        // which is what we need for compositor-driven zoning. Tab WebViews
        // are overlays; a `get-child-position` callback computes their
        // content-area rect from the Overlay's current allocation.
        let chrome_webview = chrome.webview().clone();
        chrome.gtk_window().set_child(None::<&gtk4::Widget>);
        let container = gtk4::Overlay::new();
        container.set_child(Some(&chrome_webview));
        container.connect_get_child_position(|overlay, _child| {
            let area = chrome::content_area(overlay.width(), overlay.height());
            Some(gdk4::Rectangle::new(
                area.x,
                area.y,
                area.width,
                area.height,
            ))
        });
        chrome.gtk_window().set_child(Some(&container));

        // Reuse the WebContext from the chrome WebView for tab WebViews so
        // they share the `app:///` URI scheme registration.
        let web_context = WebViewExt::web_context(&chrome_webview).expect("web context");

        // Tab network session: persistent SQLite cookies, data + cache dirs.
        let data_dir = glib::user_data_dir().join("sola").join("browser");
        let cache_dir = glib::user_cache_dir().join("sola").join("browser");
        std::fs::create_dir_all(&data_dir).ok();
        std::fs::create_dir_all(&cache_dir).ok();
        let network_session = webkit6::NetworkSession::new(
            Some(data_dir.to_str().unwrap()),
            Some(cache_dir.to_str().unwrap()),
        );
        if let Some(cookie_mgr) = network_session.cookie_manager() {
            let cookie_db = data_dir.join("cookies.db");
            cookie_mgr.set_persistent_storage(
                cookie_db.to_str().unwrap(),
                webkit6::CookiePersistentStorage::Sqlite,
            );
        }

        // One-shot legacy JSON migrator: emits BrowserTab/Config/History
        // for any pre-bus state on disk so the new namespace files get
        // populated on first run.
        if let Some(plan) = crate::migrate::compute_migration(&sola_core::config::sola_config_dir())
        {
            for tab in plan.tabs {
                ctx.emit(Topic::BrowserTab(tab));
            }
            ctx.emit(Topic::BrowserConfig(plan.config));
            ctx.emit(Topic::BrowserHistory(plan.history));
            crate::migrate::mark_migrated(&sola_core::config::sola_config_dir());
        }

        ctx.emit(Topic::SetAppMenu(browser_menu()));
        tracing::info!("registered browser menu");

        Self {
            chrome,
            container,
            web_context,
            network_session,
            tabs: Vec::new(),
            tabs_by_id: HashMap::new(),
            config: BrowserConfig::default(),
            realized_active_tab_id: None,
            history: BrowserHistory::default(),
        }
    }

    fn after_runtime_ready(&mut self, runtime: Weak<RefCell<AppRuntime<Self>>>, _ctx: &mut AppCtx) {
        // Publish the runtime weak for webview signal handlers inside
        // create_tab (title/uri/is-loading/decide-policy). Resize is
        // handled by the Overlay's get-child-position callback, so no
        // window-size notify hooks are needed here.
        RUNTIME_WEAK.with(|slot| {
            *slot.borrow_mut() = Some(runtime);
        });
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        let result: Value = match cmd {
            "ready" => self.cmd_ready(),
            "create_tab" => self.cmd_create_tab(args, ctx),
            "close_tab" => self.cmd_close_tab(args, ctx),
            "switch_tab" => self.cmd_switch_tab(args, ctx),
            "navigate" => self.cmd_navigate(args),
            "go_back" => {
                self.go_back();
                json!("ok")
            }
            "go_forward" => {
                self.go_forward();
                json!("ok")
            }
            "reload" => {
                self.reload();
                json!("ok")
            }
            "history_search" => self.cmd_history_search(args),
            other => {
                tracing::warn!(cmd = other, "unknown js command");
                json!({ "error": "unknown command" })
            }
        };

        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        // Default CloseApp handler is inherited from the trait — don't re-register.
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
        bus.on(TopicKind::OpenUrl, Self::on_open_url);
        bus.on(TopicKind::BrowserTab, Self::on_browser_tab);
        bus.on(TopicKind::BrowserConfig, Self::on_browser_config);
        bus.on(TopicKind::BrowserHistory, Self::on_browser_history);
    }

    fn on_shutdown(&mut self, ctx: &mut AppCtx) {
        tracing::info!("browser shutdown: capturing per-tab session state");
        self.capture_all_session_state(ctx);
    }
}

impl BrowserApp {
    fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = delivery.topic else {
            return;
        };
        if app_id != Self::APP_ID {
            return;
        }
        match action_id.as_str() {
            "new_tab" => {
                let tab_id = uuid::Uuid::new_v4().to_string();
                let ordinal = self.next_ordinal();
                let tab = BrowserTab {
                    id: tab_id.clone(),
                    url: String::new(),
                    title: String::new(),
                    ordinal,
                    session_state: None,
                };
                self.tabs_by_id.insert(tab_id.clone(), tab.clone());
                self.create_webview_for_tab(&tab);
                ctx.emit(Topic::BrowserTab(tab));
                self.set_active_tab(Some(tab_id.clone()), ctx);

                self.emit_to_chrome(
                    "bus_new_tab",
                    json!({
                        "tabId": tab_id,
                        "url": "",
                        "activate": true,
                    }),
                );
                self.chrome.webview().grab_focus();
                tracing::debug!("new tab {tab_id}");
            }
            "close_tab" => {
                if let Some(id) = self.realized_active_tab_id.clone() {
                    self.close_tab(&id, ctx);
                }
            }
            "focus_address" => {
                self.emit_to_chrome("bus_focus_address", json!({}));
                self.chrome.webview().grab_focus();
                tracing::debug!("focus address bar");
            }
            "quit" => {
                ctx.emit(Topic::Shutdown);
            }
            _ => {}
        }
    }

    fn on_open_url(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::OpenUrl(OpenUrlRequest { url, activate }) = delivery.topic else {
            return;
        };
        let tab_id = uuid::Uuid::new_v4().to_string();
        let ordinal = self.next_ordinal();
        let tab = BrowserTab {
            id: tab_id.clone(),
            url: url.clone(),
            title: String::new(),
            ordinal,
            session_state: None,
        };
        self.tabs_by_id.insert(tab_id.clone(), tab.clone());
        self.create_webview_for_tab(&tab);
        ctx.emit(Topic::BrowserTab(tab));
        if *activate {
            self.set_active_tab(Some(tab_id.clone()), ctx);
        }
        self.emit_to_chrome(
            "bus_new_tab",
            json!({
                "tabId": tab_id,
                "url": url,
                "activate": activate,
            }),
        );
        tracing::info!(url = %url, "OpenUrl: created tab {tab_id}");
    }

    fn on_browser_tab(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        // Idempotent merge. Every local mutation (cmd_create_tab,
        // capture_tab_session_state, ...) updates tabs_by_id
        // synchronously before emitting, so the echo's payload matches
        // the current local state and produces no chrome events.
        // External deliveries — sticky restoration on startup, or any
        // future external writer — create or update local state and
        // surface only the actual diff to the strip.
        let Topic::BrowserTab(tab) = delivery.topic else {
            return;
        };
        if delivery.retracted {
            if self.tabs_by_id.remove(&tab.id).is_some() {
                self.destroy_webview(&tab.id);
                self.emit_to_chrome(
                    "tab_closed",
                    json!({ "tabId": tab.id, "nextTabId": Value::Null }),
                );
            }
        } else {
            let prev = self.tabs_by_id.get(&tab.id).cloned();
            self.tabs_by_id.insert(tab.id.clone(), tab.clone());
            if prev.is_none() {
                self.create_webview_for_tab(tab);
                self.emit_to_chrome(
                    "bus_new_tab",
                    json!({
                        "tabId": tab.id,
                        "url": tab.url,
                        "title": tab.title,
                        "activate": false,
                    }),
                );
            }
            // Only surface URL/title diffs that actually move the value.
            if prev.as_ref().map(|p| &p.url) != Some(&tab.url) {
                self.emit_to_chrome(
                    "tab_url_changed",
                    json!({ "tabId": tab.id, "url": tab.url }),
                );
            }
            if prev.as_ref().map(|p| &p.title) != Some(&tab.title) {
                self.emit_to_chrome(
                    "tab_title_changed",
                    json!({ "tabId": tab.id, "title": tab.title }),
                );
            }
        }
        self.realize_active(ctx);
    }

    fn on_browser_config(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::BrowserConfig(cfg) = delivery.topic else {
            return;
        };
        if self.config == *cfg {
            return;
        }
        self.config = cfg.clone();
        self.realize_active(ctx);
    }

    fn on_browser_history(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        // Skip our own self-echo: only on-disk restoration ("sola-bus")
        // and external writers (none today, but a future sola-history
        // service might) are interesting. Idempotent merge would suffice
        // for correctness, but the explicit filter keeps the local
        // aggregate untouched on every emit.
        if delivery.source == Self::APP_ID {
            return;
        }
        let Topic::BrowserHistory(h) = delivery.topic else {
            return;
        };
        self.history = h.clone();
    }

    pub(crate) fn emit_to_chrome(&self, event: &str, mut data: Value) {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("event".into(), json!(event));
        }
        self.chrome.send_to_js(&data);
    }

    /// Apply selection: prefer `config.active_tab_id` when its tab
    /// exists; otherwise the lowest-ordinal tab; otherwise nothing.
    /// Idempotent — calls with no resulting change are no-ops.
    pub(crate) fn realize_active(&mut self, _ctx: &mut AppCtx) {
        let target = select_active(
            &self.tabs_by_id,
            self.config.active_tab_id.as_deref(),
            self.realized_active_tab_id.as_deref(),
        );
        if let Some(target) = target {
            self.realized_active_tab_id = target.clone();
            match &target {
                Some(id) => self.show_tab(id),
                None => self.hide_all_tabs(),
            }
            // Notify JS so the strip's active highlight follows.
            // `null` clears highlight when no tab remains.
            self.emit_to_chrome(
                "active_tab_changed",
                json!({ "tabId": target }),
            );
        }
    }

    /// Compute the next ordinal: max existing + 1, or 0 if empty.
    fn next_ordinal(&self) -> u32 {
        self.tabs_by_id
            .values()
            .map(|t| t.ordinal)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    }

    /// Create a WebView for a freshly-discovered tab and register signal
    /// handlers. Does not touch `tabs_by_id` or emit anything — caller
    /// owns those.
    pub(crate) fn create_webview_for_tab(&mut self, tab: &BrowserTab) {
        if self.tabs.iter().any(|t| t.id == tab.id) {
            return; // already materialized
        }
        let cfg = crate::tabs::TabConfig {
            url: if tab.url.is_empty() {
                None
            } else {
                Some(tab.url.clone())
            },
            session_state_b64: tab.session_state.clone(),
        };
        let webview = build_web_page_view(&self.web_context, &self.network_session, &cfg);
        self.container.add_overlay(&webview);
        webview.set_visible(false);
        crate::tabs::wire_signals(&webview, &tab.id, self.chrome.clone(), self.runtime_weak());
        self.tabs.push(Tab {
            id: tab.id.clone(),
            webview,
        });
    }

    /// Tear down a tab's WebView. Used on `BrowserTab` retraction.
    pub(crate) fn destroy_webview(&mut self, tab_id: &str) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) {
            let tab = self.tabs.remove(pos);
            self.container.remove_overlay(&tab.webview);
        }
        if self.realized_active_tab_id.as_deref() == Some(tab_id) {
            self.realized_active_tab_id = None;
        }
    }

    fn show_tab(&self, tab_id: &str) {
        for tab in &self.tabs {
            if tab.id == tab_id {
                tab.webview.set_visible(true);
                tab.webview.grab_focus();
            } else {
                tab.webview.set_visible(false);
            }
        }
    }

    fn hide_all_tabs(&self) {
        for tab in &self.tabs {
            tab.webview.set_visible(false);
        }
    }

    /// Close a tab: retract its `BrowserTab` (which the echo handler
    /// will clean up locally and surface to the strip) and pick a
    /// fallback active tab if the closed one was active.
    pub(crate) fn close_tab(&mut self, tab_id: &str, ctx: &mut AppCtx) {
        let Some(tab) = self.tabs_by_id.get(tab_id).cloned() else {
            return;
        };
        ctx.retract(Topic::BrowserTab(tab));
        if self.config.active_tab_id.as_deref() == Some(tab_id) {
            let fallback = self
                .tabs_by_id
                .values()
                .filter(|t| t.id != tab_id)
                .min_by_key(|t| t.ordinal)
                .map(|t| t.id.clone());
            self.set_active_tab(fallback, ctx);
        }
    }

    /// Set the persisted active tab and emit the updated config. The
    /// visible WebView change happens in `realize_active`, which also
    /// emits `active_tab_changed` to JS.
    pub(crate) fn set_active_tab(&mut self, tab_id: Option<String>, ctx: &mut AppCtx) {
        if self.config.active_tab_id == tab_id {
            return;
        }
        self.config.active_tab_id = tab_id;
        ctx.emit(Topic::BrowserConfig(self.config.clone()));
        // Optimistic local realize so the WebView swaps now without
        // waiting for the bus echo. realize_active is idempotent, so
        // the echo's call is a no-op.
        self.realize_active(ctx);
    }

    pub(crate) fn navigate_active(&mut self, url: &str) {
        if let Some(id) = self.realized_active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tracing::info!(tab_id = %id, %url, "navigate");
                tab.webview.load_uri(url);
            } else {
                tracing::warn!(tab_id = %id, %url, "navigate: active tab not found");
            }
        } else {
            tracing::warn!(%url, "navigate: no active tab");
        }
    }

    pub(crate) fn go_back(&mut self) {
        if let Some(id) = self.realized_active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tab.webview.go_back();
            }
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if let Some(id) = self.realized_active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tab.webview.go_forward();
            }
        }
    }

    pub(crate) fn reload(&mut self) {
        if let Some(id) = self.realized_active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tab.webview.reload();
            }
        }
    }

    /// Capture WebViewSessionState for every tab and emit `BrowserTab`
    /// updates for each. Called on shutdown so back/forward state and
    /// scroll positions persist across browser restarts.
    pub(crate) fn capture_all_session_state(&mut self, ctx: &mut AppCtx) {
        let mut updates: Vec<BrowserTab> = Vec::new();
        for tab in &self.tabs {
            let Some(meta) = self.tabs_by_id.get(&tab.id) else {
                continue;
            };
            let mut updated = meta.clone();
            if let Some(uri) = tab.webview.uri() {
                updated.url = uri.to_string();
            }
            if let Some(title) = tab.webview.title() {
                updated.title = title.to_string();
            }
            if let Some(session) = tab.webview.session_state() {
                if let Some(bytes) = session.serialize() {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
                    updated.session_state = Some(b64);
                }
            }
            updates.push(updated);
        }
        for tab in updates {
            self.tabs_by_id.insert(tab.id.clone(), tab.clone());
            ctx.emit(Topic::BrowserTab(tab));
        }
    }

    /// Snapshot a single tab's session state and emit the updated
    /// `BrowserTab`. Called from the `notify::uri` handler on each
    /// committed navigation.
    pub(crate) fn capture_tab_session_state(&mut self, tab_id: &str, ctx: &mut AppCtx) {
        let Some(tab_pos) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let Some(meta) = self.tabs_by_id.get(tab_id).cloned() else {
            return;
        };
        let wv = &self.tabs[tab_pos].webview;
        let mut updated = meta;
        if let Some(uri) = wv.uri() {
            updated.url = uri.to_string();
        }
        if let Some(title) = wv.title() {
            updated.title = title.to_string();
        }
        if let Some(session) = wv.session_state() {
            if let Some(bytes) = session.serialize() {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
                updated.session_state = Some(b64);
            }
        }
        self.tabs_by_id.insert(tab_id.to_string(), updated.clone());
        ctx.emit(Topic::BrowserTab(updated));
    }

    fn runtime_weak(&self) -> Weak<RefCell<AppRuntime<BrowserApp>>> {
        RUNTIME_WEAK.with(|slot| {
            slot.borrow()
                .as_ref()
                .cloned()
                .expect("runtime weak not initialised; after_runtime_ready must run first")
        })
    }

    // --- JS command handlers ---

    fn cmd_ready(&self) -> Value {
        // Snapshot of current bus-driven state. If sticky restoration
        // hasn't completed yet, additional `bus_new_tab` events fire as
        // the bus delivers each `BrowserTab`.
        let mut tabs: Vec<&BrowserTab> = self.tabs_by_id.values().collect();
        tabs.sort_by_key(|t| t.ordinal);
        let tabs_json: Vec<Value> = tabs
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "url": t.url,
                    "title": t.title,
                })
            })
            .collect();
        let active_id = self
            .realized_active_tab_id
            .clone()
            .or_else(|| self.config.active_tab_id.clone())
            .unwrap_or_default();
        json!({
            "tabs": tabs_json,
            "activeTabId": active_id,
        })
    }

    fn cmd_create_tab(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let Some(tab_id) = args["tabId"].as_str() else {
            return json!({ "error": "missing tabId" });
        };
        let url = args["url"].as_str().unwrap_or("");
        let activate = args["activate"].as_bool().unwrap_or(true);
        let tab_id = tab_id.to_string();
        let ordinal = self.next_ordinal();
        let tab = BrowserTab {
            id: tab_id.clone(),
            url: url.to_string(),
            title: String::new(),
            ordinal,
            session_state: None,
        };
        self.tabs_by_id.insert(tab_id.clone(), tab.clone());
        self.create_webview_for_tab(&tab);
        ctx.emit(Topic::BrowserTab(tab));
        if activate {
            self.set_active_tab(Some(tab_id), ctx);
        }
        json!("ok")
    }

    fn cmd_close_tab(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let Some(tab_id) = args["tabId"].as_str() else {
            return json!({ "error": "missing tabId" });
        };
        let tab_id = tab_id.to_string();
        self.close_tab(&tab_id, ctx);
        json!("ok")
    }

    fn cmd_switch_tab(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let Some(tab_id) = args["tabId"].as_str() else {
            return json!({ "error": "missing tabId" });
        };
        let tab_id = tab_id.to_string();
        self.set_active_tab(Some(tab_id), ctx);
        json!("ok")
    }

    fn cmd_navigate(&mut self, args: &Value) -> Value {
        let Some(url) = args["url"].as_str() else {
            return json!({ "error": "missing url" });
        };
        let url = url.to_string();
        self.navigate_active(&url);
        json!("ok")
    }

    fn cmd_history_search(&self, args: &Value) -> Value {
        let Some(query) = args["query"].as_str() else {
            return json!({ "error": "missing query" });
        };
        let results: Vec<Value> = self
            .history
            .search(query, 10)
            .into_iter()
            .map(|e| {
                json!({
                    "url": e.url,
                    "title": e.title,
                    "visits": e.visits,
                })
            })
            .collect();
        json!(results)
    }
}

/// Pure selection function for tests. Returns `Some(target)` to switch
/// to a new selection (`target` itself may be `None` to clear), or
/// `None` to indicate "no change needed" (current selection still
/// matches the desired outcome).
pub(crate) fn select_active(
    tabs_by_id: &HashMap<String, BrowserTab>,
    desired_active_id: Option<&str>,
    realized: Option<&str>,
) -> Option<Option<String>> {
    let target = desired_active_id
        .filter(|id| tabs_by_id.contains_key(*id))
        .map(str::to_string)
        .or_else(|| {
            tabs_by_id
                .values()
                .min_by_key(|t| t.ordinal)
                .map(|t| t.id.clone())
        });
    if target.as_deref() == realized {
        None
    } else {
        Some(target)
    }
}

// Stash the runtime weak for use inside signal handlers fired from GTK.
// This is populated in `after_runtime_ready` and read by the handlers
// installed inside `create_webview_for_tab`. Using a thread-local (GTK is
// single-thread) is simpler than threading the weak through every helper.
thread_local! {
    pub(crate) static RUNTIME_WEAK: RefCell<Option<Weak<RefCell<AppRuntime<BrowserApp>>>>> =
        const { RefCell::new(None) };
}

pub(crate) fn browser_menu() -> sola_bus::topics::AppMenuPayload {
    use sola_bus::topics::{AppMenuPayload, MenuDefinition, MenuItem};
    use sola_core::KeyCode;

    AppMenuPayload {
        app_id: BrowserApp::APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Browser".into(),
                items: vec![
                    MenuItem::Action {
                        id: "new_tab".into(),
                        label: "New Tab".into(),
                        shortcut: Some(KeyCode::T.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Action {
                        id: "close_tab".into(),
                        label: "Close Tab".into(),
                        shortcut: Some(KeyCode::W.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Browser".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Edit".into(),
                items: vec![MenuItem::Action {
                    id: "focus_address".into(),
                    label: "Focus Address Bar".into(),
                    shortcut: Some(KeyCode::L.meta()),
                    disabled: false,
                    checked: false,
                }],
            },
        ],
    }
}

#[cfg(test)]
mod realize_tests {
    use super::*;
    use sola_bus::topics::BrowserTab;
    use std::collections::HashMap;

    fn tab(id: &str, ord: u32) -> BrowserTab {
        BrowserTab {
            id: id.into(),
            url: String::new(),
            title: String::new(),
            ordinal: ord,
            session_state: None,
        }
    }

    #[test]
    fn no_change_when_target_matches_realized() {
        let tabs: HashMap<_, _> = [(String::from("a"), tab("a", 0))].into();
        assert_eq!(select_active(&tabs, Some("a"), Some("a")), None);
    }

    #[test]
    fn switches_to_desired_when_present() {
        let tabs: HashMap<_, _> = [
            (String::from("a"), tab("a", 0)),
            (String::from("b"), tab("b", 1)),
        ]
        .into();
        assert_eq!(
            select_active(&tabs, Some("b"), Some("a")),
            Some(Some("b".into()))
        );
    }

    #[test]
    fn falls_back_to_lowest_ordinal_when_desired_missing() {
        let tabs: HashMap<_, _> = [
            (String::from("b"), tab("b", 5)),
            (String::from("c"), tab("c", 1)),
        ]
        .into();
        assert_eq!(
            select_active(&tabs, Some("a"), None),
            Some(Some("c".into()))
        );
    }

    #[test]
    fn falls_back_to_lowest_when_no_desired() {
        let tabs: HashMap<_, _> = [
            (String::from("z"), tab("z", 9)),
            (String::from("y"), tab("y", 3)),
        ]
        .into();
        assert_eq!(
            select_active(&tabs, None, None),
            Some(Some("y".into()))
        );
    }

    #[test]
    fn clears_when_no_tabs_and_realized_was_set() {
        let tabs: HashMap<String, BrowserTab> = HashMap::new();
        assert_eq!(select_active(&tabs, Some("a"), Some("a")), Some(None));
    }

    #[test]
    fn no_change_when_no_tabs_and_no_realized() {
        let tabs: HashMap<String, BrowserTab> = HashMap::new();
        assert_eq!(select_active(&tabs, None, None), None);
    }
}
