use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use sola_app::{
    AppCtx, AsyncDispatcher, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};
use sola_bus::topics::{
    MenuActionPayload, OpenUrlRequest, TerminalConfig, TerminalSession, Topic, TopicKind,
};

mod commands;
mod menu;
mod pty;
mod state;
mod tmux;

/// Channel ops drained from the async PTY thread back to the GTK loop.
/// Retract is its own variant so closes turn into bus retractions
/// rather than bulk re-emits.
pub enum BusOp {
    Emit(Topic),
    Retract(Topic),
}

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/terminal-pane.ts" => (include_str!("../web/src/terminal-pane.ts"), TypeScript),
    "/src/components/sidebar.ts" => (include_str!("../web/src/components/sidebar.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
    "/vendor/xterm.mjs" => (include_str!("../web/vendor/xterm.mjs"), JavaScript),
    "/vendor/xterm.css" => (include_str!("../web/vendor/xterm.css"), Css),
    "/vendor/addon-fit.mjs" => (include_str!("../web/vendor/addon-fit.mjs"), JavaScript),
    "/vendor/addon-web-links.mjs" => (include_str!("../web/vendor/addon-web-links.mjs"), JavaScript),
};

struct TerminalApp {
    main_window: WindowHandle,
    dispatcher: AsyncDispatcher,
    state: Arc<state::TerminalState>,
    config: TerminalConfig,
    /// Live tmux sessions, queried lazily on the first replayed
    /// `TerminalSession`. `None` until the first replay arrives.
    live_tmux: Option<HashSet<String>>,
}

impl SolaApp for TerminalApp {
    const APP_ID: &'static str = "sola-terminal";

    fn new(ctx: &mut AppCtx) -> Self {
        tmux::cleanup_stale_socket();
        tmux::kill_orphaned_clients();
        tmux::reload_config();

        let terminal_state = Arc::new(state::TerminalState::new());

        // Initial JS state: empty tabs + default config. The bus replays
        // the persisted TerminalConfig and per-tab TerminalSession entries
        // into our handlers a few ms after subscription, and we push the
        // real state to JS at that point.
        let initial_state = serde_json::to_string(&state_payload(&[], &TerminalConfig::default()))
            .unwrap_or_default();

        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (1920, 1080),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(initial_state),
            zoned: true,
            keyboard_target: true,
        });

        // Bridge dispatcher → JS for PTY events.
        let (event_tx, event_rx) = std::sync::mpsc::channel::<String>();
        let mw_for_events = main_window.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(msg) = event_rx.try_recv() {
                mw_for_events.send_raw_json_to_js(&msg);
            }
            gtk4::glib::ControlFlow::Continue
        });

        // Bridge dispatcher → bus for topic emits + retracts. AppCtx is
        // GTK-thread-bound (Rc<RefCell<BusClient>>), so we can't share it
        // with the tokio runtime. The handler sends BusOps through this
        // channel and the GTK main loop drains them via ctx.{emit,retract}.
        let (bus_tx, bus_rx) = std::sync::mpsc::channel::<BusOp>();
        let ctx_proxy = ctx.bus_proxy();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(op) = bus_rx.try_recv() {
                match op {
                    BusOp::Emit(topic) => ctx_proxy.emit(topic),
                    BusOp::Retract(topic) => ctx_proxy.retract(topic),
                }
            }
            gtk4::glib::ControlFlow::Continue
        });

        let dispatcher = AsyncDispatcher::spawn(commands::TerminalHandler {
            state: terminal_state.clone(),
            event_tx,
            bus_tx,
        });

        ctx.emit(Topic::SetAppMenu(menu::terminal_menu(0)));
        tracing::info!("registered terminal menu");

        Self {
            main_window,
            dispatcher,
            state: terminal_state,
            config: TerminalConfig::default(),
            live_tmux: None,
        }
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        _source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        match cmd {
            "open_url" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
                if url.is_empty() {
                    tracing::warn!("open_url command with empty url");
                    return;
                }
                let activate = args
                    .get("activate")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                ctx.emit(Topic::OpenUrl(OpenUrlRequest {
                    url: url.to_string(),
                    activate,
                }));
                if let Some(id) = id {
                    self.main_window
                        .send_to_js(&json!({ "id": id, "result": "ok" }));
                }
            }
            "set_sidebar" => {
                let width = args
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(self.config.sidebar_width);
                let collapsed = args
                    .get("collapsed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(self.config.sidebar_collapsed);
                if width != self.config.sidebar_width
                    || collapsed != self.config.sidebar_collapsed
                {
                    self.config.sidebar_width = width;
                    self.config.sidebar_collapsed = collapsed;
                    ctx.emit(Topic::TerminalConfig(self.config.clone()));
                }
                if let Some(id) = id {
                    self.main_window
                        .send_to_js(&json!({ "id": id, "result": "ok" }));
                }
            }
            _ => {
                let source = self.main_window.clone();
                let args = args.clone();
                self.dispatcher
                    .dispatch(cmd.to_string(), args, move |result| {
                        if let Some(id) = id {
                            source.send_to_js(&json!({ "id": id, "result": result }));
                        }
                    });
            }
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
        bus.on(TopicKind::TerminalConfig, Self::on_terminal_config);
        bus.on(TopicKind::TerminalSession, Self::on_terminal_session);
    }
}

impl TerminalApp {
    fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = delivery.topic else {
            return;
        };
        if app_id != Self::APP_ID {
            return;
        }
        match action_id.as_str() {
            "new_tab" => {
                self.main_window.send_to_js(&json!({"event": "new_tab"}));
            }
            id if id.starts_with("select_tab_") => {
                if let Ok(index) = id.strip_prefix("select_tab_").unwrap().parse::<usize>() {
                    self.main_window
                        .send_to_js(&json!({"event": "select_tab", "index": index}));
                }
            }
            "quit" => std::process::exit(0),
            _ => {
                tracing::debug!(action_id, "unknown menu action");
            }
        }
    }

    fn on_terminal_config(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::TerminalConfig(cfg) = delivery.topic else {
            return;
        };
        self.config = cfg.clone();
        self.push_state_to_js();
    }

    fn on_terminal_session(&mut self, delivery: &sola_bus::Delivery, ctx: &mut AppCtx) {
        let Topic::TerminalSession(session) = delivery.topic else {
            return;
        };

        if delivery.retracted {
            self.remove_tab(&session.id);
            self.refresh_view(ctx);
            return;
        }

        // Lazy tmux reconciliation: query live sessions on the first
        // non-retract delivery, then for each replayed tab whose tmux
        // session is gone, retract it instead of admitting it.
        let live = self
            .live_tmux
            .get_or_insert_with(|| tmux::list_sessions().unwrap_or_default().into_iter().collect());

        if !live.is_empty() && !live.contains(&session.tmux_session) {
            tracing::info!(
                id = %session.id,
                tmux = %session.tmux_session,
                "retracting stale tab (tmux session gone)"
            );
            ctx.retract(Topic::TerminalSession(session.clone()));
            return;
        }

        self.upsert_tab(session.clone());
        self.refresh_view(ctx);
    }

    /// Replace or insert a tab by id, keeping the in-memory mirror sorted
    /// by ordinal. Caller-provided ordinal wins (gaps are fine).
    fn upsert_tab(&mut self, session: TerminalSession) {
        let Ok(mut tabs) = self.state.tabs.try_write() else {
            tracing::warn!(
                id = %session.id,
                "skipped tab upsert (tabs locked); next event will refresh"
            );
            return;
        };
        if let Some(existing) = tabs.iter_mut().find(|t| t.pty_id == session.id) {
            existing.tmux_session = session.tmux_session;
            existing.cwd = session.cwd;
            existing.ordinal = session.ordinal;
        } else {
            tabs.push(state::TabEntry {
                pty_id: session.id,
                tmux_session: session.tmux_session,
                cwd: session.cwd,
                ordinal: session.ordinal,
            });
        }
        tabs.sort_by_key(|t| t.ordinal);
    }

    fn remove_tab(&mut self, id: &str) {
        let Ok(mut tabs) = self.state.tabs.try_write() else {
            tracing::warn!(id, "skipped tab remove (tabs locked); next event will refresh");
            return;
        };
        tabs.retain(|t| t.pty_id != id);
    }

    /// Re-emit the app menu (whose tab count changed) and push state to JS.
    fn refresh_view(&self, ctx: &mut AppCtx) {
        let count = self
            .state
            .tabs
            .try_read()
            .map(|t| t.len())
            .unwrap_or_default();
        ctx.emit(Topic::SetAppMenu(menu::terminal_menu(count)));
        self.push_state_to_js();
    }

    fn push_state_to_js(&self) {
        let Ok(tabs) = self.state.tabs.try_read() else {
            return;
        };
        let mapped: Vec<JsTab> = tabs
            .iter()
            .map(|t| JsTab {
                id: t.pty_id.clone(),
                tmux_session: t.tmux_session.clone(),
                cwd: t.cwd.clone(),
            })
            .collect();
        drop(tabs);
        let payload = state_payload(&mapped, &self.config);
        self.main_window
            .send_to_js(&json!({ "event": "state", "state": payload }));
    }
}

/// JS-facing tab shape. Mirrors the old `TerminalTab` so the frontend
/// keeps consuming `{id, tmux_session, cwd}` arrays — `ordinal` lives
/// only on the bus side.
#[derive(Serialize)]
struct JsTab {
    id: String,
    tmux_session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
}

fn state_payload(tabs: &[JsTab], config: &TerminalConfig) -> Value {
    json!({
        "tabs": tabs,
        "config": {
            "sidebar_width": config.sidebar_width,
            "sidebar_collapsed": config.sidebar_collapsed,
        },
    })
}

fn main() {
    sola_app::run::<TerminalApp>();
}
