//! Kit UI: three-pane mail client (graphite list + reading composition).

use std::sync::Arc;

use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::{button, column, container, row, scrollable, text, text_editor, Space};
use iced::{Background, Border, Color, Element, Event, Length, Padding, Subscription, Task, Theme};
use sola_bus::Message;
use sola_bus::topics::{MailConfig, MailRule, Topic};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::style::{
    mix_white, HAIRLINE_A, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{button as kit_btn, sidebar, SidebarItem, SidebarSection};
use sola_kit::fonts;
use sola_kit::theme::{self, default_theme};

use crate::bridge::{self, mail_send};
use crate::protocol::{folder_count_badge, folder_label, Folder, MessageBody, MessageSummary};
use crate::worker::{MailCmd, MailEvent};

const APP_ID: &str = "sola-mail";
const PAGE: u32 = 50;
const LIST_W: f32 = 300.0;
const SIDEBAR_W: f32 = 200.0;
/// Comfortable reading measure (~65ch at 13px).
const READ_MAX_W: f32 = 560.0;

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Worker(MailEvent),
    SelectFolder(String),
    SelectMessage(u32),
    SearchChanged(String),
    SearchSubmit,
    ClearSearch,
    LoadMore,
    Compose,
    Reply { all: bool },
    CancelCompose,
    ComposeFrom(String),
    ComposeTo(String),
    ComposeCc(String),
    ComposeSubject(String),
    ComposeBodyAction(text_editor::Action),
    Send,
    MoveSelected(String),
    Undo,
    EmptyFolder,
    OpenUrl(String),
    DismissToast,
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    /// Quiet background re-fetch (IDLE + multi-client safety net).
    PollRefresh,
}

#[derive(Debug, Clone)]
struct LastMove {
    uid: u32,
    from_folder: String,
    to_folder: String,
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
    pub fn title(&self) -> String {
        "Mail".into()
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
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
            Msg::Worker(ev) => self.on_worker(ev),
            Msg::SelectFolder(name) => {
                self.selected_folder = name.clone();
                self.selected_uid = None;
                self.message_body = None;
                self.composing = false;
                self.search_active = false;
                self.search_query.clear();
                self.load_folder(name);
                Task::none()
            }
            Msg::SelectMessage(uid) => {
                self.select_message(uid);
                Task::none()
            }
            Msg::SearchChanged(q) => {
                self.search_query = q;
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
            Msg::ClearSearch => {
                self.search_active = false;
                self.search_query.clear();
                self.search_total = 0;
                self.load_folder(self.selected_folder.clone());
                Task::none()
            }
            Msg::LoadMore => {
                if self.is_loading_more || self.search_active {
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
                self.move_and_advance(uid, dest);
                Task::none()
            }
            Msg::Undo => {
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
                Task::none()
            }
            Msg::EmptyFolder => {
                mail_send(MailCmd::EmptyFolder {
                    folder: self.selected_folder.clone(),
                });
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
                if self.connected && !self.composing && !self.loading {
                    // Silent refresh — do not toast on transient failures.
                    self.refresh_all();
                }
                Task::none()
            }
        }
    }

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            mail_send(MailCmd::Shutdown);
            return iced::exit();
        }
        if let Some(Topic::MailConfig(cfg)) = Topic::parse(message) {
            self.mail_config = cfg.clone();
            mail_send(MailCmd::Reconfigure(cfg));
            self.loading = true;
        }
        Task::none()
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
            }
            MailEvent::NotConfigured => {
                self.connected = false;
                self.not_configured = true;
                self.loading = false;
                self.folders.clear();
                self.messages.clear();
            }
            MailEvent::Folders {
                folders,
                smart_counts,
            } => {
                self.folders = folders;
                self.smart_counts = smart_counts;
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
            }
            MailEvent::SearchResults { messages, total } => {
                self.folder_loading = false;
                self.messages = messages;
                self.search_total = total;
                self.total_messages = total;
            }
            MailEvent::Body(body) => {
                self.message_body = Some(body);
            }
            MailEvent::Sent => {
                self.composing = false;
                self.toast = Some("Message sent".into());
                self.toast_undo = false;
                mail_send(MailCmd::ListFolders);
            }
            MailEvent::Moved { .. } => {}
            MailEvent::Emptied { .. } => {
                self.load_folder(self.selected_folder.clone());
            }
            MailEvent::RulesApplied { moved } => {
                if moved > 0 {
                    self.toast = Some(format!("Moved {moved} messages"));
                    self.toast_undo = false;
                    self.refresh_all();
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
            }
            MailEvent::Disconnected { reason } => {
                self.connected = false;
                self.toast = Some(reason);
                self.toast_undo = false;
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
        if mods.control() || mods.alt() || mods.logo() {
            return Task::none();
        }
        if matches!(
            key,
            keyboard::Key::Named(
                NamedKey::Tab
                    | NamedKey::Enter
                    | NamedKey::Escape
                    | NamedKey::ArrowUp
                    | NamedKey::ArrowDown
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
            "j" => self.move_and_advance(uid, "Junk".into()),
            "i" => self.move_and_advance(uid, "INBOX".into()),
            "a" => self.move_and_advance(uid, "Archive".into()),
            "d" => self.move_and_advance(uid, "Trash".into()),
            "u" => {
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

    fn is_emptyable_folder(&self) -> bool {
        matches!(
            self.selected_folder.as_str(),
            "Trash" | "Junk" | "trash" | "junk"
        ) || self
            .selected_folder
            .eq_ignore_ascii_case("Trash")
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

    fn refresh_all(&mut self) {
        if self.search_active {
            return;
        }
        mail_send(MailCmd::ListFolders);
        self.load_folder(self.selected_folder.clone());
    }

    fn select_message(&mut self, uid: u32) {
        self.selected_uid = Some(uid);
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
            }
        }
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
        mail_send(MailCmd::Move {
            folder,
            uid,
            dest,
        });
        self.messages.retain(|m| m.uid != uid);
        if self.messages.is_empty() {
            self.selected_uid = None;
            self.message_body = None;
        } else if let Some(i) = idx {
            let next = if i > 0 { i - 1 } else { 0 };
            let next_uid = self.messages[next.min(self.messages.len() - 1)].uid;
            self.select_message(next_uid);
        }
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
        self.select_message(self.messages[idx + 1].uid);
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
                if self.composing {
                    self.view_compose()
                } else {
                    self.view_message()
                },
            ]
            .height(Length::Fill)
            .into()
        };

        let mut col = column![content].width(Length::Fill).height(Length::Fill);
        if let Some(toast) = &self.toast {
            col = col.push(self.view_toast(toast));
        }
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(canvas_style)
            .into()
    }

    fn view_toast<'a>(&'a self, toast: &'a str) -> Element<'a, Msg> {
        let mut actions = row![].spacing(SPACE_SM);
        if self.toast_undo && self.last_move.is_some() {
            actions = actions.push(
                kit_btn::labeled_sm("Undo", kit_btn::secondary).on_press(Msg::Undo),
            );
        }
        actions = actions.push(
            kit_btn::labeled_sm("Dismiss", kit_btn::ghost).on_press(Msg::DismissToast),
        );

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
                let mut item =
                    SidebarItem::new(f.name.clone(), Msg::SelectFolder(id.clone()))
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
        // Single chrome row: search + Compose (one primary).
        let search = text_input("Search…", &self.search_query)
            .on_input(Msg::SearchChanged)
            .on_submit(Msg::SearchSubmit)
            .width(Length::Fill);

        let header = row![
            search,
            kit_btn::labeled_sm("Compose", kit_btn::primary).on_press(Msg::Compose),
        ]
        .spacing(SPACE_SM)
        .padding(Padding::from([SPACE_MD, SPACE_MD]))
        .align_y(iced::Alignment::Center);

        let status = if self.search_active {
            kit_text::caption(format!("{} results", self.search_total)).style(kit_text::muted)
        } else if self.folder_loading {
            kit_text::caption("Loading…").style(kit_text::muted)
        } else {
            kit_text::caption(format!(
                "{} · {}",
                folder_label(&self.selected_folder),
                self.total_messages
            ))
            .style(kit_text::muted)
        };

        let mut status_row = row![status, Space::new().width(Length::Fill)].spacing(SPACE_SM);
        if self.is_emptyable_folder() && !self.search_active {
            status_row = status_row.push(
                kit_btn::labeled_sm("Empty", kit_btn::ghost).on_press(Msg::EmptyFolder),
            );
        }

        let status_bar = container(status_row)
            .padding(Padding {
                top: 0.0,
                right: SPACE_MD,
                bottom: SPACE_SM,
                left: SPACE_MD,
            })
            .width(Length::Fill);

        let mut list = column![].spacing(0).width(Length::Fill);
        for m in &self.messages {
            let selected = self.selected_uid == Some(m.uid);
            list = list.push(message_row(m, selected));
        }
        if !self.search_active && (self.messages.len() as u32) < self.total_messages {
            let label = if self.is_loading_more {
                "Loading…"
            } else {
                "Load more"
            };
            list = list.push(
                container(
                    kit_btn::labeled_sm(label, kit_btn::ghost)
                        .on_press(Msg::LoadMore)
                        .width(Length::Fill),
                )
                .padding(SPACE_MD)
                .width(Length::Fill),
            );
        }

        container(
            column![
                header,
                status_bar,
                scrollable(list).height(Length::Fill).width(Length::Fill),
            ]
            .height(Length::Fill),
        )
        .width(Length::Fixed(LIST_W))
        .height(Length::Fill)
        .style(list_pane_style)
        .into()
    }

    fn view_message(&self) -> Element<'_, Msg> {
        let Some(body) = &self.message_body else {
            return container(kit_text::caption("Select a message").style(kit_text::muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(read_pane_style)
                .into();
        };

        let mut toolbar = row![
            kit_btn::labeled_sm("Reply", kit_btn::secondary)
                .on_press(Msg::Reply { all: false }),
            kit_btn::labeled_sm("Reply all", kit_btn::ghost).on_press(Msg::Reply { all: true }),
            kit_btn::labeled_sm("Archive", kit_btn::ghost)
                .on_press(Msg::MoveSelected("Archive".into())),
            kit_btn::labeled_sm("Trash", kit_btn::ghost)
                .on_press(Msg::MoveSelected("Trash".into())),
            kit_btn::labeled_sm("Junk", kit_btn::ghost)
                .on_press(Msg::MoveSelected("Junk".into())),
        ]
        .spacing(SPACE_XS);

        if self.last_move.is_some() {
            toolbar = toolbar
                .push(Space::new().width(Length::Fill))
                .push(kit_btn::labeled_sm("Undo", kit_btn::ghost).on_press(Msg::Undo));
        } else {
            toolbar = toolbar.push(Space::new().width(Length::Fill));
        }

        let toolbar = container(toolbar)
            .padding(Padding::from([SPACE_MD, SPACE_LG]))
            .width(Length::Fill)
            .style(toolbar_style);

        let subj = if body.subject.is_empty() {
            "(no subject)".to_string()
        } else {
            body.subject.clone()
        };

        let mut meta = column![
            kit_text::subheading(subj),
            meta_line("From", body.from.clone()),
            meta_line("To", body.to.clone()),
        ]
        .spacing(SPACE_XS)
        .width(Length::Fill);

        let date_s = short_date(&body.date);
        if !body.cc.trim().is_empty() {
            meta = meta.push(meta_line("Cc", body.cc.clone()));
        }
        meta = meta.push(meta_line("Date", date_s));

        let display = body.display_text();
        // HTML hrefs + soft-wrap-aware text scan (line-broken URLs still clickable).
        let urls = crate::protocol::links::links_for_message(body);

        let mut body_col = column![
            kit_text::body(display).width(Length::Fill),
        ]
        .spacing(SPACE_LG)
        .width(Length::Fill);

        if !urls.is_empty() {
            let mut link_col = column![kit_text::caption("Links").style(kit_text::muted)]
                .spacing(SPACE_SM);
            for (i, u) in urls.into_iter().take(12).enumerate() {
                let label = short_url_label(&u, i);
                // Full-width row so long / wrapped targets stay one hit target.
                link_col = link_col.push(
                    kit_btn::labeled_sm(label, kit_btn::ghost)
                        .on_press(Msg::OpenUrl(u))
                        .width(Length::Fill),
                );
            }
            body_col = body_col.push(link_col);
        }

        let article = container(
            column![meta, body_col]
                .spacing(SPACE_LG)
                .width(Length::Fill)
                .padding(Padding::from([SPACE_LG, SPACE_XL])),
        )
        .width(Length::Fill)
        .max_width(READ_MAX_W);

        container(
            column![
                toolbar,
                scrollable(article).height(Length::Fill).width(Length::Fill),
            ]
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(read_pane_style)
        .into()
    }

    fn view_compose(&self) -> Element<'_, Msg> {
        let editor = text_editor(&self.draft.body)
            .placeholder("Message…")
            .height(Length::Fill)
            .padding(12)
            .style(compose_editor_style)
            .on_action(Msg::ComposeBodyAction);

        let form = column![
            kit_text::subheading("Compose"),
            field_label("From"),
            text_input("from@", &self.draft.from).on_input(Msg::ComposeFrom),
            field_label("To"),
            text_input("to@", &self.draft.to).on_input(Msg::ComposeTo),
            field_label("Cc"),
            text_input("cc@", &self.draft.cc).on_input(Msg::ComposeCc),
            field_label("Subject"),
            text_input("Subject", &self.draft.subject).on_input(Msg::ComposeSubject),
            field_label("Body"),
            container(editor)
                .width(Length::Fill)
                .height(Length::Fill)
                .max_height(420.0)
                .style(compose_field_style),
            row![
                kit_btn::labeled("Send", kit_btn::primary).on_press(Msg::Send),
                kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::CancelCompose),
            ]
            .spacing(SPACE_SM),
        ]
        .spacing(SPACE_SM)
        .padding(Padding::from([SPACE_LG, SPACE_XL]))
        .width(Length::Fill)
        .max_width(READ_MAX_W);

        container(scrollable(form).height(Length::Fill))
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

    let unread_dot: Element<'_, Msg> = if m.seen {
        Space::new().width(8).height(8).into()
    } else {
        container(Space::new().width(6).height(6))
            .width(8)
            .height(8)
            .center_x(Length::Fixed(8.0))
            .center_y(Length::Fixed(8.0))
            .style(unread_dot_style)
            .into()
    };

    // Keep sender to one visual line (list column is narrow).
    let from_disp = if from.chars().count() > 28 {
        let mut s: String = from.chars().take(27).collect();
        s.push('…');
        s
    } else {
        from
    };

    let from_text = if m.seen {
        text(from_disp)
            .font(fonts::ui())
            .size(13)
            .style(|t: &Theme| kit_text::muted(t))
    } else {
        text(from_disp).font(fonts::ui_medium()).size(13)
    };

    let subj_text = if m.seen {
        kit_text::caption(subj).style(kit_text::muted)
    } else {
        kit_text::body(subj)
    };

    let top = row![
        unread_dot,
        from_text.width(Length::Fill),
        kit_text::caption(date).style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill);

    let content = column![top, subj_text]
        .spacing(SPACE_XS)
        .width(Length::Fill)
        .padding(Padding::from([SPACE_MD, SPACE_MD]));

    button(content)
        .on_press(Msg::SelectMessage(m.uid))
        .padding(0)
        .width(Length::Fill)
        .style(if selected {
            list_row_selected
        } else {
            list_row_idle
        })
        .into()
}

// ── Display helpers ───────────────────────────────────────────────────

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
        "jan" | "feb" | "mar" | "apr" | "may" | "jun" | "jul" | "aug" | "sep" | "oct" | "nov"
            | "dec"
    )
}

fn short_url_label(url: &str, index: usize) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = stripped.split('/').next().unwrap_or(stripped);
    if host.is_empty() {
        format!("link {}", index + 1)
    } else if host.len() > 28 {
        format!("{}…", &host[..25])
    } else {
        host.to_string()
    }
}

fn meta_line(label: &str, value: String) -> Element<'static, Msg> {
    row![
        kit_text::caption(format!("{label}:"))
            .style(kit_text::muted)
            .width(Length::Fixed(44.0)),
        kit_text::caption(value).style(kit_text::muted),
    ]
    .spacing(SPACE_SM)
    .into()
}

fn field_label(label: &str) -> Element<'_, Msg> {
    kit_text::caption(label.to_string()).style(kit_text::muted).into()
}

fn v_hairline() -> Element<'static, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(hairline_style)
        .into()
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

fn unread_dot_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.primary.base.color)),
        border: Border {
            radius: 99.0.into(),
            ..Default::default()
        },
        ..container::Style::default()
    }
}

fn list_row_idle(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if hover {
            Some(Background::Color(Color {
                a: 0.55,
                ..p.background.strong.color
            }))
        } else {
            None
        },
        text_color: p.background.base.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Default::default(),
        snap: true,
    }
}

fn list_row_selected(theme: &Theme, _status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    button::Style {
        background: Some(Background::Color(theme::selection())),
        text_color: p.background.base.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Default::default(),
        snap: true,
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
        selection: p.primary.weak.color,
    }
}
