//! HTML/CSS Mail (lab). Distinct from iced `sola-mail`.
//!
//! Identity: `sola-mail-lab` / `Mail (lab)`. Same worker + IMAP as iced,
//! same bus topics (`MailConfig`, menus, theme, float). Does **not**
//! publish `Topic::MailStatus` so the menubar chip stays on iced mail.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use sola_bus::topics::{
    AppMenuPayload, MailConfig, MailRule, MenuActionPayload, MenuDefinition, MenuItem, Topic,
    TopicKind, Window as BusWindow, WindowFloating,
};
use sola_bus::{BusClient, Message};
use sola_core::KeyCode;
use sola_mail_core::bridge::{self, mail_send};
use sola_mail_core::protocol::{
    Folder, MessageBody, MessageSummary, ProseBlock, folder_count_badge, folder_label, visible_text,
};
use sola_mail_core::worker::{MailCmd, MailEvent};

use crate::app::Click;
use crate::components::button::Kind as Btn;
use crate::components::list_item::ListItem;
use crate::components::{
    Sidebar, SidebarItem, button, field, prose, split, text, titlebar, toast, toolbar,
};
use crate::css::{Sheet, parse_sheet};
use crate::dom::{Elem, parse_html};
use crate::gpu::Quad;
use crate::host::Surface;
use crate::icons::Icons;
use crate::layout::{
    PaintItem, append_scrollbars, apply_pointer_hover, hit_test, hover_at, layout_tree,
    point_in_item,
};
use crate::markup::{self};
use crate::paint::{Fonts, PaintPass, paint_glyphs};

pub const APP_ID: &str = "sola-mail-lab";
pub const WINDOW_TITLE: &str = "Mail (lab)";

const CSS: &str = include_str!("../assets/kit.css");
const HTML: &str = include_str!("../assets/mail.html");

const PAGE: u32 = 50;
const TITLEBAR_H: f32 = 38.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Prose,
}

struct LastMove {
    uid: u32,
    from_folder: String,
    to_folder: String,
}

pub struct Mail {
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
    caret: usize,
    sel: Option<(usize, usize)>,
    scroll_x: f32,
    caret_blink_at: f32,
    last_input_click: Option<(String, Instant, u8)>,
    fields: HashMap<String, String>,
    time: f32,
    bus: BusClient,
    events: Receiver<MailEvent>,
    assets: PathBuf,
    html: String,
    html_root: Elem,
    css_path: PathBuf,
    html_path: PathBuf,
    css_mtime: Option<SystemTime>,
    html_mtime: Option<SystemTime>,
    window_ids: HashMap<String, u32>,
    floating: HashSet<u32>,
    running: Vec<BusWindow>,
    layout_dirty: bool,
    hover_dirty: bool,
    drag: Drag,
    prose_sel: Option<(u32, f32, f32)>,
    mail_config: MailConfig,
    connected: bool,
    not_configured: bool,
    folders: Vec<Folder>,
    smart_counts: Vec<Folder>,
    selected_folder: String,
    messages: Vec<MessageSummary>,
    total_messages: u32,
    selected_uid: Option<u32>,
    message_body: Option<MessageBody>,
    reading_blocks: Vec<ProseBlock>,
    from_addresses: Vec<String>,
    rules: Vec<MailRule>,
    search_active: bool,
    search_total: u32,
    loading: bool,
    folder_loading: bool,
    is_loading_more: bool,
    toast: Option<String>,
    toast_undo: bool,
    composing: bool,
    last_move: Option<LastMove>,
    last_poll: Instant,
}

pub fn run() {
    sola_mail_core::install_crypto();
    crate::host::run_with(
        APP_ID,
        WINDOW_TITLE,
        Box::new(Mail::new(1200.0, 800.0, 1.0)),
    );
}

impl Mail {
    fn new(css_w: f32, css_h: f32, scale: f32) -> Self {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let css_path = assets.join("kit.css");
        let html_path = assets.join("mail.html");
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
        let html_root = parse_html(&html);
        let mut bus = BusClient::new();
        bus.set_app_id(APP_ID);
        bus.connect_blocking(Duration::from_millis(250));
        let _ = bus.subscribe(TopicKind::ALL);
        let _ = bus.emit(Topic::SetAppMenu(AppMenuPayload {
            app_id: APP_ID.into(),
            menus: mail_menus(),
        }));
        bridge::init_channels();
        worker_start();
        let events = bridge::take_event_rx();
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
            fields: HashMap::from([("search".into(), String::new())]),
            time: 0.0,
            bus,
            events,
            assets,
            html,
            html_root,
            css_path,
            html_path,
            css_mtime,
            html_mtime,
            window_ids: HashMap::new(),
            floating: HashSet::new(),
            running: Vec::new(),
            layout_dirty: true,
            hover_dirty: false,
            drag: Drag::None,
            prose_sel: None,
            mail_config: MailConfig::default(),
            connected: false,
            not_configured: false,
            folders: Vec::new(),
            smart_counts: Vec::new(),
            selected_folder: "INBOX".into(),
            messages: Vec::new(),
            total_messages: 0,
            selected_uid: None,
            message_body: None,
            reading_blocks: Vec::new(),
            from_addresses: Vec::new(),
            rules: Vec::new(),
            search_active: false,
            search_total: 0,
            loading: true,
            folder_loading: false,
            is_loading_more: false,
            toast: None,
            toast_undo: false,
            composing: false,
            last_move: None,
            last_poll: Instant::now(),
        }
    }

    fn bump_layout(&mut self) {
        self.layout_dirty = true;
        self.hover_dirty = false;
    }

    fn picked(&mut self) -> Click {
        self.bump_layout();
        Click::Select
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
            self.bump_layout();
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

    fn search_query(&self) -> String {
        self.fields.get("search").cloned().unwrap_or_default()
    }

    fn field(&self, id: &str) -> String {
        self.fields.get(id).cloned().unwrap_or_default()
    }

    fn real_folder(&self) -> String {
        if self.selected_folder.starts_with("smart:") {
            "INBOX".into()
        } else {
            self.selected_folder.clone()
        }
    }

    fn is_emptyable_folder(&self) -> bool {
        self.selected_folder.eq_ignore_ascii_case("Trash")
            || self.selected_folder.eq_ignore_ascii_case("Junk")
    }

    fn load_folder(&mut self, name: String) {
        self.folder_loading = true;
        mail_send(MailCmd::ListMessages {
            folder: name,
            offset: 0,
            limit: PAGE,
        });
        self.bump_layout();
    }

    fn refresh_all(&mut self) {
        if self.search_active {
            return;
        }
        mail_send(MailCmd::ListFolders);
        self.load_folder(self.selected_folder.clone());
    }

    fn select_folder(&mut self, name: String) {
        self.selected_folder = name.clone();
        self.selected_uid = None;
        self.message_body = None;
        self.reading_blocks.clear();
        self.composing = false;
        self.search_active = false;
        self.fields.insert("search".into(), String::new());
        self.load_folder(name);
    }

    fn select_message(&mut self, uid: u32) {
        self.selected_uid = Some(uid);
        self.composing = false;
        self.prose_sel = None;
        let folder = self.real_folder();
        mail_send(MailCmd::FetchBody {
            folder: folder.clone(),
            uid,
        });
        if let Some(msg) = self.messages.iter_mut().find(|m| m.uid == uid) {
            if !msg.seen {
                msg.seen = true;
                mail_send(MailCmd::MarkRead {
                    folder: folder.clone(),
                    uid,
                });
                if let Some(f) = self.folders.iter_mut().find(|f| f.name == folder) {
                    f.unread = f.unread.saturating_sub(1);
                }
            }
        }
        self.bump_layout();
    }

    fn load_more(&mut self) {
        if self.is_loading_more || self.search_active || self.folder_loading {
            return;
        }
        if (self.messages.len() as u32) >= self.total_messages {
            return;
        }
        self.is_loading_more = true;
        mail_send(MailCmd::ListMessages {
            folder: self.selected_folder.clone(),
            offset: self.messages.len() as u32,
            limit: PAGE,
        });
    }

    fn begin_compose(&mut self) {
        let from = self
            .from_addresses
            .first()
            .cloned()
            .unwrap_or_else(|| self.mail_config.email.clone());
        self.fields.insert("compose-from".into(), from);
        self.fields.insert("compose-to".into(), String::new());
        self.fields.insert("compose-cc".into(), String::new());
        self.fields.insert("compose-subject".into(), String::new());
        self.fields.insert("compose-body".into(), String::new());
        self.fields.insert("compose-reply".into(), String::new());
        self.composing = true;
        self.set_focus(Some("compose-to".into()));
        self.bump_layout();
    }

    fn begin_reply(&mut self, all: bool) {
        let Some(body) = self.message_body.clone() else {
            return;
        };
        let from = self
            .from_addresses
            .first()
            .cloned()
            .unwrap_or_else(|| self.mail_config.email.clone());
        let to = body.from.clone();
        let mut cc = String::new();
        if all {
            let mut others: Vec<String> = body
                .to
                .split(',')
                .chain(body.cc.split(','))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .filter(|s| !s.contains(&from))
                .collect();
            others.sort();
            others.dedup();
            if !others.is_empty() {
                cc = others.join(", ");
            }
        }
        let subj = if body.subject.to_lowercase().starts_with("re:") {
            body.subject.clone()
        } else {
            format!("Re: {}", body.subject)
        };
        let quoted = body
            .display_text()
            .lines()
            .map(|l| format!("> {l}"))
            .collect::<Vec<_>>()
            .join("\n");
        let draft = format!("\n\nOn {} {} wrote:\n{quoted}", body.date, body.from);
        self.fields.insert("compose-from".into(), from);
        self.fields.insert("compose-to".into(), to);
        self.fields.insert("compose-cc".into(), cc);
        self.fields.insert("compose-subject".into(), subj);
        self.fields.insert("compose-body".into(), draft);
        self.fields.insert(
            "compose-reply".into(),
            body.message_id.clone().unwrap_or_default(),
        );
        self.composing = true;
        self.set_focus(Some("compose-body".into()));
        self.bump_layout();
    }

    fn send_draft(&mut self) {
        let to = self.field("compose-to");
        if to.trim().is_empty() {
            self.toast = Some("To address is required".into());
            self.toast_undo = false;
            self.bump_layout();
            return;
        }
        let in_reply_to = {
            let s = self.field("compose-reply");
            if s.is_empty() { None } else { Some(s) }
        };
        mail_send(MailCmd::Send {
            from: self.field("compose-from"),
            to,
            cc: self.field("compose-cc"),
            subject: self.field("compose-subject"),
            body: self.field("compose-body"),
            in_reply_to,
        });
    }

    fn move_and_advance(&mut self, uid: u32, dest: String) {
        let folder = self.real_folder();
        let idx = self.messages.iter().position(|m| m.uid == uid);
        self.last_move = Some(LastMove {
            uid,
            from_folder: folder.clone(),
            to_folder: dest.clone(),
        });
        self.toast = Some(format!("Moved to {dest}"));
        self.toast_undo = true;
        mail_send(MailCmd::Move { folder, uid, dest });
        self.messages.retain(|m| m.uid != uid);
        if self.messages.is_empty() {
            self.selected_uid = None;
            self.message_body = None;
            self.reading_blocks.clear();
        } else if let Some(i) = idx {
            let next = if i > 0 { i - 1 } else { 0 };
            let next_uid = self.messages[next.min(self.messages.len() - 1)].uid;
            self.select_message(next_uid);
            return;
        }
        self.bump_layout();
    }

    fn undo_move(&mut self) {
        if let Some(lm) = self.last_move.take() {
            mail_send(MailCmd::Move {
                folder: lm.to_folder,
                uid: lm.uid,
                dest: lm.from_folder,
            });
            self.toast = Some("Move undone".into());
            self.toast_undo = false;
            self.load_folder(self.selected_folder.clone());
        }
    }

    fn begin_empty(&mut self, folder: &str) {
        self.toast = Some(format!("Erasing {}…", folder_label(folder)));
        self.toast_undo = false;
        if self.selected_folder.eq_ignore_ascii_case(folder) {
            self.folder_loading = true;
            self.messages.clear();
            self.selected_uid = None;
            self.message_body = None;
            self.reading_blocks.clear();
        }
        mail_send(MailCmd::EmptyFolder {
            folder: folder.to_string(),
        });
        self.bump_layout();
    }

    fn select_next(&mut self) {
        let Some(uid) = self.selected_uid else {
            if let Some(first) = self.messages.first() {
                self.select_message(first.uid);
            }
            return;
        };
        let Some(idx) = self.messages.iter().position(|m| m.uid == uid) else {
            return;
        };
        if idx + 1 >= self.messages.len() {
            return;
        }
        self.select_message(self.messages[idx + 1].uid);
    }

    fn select_prev(&mut self) {
        let Some(uid) = self.selected_uid else {
            if let Some(last) = self.messages.last() {
                self.select_message(last.uid);
            }
            return;
        };
        let Some(idx) = self.messages.iter().position(|m| m.uid == uid) else {
            return;
        };
        if idx == 0 {
            return;
        }
        self.select_message(self.messages[idx - 1].uid);
    }

    fn copy_letter(&self) {
        let t = sola_mail_core::protocol::flatten(&self.reading_blocks);
        if !t.is_empty() {
            clipboard_copy(&t);
        }
    }

    fn copy_visible(&self) {
        let vis = visible_text(&self.reading_blocks);
        if vis.is_empty() {
            self.copy_letter();
        } else {
            clipboard_copy(&vis);
        }
    }

    fn submit_search(&mut self) {
        let q = self.search_query().trim().to_string();
        if q.is_empty() {
            return;
        }
        self.search_active = true;
        self.folder_loading = true;
        mail_send(MailCmd::Search { query: q });
        self.bump_layout();
    }

    fn clear_search(&mut self) {
        self.fields.insert("search".into(), String::new());
        if self.search_active {
            self.search_active = false;
            self.search_total = 0;
            self.load_folder(self.selected_folder.clone());
        } else {
            self.bump_layout();
        }
    }

    fn on_menu(&mut self, action: &str) {
        match action {
            "quit" => {
                mail_send(MailCmd::Shutdown);
                std::process::exit(0);
            }
            "compose" => self.begin_compose(),
            "reply" => self.begin_reply(false),
            "reply_all" => self.begin_reply(true),
            "archive" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Archive".into());
                }
            }
            "trash" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Trash".into());
                }
            }
            "junk" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Junk".into());
                }
            }
            "inbox" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "INBOX".into());
                }
            }
            "undo" => self.undo_move(),
            "copy" | "copy_message" => self.copy_letter(),
            "select_all" => {
                if self.focused.is_some() {
                    self.select_all();
                } else if self.message_body.is_some() {
                    self.copy_visible();
                }
            }
            "paste" => {
                if let Some(t) = clipboard_paste() {
                    self.type_text(&t);
                }
            }
            "empty_junk" => self.begin_empty("Junk"),
            "empty_trash" => self.begin_empty("Trash"),
            "refresh" => {
                if self.connected && !self.loading {
                    self.refresh_all();
                }
            }
            "next" => self.select_next(),
            "prev" => self.select_prev(),
            _ => {}
        }
    }

    fn on_worker(&mut self, ev: MailEvent) {
        match ev {
            MailEvent::Connected {
                folders,
                smart_counts,
                from_addresses,
                rules,
            } => {
                self.connected = true;
                self.not_configured = false;
                self.loading = false;
                self.folders = folders;
                self.smart_counts = smart_counts;
                self.from_addresses = from_addresses;
                self.rules = rules;
                self.load_folder(self.selected_folder.clone());
            }
            MailEvent::NotConfigured => {
                self.connected = false;
                self.not_configured = true;
                self.loading = false;
                self.folders.clear();
                self.messages.clear();
                self.bump_layout();
            }
            MailEvent::Folders {
                folders,
                smart_counts,
            } => {
                self.folders = folders;
                self.smart_counts = smart_counts;
                self.bump_layout();
            }
            MailEvent::Messages {
                folder,
                messages,
                total,
                offset,
            } => {
                if folder != self.selected_folder && !self.search_active {
                    return;
                }
                self.folder_loading = false;
                self.is_loading_more = false;
                self.total_messages = total;
                if let Some(f) = self.folders.iter_mut().find(|f| f.name == folder) {
                    f.total = total;
                }
                if offset == 0 {
                    self.messages = messages;
                } else {
                    self.messages.extend(messages);
                }
                self.bump_layout();
            }
            MailEvent::SearchResults { messages, total } => {
                self.folder_loading = false;
                self.messages = messages;
                self.search_total = total;
                self.total_messages = total;
                self.bump_layout();
            }
            MailEvent::Body(body) => {
                self.reading_blocks = body.reading_blocks();
                self.message_body = Some(body);
                self.bump_layout();
            }
            MailEvent::Sent => {
                self.composing = false;
                self.toast = Some("Message sent".into());
                self.toast_undo = false;
                mail_send(MailCmd::ListFolders);
                self.bump_layout();
            }
            MailEvent::Moved => {}
            MailEvent::Emptied { folder } => {
                self.folder_loading = false;
                self.toast = Some(format!("{} erased", folder_label(&folder)));
                self.toast_undo = false;
                mail_send(MailCmd::ListFolders);
                if self.selected_folder.eq_ignore_ascii_case(&folder) {
                    self.load_folder(folder);
                } else {
                    self.bump_layout();
                }
            }
            MailEvent::NewMail => self.refresh_all(),
            MailEvent::Error { context, message } => {
                self.loading = false;
                self.folder_loading = false;
                self.is_loading_more = false;
                self.toast = Some(format!("{context}: {message}"));
                self.toast_undo = false;
                if context == "connect" {
                    self.connected = false;
                }
                if context == "empty_folder" {
                    self.load_folder(self.selected_folder.clone());
                } else {
                    self.bump_layout();
                }
            }
        }
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
            Topic::MailConfig(cfg) => {
                self.mail_config = cfg.clone();
                mail_send(MailCmd::Reconfigure(cfg));
                self.loading = true;
                true
            }
            Topic::MenuAction(MenuActionPayload { app_id, action_id }) if app_id == APP_ID => {
                self.on_menu(&action_id);
                true
            }
            Topic::CloseApp(id) if id == APP_ID => {
                mail_send(MailCmd::Shutdown);
                std::process::exit(0);
            }
            _ => false,
        }
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

    fn fill_sidebar(&self, root: &mut Elem) {
        let mut next = markup::next_uid(root);
        let mut items = vec![SidebarItem::header("MAILBOXES")];
        for f in &self.folders {
            let mut item = SidebarItem::new(&f.name, folder_label(&f.name))
                .action("folder")
                .active(self.selected_folder == f.name);
            if let Some(b) = folder_count_badge(f.unread, f.total) {
                item = item.badge(b);
            }
            items.push(item);
        }
        if !self.smart_counts.is_empty() {
            items.push(SidebarItem::header("SMART"));
            for f in &self.smart_counts {
                let id = format!("smart:{}", f.name);
                let mut item = SidebarItem::new(&id, f.name.clone())
                    .action("folder")
                    .active(self.selected_folder == id);
                if let Some(b) = folder_count_badge(f.unread, f.total) {
                    item = item.badge(b);
                }
                items.push(item);
            }
        }
        let sb = Sidebar::new(items)
            .class("sidebar-mail")
            .data_id("sidebar")
            .nav_id("folder-nav")
            .build(&mut next);
        markup::replace_slot(root, "sidebar", sb);
    }

    fn fill_list_toolbar(&self, root: &mut Elem) {
        let mut next = 12_000u32;
        let mut search = field::input(&mut next, "search");
        markup::add_class(&mut search, "mail-search");
        let count = if self.search_active {
            format_count(self.search_total)
        } else if self.folder_loading {
            "…".into()
        } else {
            format_count(self.total_messages)
        };
        let mut cap = text::caption(&mut next, &count);
        markup::add_class(&mut cap, "nowrap");
        markup::add_class(&mut cap, "toolbar-count");
        let mut kids = vec![search, cap];
        if !self.search_query().is_empty() || self.search_active {
            kids.push(toolbar::icon_btn(
                &mut next,
                Some("search-clear"),
                None,
                "lucide/x",
            ));
        }
        if self.is_emptyable_folder() && !self.search_active {
            kids.push(toolbar::icon_btn(
                &mut next,
                Some("empty-folder"),
                None,
                "lucide/trash-2",
            ));
        }
        markup::replace_slot(
            root,
            "list-toolbar",
            toolbar::bar(&mut next, "list-toolbar", &["mail-toolbar"], kids),
        );
    }

    fn fill_list(&self, root: &mut Elem) {
        let mut next = 30_000u32;
        let mut rows = Vec::new();
        if self.messages.is_empty() && !self.folder_loading {
            let mut empty = markup::node(&mut next, &["mail-center"], None, None, "");
            empty.children.push(text::caption(
                &mut next,
                if self.search_active {
                    "No matching messages"
                } else {
                    "No messages"
                },
            ));
            rows.push(empty);
        }
        for m in &self.messages {
            let subj = if m.subject.is_empty() {
                "(no subject)".to_string()
            } else {
                m.subject.clone()
            };
            rows.push(
                ListItem::new(m.uid.to_string(), short_from(&m.from))
                    .action("msg")
                    .subtitle(subj)
                    .meta(short_date(&m.date))
                    .selected(self.selected_uid == Some(m.uid))
                    .strong(!m.seen)
                    .build(&mut next),
            );
        }
        markup::fill_slot(root, "list-rows", rows);
    }

    fn fill_reader_toolbar(&self, root: &mut Elem) {
        let mut next = 14_000u32;
        let has_msg = self.selected_uid.is_some() && !self.composing;
        let mut kids = vec![
            toolbar::icon_btn(&mut next, Some("compose"), None, "lucide/square-pen"),
            toolbar::icon_btn(&mut next, has_msg.then_some("reply"), None, "lucide/reply"),
            toolbar::icon_btn(
                &mut next,
                has_msg.then_some("reply-all"),
                None,
                "lucide/reply-all",
            ),
            toolbar::icon_btn(
                &mut next,
                has_msg.then_some("archive"),
                None,
                "lucide/archive",
            ),
            toolbar::icon_btn(
                &mut next,
                has_msg.then_some("trash"),
                None,
                "lucide/trash-2",
            ),
            toolbar::icon_btn(&mut next, has_msg.then_some("junk"), None, "lucide/ban"),
        ];
        kids.push(markup::node(&mut next, &["spacer"], None, None, ""));
        if has_msg {
            kids.push(toolbar::icon_btn(
                &mut next,
                Some("copy-message"),
                None,
                "lucide/copy",
            ));
        }
        if self.last_move.is_some() {
            kids.push(toolbar::icon_btn(
                &mut next,
                Some("undo"),
                None,
                "lucide/undo-2",
            ));
        }
        markup::replace_slot(
            root,
            "reader-toolbar",
            toolbar::bar(&mut next, "reader-toolbar", &["mail-toolbar"], kids),
        );
    }

    fn fill_reader(&self, root: &mut Elem) {
        let mut next = 40_000u32;
        if self.composing {
            markup::replace_slot(root, "reader", self.compose_form(&mut next));
            return;
        }
        let Some(body) = &self.message_body else {
            let mut empty =
                markup::node(&mut next, &["mail-center"], None, Some("letter-scroll"), "");
            let mut stack = markup::node(&mut next, &["mail-empty"], None, None, "");
            stack
                .children
                .push(text::muted(&mut next, "No message selected"));
            stack.children.push(text::caption(
                &mut next,
                "Pick one from the list, or press ↓",
            ));
            empty.children.push(stack);
            markup::replace_slot(root, "reader", empty);
            return;
        };
        let mut col = markup::node(&mut next, &["letter-col"], None, None, "");
        let subj = if body.subject.is_empty() {
            "(no subject)"
        } else {
            body.subject.as_str()
        };
        col.children.push(markup::node(
            &mut next,
            &["letter-subject"],
            None,
            None,
            subj,
        ));
        col.children.push(letter_header(&mut next, body));
        col.children.push(split::hairline(&mut next));
        let blocks = to_kit_prose(&self.reading_blocks);
        let mut body_col = markup::node(&mut next, &["letter-body"], None, None, "");
        body_col
            .children
            .extend(prose::document(&mut next, &blocks));
        col.children.push(body_col);
        let mut pane = markup::node(&mut next, &["mail-letter"], None, Some("letter-scroll"), "");
        pane.children.push(col);
        markup::replace_slot(root, "reader", pane);
    }

    fn compose_form(&self, next: &mut u32) -> Elem {
        let mut form = markup::node(next, &["compose-form"], None, Some("letter-scroll"), "");
        form.children
            .push(field::stack(next, "From", "compose-from"));
        form.children.push(field::stack(next, "To", "compose-to"));
        form.children.push(field::stack(next, "Cc", "compose-cc"));
        form.children
            .push(field::stack(next, "Subject", "compose-subject"));
        form.children.push(field::textarea(next, "compose-body"));
        let send = button(next, Btn::Primary, false, "send", None, "Send");
        let cancel = button(next, Btn::Ghost, true, "cancel-compose", None, "Cancel");
        form.children.push(button::row(next, vec![send, cancel]));
        form
    }

    fn rebuild(&mut self) {
        let mut root = self.html_root.clone();
        let mut next = markup::next_uid(&root);
        markup::replace_slot(&mut root, "titlebar", titlebar(&mut next, WINDOW_TITLE));
        if !self.is_floating() {
            markup::hide_if(&mut root, |el| {
                el.data_id.as_deref() == Some("csd") || el.classes.iter().any(|c| c == "titlebar")
            });
        }
        markup::walk_mut(&mut root, &mut |el| {
            if self.is_floating() && el.classes.iter().any(|c| c == "app") {
                markup::add_class(el, "is-float");
            }
        });
        if self.loading || self.not_configured {
            let mut stage = markup::node(&mut next, &["mail-center"], None, Some("stage"), "");
            if self.loading {
                stage.children.push(text::body(&mut next, "Connecting…"));
            } else {
                stage
                    .children
                    .push(text::heading(&mut next, "Mail not configured"));
                stage.children.push(text::caption(
                    &mut next,
                    "Add your account in Settings → Mail, then reopen.",
                ));
            }
            markup::replace_slot(&mut root, "stage", stage);
        } else {
            markup::replace_slot(&mut root, "rule-folders", split::vline(&mut next));
            markup::replace_slot(&mut root, "rule-list", split::vline(&mut next));
            self.fill_sidebar(&mut root);
            self.fill_list_toolbar(&mut root);
            self.fill_list(&mut root);
            self.fill_reader_toolbar(&mut root);
            self.fill_reader(&mut root);
        }
        if let Some(msg) = &self.toast {
            let mut acts = Vec::new();
            if self.toast_undo && self.last_move.is_some() {
                acts.push(button(
                    &mut next,
                    Btn::Secondary,
                    true,
                    "toast-undo",
                    None,
                    "Undo",
                ));
            }
            acts.push(button(
                &mut next,
                Btn::Ghost,
                true,
                "toast-dismiss",
                None,
                "Dismiss",
            ));
            markup::replace_slot(&mut root, "toast", toast::bar(&mut next, msg, acts));
        } else {
            markup::replace_slot(
                &mut root,
                "toast",
                markup::node(&mut next, &["is-hidden"], None, Some("toast"), ""),
            );
        }
        markup::apply_fields(&mut root, &self.fields);
        markup::apply_focus(&mut root, self.focused.as_deref());
        let search_empty = self.search_query().is_empty();
        let search_focus = self.focused.as_deref() == Some("search");
        if search_empty && !search_focus {
            markup::walk_mut(&mut root, &mut |el| {
                if el.data_id.as_deref() == Some("search") && el.has_class("input") {
                    el.text = "Search".into();
                    markup::add_class(el, "is-placeholder");
                }
            });
        }
        self.last_items = layout_tree(
            &root,
            &self.sheet,
            self.hover,
            self.css_w,
            self.css_h,
            &mut self.fonts,
            &self.scrolls,
        );
        append_scrollbars(&mut self.last_items, &self.scrolls);
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
}

fn worker_start() {
    sola_mail_core::worker::start();
}

impl Surface for Mail {
    fn set_view(&mut self, w: f32, h: f32, scale: f32) {
        if (w - self.css_w).abs() > 0.5
            || (h - self.css_h).abs() > 0.5
            || (scale - self.scale).abs() > 0.01
        {
            self.bump_layout();
        }
        self.css_w = w;
        self.css_h = h;
        self.scale = scale;
    }
    fn tick(&mut self, dt: f32) {
        self.time += dt;
        if self.connected
            && !self.loading
            && !self.composing
            && self.last_poll.elapsed() >= Duration::from_secs(45)
        {
            self.last_poll = Instant::now();
            self.refresh_all();
        }
    }
    fn time(&self) -> f32 {
        self.time
    }
    fn needs_frame(&self) -> bool {
        self.focused.is_some() || self.layout_dirty
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
        if self.composing {
            self.composing = false;
            self.set_focus(None);
            self.bump_layout();
            return true;
        }
        if self.search_active || !self.search_query().is_empty() {
            self.clear_search();
            return true;
        }
        if self.toast.is_some() {
            self.toast = None;
            self.toast_undo = false;
            self.bump_layout();
            return true;
        }
        false
    }
    fn type_text(&mut self, s: &str) {
        if self.focused.is_some() {
            self.delete_sel();
            let id = self.focused.clone().unwrap();
            let val = self.fields.entry(id).or_default();
            let i = char_byte(val, self.caret);
            val.insert_str(i, s);
            self.caret += s.chars().count();
            self.sel = None;
            self.ping_caret();
            self.bump_layout();
            return;
        }
        if self.composing {
            return;
        }
        match s {
            "j" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Junk".into());
                }
            }
            "i" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "INBOX".into());
                }
            }
            "a" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Archive".into());
                }
            }
            "d" => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Trash".into());
                }
            }
            "u" => self.undo_move(),
            "w" => self.select_prev(),
            "s" => self.select_next(),
            _ => {}
        }
    }
    fn backspace(&mut self) {
        if self.delete_sel() {
            self.ping_caret();
            self.bump_layout();
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
        self.bump_layout();
    }
    fn tab(&mut self, back: bool) {
        let order: &[&str] = if self.composing {
            &[
                "compose-from",
                "compose-to",
                "compose-cc",
                "compose-subject",
                "compose-body",
            ]
        } else {
            &["search"]
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
    fn arrow(&mut self, up: bool) {
        if self.composing && self.focused.is_some() {
            return;
        }
        if up {
            self.select_prev();
        } else {
            self.select_next();
        }
    }
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
            self.bump_layout();
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
        self.bump_layout();
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
        self.bump_layout();
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
            self.bump_layout();
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
        self.bump_layout();
    }
    fn enter(&mut self) {
        if self.focused.as_deref() == Some("search") {
            self.submit_search();
            return;
        }
        if self.focused.as_deref() == Some("compose-body") {
            self.type_text("\n");
            return;
        }
        if self.composing {
            self.send_draft();
        }
    }
    fn mouse_up(&mut self) {
        self.drag = Drag::None;
    }
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
                    self.html = s.clone();
                    self.html_root = parse_html(&s);
                    self.html_mtime = m;
                    changed = true;
                }
            }
        }
        let _ = self.assets;
        if changed {
            self.bump_layout();
        }
        changed
    }
    fn floating_chrome(&self) -> bool {
        self.is_floating()
    }
    fn live_layers(&mut self) -> (Vec<Quad>, Option<Vec<u32>>) {
        let need_layout = self.layout_dirty || self.last_items.is_empty();
        let hover_only =
            self.hover_dirty && !need_layout && self.focused.is_none() && self.drag == Drag::None;
        if need_layout {
            self.rebuild();
            self.layout_dirty = false;
            self.hover_dirty = false;
        } else if self.hover_dirty {
            apply_pointer_hover(&mut self.last_items, self.hover);
            self.hover_dirty = false;
        }
        let (bw, bh) = self.buffer_size();
        let quads = crate::app::chrome_quads(
            &self.last_items,
            self.scale,
            bw,
            bh,
            0.5,
            crate::css::Rgba::rgb(0x3d, 0xd6, 0xf5),
        );
        if hover_only {
            return (quads, None);
        }
        let caret = self.caret_px();
        let sel = self.sel_px().or(self.prose_sel);
        let focus_uid = self.focused.as_deref().and_then(|id| {
            self.last_items
                .iter()
                .find(|i| {
                    i.data_id.as_deref() == Some(id) && i.classes.iter().any(|c| c == "input")
                })
                .map(|i| i.uid)
        });
        let pix = paint_glyphs(
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
        );
        (quads, Some(pix))
    }
    fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let mut found = None;
        for item in &self.last_items {
            if item.overflow_scroll && point_in_item(item, x, y) {
                if let Some(id) = item.data_id.as_deref() {
                    found = Some((id.to_string(), (item.content_h - item.h).max(0.0), item.h));
                }
            }
        }
        let Some((id, max, view_h)) = found else {
            return false;
        };
        let cur = self.scrolls.get(&id).copied().unwrap_or(0.0);
        let next = (cur + dy).clamp(0.0, max);
        if (next - cur).abs() < 0.5 {
            return false;
        }
        self.scrolls.insert(id.clone(), next);
        if id == "list-scroll" {
            let remain = max - next;
            if remain < 280.0 {
                self.load_more();
            }
        }
        let _ = (view_h, TITLEBAR_H);
        self.bump_layout();
        true
    }
    fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let mut dirty = false;
        if self.drag == Drag::Prose {
            if let Some((uid, a, _)) = self.prose_sel {
                self.prose_sel = Some((uid, a, x));
                dirty = true;
            }
        }
        let hover = hover_at(&self.last_items, x, y);
        if hover != self.hover {
            self.hover = hover;
            self.hover_dirty = true;
            dirty = true;
        }
        dirty
    }
    fn cursor_at(&self, x: f32, y: f32) -> crate::host::CursorKind {
        let hit = self
            .last_items
            .iter()
            .rev()
            .find(|i| point_in_item(i, x, y));
        let Some(hit) = hit else {
            return crate::host::CursorKind::Default;
        };
        if hit
            .classes
            .iter()
            .any(|c| c == "input" || c == "t-prose" || c == "prose-p")
        {
            crate::host::CursorKind::Text
        } else if hit.classes.iter().any(|c| {
            c == "btn"
                || c == "toolbar-icon"
                || c == "row"
                || c == "prose-link"
                || c == "sb-thumb"
                || c == "sb-track"
        }) {
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
            .find(|i| point_in_item(i, x, y) && i.classes.iter().any(|c| c == "input"))
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
            return self.picked();
        }
        let Some(hit) = hit_test(&self.last_items, x, y) else {
            self.set_focus(None);
            return Click::None;
        };
        let action = hit.data_action.clone();
        let id = hit.data_id.clone();
        let uid = hit.uid;
        if action.as_deref() != Some("focus") {
            self.set_focus(None);
        }
        match action.as_deref() {
            Some("close") => {
                mail_send(MailCmd::Shutdown);
                return Click::Close;
            }
            Some("drag") => return Click::Drag,
            Some("folder") => {
                if let Some(name) = id {
                    self.select_folder(name);
                }
                return self.picked();
            }
            Some("msg") => {
                if let Some(s) = id.and_then(|s| s.parse().ok()) {
                    self.select_message(s);
                }
                return self.picked();
            }
            Some("compose") => {
                self.begin_compose();
                return self.picked();
            }
            Some("reply") => {
                self.begin_reply(false);
                return self.picked();
            }
            Some("reply-all") => {
                self.begin_reply(true);
                return self.picked();
            }
            Some("archive") => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Archive".into());
                }
                return self.picked();
            }
            Some("trash") => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Trash".into());
                }
                return self.picked();
            }
            Some("junk") => {
                if let Some(uid) = self.selected_uid {
                    self.move_and_advance(uid, "Junk".into());
                }
                return self.picked();
            }
            Some("copy-message") => {
                self.copy_letter();
                return self.picked();
            }
            Some("undo") | Some("toast-undo") => {
                self.undo_move();
                return self.picked();
            }
            Some("search-clear") => {
                self.clear_search();
                return self.picked();
            }
            Some("empty-folder") => {
                let folder = self.selected_folder.clone();
                self.begin_empty(&folder);
                return self.picked();
            }
            Some("send") => {
                self.send_draft();
                return self.picked();
            }
            Some("cancel-compose") => {
                self.composing = false;
                return self.picked();
            }
            Some("toast-dismiss") => {
                self.toast = None;
                self.toast_undo = false;
                return self.picked();
            }
            Some("open-url") => {
                if let Some(url) = id {
                    sola_core::open_url_logged(&url);
                }
                return self.picked();
            }
            Some("prose") => {
                self.drag = Drag::Prose;
                self.prose_sel = Some((uid, x, x));
                return self.picked();
            }
            Some("focus") => {
                self.set_focus(id);
                return self.picked();
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
        while let Ok(ev) = self.events.try_recv() {
            self.on_worker(ev);
            dirty = true;
        }
        if dirty {
            self.bump_layout();
        }
        dirty
    }
}

fn to_kit_prose(blocks: &[ProseBlock]) -> Vec<prose::Block> {
    blocks
        .iter()
        .map(|b| match b {
            ProseBlock::Paragraph(runs) => prose::Block::Paragraph(
                runs.iter()
                    .map(|r| prose::Run {
                        text: r.text.clone(),
                        url: r.url.clone(),
                    })
                    .collect(),
            ),
            ProseBlock::Quote(runs) => prose::Block::Quote(
                runs.iter()
                    .map(|r| prose::Run {
                        text: r.text.clone(),
                        url: r.url.clone(),
                    })
                    .collect(),
            ),
        })
        .collect()
}

fn letter_header(next: &mut u32, body: &MessageBody) -> Elem {
    let (name, addr) = split_address(&body.from);
    let mut meta = markup::node(next, &["letter-meta"], None, None, "");
    meta.children
        .push(meta_row(next, "From", &name, addr.as_deref()));
    let date = letter_date(&body.date);
    if !date.is_empty() {
        meta.children.push(meta_row(next, "Date", &date, None));
    }
    let to = body.to.trim();
    if !to.is_empty() {
        meta.children.push(meta_row(next, "To", to, None));
    }
    let cc = body.cc.trim();
    if !cc.is_empty() {
        meta.children.push(meta_row(next, "Cc", cc, None));
    }
    meta
}

fn meta_row(next: &mut u32, key: &str, value: &str, sub: Option<&str>) -> Elem {
    let mut row = markup::node(next, &["letter-meta-row"], None, None, "");
    let mut k = text::caption(next, key);
    markup::add_class(&mut k, "letter-meta-key");
    row.children.push(k);
    let mut stack = markup::node(next, &["letter-from"], None, None, "");
    if sub.is_some() {
        stack
            .children
            .push(markup::node(next, &["letter-from-name"], None, None, value));
        if let Some(sub) = sub {
            stack.children.push(text::caption(next, sub));
        }
    } else {
        stack.children.push(text::caption(next, value));
    }
    row.children.push(stack);
    row
}

fn mail_menus() -> Vec<MenuDefinition> {
    vec![
        MenuDefinition {
            label: "Mail".into(),
            items: vec![
                item("refresh", "Get New Mail", Some(KeyCode::N.meta_shift())),
                MenuItem::Divider,
                MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Mail".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                },
            ],
        },
        MenuDefinition {
            label: "Edit".into(),
            items: vec![
                item("copy", "Copy", Some(KeyCode::C.meta())),
                item("paste", "Paste", Some(KeyCode::V.meta())),
                MenuItem::Divider,
                item("select_all", "Select All", Some(KeyCode::A.meta())),
                item("copy_message", "Copy Message", None),
            ],
        },
        MenuDefinition {
            label: "Mailbox".into(),
            items: vec![
                item("empty_junk", "Erase Junk Mail", None),
                item("empty_trash", "Erase Deleted Items", None),
            ],
        },
        MenuDefinition {
            label: "Message".into(),
            items: vec![
                item("compose", "New Message", Some(KeyCode::N.meta())),
                MenuItem::Divider,
                item("reply", "Reply", Some(KeyCode::R.meta())),
                item("reply_all", "Reply All", Some(KeyCode::R.meta_shift())),
                MenuItem::Divider,
                item("archive", "Archive", Some(KeyCode::A.chord())),
                item("inbox", "Move to Inbox", Some(KeyCode::I.chord())),
                item("junk", "Move to Junk", Some(KeyCode::J.chord())),
                item("trash", "Delete", Some(KeyCode::D.chord())),
                MenuItem::Divider,
                item("undo", "Undo Move", Some(KeyCode::U.chord())),
            ],
        },
        MenuDefinition {
            label: "View".into(),
            items: vec![
                item("next", "Next Message", Some(KeyCode::S.chord())),
                item("prev", "Previous Message", Some(KeyCode::W.chord())),
            ],
        },
    ]
}

fn item(id: &str, label: &str, shortcut: Option<sola_core::KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}

fn format_count(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn split_address(from: &str) -> (String, Option<String>) {
    let t = from.trim();
    if let Some(start) = t.find('<') {
        let name = t[..start].trim().trim_matches('"');
        let addr = t[start + 1..].trim().trim_end_matches('>').trim();
        if !name.is_empty() {
            return (name.to_string(), Some(addr.to_string()));
        }
        if !addr.is_empty() {
            return (addr.to_string(), None);
        }
    }
    (short_from(t), None)
}

fn short_from(from: &str) -> String {
    let t = from.trim();
    if let Some(start) = t.find('<') {
        let name = t[..start].trim().trim_matches('"');
        if !name.is_empty() {
            return name.to_string();
        }
        if let Some(end) = t.rfind('>') {
            return t[start + 1..end].trim().to_string();
        }
    }
    if let Some((local, _)) = t.split_once('@') {
        if !local.is_empty() {
            return local.to_string();
        }
    }
    t.to_string()
}

fn letter_date(date: &str) -> String {
    let t = date.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') && t.as_bytes().get(7) == Some(&b'-') {
        if let (Ok(year), Ok(mo), Ok(d)) = (
            t[0..4].parse::<u16>(),
            t[5..7].parse::<u8>(),
            t[8..10].parse::<u8>(),
        ) {
            return format!("{d} {} {year}", month_name(mo));
        }
    }
    short_date(date)
}

fn short_date(date: &str) -> String {
    let t = date.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') && t.as_bytes().get(7) == Some(&b'-') {
        if let (Ok(mo), Ok(d)) = (t[5..7].parse::<u8>(), t[8..10].parse::<u8>()) {
            return format!("{d} {}", month_abbr(mo));
        }
    }
    let tokens: Vec<&str> = t.split_whitespace().collect();
    for i in 0..tokens.len().saturating_sub(1) {
        let day = tokens[i].trim_end_matches(',');
        if day.parse::<u8>().is_ok() && is_month_token(tokens[i + 1]) {
            let mon = tokens[i + 1];
            let mon = &mon[..mon.len().min(3)];
            return format!("{day} {mon}");
        }
    }
    tokens
        .into_iter()
        .filter(|tok| !tok.starts_with('+') && !tok.contains(':'))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

fn month_name(mo: u8) -> &'static str {
    match mo {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => month_abbr(mo),
    }
}

fn month_abbr(mo: u8) -> &'static str {
    match mo {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

fn is_month_token(s: &str) -> bool {
    let head = s.chars().take(3).collect::<String>().to_ascii_lowercase();
    matches!(
        head.as_str(),
        "jan"
            | "feb"
            | "mar"
            | "apr"
            | "may"
            | "jun"
            | "jul"
            | "aug"
            | "sep"
            | "oct"
            | "nov"
            | "dec"
    )
}

fn clipboard_copy(s: &str) {
    let _ = std::process::Command::new("wl-copy")
        .arg("-n")
        .arg(s)
        .status();
}

fn clipboard_paste() -> Option<String> {
    let out = std::process::Command::new("wl-paste")
        .arg("-n")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
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
