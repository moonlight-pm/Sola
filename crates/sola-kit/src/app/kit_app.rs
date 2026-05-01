use serde::Deserialize;
use serde_json::{Value, json};
use sola_bus::topics::{Topic, TopicKind};
use sola_core::theme::Theme;
use sola_kit::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/app/index.html"), Html),
    "/src/main.ts" => (include_str!("../../web/app/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../../web/app/src/app.ts"), TypeScript),
    "/src/app.css" => (include_str!("../../web/app/src/app.css"), Css),
    "/src/sidebar.ts" => (include_str!("../../web/app/src/sidebar.ts"), TypeScript),
    "/src/token-edit.ts" => (include_str!("../../web/app/src/token-edit.ts"), TypeScript),
    "/src/preview/tokens-colors.ts" => (include_str!("../../web/app/src/preview/tokens-colors.ts"), TypeScript),
};

#[derive(Deserialize)]
struct ThemeSetArgs {
    theme: Theme,
}

pub struct KitApp {
    theme: Theme,
    main_window: WindowHandle,
}

impl SolaApp for KitApp {
    const APP_ID: &'static str = "sola-kit";

    fn new(ctx: &mut AppCtx) -> Self {
        let theme = Theme::default();
        use super::catalog::{CATALOG, Group};
        let catalog_json: Vec<serde_json::Value> = CATALOG
            .iter()
            .map(|e| serde_json::json!({
                "name": e.name,
                "group": match e.group { Group::Atom => "atom", Group::Component => "component" },
                "tokens": e.tokens,
            }))
            .collect();
        let initial_state = serde_json::to_string(&serde_json::json!({
            "theme": &theme,
            "catalog": catalog_json,
        })).ok();

        let main_window = ctx.add_window(WindowConfig {
            title: "Theme".into(),
            size: (1100, 720),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            initial_state,
            zoned: true,
            keyboard_target: true,
        });

        // Publish current theme so any pre-existing subscribers see something
        // immediately. The bus persistence layer replays the stored Theme over
        // this on first subscribe; the order doesn't matter — the persisted
        // value wins.
        ctx.emit(Topic::Theme(theme.clone()));

        Self { theme, main_window }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::CloseApp, Self::on_close_app);
        bus.on(TopicKind::Theme, Self::on_theme);
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
            "theme_set" => self.handle_theme_set(args, ctx),
            "theme_reset" => self.handle_theme_reset(ctx),
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        };
        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    }
}

impl KitApp {
    fn on_theme(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Theme(theme) = delivery.topic else { return };
        // Persisted replay or peer update: refresh in-memory copy.
        self.theme = theme.clone();
        // Push to the JS frontend so its mirror updates too.
        self.main_window.send_to_js(&json!({
            "event": "theme",
            "vars": self.theme.to_css_vars(),
        }));
    }

    fn handle_theme_set(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let parsed: ThemeSetArgs = match serde_json::from_value(args.clone()) {
            Ok(p) => p,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        self.theme = parsed.theme;
        ctx.emit(Topic::Theme(self.theme.clone()));
        json!({ "ok": true })
    }

    fn handle_theme_reset(&mut self, ctx: &mut AppCtx) -> Value {
        self.theme = Theme::default();
        ctx.emit(Topic::Theme(self.theme.clone()));
        json!({ "ok": true })
    }
}
