//! Kit UI: three-pane mail client.

use std::sync::Arc;

use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::{column, container, row, scrollable, Space};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use sola_bus::Message;
use sola_bus::topics::{MailConfig, MailRule, OpenUrlRequest, Topic};
use sola_kit::app::{apply_theme_update, bus, bus_subscription, is_self_quit};
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    button as kit_btn, sidebar, SidebarItem, SidebarSection,
};
use sola_kit::theme::default_theme;

use crate::bridge::{self, mail_send};
use crate::protocol::{Folder, MessageBody, MessageSummary};
use crate::worker::{MailCmd, MailEvent};

const APP_ID: &str = "sola-mail";
const PAGE: u32 = 50;

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
    ComposeBody(String),
    Send,
    MoveSelected(String),
    Undo,
    BulkMove(String),
    EmptyFolder,
    OpenUrl(String),
    DismissToast,
    KeyPressed(keyboard::Key, keyboard::Modifiers),
}

#[derive(Debug, Clone)]
struct LastMove {
    uid: u32,
    from_folder: String,
    to_folder: String,
}

#[derive(Debug, Clone)]
struct ComposeDraft {
    from: String,
    to: String,
    cc: String,
    subject: String,
    body: String,
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
    bulk_in_progress: bool,
    toast: Option<String>,
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
            bulk_in_progress: false,
            toast: None,
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
        body: String::new(),
        in_reply_to: None,
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
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            bridge::mail_subscription().map(Msg::Worker),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Msg::KeyPressed(key, modifiers))
                }
                _ => None,
            }),
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
                self.draft = ComposeDraft {
                    from,
                    to,
                    cc,
                    subject: subj,
                    body: format!("\n\nOn {} {} wrote:\n{quoted}", body.date, body.from),
                    in_reply_to: body.message_id.clone(),
                };
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
            Msg::ComposeBody(s) => {
                self.draft.body = s;
                Task::none()
            }
            Msg::Send => {
                if self.draft.to.trim().is_empty() {
                    self.toast = Some("To address is required".into());
                    return Task::none();
                }
                mail_send(MailCmd::Send {
                    from: self.draft.from.clone(),
                    to: self.draft.to.clone(),
                    cc: self.draft.cc.clone(),
                    subject: self.draft.subject.clone(),
                    body: self.draft.body.clone(),
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
                    self.load_folder(self.selected_folder.clone());
                }
                Task::none()
            }
            Msg::BulkMove(dest) => {
                if self.bulk_in_progress {
                    return Task::none();
                }
                self.bulk_in_progress = true;
                let source = if self.selected_folder.starts_with("smart:") {
                    "INBOX".to_string()
                } else {
                    self.selected_folder.clone()
                };
                for m in &self.messages {
                    mail_send(MailCmd::Move {
                        folder: source.clone(),
                        uid: m.uid,
                        dest: dest.clone(),
                    });
                }
                self.load_folder(self.selected_folder.clone());
                self.bulk_in_progress = false;
                Task::none()
            }
            Msg::EmptyFolder => {
                mail_send(MailCmd::EmptyFolder {
                    folder: self.selected_folder.clone(),
                });
                Task::none()
            }
            Msg::OpenUrl(url) => {
                if let Ok(mut b) = bus().lock() {
                    let _ = b.emit(Topic::OpenUrl(OpenUrlRequest {
                        url,
                        activate: true,
                    }));
                }
                Task::none()
            }
            Msg::DismissToast => {
                self.toast = None;
                Task::none()
            }
            Msg::KeyPressed(key, mods) => self.on_key(key, mods),
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
                mail_send(MailCmd::ListFolders);
            }
            MailEvent::Moved { .. } => {}
            MailEvent::Emptied { .. } => {
                self.load_folder(self.selected_folder.clone());
            }
            MailEvent::RulesApplied { moved } => {
                if moved > 0 {
                    self.toast = Some(format!("Moved {moved} messages"));
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
                if context == "connect" {
                    self.connected = false;
                }
            }
            MailEvent::Disconnected { reason } => {
                self.connected = false;
                self.toast = Some(reason);
            }
        }
        Task::none()
    }

    fn on_key(&mut self, key: keyboard::Key, mods: keyboard::Modifiers) -> Task<Msg> {
        if self.composing {
            return Task::none();
        }
        if mods.control() || mods.alt() || mods.logo() {
            return Task::none();
        }
        // Ignore pure navigation keys we don't handle.
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

    pub fn view(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = if self.loading {
            container(kit_text::body("Connecting…"))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
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
            .into()
        } else {
            row![
                self.view_folders(),
                self.view_message_list(),
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
            col = col.push(
                container(
                    row![
                        kit_text::body(toast.clone()),
                        Space::new().width(Length::Fill),
                        kit_btn::labeled_sm("Dismiss", kit_btn::ghost)
                            .on_press(Msg::DismissToast),
                    ]
                    .spacing(SPACE_SM)
                    .align_y(iced::Alignment::Center),
                )
                .padding(SPACE_MD)
                .width(Length::Fill),
            );
        }
        col.into()
    }

    fn view_folders(&self) -> Element<'_, Msg> {
        let mut items: Vec<SidebarItem<'_, Msg>> = Vec::new();
        for f in &self.folders {
            let label = if f.unread > 0 {
                format!("{} ({})", f.name, f.unread)
            } else {
                f.name.clone()
            };
            items.push(
                SidebarItem::new(label, Msg::SelectFolder(f.name.clone()))
                    .active(self.selected_folder == f.name),
            );
        }
        for f in &self.smart_counts {
            let id = format!("smart:{}", f.name);
            let label = if f.unread > 0 {
                format!("★ {} ({})", f.name, f.unread)
            } else {
                format!("★ {}", f.name)
            };
            items.push(
                SidebarItem::new(label, Msg::SelectFolder(id.clone()))
                    .active(self.selected_folder == id),
            );
        }

        container(sidebar(vec![SidebarSection::new("Mailboxes", items).fill()]))
            .width(Length::Fixed(220.0))
            .height(Length::Fill)
            .into()
    }

    fn view_message_list(&self) -> Element<'_, Msg> {
        let search_row = row![
            text_input("Search…", &self.search_query)
                .on_input(Msg::SearchChanged)
                .on_submit(Msg::SearchSubmit)
                .width(Length::Fill),
            kit_btn::labeled_sm("Go", kit_btn::secondary).on_press(Msg::SearchSubmit),
            kit_btn::labeled_sm("Clear", kit_btn::ghost).on_press(Msg::ClearSearch),
        ]
        .spacing(SPACE_XS)
        .padding(SPACE_SM);

        let actions = row![
            kit_btn::labeled_sm("Compose", kit_btn::primary).on_press(Msg::Compose),
            kit_btn::labeled_sm("Archive all", kit_btn::ghost)
                .on_press(Msg::BulkMove("Archive".into())),
            kit_btn::labeled_sm("Trash all", kit_btn::ghost)
                .on_press(Msg::BulkMove("Trash".into())),
            kit_btn::labeled_sm("Empty", kit_btn::ghost).on_press(Msg::EmptyFolder),
        ]
        .spacing(SPACE_XS)
        .padding(SPACE_SM);

        let header = if self.search_active {
            kit_text::caption(format!("{} results", self.search_total)).style(kit_text::muted)
        } else if self.folder_loading {
            kit_text::caption("Loading…").style(kit_text::muted)
        } else {
            kit_text::caption(format!(
                "{} · {} messages",
                self.selected_folder, self.total_messages
            ))
            .style(kit_text::muted)
        };

        let mut list = column![header].spacing(SPACE_XS).padding(SPACE_SM);
        for m in &self.messages {
            let selected = self.selected_uid == Some(m.uid);
            let subj = if m.subject.is_empty() {
                "(no subject)".into()
            } else {
                m.subject.clone()
            };
            let line = if m.seen {
                format!("{} — {}", m.from, subj)
            } else {
                format!("● {} — {}", m.from, subj)
            };
            let style = if selected {
                kit_btn::secondary
            } else {
                kit_btn::ghost
            };
            list = list.push(
                kit_btn::labeled_sm(line, style)
                    .on_press(Msg::SelectMessage(m.uid))
                    .width(Length::Fill),
            );
        }
        if !self.search_active && (self.messages.len() as u32) < self.total_messages {
            let label = if self.is_loading_more {
                "Loading…"
            } else {
                "Load more"
            };
            list = list.push(kit_btn::labeled_sm(label, kit_btn::ghost).on_press(Msg::LoadMore));
        }

        container(
            column![search_row, actions, scrollable(list).height(Length::Fill)]
                .height(Length::Fill),
        )
        .width(Length::Fixed(340.0))
        .height(Length::Fill)
        .into()
    }

    fn view_message(&self) -> Element<'_, Msg> {
        let Some(body) = &self.message_body else {
            return container(kit_text::caption("Select a message").style(kit_text::muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        };

        let toolbar = row![
            kit_btn::labeled_sm("Reply", kit_btn::secondary)
                .on_press(Msg::Reply { all: false }),
            kit_btn::labeled_sm("Reply all", kit_btn::ghost).on_press(Msg::Reply { all: true }),
            kit_btn::labeled_sm("Archive", kit_btn::ghost)
                .on_press(Msg::MoveSelected("Archive".into())),
            kit_btn::labeled_sm("Trash", kit_btn::ghost)
                .on_press(Msg::MoveSelected("Trash".into())),
            kit_btn::labeled_sm("Junk", kit_btn::ghost)
                .on_press(Msg::MoveSelected("Junk".into())),
            kit_btn::labeled_sm("Undo", kit_btn::ghost).on_press(Msg::Undo),
        ]
        .spacing(SPACE_XS)
        .padding(SPACE_SM);

        let subj = if body.subject.is_empty() {
            "(no subject)".to_string()
        } else {
            body.subject.clone()
        };
        let meta = column![
            kit_text::subheading(subj),
            kit_text::caption(format!("From: {}", body.from)).style(kit_text::muted),
            kit_text::caption(format!("To: {}", body.to)).style(kit_text::muted),
            kit_text::caption(if body.cc.is_empty() {
                String::new()
            } else {
                format!("Cc: {}", body.cc)
            })
            .style(kit_text::muted),
            kit_text::caption(format!("Date: {}", body.date)).style(kit_text::muted),
        ]
        .spacing(SPACE_XS)
        .padding(SPACE_SM);

        let display = body.display_text();
        let urls = extract_urls(&display);
        let mut url_row = row![].spacing(SPACE_XS);
        for u in urls.into_iter().take(8) {
            let label = if u.len() > 48 {
                format!("{}…", &u[..45])
            } else {
                u.clone()
            };
            url_row = url_row.push(
                kit_btn::labeled_sm(label, kit_btn::ghost).on_press(Msg::OpenUrl(u)),
            );
        }

        let body_view = scrollable(
            column![kit_text::body(display), url_row]
                .spacing(SPACE_MD)
                .padding(SPACE_MD),
        )
        .height(Length::Fill);

        container(column![toolbar, meta, body_view].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_compose(&self) -> Element<'_, Msg> {
        let form = column![
            kit_text::subheading("Compose"),
            kit_text::caption("From").style(kit_text::muted),
            text_input("from@", &self.draft.from).on_input(Msg::ComposeFrom),
            kit_text::caption("To").style(kit_text::muted),
            text_input("to@", &self.draft.to).on_input(Msg::ComposeTo),
            kit_text::caption("Cc").style(kit_text::muted),
            text_input("cc@", &self.draft.cc).on_input(Msg::ComposeCc),
            kit_text::caption("Subject").style(kit_text::muted),
            text_input("Subject", &self.draft.subject).on_input(Msg::ComposeSubject),
            kit_text::caption("Body").style(kit_text::muted),
            text_input("Message…", &self.draft.body)
                .on_input(Msg::ComposeBody)
                .width(Length::Fill),
            row![
                kit_btn::labeled("Send", kit_btn::primary).on_press(Msg::Send),
                kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::CancelCompose),
            ]
            .spacing(SPACE_SM),
        ]
        .spacing(SPACE_SM)
        .padding(SPACE_MD);

        container(scrollable(form))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn extract_urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| {
            matches!(c, ')' | ']' | '>' | '"' | '\'' | ',' | '.' | ';' | ':')
        });
        if (t.starts_with("http://") || t.starts_with("https://")) && !out.iter().any(|u| u == t)
        {
            out.push(t.to_string());
        }
    }
    out
}
