use std::sync::Arc;

use serde_json::Value;
use sola_app::{AppCtx, AsyncDispatcher, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind};
use sola_core::KeyCode;

mod active;
mod agent;
mod handler;
mod session;
mod sync;
mod storage;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/style.css" => (include_str!("../web/src/style.css"), Css),
    "/vendor/marked.mjs" => (include_str!("../web/vendor/marked.mjs"), JavaScript),
};

struct AgentApp {
    main_window: WindowHandle,
    dispatcher: AsyncDispatcher,
}

impl SolaApp for AgentApp {
    const APP_ID: &'static str = "sola-agent";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (1400, 900),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: None,
            zoned: true,
            keyboard_target: true,
        });

        // Bridge handler events (mpsc) into the main window's JS.
        let (event_tx, event_rx) = std::sync::mpsc::channel::<String>();
        let mw_for_events = main_window.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(msg) = event_rx.try_recv() {
                mw_for_events.send_raw_json_to_js(&msg);
            }
            gtk4::glib::ControlFlow::Continue
        });

        // Poll for terminal-live sessions; emit an event on change.
        let active_tx = event_tx.clone();
        std::thread::spawn(move || {
            let mut last: Option<std::collections::HashSet<String>> = None;
            loop {
                let cur = active::detect();
                if last.as_ref() != Some(&cur) {
                    let mut ids: Vec<&String> = cur.iter().collect();
                    ids.sort();
                    let payload = serde_json::json!({
                        "event": "active_sessions",
                        "ids": ids,
                    }).to_string();
                    if active_tx.send(payload).is_err() { break; }
                    last = Some(cur);
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });

        let session_mgr = Arc::new(session::SessionManager::new());
        let process_mgr = Arc::new(tokio::sync::Mutex::new(agent::ClaudeProcessManager::new()));
        let dispatcher = AsyncDispatcher::spawn(handler::AgentHandler {
            session_mgr,
            event_tx: event_tx.clone(),
            process_mgr,
        });

        // Background sync of CLI sessions. Rebuilds stale view models
        // (CLI JSONL newer than our cli_synced_at) and emits progress
        // events so the frontend can show an indicator.
        let sync_tx = event_tx.clone();
        std::thread::spawn(move || {
            sync::run_sync(&sync_tx);
        });

        ctx.emit_sticky(Topic::SetAppMenu(agent_menu()));

        Self {
            main_window,
            dispatcher,
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        _source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        let source = self.main_window.clone();
        let args = args.clone();
        self.dispatcher
            .dispatch(cmd.to_string(), args, move |result| {
                if let Some(id) = id {
                    source.send_to_js(&serde_json::json!({ "id": id, "result": result }));
                }
            });
    }

}

impl AgentApp {
    fn on_menu_action(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic {
            if app_id == Self::APP_ID && action_id == "quit" {
                std::process::exit(0);
            }
        }
    }
}

fn main() {
    sola_app::run::<AgentApp>();
}

fn agent_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: AgentApp::APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Agent".into(),
                items: vec![
                    MenuItem::Action {
                        id: "about".into(),
                        label: "About Agent".into(),
                        shortcut: None,
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Agent".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
        ],
    }
}
