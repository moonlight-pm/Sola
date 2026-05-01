use sola_kit::{AppCtx, BusRegistry, SolaApp, WindowConfig, asset_bundle};

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/app/index.html"), Html),
    "/src/main.ts" => (include_str!("../../web/app/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../../web/app/src/app.ts"), TypeScript),
    "/src/app.css" => (include_str!("../../web/app/src/app.css"), Css),
};

pub struct KitApp;

impl SolaApp for KitApp {
    const APP_ID: &'static str = "sola-kit";

    fn new(ctx: &mut AppCtx) -> Self {
        ctx.add_window(WindowConfig {
            title: "Theme".into(),
            size: (1100, 720),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: None,
            zoned: true,
            keyboard_target: true,
        });
        Self
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(sola_bus::topics::TopicKind::CloseApp, Self::on_close_app);
    }
}
