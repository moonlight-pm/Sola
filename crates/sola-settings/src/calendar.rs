//! Calendar accounts and shelf — Google, iCloud, CalDAV, ICS/webcal URLs,
//! plus labels / colours / hidden. Persistent `Topic::CalendarConfig`.

use iced::widget::{checkbox, column, container, mouse_area, row, scrollable, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Task, Theme};

use sola_bus::topics::{
    calendar_url_label, google_calendar_client_id, normalize_calendar_url, CalendarAccount,
    CalendarAccountKind, CalendarConfig, CalendarShelf, Topic,
};
use sola_core::Encrypted;
use sola_kit::app::bus;
use sola_kit::components::color_picker;
use sola_kit::components::form::{checkbox_style, form_row};
use sola_kit::components::style::{RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    button as kit_btn, card, field, popover, popover_anchored, text_input as kit_input, ColorPicker,
};
use sola_kit::theme::{color_to_hex, try_parse};

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
    pub detail: Detail,
    pub error: Option<String>,
    pub status: Option<String>,
    pub signing_in: bool,
    pub color_picker: Option<(String, ColorPicker)>,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self {
            last_canonical: CalendarConfig::default(),
            detail: Detail::Closed,
            error: None,
            status: None,
            signing_in: false,
            color_picker: None,
        }
    }
}

impl CalendarState {
    pub fn sync_from_canonical(&mut self, cfg: &CalendarConfig) {
        self.last_canonical = cfg.clone();
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
            Detail::Calendar { id, .. } => {
                if !cfg.calendars.iter().any(|c| c.id == *id) {
                    self.detail = Detail::Closed;
                    self.color_picker = None;
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
    Calendar { id: String, alias: String },
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
    Select(String),
    SelectCal(String),
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
    CalAlias(String),
    SaveCal,
    CalShow(bool),
    ToggleCalColor,
    ColorMsg(color_picker::Message),
    ColorDismiss,
}

pub fn update(msg: CalMsg, cfg: &mut CalendarConfig, ui: &mut CalendarState) -> Task<CalMsg> {
    match msg {
        CalMsg::Select(id) => {
            if ui.signing_in {
                return Task::none();
            }
            ui.color_picker = None;
            if let Some(acc) = cfg.accounts.iter().find(|a| a.id == id) {
                ui.detail = Detail::Edit {
                    id: acc.id.clone(),
                    draft: Draft::from_account(acc),
                };
                ui.error = None;
            }
        }
        CalMsg::SelectCal(id) => {
            if ui.signing_in {
                return Task::none();
            }
            ui.color_picker = None;
            if let Some(cal) = cfg.calendars.iter().find(|c| c.id == id) {
                ui.detail = Detail::Calendar {
                    id: cal.id.clone(),
                    alias: shelf_display(cal).to_string(),
                };
                ui.error = None;
                ui.status = None;
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
            let client_id = google_calendar_client_id();
            cfg.google_client_id = client_id.clone();
            emit_accounts_only(cfg);
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
                    cfg.google_client_id = google_calendar_client_id();
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
                    emit_accounts_only(cfg);
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
                emit_accounts_only(cfg);
            }
        }
        CalMsg::Close => {
            if ui.signing_in {
                return Task::none();
            }
            ui.detail = Detail::Closed;
            ui.color_picker = None;
            ui.error = None;
        }
        CalMsg::CalAlias(v) => {
            if let Detail::Calendar { alias, .. } = &mut ui.detail {
                *alias = v;
            }
            ui.error = None;
        }
        CalMsg::SaveCal => {
            publish_shelf(cfg, ui, "Saved calendar");
        }
        CalMsg::CalShow(show) => {
            if let Detail::Calendar { id, .. } = &ui.detail {
                if let Some(cal) = cfg.calendars.iter_mut().find(|c| c.id == *id) {
                    cal.hidden = !show;
                    cal.visible = show;
                }
            }
            publish_shelf(cfg, ui, if show { "Shown in sidebar" } else { "Hidden from sidebar" });
        }
        CalMsg::ToggleCalColor => {
            let Some(id) = calendar_detail_id(ui) else {
                return Task::none();
            };
            if ui.color_picker.as_ref().is_some_and(|(cid, _)| cid == &id) {
                ui.color_picker = None;
            } else {
                let seed = cfg
                    .calendars
                    .iter()
                    .find(|c| c.id == id)
                    .and_then(|c| try_parse(shelf_color(c)))
                    .unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
                ui.color_picker = Some((id, ColorPicker::new(seed)));
            }
        }
        CalMsg::ColorMsg(m) => {
            if let Some((id, picker)) = &mut ui.color_picker {
                picker.update(m);
                let hex = color_to_hex(picker.color());
                let id = id.clone();
                if let Some(cal) = cfg.calendars.iter_mut().find(|c| c.id == id) {
                    cal.color_override = Some(hex);
                }
            }
            publish_shelf(cfg, ui, "Saved colour");
        }
        CalMsg::ColorDismiss => {
            ui.color_picker = None;
        }
    }
    Task::none()
}

fn start_draft(ui: &mut CalendarState, kind: CalendarAccountKind) {
    if ui.signing_in {
        return;
    }
    ui.color_picker = None;
    ui.detail = Detail::Draft(Draft::of(kind));
    ui.error = None;
    ui.status = None;
}

fn save_draft(cfg: &mut CalendarConfig, ui: &mut CalendarState) -> Task<CalMsg> {
    let draft = match &ui.detail {
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => d.clone(),
        Detail::Closed | Detail::Calendar { .. } => return Task::none(),
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
        Detail::Closed | Detail::Calendar { .. } => {}
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
        Detail::Closed | Detail::Calendar { .. } => {}
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
        Detail::Closed | Detail::Calendar { .. } => {}
    }
    publish(cfg, ui, "Saved CalDAV account");
    Task::none()
}

fn publish(cfg: &mut CalendarConfig, ui: &mut CalendarState, status: &str) {
    cfg.google_client_id = google_calendar_client_id();
    ui.last_canonical = cfg.clone();
    ui.error = None;
    ui.status = Some(status.into());
    emit_accounts_only(cfg);
}

fn publish_shelf(cfg: &mut CalendarConfig, ui: &mut CalendarState, status: &str) {
    write_calendar_draft(cfg, ui);
    cfg.google_client_id = google_calendar_client_id();
    ui.last_canonical = cfg.clone();
    ui.error = None;
    ui.status = Some(status.into());
    emit(Topic::CalendarConfig(cfg.clone()));
}

fn write_calendar_draft(cfg: &mut CalendarConfig, ui: &CalendarState) {
    let Detail::Calendar { id, alias } = &ui.detail else {
        return;
    };
    if let Some(cal) = cfg.calendars.iter_mut().find(|c| c.id == *id) {
        let a = alias.trim();
        cal.alias = if a.is_empty() || a == cal.name {
            None
        } else {
            Some(a.to_string())
        };
    }
}

fn emit_accounts_only(cfg: &mut CalendarConfig) {
    let calendars = std::mem::take(&mut cfg.calendars);
    emit(Topic::CalendarConfig(cfg.clone()));
    cfg.calendars = calendars;
}

fn calendar_detail_id(ui: &CalendarState) -> Option<String> {
    match &ui.detail {
        Detail::Calendar { id, .. } => Some(id.clone()),
        _ => None,
    }
}

fn shelf_display(s: &CalendarShelf) -> &str {
    match s.alias.as_deref().map(str::trim) {
        Some(a) if !a.is_empty() => a,
        _ => s.name.as_str(),
    }
}

fn shelf_color(s: &CalendarShelf) -> &str {
    match s.color_override.as_deref().map(str::trim) {
        Some(c) if !c.is_empty() => c,
        _ => s.color.as_str(),
    }
}

fn open_draft(ui: &mut CalendarState) -> Option<&mut Draft> {
    match &mut ui.detail {
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => Some(d),
        Detail::Closed | Detail::Calendar { .. } => None,
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
        kit_text::subheading("Calendar"),
        kit_text::caption(
            "Accounts connect here. Hidden calendars, labels, and colours live under Calendars."
        )
        .style(kit_text::muted),
    ]
    .spacing(SPACE_SM);

    let split = row![side_list(cfg, ui), detail_pane(cfg, ui)]
        .spacing(SPACE_XL)
        .width(Length::Fill)
        .height(Length::Fill);

    column![header, split]
        .spacing(SPACE_LG)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn side_list<'a>(cfg: &'a CalendarConfig, ui: &'a CalendarState) -> Element<'a, CalMsg> {
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

    rows = rows.push(Space::new().height(SPACE_MD));
    rows = rows.push(kit_text::caption("Calendars").style(kit_text::muted));
    if cfg.calendars.is_empty() {
        rows = rows.push(
            kit_text::caption("Open Calendar once to list them here.")
                .style(kit_text::muted),
        );
    }
    for cal in &cfg.calendars {
        let selected = matches!(&ui.detail, Detail::Calendar { id, .. } if *id == cal.id);
        let fill = try_parse(shelf_color(cal)).unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
        let caption = if cal.hidden {
            "Hidden"
        } else {
            kind_caption(&cal.kind)
        };
        let label = row![
            cal_disc(fill),
            column![
                kit_text::body(shelf_display(cal)),
                kit_text::caption(caption).style(kit_text::muted),
            ]
            .spacing(SPACE_XS)
            .width(Length::Fill),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        rows = rows.push(
            iced::widget::button(label)
                .on_press(CalMsg::SelectCal(cal.id.clone()))
                .padding(ROW_PAD)
                .width(Length::Fill)
                .style(kit_btn::list_item(selected)),
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

fn detail_pane<'a>(cfg: &'a CalendarConfig, ui: &'a CalendarState) -> Element<'a, CalMsg> {
    let body: Element<'a, CalMsg> = match &ui.detail {
        Detail::Closed => container(
            column![
                kit_text::subheading("No selection").style(kit_text::muted),
                kit_text::caption(
                    "Add an account, or pick a calendar to rename, recolour, or show again."
                )
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
        Detail::Calendar { id, alias } => calendar_editor(cfg, ui, id, alias),
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
                    "Sign-in opens the browser. Add another Google account with + Google.",
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

fn calendar_editor<'a>(
    cfg: &'a CalendarConfig,
    ui: &'a CalendarState,
    id: &'a str,
    alias: &'a str,
) -> Element<'a, CalMsg> {
    let Some(cal) = cfg.calendars.iter().find(|c| c.id == id) else {
        return kit_text::caption("That calendar is gone.")
            .style(kit_text::muted)
            .into();
    };
    let fill = try_parse(shelf_color(cal)).unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
    let swatch = color_swatch(fill, CalMsg::ToggleCalColor);
    let swatch: Element<'a, CalMsg> =
        match ui.color_picker.as_ref().filter(|(cid, _)| cid == id) {
            Some((_, picker)) => popover_anchored(
                swatch,
                popover(picker.view().map(CalMsg::ColorMsg)),
                CalMsg::ColorDismiss,
            )
            .into(),
            None => swatch,
        };
    let mut col = column![
        kit_text::subheading(shelf_display(cal)),
        kit_text::caption(format!("Original name · {}", cal.name)).style(kit_text::muted),
    ]
    .spacing(SPACE_LG)
    .padding(SPACE_SM)
    .width(Length::Fill);
    if let Some(err) = ui.error.as_deref() {
        col = col.push(kit_text::caption(err).style(kit_text::danger));
    }
    if let Some(st) = ui.status.as_deref() {
        col = col.push(kit_text::caption(st).style(kit_text::muted));
    }
    col = col.push(field(
        "Label",
        text_input("Calendar name", alias)
            .id(cal_alias_field())
            .on_input(CalMsg::CalAlias)
            .on_submit(CalMsg::SaveCal)
            .size(13)
            .style(kit_input::style),
        Some("Shown in the Calendar sidebar. Leave empty for the original name."),
        None,
    ));
    col = col.push(
        kit_btn::labeled("Save label", kit_btn::primary)
            .on_press(CalMsg::SaveCal)
            .width(Length::Fill),
    );
    col = col.push(form_row("Colour", swatch));
    col = col.push(form_row(
        "Show in sidebar",
        checkbox(!cal.hidden)
            .on_toggle(CalMsg::CalShow)
            .style(checkbox_style),
    ));
    if cal.hidden {
        col = col.push(
            kit_text::caption("Hidden calendars stay off the board until you show them again.")
                .style(kit_text::muted),
        );
    }
    col = col.push(kit_btn::labeled("Close", kit_btn::ghost).on_press(CalMsg::Close));
    col.into()
}

fn cal_disc<'a>(color: Color) -> Element<'a, CalMsg> {
    container(Space::new().width(10).height(10))
        .width(Length::Fixed(10.0))
        .height(Length::Fixed(10.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color,
                width: 1.5,
                radius: 5.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn color_swatch<'a>(color: Color, on_press: CalMsg) -> Element<'a, CalMsg> {
    const SIZE: f32 = 16.0;
    let ink = if (0.299 * color.r + 0.587 * color.g + 0.114 * color.b) > 0.55 {
        Color::from_rgb(0.12, 0.12, 0.14)
    } else {
        Color::WHITE
    };
    let tile = container(Space::new())
        .width(Length::Fixed(SIZE))
        .height(Length::Fixed(SIZE))
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: Color { a: 0.55, ..ink },
                width: 1.0,
                radius: RADIUS_SM.into(),
            },
            ..container::Style::default()
        });
    mouse_area(tile)
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(on_press)
        .into()
}

fn kind_caption(kind: &str) -> &'static str {
    match kind {
        "local" => "On This Computer",
        "google" => "Google",
        "apple" => "iCloud",
        "url" => "URL",
        "caldav" => "CalDAV",
        _ => "Calendar",
    }
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
fn cal_alias_field() -> iced::widget::Id {
    iced::widget::Id::new("settings-cal-alias")
}

pub fn focused_value(ui: &CalendarState, id: &iced::widget::Id) -> Option<String> {
    if id == &cal_alias_field() {
        if let Detail::Calendar { alias, .. } = &ui.detail {
            return Some(alias.clone());
        }
    }
    let draft = match &ui.detail {
        Detail::Draft(d) | Detail::Edit { draft: d, .. } => d,
        Detail::Closed | Detail::Calendar { .. } => return None,
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
    if id == &cal_alias_field() {
        if let Detail::Calendar { alias, .. } = &mut ui.detail {
            *alias = value.to_string();
            return true;
        }
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
