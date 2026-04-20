use serde::Deserialize;
use serde_json::{Value, json};
use sola_app::config::JsonConfigIn;
use sola_app::{AppCtx, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_core::applications::{Application, ApplicationsConfig};

static APP_ASSETS: &sola_app::AssetBundle = &asset_bundle! {
    "/index.html" => (include_str!("../web/index.html"), Html),
    "/src/main.ts" => (include_str!("../web/src/main.ts"), TypeScript),
    "/src/app.ts" => (include_str!("../web/src/app.ts"), TypeScript),
    "/src/theme.css" => (include_str!("../web/src/theme.css"), Css),
};

#[derive(Deserialize)]
struct AddArgs {
    app_id: String,
    label: String,
    command: String,
    icon: String,
}

#[derive(Deserialize)]
struct UpdateArgs {
    old_app_id: String,
    app_id: String,
    label: String,
    command: String,
    icon: String,
}

#[derive(Deserialize)]
struct RemoveArgs {
    app_id: String,
}

struct SettingsApp {
    applications: ApplicationsConfig,
    #[allow(dead_code)]
    main_window: WindowHandle,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let applications = ApplicationsConfig::load();
        let initial_state = serde_json::to_string(&json!({
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

        Self {
            applications,
            main_window,
        }
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        let result = match cmd {
            "applications_add" => self.handle_add(args),
            "applications_update" => self.handle_update(args),
            "applications_remove" => self.handle_remove(args),
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

impl SettingsApp {
    fn handle_add(&mut self, args: &Value) -> Value {
        let args: AddArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if let Err(e) = self.applications.add(Application {
            app_id: args.app_id,
            label: args.label,
            command: args.command,
            icon: args.icon,
        }) {
            return json!({ "error": e.to_string() });
        }
        self.applications.save();
        json!(self.applications.apps)
    }

    fn handle_update(&mut self, args: &Value) -> Value {
        let args: UpdateArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if let Err(e) = self.applications.update(
            &args.old_app_id,
            Application {
                app_id: args.app_id,
                label: args.label,
                command: args.command,
                icon: args.icon,
            },
        ) {
            return json!({ "error": e.to_string() });
        }
        self.applications.save();
        json!(self.applications.apps)
    }

    fn handle_remove(&mut self, args: &Value) -> Value {
        let args: RemoveArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        self.applications.remove(&args.app_id);
        self.applications.save();
        json!(self.applications.apps)
    }
}

fn main() {
    sola_app::run::<SettingsApp>();
}
