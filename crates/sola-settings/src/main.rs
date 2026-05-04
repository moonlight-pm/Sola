use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Value, json};
use sola_app::{AppCtx, BusRegistry, SolaApp, WindowConfig, WindowHandle, asset_bundle};
use sola_bus::topics::{
    AppMenuPayload, ApplicationsConfig, MailConfig, MailRule, MailRuleCondition, MenuActionPayload,
    MenuDefinition, MenuItem, Topic, TopicKind, Window as BusWindow,
};
use sola_core::Encrypted;
use sola_core::KeyCode;
use sola_core::applications::{Application, command_exists, is_builtin, resolve_in_path};

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
        // Both Applications and MailConfig start empty; the bus replays
        // the persisted stickies for `Topic::Applications` and
        // `Topic::MailConfig` to us once we subscribe.
        let applications = ApplicationsConfig::default();
        let mail = MailConfig::default();
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

        ctx.emit(Topic::SetAppMenu(AppMenuPayload {
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
        // Renaming `app_id` changes the keyed slot — retract under the
        // old key first so `state.toml` doesn't end up with two
        // `[[Application]]` records for the same logical app.
        if id_changed {
            if let Some(old) = prev {
                ctx.retract(Topic::Application(old));
            }
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

    fn on_mail_config(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::MailConfig(cfg) = delivery.topic else {
            return;
        };
        self.mail = cfg.clone();
        self.send_state_event();
    }

    fn on_application(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Application(app) = delivery.topic else {
            return;
        };
        if delivery.retracted {
            self.applications.remove(&app.app_id);
        } else {
            // Sticky replay or peer update — upsert by app_id. We may be
            // receiving our own emit back; that's idempotent.
            self.applications.remove(&app.app_id);
            self.applications.apps.push(app.clone());
        }
        self.send_state_event();
    }

    fn on_menu_action(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(MenuActionPayload { app_id, action_id }) = delivery.topic
            && app_id == Self::APP_ID
            && action_id == "quit"
        {
            std::process::exit(0);
        }
    }

    fn on_windows(&mut self, delivery: &sola_bus::Delivery, _ctx: &mut AppCtx) {
        let Topic::Windows(windows) = delivery.topic else {
            return;
        };
        self.running = windows.clone();
        self.send_state_event();
    }

    fn current_state(&self) -> Value {
        state_payload(&self.applications, &self.running, &self.mail)
    }

    /// Push the latest state to the JS frontend as a `state` event.
    /// Spreads the payload's fields (`applications`, `mail`) at the top
    /// level alongside `event` — matches the convention used by
    /// `sola-terminal` and what the JS handler in `web/src/app.ts` reads.
    fn send_state_event(&self) {
        let mut payload = self.current_state();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("event".into(), json!("state"));
        }
        self.main_window.send_to_js(&payload);
    }
}

/// View of the mail config that the editor UI receives. The bus-side
/// `Encrypted<String>` would encrypt on JSON serialization (JSON is
/// human-readable), but the user just typed the password — they need
/// to see it. We expose the cleartext explicitly here.
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
            let suggested = suggest_command(&a.app_id, a.pid);
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

/// Best-effort suggestion of a launch command for a window we just
/// noticed. Order:
///
/// 1. **Search `PATH` using names derived from `app_id`.** This wins
///    for distro-packaged and NixOS-wrapped apps where the launcher is
///    a shell wrapper named after the app (e.g. Bitwarden's
///    `/run/current-system/sw/bin/bitwarden`). The wrapper sets up
///    library paths and arguments — invoking the raw binary it dispatches
///    to (`/nix/store/.../electron …`) bypasses that setup and the app
///    won't start the same way.
/// 2. **Procfs fallback.** When no `PATH` match exists (AppImage in
///    `~/Applications`, custom build, etc.), use the running process's
///    `/proc/<pid>/exe` or `/proc/<pid>/cmdline`. See
///    [`resolve_binary_for_pid`].
fn suggest_command(app_id: &str, pid: Option<u32>) -> Option<String> {
    if let Some(path) = resolve_from_app_id(app_id) {
        return Some(path);
    }
    pid.and_then(resolve_binary_for_pid)
}

/// Try every plausible `PATH` name derived from `app_id`. Returns the
/// absolute path of the first match. Tried in order:
///
/// 1. `app_id` lowercased verbatim — handles plain names like
///    `Bitwarden` → `bitwarden`.
/// 2. The last `.`-segment, lowercased — handles reverse-DNS forms
///    like `org.mozilla.Firefox` → `firefox`,
///    `dev.zed.Zed` → `zed`.
/// 3. The second-to-last `.`-segment — handles the
///    `com.<brand>.platform` shape (e.g. `com.bitwarden.desktop`)
///    where the brand sits in the middle.
fn resolve_from_app_id(app_id: &str) -> Option<String> {
    let trimmed = app_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut tried: Vec<String> = Vec::new();
    let try_name = |name: &str, tried: &mut Vec<String>| -> Option<String> {
        if name.is_empty() || tried.iter().any(|t| t == name) {
            return None;
        }
        tried.push(name.to_string());
        resolve_in_path(name).map(|p| p.to_string_lossy().into_owned())
    };

    if let Some(hit) = try_name(&trimmed.to_ascii_lowercase(), &mut tried) {
        return Some(hit);
    }
    let segments: Vec<&str> = trimmed.split('.').collect();
    if segments.len() > 1 {
        let last = segments[segments.len() - 1].to_ascii_lowercase();
        if let Some(hit) = try_name(&last, &mut tried) {
            return Some(hit);
        }
        let second = segments[segments.len() - 2].to_ascii_lowercase();
        if let Some(hit) = try_name(&second, &mut tried) {
            return Some(hit);
        }
    }
    None
}

/// `/proc`-based fallback used when the `PATH` lookup in
/// [`suggest_command`] finds nothing. See that function's docs for the
/// overall ordering. Within this fallback:
///
/// 1. `readlink /proc/<pid>/exe`. The kernel exposes this symlink mode
///    0700 (root-owned) when the process has `PR_SET_DUMPABLE=0` —
///    Electron, Chromium, and other sandboxed apps set that even when
///    the process itself runs as the user. In that case `read_link`
///    fails and we fall through.
///    We also strip Linux's `" (deleted)"` suffix that appears when
///    the binary has been replaced since launch.
/// 2. If the resolved exe is a wrapper / multi-arg launcher
///    (`bwrap`, `flatpak-spawn`, AppImage `AppRun`, `electron`, …),
///    return positional `argv[0..N]` from `/proc/<pid>/cmdline` —
///    just `argv[0]` would be the wrapper itself, which can't launch
///    the app on its own.
/// 3. Otherwise, return positional `argv` from `cmdline`. This handles
///    sandboxed apps whose `/proc/<pid>/exe` is unreadable.
fn resolve_binary_for_pid(pid: u32) -> Option<String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let cleaned = exe.map(|p| {
        let s = p.to_string_lossy().into_owned();
        s.strip_suffix(" (deleted)")
            .map(str::to_string)
            .unwrap_or(s)
    });

    let file_name = cleaned.as_deref().and_then(|c| {
        Path::new(c)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    });

    let need_cmdline = file_name
        .as_deref()
        .is_none_or(is_multi_arg_launcher);
    if need_cmdline {
        return cmdline_positional(pid);
    }
    cleaned
}

/// App IDs that are part of Sola itself and should never appear as
/// "running, not configured" candidates — adding them to the
/// applications list would let the launcher spawn duplicates. Covers
/// the shell itself plus every built-in (Settings, Monitor, ...).
fn is_system_app(app_id: &str) -> bool {
    app_id == "sola-shell" || is_builtin(app_id)
}

/// Binaries whose own path doesn't identify any specific app — the
/// user-meaningful command is in `argv[1..]` (e.g. `electron app.asar`,
/// `bwrap … realbin`, `flatpak run com.app.Foo`).
fn is_multi_arg_launcher(name: &str) -> bool {
    matches!(
        name,
        "bwrap"
            | "flatpak-spawn"
            | "flatpak"
            | "AppRun"
            | "snap"
            | "snap-confine"
            | "electron"
    )
}

/// Return `/proc/<pid>/cmdline`'s leading positional arguments
/// (everything up to the first arg that starts with `-`), joined by
/// spaces. That covers the common shapes:
/// - `firefox` → `firefox`
/// - `electron app.asar --flag …` → `electron app.asar`
/// - `bwrap … --bind … realbin --foo` → just `bwrap` (still better than
///   nothing; the user can edit the suggestion before saving).
///
/// Spaces inside individual args are preserved verbatim — TOML/JSON
/// quoting is the user's concern when they paste the suggestion into
/// the command field.
fn cmdline_positional(pid: u32) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let parts: Vec<&[u8]> = data
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut take = 1;
    for arg in &parts[1..] {
        if arg.first() == Some(&b'-') {
            break;
        }
        take += 1;
    }
    let joined: Vec<String> = parts[..take]
        .iter()
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Some(joined.join(" "))
}

fn main() {
    sola_app::run::<SettingsApp>();
}
