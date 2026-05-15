//! Settings app — kit-side implementation.
//!
//! One window, two sticky-replayed topics it owns (`Application` +
//! `MailConfig`), bus-driven state push via `__solaRecv`. The
//! "running but not configured" candidates list is derived from
//! the `Windows` topic on every state push.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};
use sola_bus::topics::{
    AppMenuPayload, ApplicationsConfig, MailConfig, MailRule, MailRuleCondition,
    MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
    Window as BusWindow,
};
use sola_core::Encrypted;
use sola_core::KeyCode;
use sola_core::applications::Application;
use sola_kit::{
    AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle,
};

use crate::procfs;

static APP_DIR: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web");

static APP_ASSETS: &sola_kit::AssetBundle = &asset_bundle! {
    @dir "/" => &APP_DIR,
};

// ---------- JS command argument types ----------

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
struct MailUpdateRuleArgs {
    index: usize,
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

// ---------- App state ----------

pub struct SettingsApp {
    applications: ApplicationsConfig,
    mail: MailConfig,
    main_window: WindowHandle,
    running: Vec<BusWindow>,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self {
        let applications = ApplicationsConfig::default();
        let mail = MailConfig::default();

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

        ctx.emit(Topic::SetAppMenu(AppMenuPayload {
            app_id: Self::APP_ID.into(),
            menus: vec![
                MenuDefinition {
                    label: "Settings".into(),
                    items: vec![MenuItem::Action {
                        id: "quit".into(),
                        label: "Quit Settings".into(),
                        shortcut: Some(KeyCode::Q.meta()),
                        disabled: false,
                        checked: false,
                    }],
                },
                MenuDefinition {
                    label: "Edit".into(),
                    items: vec![
                        MenuItem::Action {
                            id: "cut".into(),
                            label: "Cut".into(),
                            shortcut: Some(KeyCode::X.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Action {
                            id: "copy".into(),
                            label: "Copy".into(),
                            shortcut: Some(KeyCode::C.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Action {
                            id: "paste".into(),
                            label: "Paste".into(),
                            shortcut: Some(KeyCode::V.meta()),
                            disabled: false,
                            checked: false,
                        },
                        MenuItem::Divider,
                        MenuItem::Action {
                            id: "select_all".into(),
                            label: "Select All".into(),
                            shortcut: Some(KeyCode::A.meta()),
                            disabled: false,
                            checked: false,
                        },
                    ],
                },
            ],
        }));

        tracing::info!("sola-settings ready (kit)");

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
        bus.on(TopicKind::MailConfig, Self::on_mail_config);
        bus.on(TopicKind::Application, Self::on_application);
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
            "applications_add" => self.handle_add(args, ctx),
            "applications_update" => self.handle_update(args, ctx),
            "applications_remove" => self.handle_remove(args, ctx),
            "mail_save_account" => self.handle_mail_save_account(args, ctx),
            "mail_add_rule" => self.handle_mail_add_rule(args, ctx),
            "mail_update_rule" => self.handle_mail_update_rule(args, ctx),
            "mail_remove_rule" => self.handle_mail_remove_rule(args, ctx),
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
    fn on_close_app(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        if let Topic::CloseApp(app_id) = delivery.topic
            && app_id == Self::APP_ID
        {
            std::process::exit(0);
        }
    }

    fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = delivery.topic
            && app_id == Self::APP_ID
        {
            match action_id.as_str() {
                "quit" => std::process::exit(0),
                "cut" => self.main_window.cut(),
                "copy" => self.main_window.copy(),
                "paste" => self.main_window.paste(),
                "select_all" => self.main_window.select_all(),
                _ => {}
            }
        }
    }

    fn on_windows(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Windows(windows) = delivery.topic else {
            return;
        };
        self.running = windows.clone();
        self.push_state();
    }

    fn on_mail_config(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::MailConfig(cfg) = delivery.topic else {
            return;
        };
        self.mail = cfg.clone();
        self.push_state();
    }

    fn on_application(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Application(app) = delivery.topic else {
            return;
        };
        if delivery.retracted {
            self.applications.remove(&app.app_id);
        } else {
            self.applications.remove(&app.app_id);
            self.applications.apps.push(app.clone());
        }
        self.push_state();
    }

    // ---- Applications handlers ----

    fn handle_add(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: AddArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let mut new_app = Application {
            app_id: args.app_id,
            label: args.label,
            command: args.command,
            icon: args.icon,
        };
        new_app.normalize();
        if let Err(e) = self.applications.add(new_app.clone()) {
            return json!({ "error": e.to_string() });
        }
        ctx.emit(Topic::Application(new_app));
        self.current_state()
    }

    fn handle_update(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: UpdateArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        let mut new_app = Application {
            app_id: args.app_id,
            label: args.label,
            command: args.command,
            icon: args.icon,
        };
        new_app.normalize();
        let old_app_id = args.old_app_id;
        let id_changed = old_app_id != new_app.app_id;
        let prev = self.applications.get(&old_app_id).cloned();
        if let Err(e) = self.applications.update(&old_app_id, new_app.clone()) {
            return json!({ "error": e.to_string() });
        }
        if id_changed
            && let Some(old) = prev
        {
            ctx.retract(Topic::Application(old));
        }
        ctx.emit(Topic::Application(new_app));
        self.current_state()
    }

    fn handle_remove(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: RemoveArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if let Some(removed) = self.applications.get(&args.app_id).cloned() {
            self.applications.remove(&args.app_id);
            ctx.retract(Topic::Application(removed));
        }
        self.current_state()
    }

    // ---- Mail handlers ----

    fn handle_mail_save_account(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
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
        self.mail.password = Encrypted(args.password);
        ctx.emit(Topic::MailConfig(self.mail.clone()));
        self.current_state()
    }

    fn handle_mail_add_rule(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if let Err(e) = validate_rule(&args.name, &args.conditions) {
            return json!({ "error": e });
        }
        let dest = normalize_dest(&args.action, args.dest.as_deref());
        self.mail.rules.push(MailRule {
            name: args.name,
            action: args.action,
            dest,
            conditions: args.conditions,
        });
        ctx.emit(Topic::MailConfig(self.mail.clone()));
        self.current_state()
    }

    fn handle_mail_update_rule(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailUpdateRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if args.index >= self.mail.rules.len() {
            return json!({ "error": format!("rule index {} out of range", args.index) });
        }
        if let Err(e) = validate_rule(&args.name, &args.conditions) {
            return json!({ "error": e });
        }
        let dest = normalize_dest(&args.action, args.dest.as_deref());
        self.mail.rules[args.index] = MailRule {
            name: args.name,
            action: args.action,
            dest,
            conditions: args.conditions,
        };
        ctx.emit(Topic::MailConfig(self.mail.clone()));
        self.current_state()
    }

    fn handle_mail_remove_rule(&mut self, args: &Value, ctx: &mut AppCtx) -> Value {
        let args: MailRemoveRuleArgs = match serde_json::from_value(args.clone()) {
            Ok(a) => a,
            Err(e) => return json!({ "error": e.to_string() }),
        };
        if args.index < self.mail.rules.len() {
            self.mail.rules.remove(args.index);
            ctx.emit(Topic::MailConfig(self.mail.clone()));
        }
        self.current_state()
    }

    // ---- State plumbing ----

    fn current_state(&self) -> Value {
        state_payload(&self.applications, &self.running, &self.mail)
    }

    /// Push the latest state to the JS frontend as a `state` event.
    fn push_state(&self) {
        let mut payload = self.current_state();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("event".into(), json!("state"));
        }
        self.main_window.send_to_js(&payload);
    }
}

/// Shared validation for `mail_add_rule` and `mail_update_rule`.
fn validate_rule(
    name: &str,
    conditions: &[MailRuleCondition],
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("rule name is required".into());
    }
    if conditions.is_empty() {
        return Err("at least one condition is required".into());
    }
    Ok(())
}

fn normalize_dest(action: &str, dest: Option<&str>) -> Option<String> {
    if action != "move" {
        return None;
    }
    dest.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn mail_for_js(mail: &MailConfig) -> Value {
    json!({
        "email": mail.email,
        "imap_host": mail.imap_host,
        "imap_port": mail.imap_port,
        "smtp_host": mail.smtp_host,
        "smtp_port": mail.smtp_port,
        "username": mail.username,
        "password": mail.password.0,
        "rules": mail.rules,
    })
}

fn state_payload(
    cfg: &ApplicationsConfig,
    running: &[BusWindow],
    mail: &MailConfig,
) -> Value {
    let missing: Vec<&str> = cfg
        .apps
        .iter()
        .filter(|a| procfs::command_missing(&a.command))
        .map(|a| a.app_id.as_str())
        .collect();

    let configured: HashSet<&str> = cfg.apps.iter().map(|a| a.app_id.as_str()).collect();
    let mut seen = HashSet::new();
    let candidates: Vec<Value> = running
        .iter()
        .filter(|a| !configured.contains(a.app_id.as_str()))
        .filter(|a| !procfs::is_system_app(&a.app_id))
        .filter(|a| seen.insert(a.app_id.clone()))
        .map(|a| {
            let suggested = procfs::suggest_command(&a.app_id, a.pid);
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
        "mail": mail_for_js(mail),
    })
}
