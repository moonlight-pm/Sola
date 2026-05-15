//! Settings app entrypoint. Owns one window backed by `web/main.tsx`.
//! State + bus handlers will land in Task 4.

use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};

static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

pub struct SettingsApp {
    main_window: WindowHandle,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "Settings".into(),
            size: (900, 620),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            zoned: true,
            keyboard_target: true,
        });
        tracing::info!("sola-settings ready (kit)");
        Self { main_window }
    }

    fn register_bus(&mut self, _bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {}
}
