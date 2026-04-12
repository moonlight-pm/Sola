use std::sync::Arc;

use sola_app::{asset_bundle, SolaApp};
use sola_bus::topics::{KeyEvent, Topic};

mod commands;
mod pty;
mod state;
mod tmux;

/// XKB keycode for T (evdev 20 + 8 = 28).
const KEY_T: u32 = 28;

fn main() {
    // tmux cleanup
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::reload_config();

    // Load restored state
    let restored_tabs = state::TerminalState::load_from_disk();
    let restored_json = serde_json::to_string(&restored_tabs).unwrap_or_default();

    let terminal_state = Arc::new(state::TerminalState::new());

    // Populate custom_titles from restored data
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
        .on_bus_event(|topic, send_to_js| {
            if let Topic::Key(KeyEvent {
                code: KEY_T,
                pressed: true,
                super_held: true,
                ..
            }) = topic
            {
                tracing::info!("Super+T: requesting new tab");
                send_to_js(serde_json::json!({"event": "new_tab"}));
            }
        })
        .run();
}
