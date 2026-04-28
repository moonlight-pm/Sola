use std::sync::Arc;

use serde_json::{Value, json};
use sola_app::{
    AppCtx, AsyncDispatcher, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};
use sola_bus::topics::{
    MenuActionPayload, OpenUrlRequest, TerminalConfig, TerminalSessions, TerminalTab, Topic,
    TopicKind,
};

mod commands;
mod menu;
mod pty;
mod state;
mod tmux;

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
    sessions_synced: bool,
}

impl SolaApp for TerminalApp {
    const APP_ID: &'static str = "sola-terminal";

    fn new(ctx: &mut AppCtx) -> Self {
        tmux::cleanup_stale_socket();
        tmux::kill_orphaned_clients();
        tmux::reload_config();

        let terminal_state = Arc::new(state::TerminalState::new());

        // Initial JS state: empty tabs + default config. The bus replays
        // the persisted TerminalConfig and TerminalSessions into our
        // handlers a few ms after subscription, and we push the real
        // state to JS at that point.
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

        // Bridge dispatcher → bus for topic emits. AppCtx is GTK-thread-bound
        // (Rc<RefCell<BusClient>>), so we can't share it with the tokio
        // runtime. Instead, the handler sends Topics through this channel
        // and the GTK main loop drains them via ctx.emit.
        let (emit_tx, emit_rx) = std::sync::mpsc::channel::<Topic>();
        let ctx_proxy = ctx.bus_proxy();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(topic) = emit_rx.try_recv() {
                ctx_proxy.emit(topic);
            }
            gtk4::glib::ControlFlow::Continue
        });

        let dispatcher = AsyncDispatcher::spawn(commands::TerminalHandler {
            state: terminal_state.clone(),
            event_tx,
            emit_tx,
        });

        ctx.emit(Topic::SetAppMenu(menu::terminal_menu(0)));
        tracing::info!("registered terminal menu");

        Self {
            main_window,
            dispatcher,
            state: terminal_state,
            config: TerminalConfig::default(),
            sessions_synced: false,
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
        bus.on(TopicKind::TerminalSessions, Self::on_terminal_sessions);
    }
}

impl TerminalApp {
    fn on_menu_action(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic else {
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

    fn on_terminal_config(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::TerminalConfig(cfg) = topic else {
            return;
        };
        self.config = cfg.clone();
        self.push_state_to_js();
    }

    fn on_terminal_sessions(&mut self, topic: &Topic, ctx: &mut AppCtx) {
        let Topic::TerminalSessions(sessions) = topic else {
            return;
        };

        // First replay: reconcile against live tmux. Drop tabs whose tmux
        // session is gone; preserve ordering and cwds for survivors. Re-emit
        // the cleaned set (only on this first replay) so the disk record
        // converges with reality.
        let reconciled: Vec<TerminalTab> = if !self.sessions_synced {
            self.sessions_synced = true;
            let live: std::collections::HashSet<String> = tmux::list_sessions()
                .map(|v| v.into_iter().collect())
                .unwrap_or_default();
            let kept: Vec<TerminalTab> = sessions
                .tabs
                .iter()
                .filter(|t| live.is_empty() || live.contains(&t.tmux_session))
                .cloned()
                .collect();
            if kept.len() != sessions.tabs.len() {
                ctx.emit(Topic::TerminalSessions(TerminalSessions {
                    tabs: kept.clone(),
                }));
            }
            kept
        } else {
            sessions.tabs.clone()
        };

        // Sync to in-memory TerminalState mirror.
        let entries: Vec<state::TabEntry> = reconciled
            .iter()
            .map(|t| state::TabEntry {
                pty_id: t.id.clone(),
                tmux_session: t.tmux_session.clone(),
                cwd: t.cwd.clone(),
            })
            .collect();
        match self.state.tabs.try_write() {
            Ok(mut tabs) => *tabs = entries,
            Err(_) => tracing::warn!(
                "skipped state mirror sync (tabs locked); next event will refresh"
            ),
        }

        // Re-emit menu reflecting the reconciled count.
        ctx.emit(Topic::SetAppMenu(menu::terminal_menu(reconciled.len())));

        // Push fresh state to JS.
        let payload = state_payload(&reconciled, &self.config);
        self.main_window
            .send_to_js(&json!({ "event": "state", "state": payload }));
    }

    fn push_state_to_js(&self) {
        let Ok(tabs) = self.state.tabs.try_read() else {
            return;
        };
        let mapped: Vec<TerminalTab> = tabs
            .iter()
            .map(|t| TerminalTab {
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

fn state_payload(tabs: &[TerminalTab], config: &TerminalConfig) -> Value {
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
