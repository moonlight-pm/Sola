use std::sync::Arc;

use serde_json::{Value, json};
use sola_app::{AppCtx, AsyncDispatcher, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::topics::{MenuActionPayload, OpenUrlRequest, Topic, TopicKind};

mod config;
mod handler;
mod idle;
mod imap;
mod menu;
mod rules;
mod sender;
mod state;
mod wicket;

use handler::MailHandler;
use state::MailState;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
};

struct MailApp {
    main_window: WindowHandle,
    dispatcher: AsyncDispatcher,
}

impl SolaApp for MailApp {
    const APP_ID: &'static str = "sola-mail";

    fn new(ctx: &mut AppCtx) -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (1280, 820),
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

        let state = Arc::new(MailState::new(event_tx));
        let dispatcher = AsyncDispatcher::spawn(MailHandler { state });

        // Fire-and-forget startup auto-connect.
        dispatcher.dispatch("mail_connect".into(), json!({}), |_| {});

        ctx.emit_sticky(Topic::SetAppMenu(menu::mail_menu()));

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
        ctx: &mut AppCtx,
    ) {
        if cmd == "open_url" {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() {
                tracing::warn!("open_url command with empty url");
                return;
            }
            tracing::info!(url, "open_url");
            ctx.emit(Topic::OpenUrl(OpenUrlRequest {
                url: url.to_string(),
                activate: true,
            }));
            return;
        }

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

impl MailApp {
    fn on_menu_action(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic {
            if app_id == Self::APP_ID && action_id == "quit" {
                std::process::exit(0);
            }
        }
    }
}

fn main() {
    sola_app::run::<MailApp>();
}
