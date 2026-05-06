use serde::Deserialize;
use serde_json::{Value, json};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_core::KeyCode;
use sola_core::theme::Theme;
use sola_kit::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../../web/index.html"), Html),
    "/index.tsx" => (include_str!("../../web/index.tsx"), Tsx),
    "/components/Main.tsx" => (include_str!("../../web/components/Main.tsx"), Tsx),
};

#[derive(Deserialize)]
struct ThemeSetArgs {
    theme: Theme,
}

pub struct KitApplication {
    theme: Theme,
    main_window: WindowHandle,
}

impl SolaApp for KitApplication {
    const APP_ID: &'static str = "sola-kit";

    fn new(ctx: &mut AppCtx) -> Self {
        // (Was: force GTK dark theme so the WebKit inspector inherited it.
        // Removed during the CEF port — Chromium DevTools has its own theme
        // selector and doesn't follow a host toolkit setting.)

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
        let fonts = super::fonts::discover();
        let initial_state = serde_json::to_string(&serde_json::json!({
            "theme": &theme,
            "catalog": catalog_json,
            "fonts": fonts,
        }))
        .ok();

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

        ctx.emit(Topic::SetAppMenu(kit_menu()));

        Self { theme, main_window }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::CloseApp, Self::on_close_app);
        bus.on(TopicKind::Theme, Self::on_theme);
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
            "theme_set" => self.handle_theme_set(args, ctx),
            "theme_reset" => self.handle_theme_reset(ctx),
            _ => json!({ "error": format!("unknown command: {cmd}") }),
        };
        if let Some(id) = id {
            source.send_to_js(&json!({ "id": id, "result": result }));
        }
    }
}

impl KitApplication {
    fn on_theme(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Theme(theme) = delivery.topic else {
            return;
        };
        // Persisted replay or peer update: refresh in-memory copy. The
        // framework's bus loop is responsible for pushing the rendered
        // CSS to the JS side; we don't duplicate that here.
        self.theme = theme.clone();
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

    fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = delivery.topic else {
            return;
        };
        if app_id != Self::APP_ID {
            return;
        }
        if action_id == "open_devtools" {
            // E1 will wire this through self.main_window.browser().open_devtools(),
            // which spawns a second OSR Surface for Chromium DevTools.
            self.main_window.browser().open_devtools();
        }
    }
}

fn kit_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: KitApplication::APP_ID.into(),
        menus: vec![MenuDefinition {
            label: "Sola Kit".into(),
            items: vec![MenuItem::Action {
                id: "open_devtools".into(),
                label: "Developer Tools".into(),
                shortcut: Some(KeyCode::F12.chord()),
                disabled: false,
                checked: false,
            }],
        }],
    }
}
