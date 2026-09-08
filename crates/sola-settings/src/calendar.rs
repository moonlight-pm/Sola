//! Calendar accounts — Google, iCloud, CalDAV, and ICS/webcal URLs.
//! Persistent `Topic::CalendarConfig`; sola-calendar consumes it.

use iced::widget::{column, container, row, scrollable, Space};
use iced::{Alignment, Element, Length, Padding, Task};

use sola_bus::topics::{
    calendar_url_label, normalize_calendar_url, CalendarAccount, CalendarAccountKind,
    CalendarConfig, Topic,
};
use sola_core::Encrypted;
use sola_kit::app::bus;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{button as kit_btn, card, field, text_input as kit_input};

use crate::calendar_oauth;

const LIST_WIDTH: f32 = 280.0;
const ROW_PAD: Padding = Padding {
    top: 8.0,
    bottom: 8.0,
    left: 12.0,
    right: 8.0,
};

#[derive(Debug, Clone)]
pub struct CalendarState {
    pub last_canonical: CalendarConfig,
    pub client_id: String,
    pub detail: Detail,
    pub error: Option<String>,
    pub status: Option<String>,
    pub signing_in: bool,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self {
            last_canonical: CalendarConfig::default(),
            client_id: std::env::var("SOLA_GOOGLE_CALENDAR_CLIENT_ID").unwrap_or_default(),
            detail: Detail::Closed,
            error: None,
            status: None,
            signing_in: false,
        }
    }
}

impl CalendarState {
    pub fn sync_from_canonical(&mut self, cfg: &CalendarConfig) {
        self.last_canonical = cfg.clone();
        if self.client_id.trim().is_empty() {
            self.client_id = cfg.google_client_id.clone();
        }
        match &self.detail {
            Detail::Edit { id, .. } => {
                if let Some(acc) = cfg.accounts.iter().find(|a| a.id == *id) {
                    self.detail = Detail::Edit {
                        id: acc.id.clone(),
                        draft: Draft::from_account(acc),
                    };
                } else {
                    self.detail = Detail::Closed;
                }
            }
            Detail::Draft(_) | Detail::Closed => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum Detail {
    Closed,
    Draft(Draft),
    Edit { id: String, draft: Draft },
}

#[derive(Debug, Clone)]
pub struct Draft {
    pub kind: CalendarAccountKind,
    pub label: String,
    pub apple_id: String,
    pub app_password: String,
    pub url: String,
    pub username: String,
}

impl Draft {
    fn of(kind: CalendarAccountKind) -> Self {
        Self {
            kind,
            label: String::new(),
            apple_id: String::new(),
            app_password: String::new(),
            url: String::new(),
            username: String::new(),
        }
    }
    fn from_account(acc: &CalendarAccount) -> Self {
        Self {
            kind: acc.kind,
            label: acc.label.clone(),
            apple_id: acc.apple_id.clone(),
            app_password: acc
                .app_password
                .as_ref()
                .map(|e| e.0.clone())
                .unwrap_or_default(),
            url: acc.url.clone(),
            username: acc.username.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CalMsg {
    ClientId(String),
    SaveClientId,
    Select(String),
    AddGoogle,
    AddApple,
    AddUrl,
    AddCaldav,
    Label(String),
    AppleId(String),
    ApplePassword(String),
    Url(String),
    Username(String),
    Save,
    SignInGoogle,
    GoogleDone(Result<(String, String, u64, Option<String>), String>),
    Remove,
    Close,
}

pub fn update(msg: CalMsg, cfg: &mut CalendarConfig, ui: &mut CalendarState) -> Task<CalMsg> {
    match msg {
        CalMsg::ClientId(v) => {
            ui.client_id = v;
            ui.error = None;
        }
        CalMsg::SaveClientId => {
            cfg.google_client_id = ui.client_id.trim().to_string();
            ui.last_canonical = cfg.clone();
            emit(Topic::CalendarConfig(cfg.clone()));
            ui.status = Some("Saved client ID".into());
        }
        CalMsg::Select(id) => {
            if ui.signing_in {
                return Task::none();
            }
            if let Some(acc) = cfg.accounts.iter().find(|a| a.id == id) {
                ui.detail = Detail::Edit {
                    id: acc.id.clone(),
                    draft: Draft::from_account(acc),
                };
                ui.error = None;
            }
        }
        CalMsg::AddGoogle => start_draft(ui, CalendarAccountKind::Google),
        CalMsg::AddApple => start_draft(ui, CalendarAccountKind::Apple),
        CalMsg::AddUrl => start_draft(ui, CalendarAccountKind::Url),
        CalMsg::AddCaldav => start_draft(ui, CalendarAccountKind::Caldav),
        CalMsg::Label(v) => {
            if let Some(d) = open_draft(ui) {
                d.label = v;
            }
            ui.error = None;
        }
        CalMsg::AppleId(v) => {
            if let Some(d) = open_draft(ui) {
                d.apple_id = v;
            }
            ui.error = None;
        }
        CalMsg::ApplePassword(v) => {
            if let Some(d) = open_draft(ui) {
                d.app_password = v;
            }
            ui.error = None;
        }
        CalMsg::Url(v) => {
            if let Some(d) = open_draft(ui) {
                d.url = v;
            }
            ui.error = None;
        }
        CalMsg::Username(v) => {
            if let Some(d) = open_draft(ui) {
                d.username = v;
            }
            ui.error = None;
        }
        CalMsg::Save => return save_draft(cfg, ui),
        CalMsg::SignInGoogle => {
            let client_id = ui.client_id.trim().to_string();
            if client_id.is_empty() {
                ui.error = Some("Paste a Google OAuth Desktop client ID first".into());
                return Task::none();
            }
            cfg.google_client_id = client_id.clone();
            emit(Topic::CalendarConfig(cfg.clone()));
            ui.signing_in = true;
            ui.error = None;
            ui.status = Some("Waiting for Google sign-in…".into());
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || calendar_oauth::sign_in(client_id))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.map_err(|e| e.to_string()))
                },
                |result| match result {
                    Ok((tok, email)) => CalMsg::GoogleDone(Ok((
                        tok.access_token,
                        email,
                        calendar_oauth::expiry_unix(tok.expires_in),
                        tok.refresh_token,
                    ))),
                    Err(e) => CalMsg::GoogleDone(Err(e)),
                },
            );
        }
        CalMsg::GoogleDone(result) => {
            ui.signing_in = false;
            match result {
                Ok((access, email, expiry, refresh)) => {
                    let refresh = refresh.filter(|s| !s.is_empty()).map(Encrypted);
                    let access = Encrypted(access);
                    cfg.google_client_id = ui.client_id.trim().to_string();
                    match &ui.detail {
                        Detail::Edit { id, .. } => {
                            if let Some(acc) = cfg.accounts.iter_mut().find(|a| a.id == *id) {
                                acc.kind = CalendarAccountKind::Google;
                                acc.label = email.clone();
                                acc.email = email.clone();
                                acc.access_token = Some(access);
                                if refresh.is_some() {
                                    acc.refresh_token = refresh;
                                }
                                acc.expiry_unix = expiry;
                            }
                        }
                        _ => {
                            let acc = CalendarAccount {
                                id: new_id("gacc"),
                                kind: CalendarAccountKind::Google,
                                label: email.clone(),
                                email: email.clone(),
                                access_token: Some(access),
                                refresh_token: refresh,
                                expiry_unix: expiry,
                                ..CalendarAccount::default()
                            };
                            let id = acc.id.clone();
                            cfg.accounts.push(acc);
                            ui.detail = Detail::Edit {
                                id,
                                draft: Draft::of(CalendarAccountKind::Google),
                            };
                        }
                    }
                    if let Detail::Edit { id, .. } = &ui.detail {
                        if let Some(acc) = cfg.accounts.iter().find(|a| a.id == *id) {
                            ui.detail = Detail::Edit {
                                id: id.clone(),
                                draft: Draft::from_account(acc),
                            };
                        }
                    }
                    ui.last_canonical = cfg.clone();
                    ui.status = Some(format!("Connected {email}"));
                    ui.error = None;
                    emit(Topic::CalendarConfig(cfg.clone()));
                }
                Err(e) => {
                    ui.error = Some(e);
                    ui.status = None;
                }
            }
        }
        CalMsg::Remove => {
            if let Detail::Edit { id, .. } = &ui.detail {
                cfg.accounts.retain(|a| a.id != *id);
                ui.detail = Detail::Closed;
                ui.last_canonical = cfg.clone();
                ui.status = Some("Disconnected".into());
                emit(Topic::CalendarConfig(cfg.clone()));
            }
        }
        CalMsg::Close => {
            if ui.signing_in {
                return Task::none();
            }
            ui.detail = Detail::Closed;
            ui.error = None;
        }
    }
    Task::none()
}

fn start_draft(ui: &mut CalendarState, kind: CalendarAccountKind) {
    if ui.signing_in {
        return;
    }
    ui.detail = Detail::Draft(Draft::of(kind));
    ui.error = None;
    ui.status = None;
}

fn save_draft(cfg: &mut CalendarConfig, ui: &mut CalendarState) -> Task<CalMsg> {
    let draft = match &ui.detail {
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => d.clone(),
        Detail::Closed => return Task::none(),
    };
    match draft.kind {
        CalendarAccountKind::Google => Task::none(),
        CalendarAccountKind::Apple => save_apple(cfg, ui, draft),
        CalendarAccountKind::Url => save_url(cfg, ui, draft),
        CalendarAccountKind::Caldav => save_caldav(cfg, ui, draft),
    }
}

fn save_apple(cfg: &mut CalendarConfig, ui: &mut CalendarState, draft: Draft) -> Task<CalMsg> {
    let apple_id = draft.apple_id.trim().to_string();
    let password: String = draft
        .app_password
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if apple_id.is_empty() {
        ui.error = Some("Apple ID is required".into());
        return Task::none();
    }
    let keep_secret = matches!(ui.detail, Detail::Edit { .. }) && password.is_empty();
    if password.is_empty() && !keep_secret {
        ui.error = Some("Apple ID and an app-specific password are required".into());
        return Task::none();
    }
    let label = if draft.label.trim().is_empty() {
        apple_id.clone()
    } else {
        draft.label.trim().to_string()
    };
    match &ui.detail {
        Detail::Draft(_) => {
            let acc = CalendarAccount {
                id: new_id("aacc"),
                kind: CalendarAccountKind::Apple,
                label,
                email: apple_id.clone(),
                apple_id,
                app_password: Some(Encrypted(password)),
                ..CalendarAccount::default()
            };
            let id = acc.id.clone();
            cfg.accounts.push(acc);
            ui.detail = Detail::Edit { id, draft };
        }
        Detail::Edit { id, .. } => {
            if let Some(acc) = cfg.accounts.iter_mut().find(|a| a.id == *id) {
                acc.label = label;
                acc.email = apple_id.clone();
                acc.apple_id = apple_id;
                if !password.is_empty() {
                    acc.app_password = Some(Encrypted(password));
                }
            }
        }
        Detail::Closed => {}
    }
    publish(cfg, ui, "Saved iCloud account");
    Task::none()
}

fn save_url(cfg: &mut CalendarConfig, ui: &mut CalendarState, mut draft: Draft) -> Task<CalMsg> {
    let url = normalize_calendar_url(&draft.url);
    if url.is_empty() {
        ui.error = Some("Paste an https or webcal URL".into());
        return Task::none();
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        ui.error = Some("URL must be http, https, or webcal".into());
        return Task::none();
    }
    draft.url = url.clone();
    let label = if draft.label.trim().is_empty() {
        calendar_url_label(&url)
    } else {
        draft.label.trim().to_string()
    };
    match &ui.detail {
        Detail::Draft(_) => {
            let acc = CalendarAccount {
                id: new_id("uacc"),
                kind: CalendarAccountKind::Url,
                label,
                url,
                ..CalendarAccount::default()
            };
            let id = acc.id.clone();
            cfg.accounts.push(acc);
            ui.detail = Detail::Edit { id, draft };
        }
        Detail::Edit { id, .. } => {
            if let Some(acc) = cfg.accounts.iter_mut().find(|a| a.id == *id) {
                acc.label = label;
                acc.url = url;
            }
        }
        Detail::Closed => {}
    }
    publish(cfg, ui, "Saved URL calendar");
    Task::none()
}

fn save_caldav(cfg: &mut CalendarConfig, ui: &mut CalendarState, mut draft: Draft) -> Task<CalMsg> {
    let url = normalize_calendar_url(&draft.url);
    let username = draft.username.trim().to_string();
    let password: String = draft
        .app_password
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if url.is_empty() || username.is_empty() {
        ui.error = Some("Server URL and username are required".into());
        return Task::none();
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        ui.error = Some("Server URL must be http or https".into());
        return Task::none();
    }
    let keep_secret = matches!(ui.detail, Detail::Edit { .. }) && password.is_empty();
    if password.is_empty() && !keep_secret {
        ui.error = Some("Password is required".into());
        return Task::none();
    }
    draft.url = url.clone();
    draft.username = username.clone();
    let label = if draft.label.trim().is_empty() {
        username.clone()
    } else {
        draft.label.trim().to_string()
    };
    match &ui.detail {
        Detail::Draft(_) => {
            let acc = CalendarAccount {
                id: new_id("cacc"),
                kind: CalendarAccountKind::Caldav,
                label,
                email: username.clone(),
                username,
                url,
                app_password: Some(Encrypted(password)),
                ..CalendarAccount::default()
            };
            let id = acc.id.clone();
            cfg.accounts.push(acc);
            ui.detail = Detail::Edit { id, draft };
        }
        Detail::Edit { id, .. } => {
            if let Some(acc) = cfg.accounts.iter_mut().find(|a| a.id == *id) {
                acc.label = label;
                acc.email = username.clone();
                acc.username = username;
                acc.url = url;
                if !password.is_empty() {
                    acc.app_password = Some(Encrypted(password));
                }
            }
        }
        Detail::Closed => {}
    }
    publish(cfg, ui, "Saved CalDAV account");
    Task::none()
}

fn publish(cfg: &mut CalendarConfig, ui: &mut CalendarState, status: &str) {
    cfg.google_client_id = ui.client_id.trim().to_string();
    ui.last_canonical = cfg.clone();
    ui.error = None;
    ui.status = Some(status.into());
    emit(Topic::CalendarConfig(cfg.clone()));
}

fn open_draft(ui: &mut CalendarState) -> Option<&mut Draft> {
    match &mut ui.detail {
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => Some(d),
        Detail::Closed => None,
    }
}

fn new_id(prefix: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{n:x}")
}

fn emit(topic: Topic) {
    if let Err(e) = bus().lock().unwrap().emit(topic) {
        tracing::warn!("bus emit failed: {e}");
    }
}

pub fn view<'a>(cfg: &'a CalendarConfig, ui: &'a CalendarState) -> Element<'a, CalMsg> {
    let header = column![
        kit_text::subheading("Accounts"),
        kit_text::caption(
            "Any mix of Google, iCloud, CalDAV, and ICS/webcal URLs. Local events do not need an account."
        )
        .style(kit_text::muted),
        field(
            "Google OAuth client ID",
            text_input("….apps.googleusercontent.com", &ui.client_id)
                .id(client_id_field())
                .on_input(CalMsg::ClientId)
                .size(13)
                .style(kit_input::style),
            Some("Desktop client. Redirect http://127.0.0.1:8765/oauth"),
            None,
        ),
        kit_btn::labeled_sm("Save client ID", kit_btn::secondary).on_press(CalMsg::SaveClientId),
    ]
    .spacing(SPACE_SM);

    let split = row![accounts_list(cfg, ui), accounts_detail(cfg, ui)]
        .spacing(SPACE_XL)
        .width(Length::Fill)
        .height(Length::Fill);

    column![header, split]
        .spacing(SPACE_LG)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn accounts_list<'a>(cfg: &'a CalendarConfig, ui: &'a CalendarState) -> Element<'a, CalMsg> {
    let toolbar = column![
        row![
            kit_btn::labeled_sm("+ Google", kit_btn::ghost).on_press(CalMsg::AddGoogle),
            kit_btn::labeled_sm("+ iCloud", kit_btn::ghost).on_press(CalMsg::AddApple),
        ]
        .spacing(SPACE_SM),
        row![
            kit_btn::labeled_sm("+ URL", kit_btn::ghost).on_press(CalMsg::AddUrl),
            kit_btn::labeled_sm("+ CalDAV", kit_btn::ghost).on_press(CalMsg::AddCaldav),
            Space::new().width(Length::Fill),
            kit_text::caption(format!(
                "{} {}",
                cfg.accounts.len(),
                if cfg.accounts.len() == 1 {
                    "account"
                } else {
                    "accounts"
                }
            ))
            .style(kit_text::muted),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .width(Length::Fill),
    ]
    .spacing(SPACE_SM)
    .width(Length::Fill);

    let mut rows = column![].spacing(0).width(Length::Fill);
    for acc in &cfg.accounts {
        let selected = matches!(&ui.detail, Detail::Edit { id, .. } if *id == acc.id);
        let label = column![
            kit_text::body(acc.label.clone()),
            kit_text::caption(acc.kind.label()).style(kit_text::muted),
        ]
        .spacing(SPACE_XS);
        rows = rows.push(
            iced::widget::button(label)
                .on_press(CalMsg::Select(acc.id.clone()))
                .padding(ROW_PAD)
                .width(Length::Fill)
                .style(kit_btn::list_item(selected)),
        );
    }
    if let Detail::Draft(d) = &ui.detail {
        let label = column![
            kit_text::body(d.kind.label()),
            kit_text::caption("New account").style(kit_text::muted),
        ]
        .spacing(SPACE_XS);
        rows = rows.push(
            iced::widget::button(label)
                .padding(ROW_PAD)
                .width(Length::Fill)
                .style(kit_btn::list_item(true)),
        );
    }

    container(
        card(
            column![
                toolbar,
                scrollable(rows).height(Length::Fill).width(Length::Fill)
            ]
            .spacing(SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fixed(LIST_WIDTH))
    .height(Length::Fill)
    .into()
}

fn accounts_detail<'a>(cfg: &'a CalendarConfig, ui: &'a CalendarState) -> Element<'a, CalMsg> {
    let body: Element<'a, CalMsg> = match &ui.detail {
        Detail::Closed => container(
            column![
                kit_text::subheading("No selection").style(kit_text::muted),
                kit_text::caption("Add Google, iCloud, a URL, or CalDAV, or select an account.")
                    .style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => editor(cfg, ui, d),
    };
    container(card(body).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn editor<'a>(cfg: &'a CalendarConfig, ui: &'a CalendarState, d: &'a Draft) -> Element<'a, CalMsg> {
    let mut col = column![kit_text::subheading(d.kind.label())]
        .spacing(SPACE_LG)
        .padding(SPACE_SM)
        .width(Length::Fill);
    if let Some(err) = ui.error.as_deref() {
        col = col.push(kit_text::caption(err).style(kit_text::danger));
    }
    if let Some(st) = ui.status.as_deref() {
        col = col.push(kit_text::caption(st).style(kit_text::muted));
    }
    match d.kind {
        CalendarAccountKind::Google => {
            col = col.push(
                kit_text::caption(
                    "Sign-in opens the browser. Enable Calendar API on the Desktop client. Add another Google account with + Google.",
                )
                .style(kit_text::muted),
            );
            if matches!(ui.detail, Detail::Edit { .. }) && !d.label.is_empty() {
                col = col.push(kit_text::body(d.label.clone()));
            }
            let sign_in = if ui.signing_in {
                kit_btn::labeled("Waiting for Google…", kit_btn::primary)
            } else {
                let label = if matches!(ui.detail, Detail::Edit { .. }) {
                    "Sign in again"
                } else {
                    "Sign in with Google"
                };
                kit_btn::labeled(label, kit_btn::primary).on_press(CalMsg::SignInGoogle)
            };
            col = col.push(sign_in.width(Length::Fill));
        }
        CalendarAccountKind::Apple => {
            col = col.push(
                kit_text::caption(
                    "Use an app-specific password from appleid.apple.com (Sign-In and Security).",
                )
                .style(kit_text::muted),
            );
            col = col.push(name_field(d));
            col = col.push(field(
                "Apple ID",
                text_input("you@icloud.com", &d.apple_id)
                    .id(apple_id_field())
                    .on_input(CalMsg::AppleId)
                    .size(13)
                    .style(kit_input::style),
                None,
                None,
            ));
            col = col.push(field(
                "App-specific password",
                text_input("xxxx-xxxx-xxxx-xxxx", &d.app_password)
                    .id(apple_password_field())
                    .secure(true)
                    .on_input(CalMsg::ApplePassword)
                    .size(13)
                    .style(kit_input::style),
                None,
                None,
            ));
            col = col.push(
                kit_btn::labeled("Save", kit_btn::primary)
                    .on_press(CalMsg::Save)
                    .width(Length::Fill),
            );
        }
        CalendarAccountKind::Url => {
            col = col.push(
                kit_text::caption(
                    "Subscribe to an ICS or iCal feed. webcal:// becomes https. Read-only.",
                )
                .style(kit_text::muted),
            );
            col = col.push(name_field(d));
            col = col.push(field(
                "URL",
                text_input("https://example.com/calendar.ics", &d.url)
                    .id(url_field())
                    .on_input(CalMsg::Url)
                    .size(13)
                    .style(kit_input::style),
                Some("https, http, or webcal"),
                None,
            ));
            col = col.push(
                kit_btn::labeled("Save", kit_btn::primary)
                    .on_press(CalMsg::Save)
                    .width(Length::Fill),
            );
        }
        CalendarAccountKind::Caldav => {
            col = col.push(
                kit_text::caption(
                    "Fastmail, Nextcloud, or any CalDAV host. Username and password for that server.",
                )
                .style(kit_text::muted),
            );
            col = col.push(name_field(d));
            col = col.push(field(
                "Server URL",
                text_input("https://caldav.example.com/", &d.url)
                    .id(url_field())
                    .on_input(CalMsg::Url)
                    .size(13)
                    .style(kit_input::style),
                None,
                None,
            ));
            col = col.push(field(
                "Username",
                text_input("you@example.com", &d.username)
                    .id(username_field())
                    .on_input(CalMsg::Username)
                    .size(13)
                    .style(kit_input::style),
                None,
                None,
            ));
            col = col.push(field(
                "Password",
                text_input("app password", &d.app_password)
                    .id(apple_password_field())
                    .secure(true)
                    .on_input(CalMsg::ApplePassword)
                    .size(13)
                    .style(kit_input::style),
                None,
                None,
            ));
            col = col.push(
                kit_btn::labeled("Save", kit_btn::primary)
                    .on_press(CalMsg::Save)
                    .width(Length::Fill),
            );
        }
    }
    if matches!(ui.detail, Detail::Edit { .. }) {
        col = col.push(kit_btn::labeled("Disconnect", kit_btn::danger).on_press(CalMsg::Remove));
    }
    col = col.push(kit_btn::labeled("Close", kit_btn::ghost).on_press(CalMsg::Close));
    let _ = cfg;
    col.into()
}

fn name_field(d: &Draft) -> Element<'_, CalMsg> {
    field(
        "Name",
        text_input("Optional", &d.label)
            .id(label_field())
            .on_input(CalMsg::Label)
            .size(13)
            .style(kit_input::style),
        None,
        None,
    )
    .into()
}

fn client_id_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-client-id")
}
fn apple_id_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-apple-id")
}
fn apple_password_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-apple-password")
}
fn url_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-url")
}
fn username_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-username")
}
fn label_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-label")
}

pub fn focused_value(ui: &CalendarState, id: &iced::widget::Id) -> Option<String> {
    if id == &client_id_field() {
        return Some(ui.client_id.clone());
    }
    let draft = match &ui.detail {
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => d,
        Detail::Closed => return None,
    };
    if id == &apple_id_field() {
        return Some(draft.apple_id.clone());
    }
    if id == &apple_password_field() {
        return Some(draft.app_password.clone());
    }
    if id == &url_field() {
        return Some(draft.url.clone());
    }
    if id == &username_field() {
        return Some(draft.username.clone());
    }
    if id == &label_field() {
        return Some(draft.label.clone());
    }
    None
}

pub fn set_focused_value(ui: &mut CalendarState, id: &iced::widget::Id, value: &str) -> bool {
    if id == &client_id_field() {
        ui.client_id = value.to_string();
        return true;
    }
    let Some(d) = open_draft(ui) else {
        return false;
    };
    if id == &apple_id_field() {
        d.apple_id = value.to_string();
        return true;
    }
    if id == &apple_password_field() {
        d.app_password = value.to_string();
        return true;
    }
    if id == &url_field() {
        d.url = value.to_string();
        return true;
    }
    if id == &username_field() {
        d.username = value.to_string();
        return true;
    }
    if id == &label_field() {
        d.label = value.to_string();
        return true;
    }
    false
}
