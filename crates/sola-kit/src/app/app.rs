use serde::Deserialize;
use serde_json::{Value, json};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_core::KeyCode;
use sola_core::theme::Theme;
use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle, kit_default_theme,
};

// Storybook's app-specific assets — everything under `web/app/`
// served at the root of the app:// scheme. `index.html` and
// `index.tsx` come from `platform_assets()`; the vendored Remix v3
// source and every kit-shipped component (sidebar, button, root,
// stack, …) come from there too.
//
// The whole `web/app/` subtree is mounted at `/` so dropping a new
// file (e.g. another showcase) is a zero-touch addition — no entry
// in this asset bundle to maintain. The kit's built-in `index.tsx`
// imports `Main` via the bare specifier `@sola/app-root`, which the
// importmap injection wires to the URL declared on
// `KitApp::ROOT_COMPONENT` — `/main.tsx` by default.
static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/app");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

#[derive(Deserialize)]
struct ThemeSetArgs {
    theme: Theme,
}

pub struct KitApp {
    theme: Theme,
    /// Main storybook window — used to target devtools toggling from
    /// the shell's MenuAction dispatch.
    main_window: WindowHandle,
}

impl SolaApp for KitApp {
    const APP_ID: &'static str = "sola-kit";

    fn new(ctx: &mut AppCtx) -> Self {
        let theme = kit_default_theme();

        let main_window = ctx.add_window(WindowConfig {
            title: "Theme".into(),
            size: (1100, 720),
            position: None,
            decorated: false,
            transparent: false,
            assets: APP_ASSETS,
            zoned: true,
            keyboard_target: true,
        });

        // Publish the storybook's app menu to the shell so the menubar
        // can show "Sola Kit → Developer Tools" and register the F12
        // chord for the action.
        ctx.emit(Topic::SetAppMenu(kit_menu()));

        // Seed the bus with the default theme so the kit's bus-pump
        // has something to lower into CSS on (re)connect.
        //
        // TODO: move this emission out of the storybook. Other apps
        // need a theme even when sola-kit isn't running, so the
        // owner should be either sola-shell (already responsible for
        // persistent shell state) or sola-bus itself (a generic
        // "default value for a persistent topic" mechanism would let
        // any sticky topic seed itself from a Default impl). The
        // storybook is bootstrapping today because nothing else is.
        ctx.emit(Topic::Theme(theme.clone()));

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
            "ping" => json!({ "pong": true, "echo": args }),
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
        let Topic::Theme(theme) = delivery.topic else {
            return;
        };
        // Persisted replay or peer update: refresh in-memory copy. The
        // kit's bus pump (BusPumpTask::execute in lib.rs) pushes the
        // rendered CSS to every window before this handler runs — we
        // only mirror so future commands like `theme_reset` start from
        // the right baseline.
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
        self.theme = kit_default_theme();
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
            self.main_window.toggle_dev_tools();
        }
    }
}

fn kit_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: KitApp::APP_ID.into(),
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
