use serde_json::Value;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::Message;
use sola_bus::topics::Topic;

mod decode;

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
        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (900, 600),
            position: None,
            decorated: true,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: None,
            zoned: false,
            keyboard_target: false,
        });

        tracing::info!("sola-monitor ready");

        Self { main_window }
    }

    fn on_raw_bus_message(&mut self, msg: &Message, _ctx: &mut AppCtx) {
        let event = decode::message_to_json(msg);
        self.main_window.send_to_js(&event);
    }

    fn on_js_command(
        &mut self,
        _cmd: &str,
        _args: &Value,
        _id: Option<u64>,
        _source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
    }

    fn on_bus_event(&mut self, _topic: &Topic, _ctx: &mut AppCtx) {
    }
}

fn main() {
    sola_app::run::<MonitorApp>();
}
