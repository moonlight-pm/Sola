use std::cell::RefCell;
use std::rc::Weak;

use base64::Engine;
use gtk4::prelude::*;
use serde_json::{Value, json};
use webkit6::prelude::*;

use sola_app::config::JsonConfig;
use sola_app::{AppCtx, AppRuntime, BusRegistry, SolaApp, WindowConfig, WindowHandle};
use sola_bus::topics::{MenuActionPayload, OpenUrlRequest, Topic, TopicKind};

use crate::chrome;
use crate::state::{BrowsingHistory, PersistedTab, TabStore};
use crate::tabs::{Tab, build_web_page_view};

pub struct BrowserApp {
    pub(crate) chrome: WindowHandle,
    pub(crate) container: gtk4::Overlay,
    pub(crate) web_context: webkit6::WebContext,
    pub(crate) network_session: webkit6::NetworkSession,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active_tab_id: Option<String>,
    pub(crate) tab_store: TabStore,
    pub(crate) history: BrowsingHistory,
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

        let tab_store = TabStore::load();
        let history = BrowsingHistory::load();

        ctx.emit_sticky(Topic::SetAppMenu(browser_menu()));
        tracing::info!("registered browser menu");

        Self {
            chrome,
            container,
            web_context,
            network_session,
            tabs: Vec::new(),
            active_tab_id: None,
            tab_store,
            history,
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
            "ready" => self.cmd_ready(ctx),
            "create_tab" => self.cmd_create_tab(args),
            "close_tab" => self.cmd_close_tab(args),
            "switch_tab" => self.cmd_switch_tab(args),
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
    }

    fn on_shutdown(&mut self, _ctx: &mut AppCtx) {
        tracing::info!("browser shutdown: flushing state");
        self.capture_session_state();
        self.persist_tabs();
        self.persist_history();
    }
}

impl BrowserApp {
    fn on_menu_action(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic else {
            return;
        };
        if app_id != "sola-browser" {
            return;
        }
        match action_id.as_str() {
            "new_tab" => {
                let tab_id = uuid::Uuid::new_v4().to_string();
                self.tab_store.tabs.push(PersistedTab {
                    url: String::new(),
                    title: String::new(),
                    session_state: None,
                });
                self.create_tab(&tab_id, None, None);
                self.switch_tab(&tab_id);

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
                if let Some(id) = self.active_tab_id.clone() {
                    self.close_tab(&id);
                    let next_id = self.tabs.last().map(|t| t.id.clone());
                    if let Some(ref next) = next_id {
                        self.switch_tab(next);
                    }
                    self.emit_to_chrome(
                        "tab_closed",
                        json!({
                            "tabId": id,
                            "nextTabId": next_id,
                        }),
                    );
                    tracing::debug!("closed tab {id}");
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

    fn on_open_url(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::OpenUrl(OpenUrlRequest { url, activate }) = topic else {
            return;
        };
        let tab_id = uuid::Uuid::new_v4().to_string();
        self.tab_store.tabs.push(PersistedTab {
            url: url.clone(),
            title: String::new(),
            session_state: None,
        });
        self.create_tab(&tab_id, Some(url), None);
        if *activate {
            self.switch_tab(&tab_id);
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

    pub(crate) fn emit_to_chrome(&self, event: &str, mut data: Value) {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("event".into(), json!(event));
        }
        self.chrome.send_to_js(&data);
    }

    pub(crate) fn persist_tabs(&self) {
        self.tab_store.save();
    }

    pub(crate) fn persist_history(&self) {
        self.history.save();
    }

    pub(crate) fn create_tab(
        &mut self,
        tab_id: &str,
        url: Option<&str>,
        session_state_b64: Option<&str>,
    ) {
        let cfg = crate::tabs::TabConfig {
            url: url.map(|s| s.to_string()),
            session_state_b64: session_state_b64.map(|s| s.to_string()),
        };
        let webview = build_web_page_view(&self.web_context, &self.network_session, &cfg);

        // Add as overlay child — the Overlay's get-child-position callback
        // sizes/positions it to the content area on every allocation.
        self.container.add_overlay(&webview);
        webview.set_visible(false);

        // Wire signal handlers. Each handler clones the chrome WindowHandle
        // for send_to_js; handlers that mutate state use the runtime weak.
        crate::tabs::wire_signals(&webview, tab_id, self.chrome.clone(), self.runtime_weak());

        self.tabs.push(Tab {
            id: tab_id.to_string(),
            webview,
        });

        self.persist_tabs();
    }

    pub(crate) fn close_tab(&mut self, tab_id: &str) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) {
            let tab = self.tabs.remove(pos);
            self.container.remove_overlay(&tab.webview);

            if pos < self.tab_store.tabs.len() {
                self.tab_store.tabs.remove(pos);
            }
            if self.active_tab_id.as_deref() == Some(tab_id) {
                self.active_tab_id = None;
                self.tab_store.active_tab_id = None;
            }
            self.persist_tabs();
        }
    }

    pub(crate) fn switch_tab(&mut self, tab_id: &str) {
        if let Some(current_id) = self.active_tab_id.as_ref() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == *current_id) {
                tab.webview.set_visible(false);
            }
        }
        if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
            tab.webview.set_visible(true);
            tab.webview.grab_focus();
        }
        self.active_tab_id = Some(tab_id.to_string());
        self.tab_store.active_tab_id = Some(tab_id.to_string());
        self.persist_tabs();
    }

    pub(crate) fn navigate_active(&mut self, url: &str) {
        if let Some(id) = self.active_tab_id.clone() {
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
        if let Some(id) = self.active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tab.webview.go_back();
            }
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if let Some(id) = self.active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tab.webview.go_forward();
            }
        }
    }

    pub(crate) fn reload(&mut self) {
        if let Some(id) = self.active_tab_id.clone() {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                tab.webview.reload();
            }
        }
    }

    /// Capture WebViewSessionState for every tab into `tab_store`.
    pub(crate) fn capture_session_state(&mut self) {
        for (i, tab) in self.tabs.iter().enumerate() {
            if i < self.tab_store.tabs.len() {
                if let Some(uri) = tab.webview.uri() {
                    self.tab_store.tabs[i].url = uri.to_string();
                }
                if let Some(title) = tab.webview.title() {
                    self.tab_store.tabs[i].title = title.to_string();
                }
                if let Some(session) = tab.webview.session_state() {
                    if let Some(bytes) = session.serialize() {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
                        self.tab_store.tabs[i].session_state = Some(b64);
                    }
                }
            }
        }
    }

    /// Snapshot a single tab's session state into `tab_store`. Called from
    /// the `notify::uri` handler before persisting.
    pub(crate) fn capture_tab_session_state(&mut self, tab_id: &str) {
        let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        if pos >= self.tab_store.tabs.len() {
            return;
        }
        let wv = &self.tabs[pos].webview;
        if let Some(uri) = wv.uri() {
            self.tab_store.tabs[pos].url = uri.to_string();
        }
        if let Some(title) = wv.title() {
            self.tab_store.tabs[pos].title = title.to_string();
        }
        if let Some(session) = wv.session_state() {
            if let Some(bytes) = session.serialize() {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
                self.tab_store.tabs[pos].session_state = Some(b64);
            }
        }
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

    fn cmd_ready(&mut self, _ctx: &mut AppCtx) -> Value {
        let tab_count = self.tab_store.tabs.len();
        let tabs_json: Vec<Value> = self
            .tab_store
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                json!({
                    "id": format!("restored-{i}"),
                    "url": t.url,
                    "title": t.title,
                })
            })
            .collect();

        // Materialize WebViews for restored tabs. Snapshot the persisted
        // entries first to avoid borrowing self.tab_store across create_tab.
        let snapshots: Vec<(String, String, Option<String>)> = self
            .tab_store
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (
                    format!("restored-{i}"),
                    t.url.clone(),
                    t.session_state.clone(),
                )
            })
            .collect();
        for (tab_id, url, session_state) in snapshots {
            self.create_tab(&tab_id, Some(&url), session_state.as_deref());
        }

        let active_id = if tab_count > 0 {
            let id = "restored-0".to_string();
            self.switch_tab(&id);
            id
        } else {
            String::new()
        };

        json!({
            "tabs": tabs_json,
            "activeTabId": active_id,
        })
    }

    fn cmd_create_tab(&mut self, args: &Value) -> Value {
        let Some(tab_id) = args["tabId"].as_str() else {
            return json!({ "error": "missing tabId" });
        };
        let url = args["url"].as_str();
        let activate = args["activate"].as_bool().unwrap_or(true);
        let tab_id = tab_id.to_string();

        // Append to tab_store before create_tab (create_tab persists only;
        // it no longer mutates the store).
        self.tab_store.tabs.push(PersistedTab {
            url: url.unwrap_or("").to_string(),
            title: String::new(),
            session_state: None,
        });
        self.create_tab(&tab_id, url, None);
        if activate {
            self.switch_tab(&tab_id);
        }
        self.persist_tabs();
        json!("ok")
    }

    fn cmd_close_tab(&mut self, args: &Value) -> Value {
        let Some(tab_id) = args["tabId"].as_str() else {
            return json!({ "error": "missing tabId" });
        };
        let tab_id = tab_id.to_string();
        self.close_tab(&tab_id);
        json!("ok")
    }

    fn cmd_switch_tab(&mut self, args: &Value) -> Value {
        let Some(tab_id) = args["tabId"].as_str() else {
            return json!({ "error": "missing tabId" });
        };
        let tab_id = tab_id.to_string();
        self.switch_tab(&tab_id);
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

// Stash the runtime weak for use inside signal handlers fired from GTK.
// This is populated in `after_runtime_ready` and read by the handlers
// installed inside `create_tab`. Using a thread-local (GTK is single-thread)
// is simpler than threading the weak through every helper.
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
