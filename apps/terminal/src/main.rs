use std::sync::Arc;

use gtk4::prelude::*;
use sola_app::{asset_bundle, SolaApp};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic,
    WindowPolicy, WindowPolicyPayload,
};

mod commands;
mod pty;
mod state;
mod tmux;

fn main() {
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

    let state_for_handler = terminal_state.clone();

    SolaApp::builder()
        .app_id("sola-terminal")
        .window_size(1920, 1080)
        .decorated(false)
        .web_assets(APP_ASSETS)
        .initial_state(&restored_json)
        .handler(move |event_tx| commands::TerminalHandler {
            state: state_for_handler.clone(),
            event_tx,
        })
        .on_bus_event(|topic, send_to_js, _emit| {
            if let Topic::MenuAction(MenuActionPayload {
                app_id,
                action_id,
            }) = topic
            {
                if app_id != "sola-terminal" {
                    return;
                }
                match action_id.as_str() {
                    "new_tab" => {
                        tracing::info!("menu action: new tab");
                        send_to_js(serde_json::json!({"event": "new_tab"}));
                    }
                    "copy" => {
                        tracing::info!("menu action: copy");
                        send_to_js(serde_json::json!({"event": "copy"}));
                    }
                    "paste" => {
                        tracing::info!("menu action: paste");
                        send_to_js(serde_json::json!({"event": "paste"}));
                    }
                    id if id.starts_with("select_tab_") => {
                        if let Ok(index) = id.strip_prefix("select_tab_").unwrap().parse::<usize>()
                        {
                            tracing::info!(index, "menu action: select tab");
                            send_to_js(
                                serde_json::json!({"event": "select_tab", "index": index}),
                            );
                        }
                    }
                    _ => {
                        tracing::debug!(action_id, "unknown menu action");
                    }
                }
            }
        })
        .on_activate(|window, _webview, bus| {
            window.set_title(Some("main"));
            let mut client = bus.borrow_mut();
            let _ = client.emit_sticky(Topic::SetWindowPolicy(WindowPolicyPayload {
                app_id: "sola-terminal".into(),
                windows: vec![WindowPolicy {
                    title: "main".into(),
                    zoned: true,
                    keyboard_target: true,
                    size: None,
                    position: None,
                }],
            }));
            let _ = client.emit_sticky(Topic::SetAppMenu(terminal_menu()));
            tracing::info!("advertised terminal policy and menu");
        })
        .run();
}

fn terminal_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: "sola-terminal".into(),
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
                        shortcut: Some("Super+Q".into()),
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
                    shortcut: Some("Super+T".into()),
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
                        shortcut: Some("Super+C".into()),
                        disabled: false,
                        checked: false,
                    },
                    MenuItem::Action {
                        id: "paste".into(),
                        label: "Paste".into(),
                        shortcut: Some("Super+V".into()),
                        disabled: false,
                        checked: false,
                    },
                ],
            },
        ],
    }
}
