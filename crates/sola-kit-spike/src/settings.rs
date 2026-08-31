//! HTML/CSS Settings (lab). Distinct from iced `sola-settings`.
//!
//! Identity: `sola-settings-lab` / `Settings (lab)`. Speaks the same
//! bus topics (Theme, Application, MailConfig, Windows, CloseApp).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use sola_bus::topics::{
    AppMenuPayload, Application, ApplicationsConfig, MailConfig, MailRule, MailRuleCondition,
    MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind, Window as BusWindow,
    WindowFloating,
};
use sola_bus::{BusClient, Message};
use sola_core::Encrypted;
use sola_core::KeyCode;
use sola_core::applications::{command_exists, resolve_in_path};

use crate::app::Click;
use crate::components::{Sidebar, SidebarItem};
use crate::css::{Sheet, parse_sheet};
use crate::gpu::Quad;
use crate::host::Surface;
use crate::icons::Icons;
use crate::layout::{PaintItem, hit_test, hover_at, layout_tree};
use crate::markup::{self};
use crate::paint::{Fonts, PaintPass, paint_glyphs};

pub const APP_ID: &str = "sola-settings-lab";
pub const WINDOW_TITLE: &str = "Settings (lab)";

const CSS: &str = include_str!("../assets/kit.css");
const HTML: &str = include_str!("../assets/settings.html");
const APPS_HTML: &str = include_str!("../assets/settings-apps.html");
const MAIL_HTML: &str = include_str!("../assets/settings-mail.html");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Apps,
    Mail,
}

pub struct Settings {
    css_w: f32,
    css_h: f32,
    scale: f32,
    sheet: Sheet,
    fonts: Fonts,
    hover: Option<u32>,
    icons: Icons,
    last_items: Vec<PaintItem>,
    scrolls: HashMap<String, f32>,
    focused: Option<String>,
    /// Char index in the focused field.
    caret: usize,
    /// Selection as char indices (anchor, caret). None = caret only.
    sel: Option<(usize, usize)>,
    scroll_x: f32,
    caret_blink_at: f32,
    last_input_click: Option<(String, Instant, u8)>,
    fields: HashMap<String, String>,
    time: f32,
    panel: Panel,
    apps: ApplicationsConfig,
    mail: MailConfig,
    running: Vec<BusWindow>,
    selected: Option<String>,
    draft: bool,
    error: String,
    mail_error: String,
    bus: BusClient,
    assets: PathBuf,
    html: String,
    css_path: PathBuf,
    html_path: PathBuf,
    css_mtime: Option<SystemTime>,
    html_mtime: Option<SystemTime>,
    apps_html: String,
    mail_html: String,
    apps_html_path: PathBuf,
    mail_html_path: PathBuf,
    apps_html_mtime: Option<SystemTime>,
    mail_html_mtime: Option<SystemTime>,
    window_ids: HashMap<String, u32>,
    floating: HashSet<u32>,
}

pub fn run() {
    crate::host::run_with(
        APP_ID,
        WINDOW_TITLE,
        Box::new(Settings::new(960.0, 680.0, 1.0)),
    );
}

impl Settings {
    fn new(css_w: f32, css_h: f32, scale: f32) -> Self {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let css_path = assets.join("kit.css");
        let html_path = assets.join("settings.html");
        let (sheet, css_mtime) = match std::fs::read_to_string(&css_path) {
            Ok(s) => {
                let m = std::fs::metadata(&css_path)
                    .ok()
                    .and_then(|m| m.modified().ok());
                (parse_sheet(&s), m)
            }
            Err(_) => (parse_sheet(CSS), None),
        };
        let (html, html_mtime) = load_text(&html_path, HTML);
        let apps_html_path = assets.join("settings-apps.html");
        let mail_html_path = assets.join("settings-mail.html");
        let (apps_html, apps_html_mtime) = load_text(&apps_html_path, APPS_HTML);
        let (mail_html, mail_html_mtime) = load_text(&mail_html_path, MAIL_HTML);
        let mut bus = BusClient::new();
        bus.set_app_id(APP_ID);
        bus.connect_blocking(Duration::from_millis(250));
        let _ = bus.subscribe(TopicKind::ALL);
        let _ = bus.emit(Topic::SetAppMenu(AppMenuPayload {
            app_id: APP_ID.into(),
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
        tracing::info!(app_id = APP_ID, "bus connected");
        Self {
            css_w,
            css_h,
            scale: scale.max(0.01),
            sheet,
            fonts: Fonts::new(),
            hover: None,
            icons: Icons::new(),
            last_items: Vec::new(),
            scrolls: HashMap::new(),
            focused: None,
            caret: 0,
            sel: None,
            scroll_x: 0.0,
            caret_blink_at: 0.0,
            last_input_click: None,
            fields: HashMap::new(),
            time: 0.0,
            panel: Panel::Apps,
            apps: ApplicationsConfig::default(),
            mail: MailConfig::default(),
            running: Vec::new(),
            selected: None,
            draft: false,
            error: String::new(),
            mail_error: String::new(),
            bus,
            assets,
            html,
            css_path,
            html_path,
            css_mtime,
            html_mtime,
            apps_html,
            mail_html,
            apps_html_path,
            mail_html_path,
            apps_html_mtime,
            mail_html_mtime,
            window_ids: HashMap::new(),
            floating: HashSet::new(),
        }
    }

    fn is_floating(&self) -> bool {
        self.window_ids
            .values()
            .any(|id| self.floating.contains(id))
    }

    fn set_focus(&mut self, id: Option<String>) {
        if self.focused != id {
            self.scroll_x = 0.0;
            self.sel = None;
        }
        self.focused = id;
        self.caret = self
            .focused
            .as_ref()
            .and_then(|k| self.fields.get(k))
            .map(|s| s.chars().count())
            .unwrap_or(0);
        self.ping_caret();
    }

    fn ping_caret(&mut self) {
        self.caret_blink_at = self.time;
    }

    fn delete_sel(&mut self) -> bool {
        let Some(id) = self.focused.clone() else {
            return false;
        };
        let Some((a, b)) = self.sel.take() else {
            return false;
        };
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == hi {
            return false;
        }
        let val = self.fields.entry(id).or_default();
        let start = char_byte(val, lo);
        let end = char_byte(val, hi);
        val.replace_range(start..end, "");
        self.caret = lo;
        true
    }

    fn caret_index_at(
        &mut self,
        x: f32,
        field: &str,
        box_x: f32,
        pad_l: f32,
        size: f32,
        weight: u16,
        family: &str,
    ) -> usize {
        let text = self.fields.get(field).map(|s| s.as_str()).unwrap_or("");
        let rel = (x - box_x - pad_l + self.scroll_x).max(0.0);
        let chars: Vec<char> = text.chars().collect();
        let mut best = 0usize;
        let mut best_d = f32::MAX;
        for i in 0..=chars.len() {
            let prefix: String = chars[..i].iter().collect();
            let w = self.fonts.measure_width(&prefix, size, weight, family);
            let d = (w - rel).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
            if w > rel {
                break;
            }
        }
        best
    }

    fn sync_scroll(&mut self, vis: f32, caret_px: f32, text_w: f32) {
        let margin = 3.0;
        let vis = vis.max(8.0);
        if caret_px - self.scroll_x > vis - margin {
            self.scroll_x = caret_px - (vis - margin);
        }
        if caret_px - self.scroll_x < margin {
            self.scroll_x = (caret_px - margin).max(0.0);
        }
        let max_scroll = (text_w - vis + margin).max(0.0);
        self.scroll_x = self.scroll_x.clamp(0.0, max_scroll);
    }

    fn caret_px(&mut self) -> Option<(u32, f32)> {
        let id = self.focused.as_deref()?;
        let (uid, origin, vis, size, weight, family) = {
            let item = self.last_items.iter().find(|i| {
                i.data_id.as_deref() == Some(id) && i.classes.iter().any(|c| c == "input")
            })?;
            let run = item.text.as_ref();
            (
                item.uid,
                item.x + item.pad[3],
                (item.w - item.pad[1] - item.pad[3]).max(8.0),
                run.map(|r| r.size).unwrap_or(13.0),
                run.map(|r| r.weight).unwrap_or(400),
                run.map(|r| r.family.clone())
                    .unwrap_or_else(|| "SF Pro Text".into()),
            )
        };
        let text = self.fields.get(id).cloned().unwrap_or_default();
        let prefix: String = text.chars().take(self.caret).collect();
        let caret_w = self.fonts.measure_width(&prefix, size, weight, &family);
        let text_w = self.fonts.measure_width(&text, size, weight, &family);
        self.sync_scroll(vis, caret_w, text_w);
        let on = (self.time - self.caret_blink_at).rem_euclid(1.0) < 0.5;
        if !on {
            return None;
        }
        Some((uid, origin + caret_w - self.scroll_x))
    }

    fn sel_px(&mut self) -> Option<(u32, f32, f32)> {
        let id = self.focused.as_deref()?;
        let (a, b) = self.sel?;
        if a == b {
            return None;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        let (uid, origin, size, weight, family) = {
            let item = self.last_items.iter().find(|i| {
                i.data_id.as_deref() == Some(id) && i.classes.iter().any(|c| c == "input")
            })?;
            let run = item.text.as_ref();
            (
                item.uid,
                item.x + item.pad[3],
                run.map(|r| r.size).unwrap_or(13.0),
                run.map(|r| r.weight).unwrap_or(400),
                run.map(|r| r.family.clone())
                    .unwrap_or_else(|| "SF Pro Text".into()),
            )
        };
        let text = self.fields.get(id).cloned().unwrap_or_default();
        let left: String = text.chars().take(lo).collect();
        let right: String = text.chars().take(hi).collect();
        let x0 = origin + self.fonts.measure_width(&left, size, weight, &family) - self.scroll_x;
        let x1 = origin + self.fonts.measure_width(&right, size, weight, &family) - self.scroll_x;
        Some((uid, x0, x1))
    }

    fn click_count(&mut self, id: &str) -> u8 {
        let now = Instant::now();
        match &self.last_input_click {
            Some((oid, t, n)) if oid == id && now.duration_since(*t).as_millis() < 400 => {
                let n = (*n).saturating_add(1).min(3);
                self.last_input_click = Some((id.to_string(), now, n));
                n
            }
            _ => {
                self.last_input_click = Some((id.to_string(), now, 1));
                1
            }
        }
    }

    fn select_word_at(&mut self, field: &str, caret: usize) {
        let text = self.fields.get(field).cloned().unwrap_or_default();
        let (a, b) = word_range(&text, caret);
        self.sel = Some((a, b));
        self.caret = b;
    }

    fn sync_float_windows(&mut self) {
        self.window_ids.clear();
        let mut live = HashSet::new();
        for w in &self.running {
            live.insert(w.window_id);
            if w.app_id == APP_ID {
                self.window_ids.insert(w.title.clone(), w.window_id);
            }
        }
        self.floating.retain(|id| live.contains(id));
    }

    fn handle_bus(&mut self, msg: Message) -> bool {
        let Some(topic) = Topic::parse(&msg) else {
            return false;
        };
        match topic {
            Topic::Theme(t) => {
                self.sheet.apply_bus_theme(&t);
                true
            }
            Topic::Application(app) => {
                self.apps.remove(&app.app_id);
                if msg.sticky {
                    self.apps.apps.push(app);
                }
                true
            }
            Topic::MailConfig(cfg) => {
                self.mail = cfg;
                self.sync_mail_fields();
                true
            }
            Topic::Windows(windows) => {
                self.running = windows;
                self.sync_float_windows();
                true
            }
            Topic::WindowFloating(WindowFloating {
                window_id,
                floating,
            }) => {
                if floating {
                    self.floating.insert(window_id);
                } else {
                    self.floating.remove(&window_id);
                }
                true
            }
            Topic::MenuAction(MenuActionPayload { app_id, action_id })
                if app_id == APP_ID && action_id == "quit" =>
            {
                std::process::exit(0);
            }
            Topic::CloseApp(id) if id == APP_ID => {
                std::process::exit(0);
            }
            _ => false,
        }
    }

    fn sync_mail_fields(&mut self) {
        self.fields.insert("email".into(), self.mail.email.clone());
        self.fields
            .insert("imap_host".into(), self.mail.imap_host.clone());
        self.fields
            .insert("imap_port".into(), self.mail.imap_port.to_string());
        self.fields
            .insert("smtp_host".into(), self.mail.smtp_host.clone());
        self.fields
            .insert("smtp_port".into(), self.mail.smtp_port.to_string());
        self.fields
            .insert("username".into(), self.mail.username.clone());
        self.fields
            .insert("password".into(), self.mail.password.0.clone());
    }

    fn load_detail_fields(&mut self) {
        if self.draft {
            return;
        }
        if let Some(a) = self.selected.clone() {
            if let Some(app) = self.apps.get(&a) {
                self.fields.insert("app_id".into(), app.app_id.clone());
                self.fields.insert("label".into(), app.label.clone());
                self.fields.insert("command".into(), app.command.clone());
                self.fields.insert("icon".into(), app.icon.clone());
            }
        }
    }

    fn rebuild(&mut self) {
        let heading = match self.panel {
            Panel::Apps => "Applications",
            Panel::Mail => "Mail",
        };
        self.fields.insert("heading".into(), heading.into());
        self.fields
            .insert("app-count".into(), format!("{} apps", self.apps.apps.len()));
        if self.mail_error.is_empty() {
            self.fields.remove("mail-error");
        } else {
            self.fields
                .insert("mail-error".into(), self.mail_error.clone());
        }
        let demo = match self.panel {
            Panel::Apps => self.apps_html.as_str(),
            Panel::Mail => self.mail_html.as_str(),
        };
        let mut root = markup::expand(&self.html, &[], WINDOW_TITLE, heading, "", demo, "", false);
        if !self.is_floating() {
            markup::hide_if(&mut root, |el| {
                el.data_id.as_deref() == Some("csd") || el.classes.iter().any(|c| c == "titlebar")
            });
        }
        let mut next = markup::next_uid(&root);
        let sb = Sidebar::new([
            SidebarItem::header("SETTINGS"),
            SidebarItem::new("apps", "Applications")
                .action("panel")
                .active(self.panel == Panel::Apps),
            SidebarItem::new("mail", "Mail")
                .action("panel")
                .active(self.panel == Panel::Mail),
        ])
        .class("sidebar-settings")
        .nav_class("settings-nav")
        .build(&mut next);
        markup::replace_slot(&mut root, "sidebar", sb);
        markup::walk_mut(&mut root, &mut |el| {
            if self.is_floating() && el.classes.iter().any(|c| c == "app") {
                markup::add_class(el, "is-float");
            }
            if el.classes.iter().any(|c| c == "stage") {
                markup::add_class(
                    el,
                    if self.panel == Panel::Mail {
                        "is-mail"
                    } else {
                        "is-apps"
                    },
                );
            }
        });
        if self.panel == Panel::Apps {
            self.fill_apps(&mut root);
        } else {
            self.fill_rules(&mut root);
        }
        markup::apply_fields(&mut root, &self.fields);
        markup::apply_focus(&mut root, self.focused.as_deref());
        self.last_items = layout_tree(
            &root,
            &self.sheet,
            self.hover,
            self.css_w,
            self.css_h,
            &mut self.fonts,
            &self.scrolls,
        );
    }

    fn fill_apps(&mut self, root: &mut crate::dom::Elem) {
        let mut next = 9000u32;
        let mut rows = Vec::new();
        let mut apps: Vec<Application> = self.apps.apps.clone();
        apps.sort_by(|a, b| {
            a.label
                .to_ascii_lowercase()
                .cmp(&b.label.to_ascii_lowercase())
                .then(a.app_id.cmp(&b.app_id))
        });
        if apps.is_empty() && self.selected.is_none() && !self.draft {
            let mut empty = markup::node(&mut next, &["empty-block"], None, None, "");
            empty.children.push(markup::node(
                &mut next,
                &["t-body", "t-muted"],
                None,
                None,
                "No applications yet",
            ));
            empty.children.push(markup::node(
                &mut next,
                &["t-caption"],
                None,
                None,
                "Add one above, or configure a running app below.",
            ));
            rows.push(empty);
        }
        for a in &apps {
            let mut classes = vec!["list-row"];
            if self.selected.as_deref() == Some(a.app_id.as_str()) && !self.draft {
                classes.push("is-active");
            }
            let title = if a.label.trim().is_empty() {
                a.app_id.as_str()
            } else {
                a.label.as_str()
            };
            let mut hit =
                markup::node(&mut next, &classes, Some("app-select"), Some(&a.app_id), "");
            hit.children
                .push(markup::node(&mut next, &["t-body"], None, None, title));
            if !command_exists(&a.command) {
                hit.children.push(markup::node(
                    &mut next,
                    &["badge", "badge-warning"],
                    None,
                    None,
                    "not found",
                ));
            }
            let rm = markup::node(
                &mut next,
                &["btn", "btn-sm", "btn-danger-outline"],
                Some("app-remove"),
                Some(&a.app_id),
                "Remove",
            );
            let mut row = markup::node(&mut next, &["app-row"], None, None, "");
            row.children.push(hit);
            row.children.push(rm);
            rows.push(row);
        }
        markup::fill_slot(root, "app-rows", rows);

        let configured: std::collections::HashSet<_> =
            self.apps.apps.iter().map(|a| a.app_id.clone()).collect();
        let mut cands = Vec::new();
        for w in &self.running {
            if configured.contains(&w.app_id) || is_system_app(&w.app_id) {
                continue;
            }
            if cands.iter().any(|c: &BusWindow| c.app_id == w.app_id) {
                continue;
            }
            cands.push(w.clone());
        }
        let mut crow = Vec::new();
        if cands.is_empty() {
            markup::hide_if(root, |el| el.data_id.as_deref() == Some("cand-wrap"));
        } else {
            for w in &cands {
                let cmd = suggest_command(&w.app_id, w.pid);
                let title = if w.title.trim().is_empty() {
                    "(no title)"
                } else {
                    w.title.as_str()
                };
                let detail = match &cmd {
                    Some(c) => format!("{title} · {c}"),
                    None => format!("{title} · command unknown — fill in manually"),
                };
                let mut copy = markup::node(&mut next, &["cand-copy"], None, None, "");
                copy.children
                    .push(markup::node(&mut next, &["t-body"], None, None, &w.app_id));
                copy.children
                    .push(markup::node(&mut next, &["t-caption"], None, None, &detail));
                let cfg = markup::node(
                    &mut next,
                    &["btn", "btn-sm", "btn-ghost"],
                    Some("app-cand"),
                    Some(&w.app_id),
                    "Configure",
                );
                let mut row = markup::node(&mut next, &["cand-row"], None, None, "");
                row.children.push(copy);
                row.children.push(cfg);
                crow.push(row);
            }
        }
        markup::fill_slot(root, "cand-rows", crow);

        let mut detail = Vec::new();
        if self.selected.is_none() && !self.draft {
            let mut empty = markup::node(&mut next, &["empty-center"], None, None, "");
            empty.children.push(markup::node(
                &mut next,
                &["t-sub", "t-muted"],
                None,
                None,
                "No selection",
            ));
            empty.children.push(markup::node(
                &mut next,
                &["t-caption"],
                None,
                None,
                "Select an app from the list, or add a new one.",
            ));
            detail.push(empty);
        } else {
            let heading = if self.draft {
                "New application".to_string()
            } else {
                self.fields
                    .get("label")
                    .filter(|s| !s.trim().is_empty())
                    .cloned()
                    .or_else(|| self.fields.get("app_id").cloned())
                    .unwrap_or_else(|| "Detail".into())
            };
            detail.push(markup::node(
                &mut next,
                &["card-title"],
                None,
                None,
                &heading,
            ));
            if self.draft {
                detail.push(markup::node(
                    &mut next,
                    &["t-caption"],
                    None,
                    None,
                    "Fill in identity and launch command.",
                ));
            } else if let Some(id) = &self.selected {
                detail.push(markup::node(&mut next, &["t-caption"], None, None, id));
            }
            for (lab, id) in [
                ("App ID", "app_id"),
                ("Label", "label"),
                ("Command", "command"),
                ("Icon", "icon"),
            ] {
                let mut field =
                    markup::node(&mut next, &["stack-field"], Some("focus"), Some(id), "");
                field
                    .children
                    .push(markup::node(&mut next, &["stack-label"], None, None, lab));
                let mut input = markup::node(&mut next, &["input"], Some("focus"), Some(id), "");
                input.data_bind = Some(id.into());
                field.children.push(input);
                detail.push(field);
            }
            if !self.error.is_empty() {
                detail.push(markup::node(
                    &mut next,
                    &["help-danger"],
                    None,
                    None,
                    &self.error,
                ));
            }
            let mut footer = markup::node(&mut next, &["btn-row"], None, None, "");
            footer.children.push(markup::node(
                &mut next,
                &["btn", "btn-sm", "btn-primary"],
                Some("app-save"),
                None,
                if self.draft { "Add" } else { "Save" },
            ));
            footer.children.push(markup::node(
                &mut next,
                &["btn", "btn-sm", "btn-ghost"],
                Some("app-discard"),
                None,
                "Discard",
            ));
            footer
                .children
                .push(markup::node(&mut next, &["spacer"], None, None, ""));
            footer.children.push(markup::node(
                &mut next,
                &["btn", "btn-sm", "btn-ghost"],
                Some("app-close"),
                None,
                "Close",
            ));
            detail.push(footer);
        }
        markup::fill_slot(root, "app-detail", detail);
    }

    fn fill_rules(&self, root: &mut crate::dom::Elem) {
        let mut next = 8000u32;
        let mut rows = Vec::new();
        for (i, r) in self.mail.rules.iter().enumerate() {
            let label = if r.name.is_empty() {
                format!("Rule {}", i + 1)
            } else {
                r.name.clone()
            };
            let mut row = markup::node(
                &mut next,
                &["list-row"],
                None,
                None,
                &format!("{} · {}", label, r.action),
            );
            row.children.push(markup::node(
                &mut next,
                &["btn", "btn-sm", "btn-danger-outline"],
                Some("rule-remove"),
                Some(&i.to_string()),
                "Remove",
            ));
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(markup::node(
                &mut next,
                &["t-body", "t-muted"],
                None,
                None,
                "No rules configured.",
            ));
        }
        markup::fill_slot(root, "rule-rows", rows);
    }

    fn save_app(&mut self) {
        let app_id = self
            .fields
            .get("app_id")
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        let label = self
            .fields
            .get("label")
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        let command = self
            .fields
            .get("command")
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        let icon = self
            .fields
            .get("icon")
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();
        if app_id.is_empty() || label.is_empty() || command.is_empty() {
            self.error = "app_id, label, and command are required".into();
            return;
        }
        let mut app = Application {
            app_id: app_id.clone(),
            label,
            command,
            icon,
        };
        app.normalize();
        if self.draft {
            match self.apps.add(app.clone()) {
                Ok(()) => {
                    let _ = self.bus.emit(Topic::Application(app.clone()));
                    self.draft = false;
                    self.selected = Some(app.app_id);
                    self.error.clear();
                }
                Err(e) => self.error = e.to_string(),
            }
        } else if let Some(orig) = self.selected.clone() {
            let prev = self.apps.get(&orig).cloned();
            match self.apps.update(&orig, app.clone()) {
                Ok(()) => {
                    if app.app_id != orig {
                        if let Some(old) = prev {
                            let _ = self.bus.retract(Topic::Application(old));
                        }
                    }
                    let _ = self.bus.emit(Topic::Application(app.clone()));
                    self.selected = Some(app.app_id);
                    self.error.clear();
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }

    fn save_mail(&mut self) {
        let port = |k: &str, default: u16| {
            self.fields
                .get(k)
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        self.mail.email = self.fields.get("email").cloned().unwrap_or_default();
        self.mail.imap_host = self.fields.get("imap_host").cloned().unwrap_or_default();
        self.mail.imap_port = port("imap_port", 993);
        self.mail.smtp_host = self.fields.get("smtp_host").cloned().unwrap_or_default();
        self.mail.smtp_port = port("smtp_port", 587);
        self.mail.username = self.fields.get("username").cloned().unwrap_or_default();
        self.mail.password = Encrypted(self.fields.get("password").cloned().unwrap_or_default());
        match self.bus.emit(Topic::MailConfig(self.mail.clone())) {
            Ok(()) => self.mail_error.clear(),
            Err(e) => self.mail_error = e.to_string(),
        }
    }
}

fn char_byte(s: &str, chars: usize) -> usize {
    s.char_indices()
        .nth(chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
}

fn word_range(text: &str, caret: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let i = if caret >= chars.len() {
        chars.len() - 1
    } else if caret > 0 && !is_word_char(chars[caret]) && is_word_char(chars[caret - 1]) {
        caret - 1
    } else {
        caret.min(chars.len() - 1)
    };
    if !is_word_char(chars[i]) {
        return (i, i + 1);
    }
    let mut a = i;
    while a > 0 && is_word_char(chars[a - 1]) {
        a -= 1;
    }
    let mut b = i + 1;
    while b < chars.len() && is_word_char(chars[b]) {
        b += 1;
    }
    (a, b)
}

fn load_text(path: &Path, fallback: &str) -> (String, Option<SystemTime>) {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let m = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
            (s, m)
        }
        Err(_) => (fallback.to_string(), None),
    }
}

fn is_system_app(app_id: &str) -> bool {
    const SYSTEM: &[&str] = &[
        "sola-shell",
        "sola-settings",
        "sola-settings-lab",
        "sola-monitor",
        "sola-monitor-lab",
        "sola-terminal",
        "sola-browser",
        "sola-kit",
        "sola-kit-spike",
        "sola-agent",
        "sola-mail",
        "sola-preview",
        "sola-paint",
        "sola-workspaces",
        "sola-arcade",
        "sola-install",
        "sola-kvm",
    ];
    SYSTEM.contains(&app_id)
}

fn suggest_command(app_id: &str, pid: Option<u32>) -> Option<String> {
    let name = app_id.trim();
    if !name.is_empty() {
        if let Some(p) = resolve_in_path(&name.to_ascii_lowercase()) {
            return Some(p.to_string_lossy().into_owned());
        }
        if let Some(last) = name.split('.').next_back() {
            if let Some(p) = resolve_in_path(&last.to_ascii_lowercase()) {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    let pid = pid?;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let s = exe.to_string_lossy();
    let s = s.strip_suffix(" (deleted)").unwrap_or(&s);
    Some(s.to_string())
}

impl Surface for Settings {
    fn set_view(&mut self, w: f32, h: f32, scale: f32) {
        self.css_w = w;
        self.css_h = h;
        self.scale = scale;
    }
    fn tick(&mut self, dt: f32) {
        self.time += dt;
    }
    fn time(&self) -> f32 {
        self.time
    }
    fn needs_frame(&self) -> bool {
        self.focused.is_some()
    }
    fn has_overlay(&self) -> bool {
        false
    }
    fn has_focus(&self) -> bool {
        self.focused.is_some()
    }
    fn blur(&mut self) {
        self.set_focus(None);
    }
    fn dismiss_overlays(&mut self) -> bool {
        false
    }
    fn type_text(&mut self, s: &str) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.delete_sel();
        let val = self.fields.entry(id).or_default();
        let i = char_byte(val, self.caret);
        val.insert_str(i, s);
        self.caret += s.chars().count();
        self.sel = None;
        self.ping_caret();
    }
    fn backspace(&mut self) {
        if self.delete_sel() {
            self.ping_caret();
            return;
        }
        let Some(id) = self.focused.clone() else {
            return;
        };
        if self.caret == 0 {
            return;
        }
        let val = self.fields.entry(id).or_default();
        let start = char_byte(val, self.caret - 1);
        let end = char_byte(val, self.caret);
        val.replace_range(start..end, "");
        self.caret -= 1;
        self.ping_caret();
    }
    fn tab(&mut self, back: bool) {
        let order: &[&str] = match self.panel {
            Panel::Apps => &["app_id", "label", "command", "icon"],
            Panel::Mail => &[
                "email",
                "imap_host",
                "imap_port",
                "smtp_host",
                "smtp_port",
                "username",
                "password",
            ],
        };
        let cur = self.focused.as_deref();
        let idx = order.iter().position(|k| Some(*k) == cur);
        let next = match (idx, back) {
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
            (None, false) => 0,
            (None, true) => order.len() - 1,
        };
        self.set_focus(Some(order[next].to_string()));
    }
    fn arrow(&mut self, _up: bool) {}
    fn arrow_horizontal(&mut self, left: bool) {
        if let Some((a, b)) = self.sel.take() {
            let (lo, hi) = (a.min(b), a.max(b));
            self.caret = if left { lo } else { hi };
            self.ping_caret();
            return;
        }
        let Some(id) = self.focused.as_deref() else {
            return;
        };
        let len = self.fields.get(id).map(|s| s.chars().count()).unwrap_or(0);
        if left {
            self.caret = self.caret.saturating_sub(1);
        } else {
            self.caret = (self.caret + 1).min(len);
        }
        self.ping_caret();
    }
    fn caret_line(&mut self, end: bool) {
        let Some(id) = self.focused.as_deref() else {
            return;
        };
        if let Some((a, b)) = self.sel.take() {
            let (lo, hi) = (a.min(b), a.max(b));
            self.caret = if end { hi } else { lo };
            self.ping_caret();
            return;
        }
        self.caret = if end {
            self.fields.get(id).map(|s| s.chars().count()).unwrap_or(0)
        } else {
            0
        };
        self.ping_caret();
    }
    fn select_all(&mut self) {
        let Some(id) = self.focused.as_deref() else {
            return;
        };
        let len = self.fields.get(id).map(|s| s.chars().count()).unwrap_or(0);
        self.sel = Some((0, len));
        self.caret = len;
        self.ping_caret();
    }
    fn delete_forward(&mut self) {
        if self.delete_sel() {
            self.ping_caret();
            return;
        }
        let Some(id) = self.focused.clone() else {
            return;
        };
        let val = self.fields.entry(id).or_default();
        let len = val.chars().count();
        if self.caret >= len {
            return;
        }
        let start = char_byte(val, self.caret);
        let end = char_byte(val, self.caret + 1);
        val.replace_range(start..end, "");
        self.ping_caret();
    }
    fn kill_to_end(&mut self) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.sel = None;
        let val = self.fields.entry(id).or_default();
        let start = char_byte(val, self.caret);
        val.replace_range(start.., "");
        self.ping_caret();
    }
    fn arrow_word(&mut self, left: bool) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.sel = None;
        let text = self.fields.get(&id).cloned().unwrap_or_default();
        let chars: Vec<char> = text.chars().collect();
        let mut i = self.caret;
        if left {
            while i > 0 && !is_word_char(chars[i - 1]) {
                i -= 1;
            }
            while i > 0 && is_word_char(chars[i - 1]) {
                i -= 1;
            }
        } else {
            while i < chars.len() && !is_word_char(chars[i]) {
                i += 1;
            }
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
        }
        self.caret = i;
        self.ping_caret();
    }
    fn kill_word_back(&mut self) {
        if self.delete_sel() {
            self.ping_caret();
            return;
        }
        let hi = self.caret;
        self.arrow_word(true);
        let lo = self.caret;
        if lo < hi {
            self.sel = Some((lo, hi));
            self.delete_sel();
        }
        self.ping_caret();
    }
    fn enter(&mut self) {
        if self.panel == Panel::Mail {
            self.save_mail();
        } else {
            self.save_app();
        }
    }
    fn mouse_up(&mut self) {}
    fn buffer_size(&self) -> (u32, u32) {
        (
            (self.css_w * self.scale).round().max(1.0) as u32,
            (self.css_h * self.scale).round().max(1.0) as u32,
        )
    }
    fn reload_if_changed(&mut self) -> bool {
        let mut changed = false;
        if let Ok(meta) = std::fs::metadata(&self.css_path) {
            let m = meta.modified().ok();
            if m != self.css_mtime {
                if let Ok(s) = std::fs::read_to_string(&self.css_path) {
                    self.sheet = parse_sheet(&s);
                    self.css_mtime = m;
                    changed = true;
                }
            }
        }
        if let Ok(meta) = std::fs::metadata(&self.html_path) {
            let m = meta.modified().ok();
            if m != self.html_mtime {
                if let Ok(s) = std::fs::read_to_string(&self.html_path) {
                    self.html = s;
                    self.html_mtime = m;
                    changed = true;
                }
            }
        }
        for (path, body, mtime) in [
            (
                &self.apps_html_path,
                &mut self.apps_html,
                &mut self.apps_html_mtime,
            ),
            (
                &self.mail_html_path,
                &mut self.mail_html,
                &mut self.mail_html_mtime,
            ),
        ] {
            if let Ok(meta) = std::fs::metadata(path) {
                let m = meta.modified().ok();
                if m != *mtime {
                    if let Ok(s) = std::fs::read_to_string(path) {
                        *body = s;
                        *mtime = m;
                        changed = true;
                    }
                }
            }
        }
        let _ = self.assets;
        changed
    }
    fn floating_chrome(&self) -> bool {
        self.is_floating()
    }
    fn live_layers(&mut self) -> (Vec<Quad>, Option<Vec<u32>>) {
        self.rebuild();
        let caret = self.caret_px();
        let sel = self.sel_px();
        let focus_uid = self.focused.as_deref().and_then(|id| {
            self.last_items
                .iter()
                .find(|i| {
                    i.data_id.as_deref() == Some(id) && i.classes.iter().any(|c| c == "input")
                })
                .map(|i| i.uid)
        });
        let (bw, bh) = self.buffer_size();
        let quads = crate::app::chrome_quads(
            &self.last_items,
            self.scale,
            bw,
            bh,
            0.5,
            crate::css::Rgba::rgb(0x3d, 0xd6, 0xf5),
        );
        let pix = Some(paint_glyphs(
            &self.last_items,
            &mut self.fonts,
            self.css_w,
            self.css_h,
            self.scale,
            &mut PaintPass {
                time: self.time,
                sel,
                caret,
                field_scroll: self.scroll_x,
                focus_uid,
                icons: &mut self.icons,
            },
        ));
        (quads, pix)
    }
    fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let mut found = None;
        for item in &self.last_items {
            if item.overflow_scroll && crate::layout::point_in_item(item, x, y) {
                if let Some(id) = item.data_id.as_deref() {
                    found = Some((id.to_string(), (item.content_h - item.h).max(0.0)));
                }
            }
        }
        let Some((id, max)) = found else {
            return false;
        };
        let cur = self.scrolls.get(&id).copied().unwrap_or(0.0);
        let next = (cur + dy).clamp(0.0, max);
        if (next - cur).abs() < 0.5 {
            return false;
        }
        self.scrolls.insert(id, next);
        true
    }
    fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let hover = hover_at(&self.last_items, x, y);
        if hover != self.hover {
            self.hover = hover;
            true
        } else {
            false
        }
    }
    fn cursor_at(&self, x: f32, y: f32) -> crate::host::CursorKind {
        let hit = self
            .last_items
            .iter()
            .rev()
            .find(|i| crate::layout::point_in_item(i, x, y));
        let Some(hit) = hit else {
            return crate::host::CursorKind::Default;
        };
        if hit.classes.iter().any(|c| c == "input") {
            crate::host::CursorKind::Text
        } else if hit.classes.iter().any(|c| c == "btn") {
            crate::host::CursorKind::Pointer
        } else {
            crate::host::CursorKind::Default
        }
    }
    fn right_click(&mut self, _x: f32, _y: f32) -> bool {
        false
    }
    fn click(&mut self, x: f32, y: f32) -> Click {
        let input_hit = self
            .last_items
            .iter()
            .rev()
            .find(|i| {
                crate::layout::point_in_item(i, x, y) && i.classes.iter().any(|c| c == "input")
            })
            .map(|item| {
                (
                    item.data_id.clone(),
                    item.x,
                    item.pad[3],
                    item.text.as_ref().map(|r| r.size).unwrap_or(13.0),
                    item.text.as_ref().map(|r| r.weight).unwrap_or(400),
                    item.text
                        .as_ref()
                        .map(|r| r.family.clone())
                        .unwrap_or_else(|| "SF Pro Text".into()),
                )
            });
        if let Some((id, box_x, pad_l, size, weight, family)) = input_hit {
            if let Some(id) = id {
                let n = self.click_count(&id);
                let idx = self.caret_index_at(x, &id, box_x, pad_l, size, weight, &family);
                self.set_focus(Some(id.clone()));
                self.caret = idx;
                match n {
                    2 => self.select_word_at(&id, idx),
                    3 => {
                        let len = self.fields.get(&id).map(|s| s.chars().count()).unwrap_or(0);
                        self.sel = Some((0, len));
                        self.caret = len;
                    }
                    _ => self.sel = None,
                }
                self.ping_caret();
            }
            return Click::Select;
        }
        if let Some(item) = self.last_items.iter().rev().find(|i| {
            crate::layout::point_in_item(i, x, y) && i.data_action.as_deref() == Some("focus")
        }) {
            self.set_focus(item.data_id.clone());
            return Click::Select;
        }
        self.set_focus(None);
        let Some(hit) = hit_test(&self.last_items, x, y) else {
            return Click::None;
        };
        let action = hit.data_action.clone();
        let id = hit.data_id.clone();
        match action.as_deref() {
            Some("close") => return Click::Close,
            Some("drag") => return Click::Drag,
            Some("panel") => {
                self.panel = if id.as_deref() == Some("mail") {
                    Panel::Mail
                } else {
                    Panel::Apps
                };
                if self.panel == Panel::Mail {
                    self.sync_mail_fields();
                }
                return Click::Select;
            }
            Some("focus") => {
                self.set_focus(id);
                return Click::Select;
            }
            Some("app-add") => {
                self.draft = true;
                self.selected = None;
                self.fields.insert("app_id".into(), String::new());
                self.fields.insert("label".into(), String::new());
                self.fields.insert("command".into(), String::new());
                self.fields.insert("icon".into(), String::new());
                self.error.clear();
                return Click::Select;
            }
            Some("app-select") => {
                self.draft = false;
                self.selected = id;
                self.error.clear();
                self.load_detail_fields();
                return Click::Select;
            }
            Some("app-cand") => {
                if let Some(cid) = id {
                    let cmd = self
                        .running
                        .iter()
                        .find(|w| w.app_id == cid)
                        .and_then(|w| suggest_command(&w.app_id, w.pid))
                        .unwrap_or_default();
                    self.draft = true;
                    self.selected = None;
                    self.fields.insert("app_id".into(), cid.clone());
                    self.fields.insert("label".into(), cid);
                    self.fields.insert("command".into(), cmd);
                    self.fields.insert("icon".into(), String::new());
                    self.error.clear();
                }
                return Click::Select;
            }
            Some("app-remove") => {
                if let Some(rid) = id {
                    if let Some(old) = self.apps.get(&rid).cloned() {
                        self.apps.remove(&rid);
                        let _ = self.bus.retract(Topic::Application(old));
                    }
                    if self.selected.as_deref() == Some(rid.as_str()) {
                        self.selected = None;
                    }
                }
                return Click::Select;
            }
            Some("app-save") => {
                self.save_app();
                return Click::Select;
            }
            Some("app-discard") => {
                if self.draft {
                    self.draft = false;
                    self.selected = None;
                } else {
                    self.load_detail_fields();
                }
                self.error.clear();
                return Click::Select;
            }
            Some("app-close") => {
                self.draft = false;
                self.selected = None;
                self.error.clear();
                return Click::Select;
            }
            Some("mail-save") => {
                self.save_mail();
                return Click::Select;
            }
            Some("mail-discard") => {
                self.sync_mail_fields();
                self.mail_error.clear();
                return Click::Select;
            }
            Some("rule-add") => {
                self.mail.rules.push(MailRule {
                    name: "New rule".into(),
                    action: "smart_mailbox".into(),
                    dest: None,
                    conditions: vec![MailRuleCondition {
                        field: "from".into(),
                        match_type: "contains".into(),
                        value: String::new(),
                    }],
                });
                return Click::Select;
            }
            Some("rule-remove") => {
                if let Some(i) = id.and_then(|s| s.parse::<usize>().ok()) {
                    if i < self.mail.rules.len() {
                        self.mail.rules.remove(i);
                    }
                }
                return Click::Select;
            }
            _ => {}
        }
        Click::None
    }
    fn poll(&mut self) -> bool {
        let mut dirty = false;
        while let Some(msg) = self.bus.try_recv() {
            if self.handle_bus(msg) {
                dirty = true;
            }
        }
        dirty
    }
}
