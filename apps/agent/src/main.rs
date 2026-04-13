use std::sync::Arc;

use sola_app::{asset_bundle, SolaApp};

mod agent;
mod handler;
mod session;
mod storage;

fn main() {
    let session_mgr = Arc::new(session::SessionManager::new());

    static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
        "/index.html" => (include_str!("../web/index.html"), Html),
        "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
        "/src/style.css" => (include_str!("../web/src/style.css"), Css),
    };

    let session_mgr_for_handler = session_mgr.clone();

    SolaApp::builder()
        .app_id("sola-agent")
        .window_size(1400, 900)
        .decorated(false)
        .web_assets(APP_ASSETS)
        .handler(move |event_tx| handler::AgentHandler {
            session_mgr: session_mgr_for_handler.clone(),
            event_tx,
        })
        .run();
}
