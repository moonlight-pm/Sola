use serde_json::Value;
use sola_app::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::Message;
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_core::KeyCode;

mod decode;

const DEFAULT_SIDEBAR_WIDTH: i32 = 240;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

struct MonitorApp {
    main_window: WindowHandle,
}

impl SolaApp for MonitorApp {
    const APP_ID: &'static str = "sola-monitor";

    fn new(ctx: &mut AppCtx) -> Self {
        let initial_state =
            serde_json::json!({ "sidebar_width": DEFAULT_SIDEBAR_WIDTH }).to_string();

        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (900, 600),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(initial_state),
            zoned: false,
            keyboard_target: false,
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

        Self { main_window }
    }

    fn on_raw_bus_message(&mut self, msg: &Message, _ctx: &mut AppCtx) {
        let event = decode::message_to_json(msg);
        self.main_window.send_to_js(&event);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        _id: Option<u64>,
        _source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        if cmd == "save_sidebar_width" {
            // TODO: migrate to persistent Topic::MonitorConfig (Phase 7).
            let _ = args.get("width").and_then(|v| v.as_i64());
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.subscribe_all();
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
    }
}

impl MonitorApp {
    fn on_menu_action(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic {
            if app_id == Self::APP_ID && action_id == "quit" {
                std::process::exit(0);
            }
        }
    }
}

fn main() {
    sola_app::run::<MonitorApp>();
}
