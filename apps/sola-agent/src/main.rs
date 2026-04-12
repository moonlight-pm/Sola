use std::sync::Arc;

use sola_app::{asset_bundle, SolaApp};

mod agent;
mod auth;
mod bus_tools;
mod handler;
mod mcp_config;
mod session;

fn main() {
    // Load auth (exit early if no credentials)
    let auth = match auth::AuthManager::load() {
        Ok(a) => Arc::new(tokio::sync::RwLock::new(a)),
        Err(e) => {
            eprintln!("Failed to load Claude credentials: {:#}", e);
            eprintln!("Make sure you're logged into Claude Code (~/.claude/.credentials.json)");
            std::process::exit(1);
        }
    };

    let session_mgr = Arc::new(session::SessionManager::new());

    static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
        "/index.html" => (include_str!("../web/index.html"), Html),
        "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
        "/src/style.css" => (include_str!("../web/src/style.css"), Css),
    };

    let auth_for_handler = auth.clone();
    let session_mgr_for_handler = session_mgr.clone();

    SolaApp::builder()
        .app_id("sola-agent")
        .window_size(1400, 900)
        .decorated(false)
        .web_assets(APP_ASSETS)
        .handler(move |event_tx| handler::AgentHandler {
            auth: auth_for_handler.clone(),
            session_mgr: session_mgr_for_handler.clone(),
            event_tx,
            bus_client: None, // Set by bus_event handler if available
        })
        .run();
}
