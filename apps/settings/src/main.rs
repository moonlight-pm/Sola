use sola_app::config::JsonConfigIn;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_applications::ApplicationsConfig;

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

struct SettingsApp {
    #[allow(dead_code)]
    main_window: WindowHandle,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let applications = ApplicationsConfig::load();
        let initial_state = serde_json::to_string(&serde_json::json!({
            "apps": applications.apps,
        }))
        .unwrap_or_default();

        let main_window = ctx.add_window(WindowConfig {
            title: "Settings".into(),
            size: (760, 560),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state: Some(initial_state),
            zoned: true,
            keyboard_target: true,
        });

        tracing::info!("sola-settings ready");

        Self { main_window }
    }
}

fn main() {
    sola_app::run::<SettingsApp>();
}
