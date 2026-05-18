//! Monitor app — kit-side implementation.
//!
//! One window. Taps every bus message via `on_raw_bus_message` and
//! forwards a decoded JSON event to the frontend. Owns one sticky
//! topic (`MonitorConfig`) so the sidebar width survives restart.

use serde_json::{Value, json};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, MonitorConfig,
    Topic, TopicKind,
};
use sola_bus::{Delivery, Message};
use sola_core::KeyCode;
use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};

use crate::decode;

static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

pub struct MonitorApp {
    main_window: WindowHandle,
    config: MonitorConfig,
}

impl SolaApp for MonitorApp {
    const APP_ID: &'static str = "sola-monitor";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (900, 600),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            zoned: false,
            keyboard_target: true,
            root_component: None,
            initial_state: None,
        });

        ctx.emit(Topic::SetAppMenu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![MenuDefinition {
                label: "Monitor".into(),
                items: vec![MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Monitor".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                }],
            }],
        }));

        tracing::info!("sola-monitor ready");

        Self {
            main_window,
            config: MonitorConfig::default(),
        }
    }

    fn on_raw_bus_message(&mut self, msg: &Message, _ctx: &mut AppCtx) {
        let event = decode::message_to_json(msg);
        self.main_window.send_to_js(&event);
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.subscribe_all();
        bus.on(TopicKind::MonitorConfig, Self::on_config);
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        source: &WindowHandle,
        ctx: &mut AppCtx,
    ) {
        let result = match cmd {
            "monitor_set_sidebar_width" => {
                if let Some(w) = args.get("width").and_then(|v| v.as_u64()) {
                    self.config.sidebar_width = w as u32;
                    ctx.emit(Topic::MonitorConfig(self.config.clone()));
                    json!(null)
                } else {
                    json!({ "error": "missing or invalid width" })
                }
            }
            _ => {
                tracing::warn!(cmd, "unknown command");
                json!({ "error": format!("unknown command: {cmd}") })
            }
        };

        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    }
}

impl MonitorApp {
    fn on_config(&mut self, d: &Delivery, _ctx: &mut AppCtx) {
        if let Topic::MonitorConfig(cfg) = d.topic {
            self.config = cfg.clone();
            self.main_window.send_to_js(&json!({
                "event": "state",
                "sidebar_width": self.config.sidebar_width,
            }));
        }
    }

    fn on_menu_action(&mut self, d: &Delivery, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = d.topic
            && app_id == Self::APP_ID
            && action_id == "quit"
        {
            std::process::exit(0);
        }
    }
}
