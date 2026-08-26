//! Kit UI: three-pane mail client (graphite list + reading composition).

mod list_sync;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::Id as ScrollId;
use iced::widget::scrollable::Viewport;
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, column, container, keyed_column, row, scrollable, text, text_editor,
};
use iced::{Background, Border, Color, Element, Event, Length, Padding, Subscription, Task, Theme};
use sola_bus::Message;
use sola_bus::topics::{MailConfig, MailRule, Topic};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::prose::prose_selectable;
use sola_kit::components::style::{
    HAIRLINE_A, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::toolbar::toolbar_icon_tip;
use sola_kit::components::{
    ProseBlock, SidebarItem, SidebarSection, button as kit_btn, field, readable, sidebar,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::bridge::{self, mail_send};
use crate::protocol::{Folder, MessageBody, MessageSummary, folder_count_badge, folder_label};
use crate::worker::{MailCmd, MailEvent};

const APP_ID: &str = "sola-mail";
const PAGE: u32 = 50;
const LIST_W: f32 = 328.0;
const SIDEBAR_W: f32 = 200.0;
/// Shared list-header / reader-toolbar height so icons sit on one line.
const CHROME_H: f32 = 40.0;
/// Comfortable reading measure (~65ch at 14px prose).
const READ_MAX_W: f32 = 640.0;
/// Wait for rapid deletes to settle before fetching the next body.
const SETTLE_SELECT: Duration = Duration::from_millis(120);

fn list_scroll_id() -> ScrollId {
    ScrollId::new("mail-message-list")
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Worker(MailEvent),
    SelectFolder(String),
    SelectMessage(u32),
    SearchChanged(String),
    SearchSubmit,
    SearchClear,
    LoadMore,
    ListScrolled(Viewport),
    Compose,
    Reply {
        all: bool,
    },
    CancelCompose,
    ComposeFrom(String),
    ComposeTo(String),
    ComposeCc(String),
    ComposeSubject(String),
    ComposeBodyAction(text_editor::Action),
    ClipboardPasted(Option<String>),
    Send,
    CopyBody,
    MoveSelected(String),
    Undo,
    EmptyFolder,
    EmptyNamed(String),
    Refresh,
    OpenUrl(String),
    /// Visible letter selection (None = caret / cleared).
    BodySelect(Option<String>),
    DismissToast,
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    /// Quiet background re-fetch (IDLE + multi-client safety net).
    PollRefresh,
    /// After rapid delete/advance, load the body of the row we landed on.
    SettleSelect {
        uid: u32,
        generation: u64,
    },
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

#[derive(Debug, Clone)]
struct LastMove {
    uid: u32,
    from_folder: String,
    to_folder: String,
    list_id: String,
    summary: Option<MessageSummary>,
}

struct RemovedMail {
    summary: MessageSummary,
    from_folder: String,
    to_folder: String,
    selected_folder: String,
}

struct ComposeDraft {
    from: String,
    to: String,
    cc: String,
    subject: String,
    body: text_editor::Content,
    in_reply_to: Option<String>,
}

pub struct App {
    theme: Theme,
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
    /// Visible in-body selection (drag or Select All). Copy uses this.
    body_selection: Option<String>,
    /// Bump to force the letter widget to select everything.
    prose_select_all: u64,
    /// Cached letter blocks for the open message.
    reading_blocks: Vec<ProseBlock>,
    from_addresses: Vec<String>,
    rules: Vec<MailRule>,
    search_query: String,
    search_active: bool,
    search_total: u32,
    loading: bool,
    folder_loading: bool,
    is_loading_more: bool,
    toast: Option<String>,
    /// When set, toast (and reading toolbar) offer Undo.
    toast_undo: bool,
    composing: bool,
    draft: ComposeDraft,
    last_move: Option<LastMove>,
    /// UIDs removed from the list before IMAP MOVE finishes.
    pending_gone: HashSet<(String, u32)>,
    pending_removed: HashMap<u32, RemovedMail>,
    /// Bumped on every selection change so delayed body fetches can cancel.
    select_gen: u64,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    /// Last inbox unread we published on `Topic::MailStatus`.
    published_inbox_unread: Option<u32>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            theme: default_theme(),
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
            body_selection: None,
            prose_select_all: 0,
            reading_blocks: Vec::new(),
            from_addresses: Vec::new(),
            rules: Vec::new(),
            search_query: String::new(),
            search_active: false,
            search_total: 0,
            loading: true,
            folder_loading: false,
            is_loading_more: false,
            toast: None,
            toast_undo: false,
            composing: false,
            draft: empty_draft(""),
            last_move: None,
            pending_gone: HashSet::new(),
            pending_removed: HashMap::new(),
            select_gen: 0,
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            published_inbox_unread: None,
        }
    }
}

fn empty_draft(from: &str) -> ComposeDraft {
    ComposeDraft {
        from: from.to_string(),
        to: String::new(),
        cc: String::new(),
        subject: String::new(),
        body: text_editor::Content::new(),
        in_reply_to: None,
    }
}

fn draft_with_body(
    from: String,
    to: String,
    cc: String,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
) -> ComposeDraft {
    ComposeDraft {
        from,
        to,
        cc,
        subject,
        body: text_editor::Content::with_text(&body),
        in_reply_to,
    }
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        (
            Self::default(),
            sola_kit::window_ready_task(Msg::WindowReady),
        )
    }

    pub fn title(&self) -> String {
        "Mail".into()
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        // Periodic refresh catches deletes when IDLE is quiet or the server
        // only notifies on keepalive (common with multi-client expunge).
        let poll = if self.connected && !self.loading {
            iced::time::every(std::time::Duration::from_secs(45)).map(|_| Msg::PollRefresh)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            bridge::mail_subscription().map(Msg::Worker),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Msg::KeyPressed(key, modifiers))
                }
                _ => None,
            }),
            poll,
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => self.on_bus(&message),
            Msg::WindowReady(id) => {
                self.window_id = id;
                Task::none()
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(APP_ID);
                Task::none()
            }
            Msg::Worker(ev) => self.on_worker(ev),
            Msg::SelectFolder(name) => {
                self.selected_folder = name.clone();
                self.selected_uid = None;
                self.message_body = None;
                self.body_selection = None;
                self.reading_blocks.clear();
                self.composing = false;
                self.search_active = false;
                self.search_query.clear();
                self.select_gen = self.select_gen.saturating_add(1);
                self.pending_gone.clear();
                self.load_folder(name);
                Task::none()
            }
            Msg::SelectMessage(uid) => {
                self.select_gen = self.select_gen.saturating_add(1);
                self.select_message(uid);
                Task::none()
            }
            Msg::SettleSelect { uid, generation } => {
                if generation != self.select_gen || self.selected_uid != Some(uid) {
                    return Task::none();
                }
                self.select_message(uid);
                Task::none()
            }
            Msg::SearchChanged(q) => {
                self.search_query = q;
                Task::none()
            }
            Msg::SearchClear => {
                self.search_query.clear();
                if self.search_active {
                    self.search_active = false;
                    self.search_total = 0;
                    self.load_folder(self.selected_folder.clone());
                }
                Task::none()
            }
            Msg::SearchSubmit => {
                let q = self.search_query.trim().to_string();
                if q.is_empty() {
                    return Task::none();
                }
                self.search_active = true;
                self.folder_loading = true;
                mail_send(MailCmd::Search { query: q });
                Task::none()
            }
            Msg::LoadMore => self.load_more(),
            Msg::ListScrolled(vp) => {
                let remain =
                    vp.content_bounds().height - vp.bounds().height - vp.absolute_offset().y;
                if remain < 280.0 {
                    return self.update(Msg::LoadMore);
                }
                Task::none()
            }
            Msg::Compose => {
                let from = self
                    .from_addresses
                    .first()
                    .cloned()
                    .unwrap_or_else(|| self.mail_config.email.clone());
                self.draft = empty_draft(&from);
                self.composing = true;
                Task::none()
            }
            Msg::Reply { all } => {
                let Some(body) = self.message_body.clone() else {
                    return Task::none();
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
                self.draft = draft_with_body(
                    from,
                    to,
                    cc,
                    subj,
                    format!("\n\nOn {} {} wrote:\n{quoted}", body.date, body.from),
                    body.message_id.clone(),
                );
                self.composing = true;
                Task::none()
            }
            Msg::CancelCompose => {
                self.composing = false;
                Task::none()
            }
            Msg::ComposeFrom(s) => {
                self.draft.from = s;
                Task::none()
            }
            Msg::ComposeTo(s) => {
                self.draft.to = s;
                Task::none()
            }
            Msg::ComposeCc(s) => {
                self.draft.cc = s;
                Task::none()
            }
            Msg::ComposeSubject(s) => {
                self.draft.subject = s;
                Task::none()
            }
            Msg::ComposeBodyAction(action) => {
                self.draft.body.perform(action);
                Task::none()
            }
            Msg::ClipboardPasted(text) => {
                if self.composing {
                    if let Some(t) = text {
                        self.draft.body.perform(text_editor::Action::Edit(
                            text_editor::Edit::Paste(std::sync::Arc::new(t)),
                        ));
                    }
                }
                Task::none()
            }
            Msg::CopyBody => {
                let t = sola_kit::components::prose::flatten(&self.reading_blocks);
                if t.is_empty() {
                    Task::none()
                } else {
                    iced::clipboard::write(t)
                }
            }
            Msg::BodySelect(sel) => {
                self.body_selection = sel;
                Task::none()
            }
            Msg::Send => {
                if self.draft.to.trim().is_empty() {
                    self.toast = Some("To address is required".into());
                    self.toast_undo = false;
                    return Task::none();
                }
                mail_send(MailCmd::Send {
                    from: self.draft.from.clone(),
                    to: self.draft.to.clone(),
                    cc: self.draft.cc.clone(),
                    subject: self.draft.subject.clone(),
                    body: self.draft.body.text(),
                    in_reply_to: self.draft.in_reply_to.clone(),
                });
                Task::none()
            }
            Msg::MoveSelected(dest) => {
                let Some(uid) = self.selected_uid else {
                    return Task::none();
                };
                self.move_and_advance(uid, dest)
            }
            Msg::Undo => self.undo_last_move(),
            Msg::EmptyFolder => {
                let folder = self.selected_folder.clone();
                self.begin_empty(&folder);
                Task::none()
            }
            Msg::EmptyNamed(folder) => {
                self.begin_empty(&folder);
                Task::none()
            }
            Msg::Refresh => {
                if self.connected && !self.loading {
                    self.refresh_all();
                }
                Task::none()
            }
            Msg::OpenUrl(url) => {
                sola_core::open_url_logged(&url);
                Task::none()
            }
            Msg::DismissToast => {
                self.toast = None;
                self.toast_undo = false;
                Task::none()
            }
            Msg::KeyPressed(key, mods) => self.on_key(key, mods),
            Msg::PollRefresh => {
                if self.connected && !self.composing && !self.loading && !self.folder_loading {
                    // Silent refresh — do not toast on transient failures.
                    self.refresh_all();
                }
                Task::none()
            }
        }
    }

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            self.retract_mail_status();
            mail_send(MailCmd::Shutdown);
            return iced::exit();
        }
        if let Some(Topic::MenuAction(p)) = Topic::parse(message) {
            if p.app_id == APP_ID {
                return self.on_menu_action(&p.action_id);
            }
        }
        if let Some(Topic::MailConfig(cfg)) = Topic::parse(message) {
            self.mail_config = cfg.clone();
            mail_send(MailCmd::Reconfigure(cfg));
            self.loading = true;
        }
        Task::none()
    }

    fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        match action {
            "cut" => self.edit_cut(),
            "copy" => self.edit_copy(),
            "paste" => self.edit_paste(),
            "select_all" => {
                if self.composing {
                    self.draft.body.perform(text_editor::Action::SelectAll);
                } else if self.message_body.is_some() {
                    self.prose_select_all = self.prose_select_all.saturating_add(1);
                    let vis = sola_kit::components::prose::visible_text(&self.reading_blocks);
                    self.body_selection = if vis.is_empty() { None } else { Some(vis) };
                }
                Task::none()
            }
            "quit" => {
                self.retract_mail_status();
                mail_send(MailCmd::Shutdown);
                iced::exit()
            }
            "compose" => self.update(Msg::Compose),
            "reply" => self.update(Msg::Reply { all: false }),
            "reply_all" => self.update(Msg::Reply { all: true }),
            "archive" => self.update(Msg::MoveSelected("Archive".into())),
            "trash" => self.update(Msg::MoveSelected("Trash".into())),
            "junk" => self.update(Msg::MoveSelected("Junk".into())),
            "inbox" => self.update(Msg::MoveSelected("INBOX".into())),
            "undo" => self.update(Msg::Undo),
            "copy_message" => self.update(Msg::CopyBody),
            "empty_junk" => self.update(Msg::EmptyNamed("Junk".into())),
            "empty_trash" => self.update(Msg::EmptyNamed("Trash".into())),
            "refresh" => self.update(Msg::Refresh),
            "next" => {
                self.select_next_or_first();
                Task::none()
            }
            "prev" => {
                self.select_prev_or_last();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn edit_copy(&self) -> Task<Msg> {
        if self.composing {
            if let Some(sel) = self.draft.body.selection() {
                if !sel.is_empty() {
                    return iced::clipboard::write(sel);
                }
            }
            let t = self.draft.body.text();
            if !t.is_empty() {
                return iced::clipboard::write(t);
            }
            return Task::none();
        }
        if let Some(sel) = self.body_selection.as_deref() {
            if !sel.is_empty() {
                return iced::clipboard::write(sel.to_string());
            }
        }
        // No selection → copy the open letter (flatten includes URLs).
        let t = sola_kit::components::prose::flatten(&self.reading_blocks);
        if !t.is_empty() {
            return iced::clipboard::write(t);
        }
        Task::none()
    }

    fn edit_cut(&mut self) -> Task<Msg> {
        if !self.composing {
            return Task::none();
        }
        let Some(sel) = self.draft.body.selection() else {
            return Task::none();
        };
        if sel.is_empty() {
            return Task::none();
        }
        self.draft
            .body
            .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
        iced::clipboard::write(sel)
    }

    fn edit_paste(&self) -> Task<Msg> {
        if !self.composing {
            return Task::none();
        }
        iced::clipboard::read().map(Msg::ClipboardPasted)
    }

    fn on_worker(&mut self, ev: MailEvent) -> Task<Msg> {
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
                self.publish_inbox_unread();
            }
            MailEvent::NotConfigured => {
                self.connected = false;
                self.not_configured = true;
                self.loading = false;
                self.folders.clear();
                self.messages.clear();
                self.publish_inbox_unread();
            }
            MailEvent::Folders {
                folders,
                smart_counts,
            } => {
                self.folders = folders;
                self.smart_counts = smart_counts;
                self.publish_inbox_unread();
            }
            MailEvent::Messages {
                folder,
                messages,
                total,
                offset,
            } => {
                if folder != self.selected_folder && !self.search_active {
                    return Task::none();
                }
                let replace = self.folder_loading;
                self.folder_loading = false;
                self.is_loading_more = false;
                let hidden = list_sync::hidden_on_server(&messages, &self.pending_gone, &folder);
                list_sync::prune_pending_gone(&mut self.pending_gone, &folder, &messages, offset);
                self.total_messages = total.saturating_sub(hidden);
                if let Some(f) = self.folders.iter_mut().find(|f| f.name == folder) {
                    f.total = self.total_messages;
                }
                if replace {
                    self.messages = messages
                        .into_iter()
                        .filter(|m| !self.pending_gone.contains(&(folder.clone(), m.uid)))
                        .collect();
                } else {
                    self.messages = list_sync::apply_message_page(
                        &self.messages,
                        messages,
                        offset,
                        &self.pending_gone,
                        &folder,
                    );
                }
            }
            MailEvent::SearchResults { messages, total } => {
                self.folder_loading = false;
                let folder = self.selected_folder.clone();
                let hidden = list_sync::hidden_on_server(&messages, &self.pending_gone, &folder);
                self.messages = messages
                    .into_iter()
                    .filter(|m| !self.pending_gone.contains(&(folder.clone(), m.uid)))
                    .collect();
                self.search_total = total.saturating_sub(hidden);
                self.total_messages = self.search_total;
            }
            MailEvent::Body(body) => {
                if self.selected_uid != Some(body.uid) {
                    return Task::none();
                }
                let blocks = body.reading_blocks();
                let plain = sola_kit::components::prose::flatten(&blocks);
                tracing::debug!(
                    uid = body.uid,
                    n_blocks = blocks.len(),
                    text_len = plain.len(),
                    has_html = body.html.is_some(),
                    "opened message body"
                );
                self.body_selection = None;
                self.reading_blocks = blocks;
                self.message_body = Some(body);
            }
            MailEvent::Sent => {
                self.composing = false;
                self.toast = Some("Message sent".into());
                self.toast_undo = false;
                mail_send(MailCmd::ListFolders);
            }
            MailEvent::Moved { uid } => {
                self.pending_removed.remove(&uid);
            }
            MailEvent::MoveFailed { uid, message } => {
                self.toast = Some(format!("move: {message}"));
                self.toast_undo = false;
                self.restore_removed(uid);
            }
            MailEvent::Emptied { folder } => {
                self.folder_loading = false;
                self.toast = Some(format!("{} erased", folder_label(&folder)));
                self.toast_undo = false;
                mail_send(MailCmd::ListFolders);
                if self.selected_folder.eq_ignore_ascii_case(&folder) {
                    self.load_folder(folder);
                }
            }
            MailEvent::NewMail => {
                self.refresh_all();
            }
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
                }
            }
        }
        Task::none()
    }

    fn on_key(&mut self, key: keyboard::Key, mods: keyboard::Modifiers) -> Task<Msg> {
        if matches!(key, keyboard::Key::Named(NamedKey::Escape)) {
            if self.composing {
                self.composing = false;
                return Task::none();
            }
            if self.search_active || !self.search_query.is_empty() {
                self.search_active = false;
                self.search_query.clear();
                self.search_total = 0;
                self.load_folder(self.selected_folder.clone());
                return Task::none();
            }
        }

        if self.composing {
            return Task::none();
        }
        if matches!(key, keyboard::Key::Named(NamedKey::ArrowUp)) {
            self.select_prev_or_last();
            return Task::none();
        }
        if matches!(key, keyboard::Key::Named(NamedKey::ArrowDown)) {
            self.select_next_or_first();
            return Task::none();
        }
        if mods.control() || mods.alt() || mods.logo() {
            return Task::none();
        }
        if matches!(
            key,
            keyboard::Key::Named(
                NamedKey::Tab
                    | NamedKey::Enter
                    | NamedKey::Escape
                    | NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
            )
        ) {
            return Task::none();
        }
        let Some(uid) = self.selected_uid else {
            return Task::none();
        };
        let ch = match &key {
            keyboard::Key::Character(c) => c.as_str(),
            _ => return Task::none(),
        };
        match ch {
            "j" => return self.move_and_advance(uid, "Junk".into()),
            "i" => return self.move_and_advance(uid, "INBOX".into()),
            "a" => return self.move_and_advance(uid, "Archive".into()),
            "d" => return self.move_and_advance(uid, "Trash".into()),
            "u" => return self.undo_last_move(),
            "w" => self.select_prev(),
            "s" => self.select_next(),
            _ => {}
        }
        Task::none()
    }

    fn real_folder(&self) -> String {
        if self.selected_folder.starts_with("smart:") {
            "INBOX".into()
        } else {
            self.selected_folder.clone()
        }
    }

    fn load_more(&mut self) -> Task<Msg> {
        if self.is_loading_more || self.search_active || self.folder_loading {
            return Task::none();
        }
        if (self.messages.len() as u32) >= self.total_messages {
            return Task::none();
        }
        self.is_loading_more = true;
        mail_send(MailCmd::ListMessages {
            folder: self.selected_folder.clone(),
            offset: self.messages.len() as u32,
            limit: PAGE,
        });
        Task::none()
    }

    fn begin_empty(&mut self, folder: &str) {
        self.toast = Some(format!("Erasing {}…", folder_label(folder)));
        self.toast_undo = false;
        self.select_gen = self.select_gen.saturating_add(1);
        if self.selected_folder.eq_ignore_ascii_case(folder) {
            self.folder_loading = true;
            self.messages.clear();
            self.pending_gone.clear();
            self.pending_removed.clear();
            self.selected_uid = None;
            self.message_body = None;
            self.body_selection = None;
            self.reading_blocks.clear();
        }
        mail_send(MailCmd::EmptyFolder {
            folder: folder.to_string(),
        });
    }

    fn is_emptyable_folder(&self) -> bool {
        matches!(
            self.selected_folder.as_str(),
            "Trash" | "Junk" | "trash" | "junk"
        ) || self.selected_folder.eq_ignore_ascii_case("Trash")
            || self.selected_folder.eq_ignore_ascii_case("Junk")
    }

    fn load_folder(&mut self, name: String) {
        self.folder_loading = true;
        mail_send(MailCmd::ListMessages {
            folder: name,
            offset: 0,
            limit: PAGE,
        });
    }

    fn refresh_list_silent(&mut self) {
        if self.search_active {
            return;
        }
        let limit = (self.messages.len() as u32)
            .saturating_add(self.pending_gone.len() as u32)
            .max(PAGE);
        mail_send(MailCmd::ListMessages {
            folder: self.selected_folder.clone(),
            offset: 0,
            limit,
        });
    }

    fn refresh_all(&mut self) {
        if self.search_active {
            return;
        }
        mail_send(MailCmd::ListFolders);
        self.refresh_list_silent();
    }

    fn select_message(&mut self, uid: u32) {
        self.selected_uid = Some(uid);
        self.body_selection = None;
        self.composing = false;
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
                self.publish_inbox_unread();
            }
        }
    }

    fn move_and_advance(&mut self, uid: u32, dest: String) -> Task<Msg> {
        let folder = self.real_folder();
        let idx = self.messages.iter().position(|m| m.uid == uid);
        let summary = idx.and_then(|i| self.messages.get(i).cloned());
        self.last_move = Some(LastMove {
            uid,
            from_folder: folder.clone(),
            to_folder: dest.clone(),
            list_id: self.selected_folder.clone(),
            summary: summary.clone(),
        });
        self.toast = Some(format!("Moved to {dest}"));
        self.toast_undo = true;
        self.pending_gone
            .insert((self.selected_folder.clone(), uid));
        if let Some(summary) = summary.clone() {
            self.adjust_counts_for_move(&summary, &folder, &dest, true);
            self.pending_removed.insert(
                uid,
                RemovedMail {
                    summary,
                    from_folder: folder.clone(),
                    to_folder: dest.clone(),
                    selected_folder: self.selected_folder.clone(),
                },
            );
        }
        self.total_messages = self.total_messages.saturating_sub(1);
        mail_send(MailCmd::Move { folder, uid, dest });
        self.messages.retain(|m| m.uid != uid);
        self.select_gen = self.select_gen.saturating_add(1);
        if self.messages.is_empty() {
            self.selected_uid = None;
            self.message_body = None;
            self.body_selection = None;
            self.reading_blocks.clear();
            return Task::none();
        }
        let Some(i) = idx else {
            return Task::none();
        };
        let next = if i > 0 { i - 1 } else { 0 };
        let next_uid = self.messages[next.min(self.messages.len() - 1)].uid;
        self.selected_uid = Some(next_uid);
        let generation = self.select_gen;
        Task::perform(tokio::time::sleep(SETTLE_SELECT), move |_| {
            Msg::SettleSelect {
                uid: next_uid,
                generation,
            }
        })
    }

    fn undo_last_move(&mut self) -> Task<Msg> {
        let Some(lm) = self.last_move.take() else {
            return Task::none();
        };
        mail_send(MailCmd::Move {
            folder: lm.to_folder.clone(),
            uid: lm.uid,
            dest: lm.from_folder.clone(),
        });
        self.toast = Some("Move undone".into());
        self.toast_undo = false;
        self.pending_gone.remove(&(lm.list_id.clone(), lm.uid));
        let summary = self
            .pending_removed
            .remove(&lm.uid)
            .map(|r| r.summary)
            .or(lm.summary);
        if let Some(summary) = summary {
            if self.selected_folder == lm.list_id {
                self.adjust_counts_for_move(&summary, &lm.from_folder, &lm.to_folder, false);
                self.total_messages = self.total_messages.saturating_add(1);
                list_sync::insert_summary_desc(&mut self.messages, summary);
            }
        }
        Task::none()
    }

    fn restore_removed(&mut self, uid: u32) {
        self.pending_gone.retain(|(_, gone)| *gone != uid);
        let Some(removed) = self.pending_removed.remove(&uid) else {
            return;
        };
        if removed.selected_folder != self.selected_folder {
            return;
        }
        self.adjust_counts_for_move(
            &removed.summary,
            &removed.from_folder,
            &removed.to_folder,
            false,
        );
        self.total_messages = self.total_messages.saturating_add(1);
        list_sync::insert_summary_desc(&mut self.messages, removed.summary);
        if let Some(lm) = &self.last_move {
            if lm.uid == uid {
                self.last_move = None;
                self.toast_undo = false;
            }
        }
    }

    fn adjust_counts_for_move(
        &mut self,
        summary: &MessageSummary,
        from: &str,
        dest: &str,
        leaving: bool,
    ) {
        let unread = if summary.seen { 0 } else { 1 };
        let (from_d, dest_d) = if leaving { (-1, 1) } else { (1, -1) };
        bump_folder(&mut self.folders, from, from_d, from_d * unread as i32);
        if !dest.is_empty() {
            bump_folder(&mut self.folders, dest, dest_d, dest_d * unread as i32);
        }
        if let Some(name) = self.selected_folder.strip_prefix("smart:") {
            bump_folder(&mut self.smart_counts, name, from_d, from_d * unread as i32);
        }
        self.publish_inbox_unread();
    }

    fn select_prev(&mut self) {
        let Some(uid) = self.selected_uid else {
            return;
        };
        let Some(idx) = self.messages.iter().position(|m| m.uid == uid) else {
            return;
        };
        if idx == 0 {
            return;
        }
        self.select_gen = self.select_gen.saturating_add(1);
        self.select_message(self.messages[idx - 1].uid);
    }

    fn select_next(&mut self) {
        let Some(uid) = self.selected_uid else {
            return;
        };
        let Some(idx) = self.messages.iter().position(|m| m.uid == uid) else {
            return;
        };
        if idx + 1 >= self.messages.len() {
            return;
        }
        self.select_gen = self.select_gen.saturating_add(1);
        self.select_message(self.messages[idx + 1].uid);
    }

    fn select_next_or_first(&mut self) {
        if self.selected_uid.is_none() {
            if let Some(first) = self.messages.first() {
                self.select_gen = self.select_gen.saturating_add(1);
                self.select_message(first.uid);
            }
            return;
        }
        self.select_next();
    }

    fn select_prev_or_last(&mut self) {
        if self.selected_uid.is_none() {
            if let Some(last) = self.messages.last() {
                self.select_gen = self.select_gen.saturating_add(1);
                self.select_message(last.uid);
            }
            return;
        }
        self.select_prev();
    }

    fn inbox_unread(&self) -> u32 {
        self.folders
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
            .map(|f| f.unread)
            .unwrap_or(0)
    }

    fn publish_inbox_unread(&mut self) {
        let n = self.inbox_unread();
        if self.published_inbox_unread == Some(n) {
            return;
        }
        self.published_inbox_unread = Some(n);
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::MailStatus(sola_bus::topics::MailStatus {
                inbox_unread: n,
            }));
        }
    }

    fn retract_mail_status(&mut self) {
        self.published_inbox_unread = None;
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.retract(Topic::MailStatus(sola_bus::topics::MailStatus {
                inbox_unread: 0,
            }));
        }
    }

    // ── View ──────────────────────────────────────────────────────────

    pub fn view(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = if self.loading {
            container(kit_text::body("Connecting…"))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(canvas_style)
                .into()
        } else if self.not_configured {
            container(
                column![
                    kit_text::heading("Mail not configured"),
                    kit_text::caption("Add your account in Settings → Mail, then reopen.")
                        .style(kit_text::muted),
                ]
                .spacing(SPACE_SM),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(canvas_style)
            .into()
        } else {
            row![
                self.view_folders(),
                v_hairline(),
                self.view_message_list(),
                v_hairline(),
                column![
                    self.view_reader_toolbar(),
                    if self.composing {
                        self.view_compose()
                    } else {
                        self.view_message()
                    },
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .height(Length::Fill)
            .into()
        };

        let mut col = column![content].width(Length::Fill).height(Length::Fill);
        if let Some(toast) = &self.toast {
            col = col.push(self.view_toast(toast));
        }
        let body: Element<'_, Msg> = container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(canvas_style)
            .into();

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Mail",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body,
        )
    }

    fn view_toast<'a>(&'a self, toast: &'a str) -> Element<'a, Msg> {
        let mut actions = row![].spacing(SPACE_SM);
        if self.toast_undo && self.last_move.is_some() {
            actions =
                actions.push(kit_btn::labeled_sm("Undo", kit_btn::secondary).on_press(Msg::Undo));
        }
        actions = actions
            .push(kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissToast));

        container(
            row![
                kit_text::body(toast.to_string()),
                Space::new().width(Length::Fill),
                actions,
            ]
            .spacing(SPACE_SM)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from([SPACE_MD, SPACE_LG]))
        .width(Length::Fill)
        .style(toast_style)
        .into()
    }

    fn view_folders(&self) -> Element<'_, Msg> {
        let mut sections = Vec::new();

        let mut mailbox_items: Vec<SidebarItem<'_, Msg>> = Vec::new();
        for f in &self.folders {
            let mut item =
                SidebarItem::new(folder_label(&f.name), Msg::SelectFolder(f.name.clone()))
                    .active(self.selected_folder == f.name);
            if let Some(badge) = folder_count_badge(f.unread, f.total) {
                item = item.secondary(badge);
            }
            mailbox_items.push(item);
        }
        sections.push(SidebarSection::new("Mailboxes", mailbox_items).fill());

        if !self.smart_counts.is_empty() {
            let mut smart_items = Vec::new();
            for f in &self.smart_counts {
                let id = format!("smart:{}", f.name);
                let mut item = SidebarItem::new(f.name.clone(), Msg::SelectFolder(id.clone()))
                    .active(self.selected_folder == id);
                if let Some(badge) = folder_count_badge(f.unread, f.total) {
                    item = item.secondary(badge);
                }
                smart_items.push(item);
            }
            sections.push(SidebarSection::new("Smart", smart_items));
        }

        container(sidebar(sections))
            .width(Length::Fixed(SIDEBAR_W))
            .height(Length::Fill)
            .into()
    }

    fn view_message_list(&self) -> Element<'_, Msg> {
        let search = text_input("Search", &self.search_query)
            .on_input(Msg::SearchChanged)
            .on_submit(Msg::SearchSubmit)
            .size(13)
            .padding(Padding {
                top: 5.0,
                right: 10.0,
                bottom: 5.0,
                left: 10.0,
            })
            .width(Length::Fill);

        let count = if self.search_active {
            format_count(self.search_total)
        } else if self.folder_loading {
            "…".into()
        } else {
            format_count(self.total_messages)
        };

        let mut header = row![search, kit_text::caption(count).style(kit_text::muted),]
            .spacing(SPACE_MD)
            .align_y(iced::Alignment::Center);
        if !self.search_query.is_empty() || self.search_active {
            header = header.push(icon_tool(
                "lucide/x",
                "Clear search",
                Some(Msg::SearchClear),
            ));
        }
        if self.is_emptyable_folder() && !self.search_active {
            header = header.push(icon_tool(
                "lucide/trash-2",
                "Erase folder",
                Some(Msg::EmptyFolder),
            ));
        }
        let header = container(header)
            .width(Length::Fill)
            .height(Length::Fixed(CHROME_H))
            .padding(Padding {
                top: 0.0,
                right: SPACE_LG,
                bottom: 0.0,
                left: SPACE_LG,
            })
            .align_y(iced::Alignment::Center)
            .style(toolbar_style);

        let empty_caption = container(
            kit_text::caption(if self.search_active {
                "No matching messages"
            } else {
                "No messages"
            })
            .style(kit_text::muted),
        )
        .padding(Padding::from([SPACE_XL, SPACE_MD]))
        .width(Length::Fill);

        let list: Element<'_, Msg> = if self.messages.is_empty() && !self.folder_loading {
            keyed_column([(0u32, empty_caption.into())])
                .spacing(0)
                .width(Length::Fill)
                .into()
        } else {
            keyed_column(
                self.messages
                    .iter()
                    .map(|m| (m.uid, message_row(m, self.selected_uid == Some(m.uid)))),
            )
            .spacing(0)
            .width(Length::Fill)
            .into()
        };

        container(
            column![
                header,
                scrollable(default_cursor(list))
                    .id(list_scroll_id())
                    .height(Length::Fill)
                    .width(Length::Fill)
                    .on_scroll(Msg::ListScrolled),
            ]
            .height(Length::Fill),
        )
        .width(Length::Fixed(LIST_W))
        .height(Length::Fill)
        .style(list_pane_style)
        .into()
    }

    fn view_reader_toolbar(&self) -> Element<'_, Msg> {
        let has_msg = self.selected_uid.is_some() && !self.composing;
        let mut bar = row![
            icon_tool("lucide/square-pen", "Compose", Some(Msg::Compose)),
            icon_tool(
                "lucide/reply",
                "Reply",
                has_msg.then_some(Msg::Reply { all: false }),
            ),
            icon_tool(
                "lucide/reply-all",
                "Reply all",
                has_msg.then_some(Msg::Reply { all: true }),
            ),
            icon_tool(
                "lucide/archive",
                "Archive",
                has_msg.then_some(Msg::MoveSelected("Archive".into())),
            ),
            icon_tool(
                "lucide/trash-2",
                "Trash",
                has_msg.then_some(Msg::MoveSelected("Trash".into())),
            ),
            icon_tool(
                "lucide/ban",
                "Junk",
                has_msg.then_some(Msg::MoveSelected("Junk".into())),
            ),
        ]
        .spacing(SPACE_XS)
        .align_y(iced::Alignment::Center);

        bar = bar.push(Space::new().width(Length::Fill));
        if has_msg {
            bar = bar.push(icon_tool(
                "lucide/copy",
                "Copy message",
                Some(Msg::CopyBody),
            ));
        }
        if self.last_move.is_some() {
            bar = bar.push(icon_tool("lucide/undo-2", "Undo move", Some(Msg::Undo)));
        }

        container(bar)
            .width(Length::Fill)
            .height(Length::Fixed(CHROME_H))
            .padding(Padding {
                top: 0.0,
                right: SPACE_LG,
                bottom: 0.0,
                left: SPACE_LG,
            })
            .align_y(iced::Alignment::Center)
            .style(toolbar_style)
            .into()
    }

    fn view_message(&self) -> Element<'_, Msg> {
        let Some(body) = &self.message_body else {
            return container(
                column![
                    kit_text::body("No message selected").style(kit_text::muted),
                    kit_text::caption("Pick one from the list, or press ↓").style(kit_text::muted),
                ]
                .spacing(SPACE_SM)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(read_pane_style)
            .into();
        };

        let letter = readable(
            column![
                letter_header(body),
                h_hairline(),
                prose_selectable(
                    self.reading_blocks.clone(),
                    &self.theme,
                    self.prose_select_all,
                    Msg::OpenUrl,
                    Msg::BodySelect,
                ),
            ]
            .spacing(20.0)
            .width(Length::Fill)
            .padding(Padding {
                top: 28.0,
                right: 8.0,
                bottom: 40.0,
                left: 8.0,
            }),
            READ_MAX_W,
        )
        .width(Length::Fill);

        container(scrollable(letter).height(Length::Fill).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(read_pane_style)
            .into()
    }

    fn view_compose(&self) -> Element<'_, Msg> {
        let editor = text_editor(&self.draft.body)
            .placeholder("Write a message…")
            .height(Length::Fill)
            .padding(12)
            .style(compose_editor_style)
            .on_action(Msg::ComposeBodyAction);

        let form = column![
            field(
                "From",
                text_input("from@", &self.draft.from).on_input(Msg::ComposeFrom),
                None,
                None,
            ),
            field(
                "To",
                text_input("to@", &self.draft.to).on_input(Msg::ComposeTo),
                None,
                None,
            ),
            field(
                "Cc",
                text_input("cc@", &self.draft.cc).on_input(Msg::ComposeCc),
                None,
                None,
            ),
            field(
                "Subject",
                text_input("Subject", &self.draft.subject).on_input(Msg::ComposeSubject),
                None,
                None,
            ),
            container(editor)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(compose_field_style),
            row![
                kit_btn::labeled("Send", kit_btn::primary).on_press(Msg::Send),
                kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::CancelCompose),
            ]
            .spacing(SPACE_SM),
        ]
        .spacing(SPACE_MD)
        .padding(Padding::from([SPACE_XL, SPACE_XL]))
        .width(Length::Fill);

        container(
            readable(form, READ_MAX_W)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(read_pane_style)
        .into()
    }
}

// ── List row ──────────────────────────────────────────────────────────

fn message_row(m: &MessageSummary, selected: bool) -> Element<'_, Msg> {
    let from = short_from(&m.from);
    let subj = if m.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        m.subject.clone()
    };
    let date = short_date(&m.date);

    // Mail.app: unread is weight, not a leading dot. Both lines clip
    // to one row so the column stays scannable.
    let from_text = text(from)
        .font(if m.seen {
            fonts::ui()
        } else {
            fonts::ui_medium()
        })
        .size(13)
        .wrapping(Wrapping::None)
        .width(Length::Fill);

    let mut subj_text = text(subj)
        .font(if m.seen {
            fonts::ui()
        } else {
            fonts::ui_medium()
        })
        .size(13)
        .wrapping(Wrapping::None)
        .width(Length::Fill);
    if m.seen {
        subj_text = subj_text.style(kit_text::muted);
    }

    let top = row![
        container(from_text).width(Length::Fill).clip(true),
        kit_text::caption(date).style(kit_text::muted),
    ]
    .spacing(SPACE_MD)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill);

    let content = column![top, container(subj_text).width(Length::Fill).clip(true)]
        .spacing(6.0)
        .width(Length::Fill)
        .padding(Padding {
            top: 14.0,
            right: 16.0,
            bottom: 14.0,
            left: 16.0,
        });

    button(content)
        .on_press(Msg::SelectMessage(m.uid))
        .padding(0)
        .width(Length::Fill)
        .style(kit_btn::list_item(selected))
        .into()
}

// ── Display helpers ───────────────────────────────────────────────────

fn icon_tool(
    name: &'static str,
    tip: &'static str,
    on_press: Option<Msg>,
) -> Element<'static, Msg> {
    toolbar_icon_tip(cached_icon(name), tip, on_press)
}

fn cached_icon(name: &'static str) -> iced::widget::svg::Handle {
    static HANDLES: OnceLock<std::collections::HashMap<&'static str, iced::widget::svg::Handle>> =
        OnceLock::new();
    HANDLES
        .get_or_init(|| {
            [
                "lucide/square-pen",
                "lucide/reply",
                "lucide/reply-all",
                "lucide/archive",
                "lucide/trash-2",
                "lucide/ban",
                "lucide/copy",
                "lucide/undo-2",
                "lucide/x",
            ]
            .into_iter()
            .map(|n| (n, icon_handle(n)))
            .collect()
        })
        .get(name)
        .cloned()
        .unwrap_or_else(|| icon_handle(name))
}

fn bump_folder(folders: &mut [Folder], name: &str, d_total: i32, d_unread: i32) {
    let Some(f) = folders
        .iter_mut()
        .find(|f| f.name.eq_ignore_ascii_case(name))
    else {
        return;
    };
    f.total = if d_total < 0 {
        f.total.saturating_sub(d_total.unsigned_abs())
    } else {
        f.total.saturating_add(d_total as u32)
    };
    f.unread = if d_unread < 0 {
        f.unread.saturating_sub(d_unread.unsigned_abs())
    } else {
        f.unread.saturating_add(d_unread as u32)
    };
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

fn letter_header(body: &MessageBody) -> Element<'static, Msg> {
    let subj = if body.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        body.subject.clone()
    };
    let (name, addr) = split_address(&body.from);
    let date = letter_date(&body.date);

    let mut from_value = column![text(name).font(fonts::ui_medium()).size(14)]
        .spacing(2)
        .width(Length::Fill);
    if let Some(addr) = addr {
        from_value = from_value.push(kit_text::caption(addr).style(kit_text::muted));
    }

    let mut meta = column![
        letter_meta_row("From", from_value.into()),
        letter_meta_row(
            "Date",
            kit_text::caption(date).style(kit_text::muted).into(),
        ),
    ]
    .spacing(SPACE_LG)
    .width(Length::Fill);

    let to = body.to.trim();
    if !to.is_empty() {
        meta = meta.push(letter_meta_row(
            "To",
            kit_text::caption(to.to_string())
                .style(kit_text::muted)
                .into(),
        ));
    }
    let cc = body.cc.trim();
    if !cc.is_empty() {
        meta = meta.push(letter_meta_row(
            "Cc",
            kit_text::caption(cc.to_string())
                .style(kit_text::muted)
                .into(),
        ));
    }

    column![
        text(subj)
            .font(fonts::ui_medium())
            .size(22)
            .width(Length::Fill),
        meta,
    ]
    .spacing(SPACE_XL)
    .width(Length::Fill)
    .into()
}

fn letter_meta_row(label: &'static str, value: Element<'static, Msg>) -> Element<'static, Msg> {
    row![
        kit_text::caption(label)
            .style(kit_text::muted)
            .width(Length::Fixed(44.0)),
        value,
    ]
    .spacing(SPACE_LG)
    .align_y(iced::Alignment::Start)
    .width(Length::Fill)
    .into()
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
    let tokens: Vec<&str> = t.split_whitespace().collect();
    for i in 0..tokens.len().saturating_sub(2) {
        let day = tokens[i].trim_end_matches(',');
        if day.parse::<u8>().is_ok() && is_month_token(tokens[i + 1]) {
            let mon = tokens[i + 1];
            let year = tokens[i + 2].trim_end_matches(',');
            if year.parse::<u16>().is_ok() {
                return format!("{day} {mon} {year}");
            }
        }
    }
    short_date(date)
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

fn short_date(date: &str) -> String {
    let t = date.trim();
    if t.is_empty() {
        return String::new();
    }
    // ISO / RFC3339: 2026-07-28T… or 2026-07-28 …
    if t.len() >= 10 && t.as_bytes().get(4) == Some(&b'-') && t.as_bytes().get(7) == Some(&b'-') {
        if let (Ok(mo), Ok(d)) = (t[5..7].parse::<u8>(), t[8..10].parse::<u8>()) {
            return format!("{d} {}", month_abbr(mo));
        }
    }
    // RFC 2822-ish: "Mon, 28 Jul 2026 01:31:42 +0000" → "28 Jul"
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

fn v_hairline() -> Element<'static, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(hairline_style)
        .into()
}

fn h_hairline() -> Element<'static, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(1)
        .style(hairline_style)
        .into()
}

// ── List cursor ───────────────────────────────────────────────────────

/// Default arrow over the message list: no I-bar, no hand, no text copy.
///
/// `list_item` buttons would otherwise advertise `Pointer`, and iced's
/// row/column take the max child interaction (`Text` from the letter
/// outranks everything). Presses are captured so a drag here cannot
/// start a letter selection.
fn default_cursor<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::overlay;
    use iced::advanced::renderer;
    use iced::advanced::widget::tree::{self, Tree};
    use iced::advanced::widget::{Operation, Widget};
    use iced::advanced::{Clipboard, Shell};
    use iced::mouse;
    use iced::{Event, Rectangle, Size, Vector};

    struct DefaultCursor<'a> {
        content: Element<'a, Msg>,
    }

    impl Widget<Msg, Theme, iced::Renderer> for DefaultCursor<'_> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }

        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }

        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }

        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }

        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn size_hint(&self) -> Size<Length> {
            self.content.as_widget().size_hint()
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content.as_widget_mut().layout(tree, renderer, limits)
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }

        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Msg>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
                && cursor.is_over(layout.bounds())
            {
                shell.capture_event();
            }
        }

        fn mouse_interaction(
            &self,
            _tree: &Tree,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
            _renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            mouse::Interaction::None
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Msg, Theme, iced::Renderer>> {
            self.content
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)
        }
    }

    Element::new(DefaultCursor {
        content: content.into(),
    })
}

// ── Styles ────────────────────────────────────────────────────────────

fn canvas_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        ..container::Style::default()
    }
}

fn list_pane_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        ..container::Style::default()
    }
}

fn read_pane_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    // Slight raise vs list canvas.
    let fill = mix_white(p.background.base.color, 0.02);
    container::Style {
        background: Some(Background::Color(fill)),
        ..container::Style::default()
    }
}

fn toolbar_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        border: Border {
            color: mix_white(p.background.base.color, HAIRLINE_A),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn toast_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: mix_white(p.background.weaker.color, HAIRLINE_A),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn hairline_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(mix_white(
            p.background.base.color,
            HAIRLINE_A,
        ))),
        ..container::Style::default()
    }
}

fn compose_field_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: Border {
            color: mix_white(p.background.weaker.color, HAIRLINE_A),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

fn compose_editor_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = theme.extended_palette();
    let _ = status;
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.color
        },
        value: p.background.base.text,
        selection: mix_white(p.background.weaker.color, 0.16),
    }
}
