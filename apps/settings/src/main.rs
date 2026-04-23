use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Value, json};
use sola_app::config::{JsonConfig, JsonConfigIn};
use sola_app::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
    Window as BusWindow,
};
use sola_core::KeyCode;
use sola_core::applications::{Application, ApplicationsConfig, command_exists};
use sola_core::config::mail::{MailConfig, MailRule, MailRuleCondition};

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

#[derive(Deserialize)]
struct MailAccountArgs {
    email: String,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct MailRuleArgs {
    name: String,
    action: String,
    #[serde(default)]
    dest: Option<String>,
    conditions: Vec<MailRuleCondition>,
}

#[derive(Deserialize)]
struct MailRemoveRuleArgs {
    index: usize,
}

struct SettingsApp {
    applications: ApplicationsConfig,
    mail: MailConfig,
    main_window: WindowHandle,
    /// Latest `Windows` snapshot from the bus. Used to compute the list of
    /// running-but-not-configured candidates for the UI.
    running: Vec<BusWindow>,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let mut applications = ApplicationsConfig::load();
        if applications.normalize() {
            applications.save();
        }
        let mail = MailConfig::load();
        let initial_state =
            serde_json::to_string(&state_payload(&applications, &[], &mail)).unwrap_or_default();

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

        ctx.emit_sticky(Topic::SetAppMenu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![MenuDefinition {
                label: "Settings".into(),
                items: vec![MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Settings".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                }],
            }],
        }));

        tracing::info!("sola-settings ready");

        Self {
            applications,
            mail,
            main_window,
            running: Vec::new(),
        }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::CloseApp, Self::on_close_app);
        bus.on(TopicKind::Windows, Self::on_windows);
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
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
            "mail_save_account" => self.handle_mail_save_account(args),
            "mail_add_rule" => self.handle_mail_add_rule(args),
            "mail_remove_rule" => self.handle_mail_remove_rule(args),
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
        self.current_state()
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
        self.current_state()
    }

    fn handle_remove(&mut self, args: &Value) -> Value {
        let args: RemoveArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        self.applications.remove(&args.app_id);
        self.applications.save();
        self.current_state()
    }

    fn handle_mail_save_account(&mut self, args: &Value) -> Value {
        let args: MailAccountArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        self.mail.email = args.email;
        self.mail.imap_host = args.imap_host;
        self.mail.imap_port = args.imap_port;
        self.mail.smtp_host = args.smtp_host;
        self.mail.smtp_port = args.smtp_port;
        self.mail.username = args.username;
        self.mail.password = args.password;
        self.mail.save();
        self.current_state()
    }

    fn handle_mail_add_rule(&mut self, args: &Value) -> Value {
        let args: MailRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if args.name.trim().is_empty() {
            return json!({ "error": "rule name is required" });
        }
        if args.conditions.is_empty() {
            return json!({ "error": "at least one condition is required" });
        }
        let dest = if args.action == "move" {
            args.dest.as_ref().filter(|d| !d.trim().is_empty()).cloned()
        } else {
            None
        };
        self.mail.rules.push(MailRule {
            name: args.name,
            action: args.action,
            dest,
            conditions: args.conditions,
        });
        self.mail.save();
        self.current_state()
    }

    fn handle_mail_remove_rule(&mut self, args: &Value) -> Value {
        let args: MailRemoveRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if args.index < self.mail.rules.len() {
            self.mail.rules.remove(args.index);
            self.mail.save();
        }
        self.current_state()
    }

    fn on_menu_action(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = topic
            && app_id == Self::APP_ID
            && action_id == "quit"
        {
            std::process::exit(0);
        }
    }

    fn on_windows(&mut self, topic: &Topic, _ctx: &mut AppCtx) {
        let Topic::Windows(windows) = topic else {
            return;
        };
        self.running = windows.clone();
        self.main_window.send_to_js(&json!({
            "event": "state",
            "state": state_payload(&self.applications, &self.running, &self.mail),
        }));
    }

    fn current_state(&self) -> Value {
        state_payload(&self.applications, &self.running, &self.mail)
    }
}

/// Full view the JS side renders from: configured applications (with
/// missing/candidate hints) and the mail config.
fn state_payload(cfg: &ApplicationsConfig, running: &[BusWindow], mail: &MailConfig) -> Value {
    let missing: Vec<&str> = cfg
        .apps
        .iter()
        .filter(|a| !command_exists(&a.command))
        .map(|a| a.app_id.as_str())
        .collect();

    let configured: HashSet<&str> = cfg.apps.iter().map(|a| a.app_id.as_str()).collect();
    let mut seen = HashSet::new();
    let candidates: Vec<Value> = running
        .iter()
        .filter(|a| !configured.contains(a.app_id.as_str()))
        .filter(|a| !is_system_app(&a.app_id))
        .filter(|a| seen.insert(a.app_id.clone()))
        .map(|a| {
            let suggested = a.pid.and_then(resolve_binary_for_pid);
            json!({
                "app_id": a.app_id,
                "title": a.title,
                "suggested_command": suggested,
            })
        })
        .collect();

    json!({
        "applications": {
            "apps": cfg.apps,
            "missing": missing,
            "candidates": candidates,
        },
        "mail": mail,
    })
}

/// Best-effort resolution of a PID to the command the user most likely
/// typed to launch it. Order:
///
/// 1. `readlink /proc/<pid>/exe` (strip Linux's `" (deleted)"` suffix
///    that appears when the binary has been replaced since launch)
/// 2. If the resolved exe is a well-known sandbox/runtime launcher
///    (`bwrap`, `flatpak-spawn`, AppImage `AppRun`, …), fall back to
///    `argv[0]` from `/proc/<pid>/cmdline` — that's closer to the user's
///    intent than the sandbox runner.
fn resolve_binary_for_pid(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let exe_str = exe.to_string_lossy().into_owned();
    let cleaned = exe_str
        .strip_suffix(" (deleted)")
        .unwrap_or(&exe_str)
        .to_string();

    let file_name = Path::new(&cleaned)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if is_sandbox_runner(&file_name) {
        if let Some(argv0) = cmdline_argv0(pid) {
            return Some(argv0);
        }
    }
    Some(cleaned)
}

/// App IDs that are part of Sola itself and should never appear as
/// "running, not configured" candidates — adding them to
/// `applications.json` would let the launcher spawn duplicates.
fn is_system_app(app_id: &str) -> bool {
    matches!(app_id, "sola-shell")
}

fn is_sandbox_runner(name: &str) -> bool {
    matches!(
        name,
        "bwrap" | "flatpak-spawn" | "flatpak" | "AppRun" | "snap" | "snap-confine"
    )
}

fn cmdline_argv0(pid: u32) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let first: &[u8] = data.split(|&b| b == 0).next()?;
    if first.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(first).into_owned())
}

fn main() {
    sola_app::run::<SettingsApp>();
}
