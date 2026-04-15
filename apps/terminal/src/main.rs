use std::sync::Arc;

use serde_json::Value;
use sola_app::{AppCtx, AsyncDispatcher, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::topics::{AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic};
use sola_core::KeyCode;

mod commands;
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
    #[allow(dead_code)]
    state: Arc<state::TerminalState>,
}

impl SolaApp for TerminalApp {
    const APP_ID: &'static str = "sola-terminal";

    fn new(ctx: &mut AppCtx) -> Self {
        tmux::cleanup_stale_socket();
        tmux::kill_orphaned_clients();
        tmux::reload_config();

        let restored_tabs = state::TerminalState::load_from_disk();
        let restored_json = serde_json::to_string(&restored_tabs).unwrap_or_default();

        let terminal_state = Arc::new(state::TerminalState::new());
        {
            let mut titles = terminal_state.custom_titles.try_write().unwrap();
            for tab in &restored_tabs {
                if let Some(ref title) = tab.custom_title {
                    titles.insert(tab.tmux_session.clone(), title.clone());
                }
            }
        }

        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (1920, 1080),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(restored_json),
            zoned: true,
            keyboard_target: true,
        });

        // Bridge TerminalHandler's mpsc event channel to the main window's JS.
        let (event_tx, event_rx) = std::sync::mpsc::channel::<String>();
        let mw_for_events = main_window.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(5), move || {
            while let Ok(msg) = event_rx.try_recv() {
                mw_for_events.send_raw_json_to_js(&msg);
            }
            gtk4::glib::ControlFlow::Continue
        });

        let dispatcher = AsyncDispatcher::spawn(commands::TerminalHandler {
            state: terminal_state.clone(),
            event_tx,
        });

        // Register the terminal's app menu.
        ctx.emit_sticky(Topic::SetAppMenu(terminal_menu()));
        tracing::info!("registered terminal menu");

        Self {
            main_window,
            dispatcher,
            state: terminal_state,
        }
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

    fn on_bus_event(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic {
            if app_id != Self::APP_ID {
                return;
            }
            match action_id.as_str() {
                "new_tab" => {
                    tracing::info!("menu action: new tab");
                    self.main_window
                        .send_to_js(&serde_json::json!({"event": "new_tab"}));
                }
                "copy" => {
                    tracing::info!("menu action: copy");
                    self.main_window
                        .send_to_js(&serde_json::json!({"event": "copy"}));
                }
                "paste" => {
                    tracing::info!("menu action: paste");
                    self.main_window
                        .send_to_js(&serde_json::json!({"event": "paste"}));
                }
                id if id.starts_with("select_tab_") => {
                    if let Ok(index) = id.strip_prefix("select_tab_").unwrap().parse::<usize>() {
                        tracing::info!(index, "menu action: select tab");
                        self.main_window.send_to_js(
                            &serde_json::json!({"event": "select_tab", "index": index}),
                        );
                    }
                }
                _ => {
                    tracing::debug!(action_id, "unknown menu action");
                }
            }
        }
    }
}

fn main() {
    sola_app::run::<TerminalApp>();
}

fn terminal_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: TerminalApp::APP_ID.into(),
        menus: vec![
            MenuDefinition {
                label: "Terminal".into(),
                items: vec![
                    MenuItem::Action {
                        id: "about".into(),
                        label: "About Terminal".into(),
                        shortcut: None,
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Divider,
                    MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Terminal".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
            MenuDefinition {
                label: "Shell".into(),
                items: vec![MenuItem::Action {
                    id: "new_tab".into(),
                    label: "New Tab".into(),
                    shortcut: Some(KeyCode::T.meta()),
                    disabled: false,
                    checked: false,
                }],
            },
            MenuDefinition {
                label: "Edit".into(),
                items: vec![
                    MenuItem::Action {
                        id: "copy".into(),
                        label: "Copy".into(),
                        shortcut: Some(KeyCode::C.meta()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Action {
                        id: "paste".into(),
                        label: "Paste".into(),
                        shortcut: Some(KeyCode::V.meta()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
        ],
    }
}
