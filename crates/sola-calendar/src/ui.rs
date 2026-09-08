//! Kit UI: calendars rail, month/week/day board, inspector.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::mouse;
use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, text, toggler,
};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Subscription, Task,
    Theme,
};
use sola_bus::Message;
use sola_bus::topics::{
    google_calendar_client_id, CalendarConfig, CalendarShelf, SplitDir, Topic,
};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::style::{
    HAIRLINE_A, ON_FILL_DARK, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, mix_white,
};
use sola_kit::components::color_picker;
use sola_kit::components::icon::{icon_handle, icon_svg, icon_svg_colored};
use sola_kit::components::prose::prose;
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input as kit_input;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    ColorPicker, DividerColors, SidebarItem, SidebarPanel, SidebarSection, button as kit_btn,
    popover, popover_anchored, split_with, toggle_style,
};
use sola_kit::fonts;
use sola_kit::theme::{color_to_hex, default_theme};

use crate::bridge;
use crate::model::{CalEvent, Calendar, Settings, Store, View, format_hm, parse_hex};
use crate::timeutil::{
    WEEKDAYS, day_title, first_of_month, month_span, month_title, month_weeks_filled, next_month,
    next_week, parse_pretty_date, pretty_date, prev_month, prev_week, today, week_of, week_title,
};
use crate::worker::{CalCmd, CalNotice};

const APP_ID: &str = "sola-calendar";
const SIDEBAR_W_MIN: f32 = 180.0;
const SIDEBAR_W_MAX: f32 = 420.0;
const INSPECTOR_W_MIN: f32 = 260.0;
const INSPECTOR_W_MAX: f32 = 520.0;
const CHROME_H: f32 = 44.0;
const TOAST_TTL: Duration = Duration::from_secs(4);
const QUICKVIEW_CAP: usize = 6;
const DRAFT_LABEL_W: f32 = 64.0;

pub struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    store: Store,
    settings: Settings,
    cursor: NaiveDate,
    pane: Pane,
    status: String,
    toast: Option<String>,
    toast_gen: u64,
    cal_picker_open: bool,
    got_config: bool,
    renaming: Option<(String, String)>,
    color_picker: Option<(String, ColorPicker)>,
    dragging_sidebar: bool,
    dragging_inspector: bool,
    window_w: f32,
    /// Last calendar-shelf fingerprint we published. Stops Snapshot from
    /// re-emitting (and re-applying) the same `CalendarConfig`.
    last_shelf_fp: Option<String>,
}

#[derive(Debug, Clone)]
enum Pane {
    Day(NaiveDate),
    Draft(Draft),
}

#[derive(Debug, Clone)]
struct Draft {
    original_id: Option<String>,
    calendar_id: String,
    title: String,
    notes: String,
    location: String,
    all_day: bool,
    date: String,
    start_time: String,
    end_time: String,
    read_only: bool,
    repeating: bool,
    remote_id: Option<String>,
    href: Option<String>,
    etag: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Worker(CalNotice),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    Prev,
    Next,
    Today,
    SetView(View),
    SelectDay(NaiveDate),
    OpenEvent(String),
    NewEvent,
    NewOn(NaiveDate),
    ToggleCal(String),
    DefaultCal(String),
    DraftTitle(String),
    DraftNotes(String),
    DraftLocation(String),
    DraftDate(String),
    DraftStart(String),
    DraftEnd(String),
    DraftAllDay(bool),
    DraftCalendar(String),
    ToggleCalPicker,
    DismissCalPicker,
    SaveDraft,
    DeleteDraft,
    ClosePane,
    OpenUrl(String),
    Refresh,
    DismissToast { generation: u64 },
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    Tick,
    MaybeMigrate,
    SidebarPress,
    InspectorPress,
    SidebarRelease,
    CursorMoved(f32),
    WindowResized(f32),
    RenameCal(String),
    RenameSelectAll,
    RenameInput(String),
    RenameCommit,
    HideCal(String),
    ToggleColor(String),
    ColorMsg(color_picker::Message),
    ColorDismiss,
}

impl Default for App {
    fn default() -> Self {
        let today = today();
        Self {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            store: Store {
                calendars: vec![Calendar::local()],
                events: Vec::new(),
                accounts: Vec::new(),
            },
            settings: Settings::default(),
            cursor: today,
            pane: Pane::Day(today),
            status: String::new(),
            toast: None,
            toast_gen: 0,
            cal_picker_open: false,
            got_config: false,
            renaming: None,
            color_picker: None,
            dragging_sidebar: false,
            dragging_inspector: false,
            window_w: 0.0,
            last_shelf_fp: None,
        }
    }
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        let app = Self::default();
        bridge::send(CalCmd::Boot);
        (
            app,
            Task::batch([
                sola_kit::window_ready_task(Msg::WindowReady),
                Task::perform(
                    async {
                        tokio::time::sleep(Duration::from_millis(400)).await;
                    },
                    |_| Msg::MaybeMigrate,
                ),
            ]),
        )
    }

    pub fn title(&self) -> String {
        "Calendar".into()
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            bridge::subscription().map(Msg::Worker),
            iced::time::every(Duration::from_secs(60)).map(|_| Msg::Tick),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Msg::KeyPressed(key, modifiers))
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::SidebarRelease)
                }
                Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Msg::WindowResized(size.width))
                }
                _ => None,
            }),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => self.on_bus(&message),
            Msg::Worker(ev) => self.on_worker(ev),
            Msg::WindowReady(id) => {
                self.window_id = id;
                match id {
                    Some(id) => iced::window::size(id).map(|s| Msg::WindowResized(s.width)),
                    None => Task::none(),
                }
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(APP_ID);
                Task::none()
            }
            Msg::Tick => Task::none(),
            Msg::Prev => {
                self.cursor = match self.settings.view {
                    View::Month => prev_month(self.cursor),
                    View::Week => prev_week(self.cursor),
                    View::Day => self.cursor - chrono::Duration::days(1),
                };
                if matches!(self.pane, Pane::Day(_)) {
                    self.pane = Pane::Day(self.cursor);
                }
                self.sync_visible();
                Task::none()
            }
            Msg::Next => {
                self.cursor = match self.settings.view {
                    View::Month => next_month(self.cursor),
                    View::Week => next_week(self.cursor),
                    View::Day => self.cursor + chrono::Duration::days(1),
                };
                if matches!(self.pane, Pane::Day(_)) {
                    self.pane = Pane::Day(self.cursor);
                }
                self.sync_visible();
                Task::none()
            }
            Msg::Today => {
                self.cursor = today();
                if matches!(self.pane, Pane::Day(_)) {
                    self.pane = Pane::Day(self.cursor);
                }
                self.sync_visible();
                Task::none()
            }
            Msg::SetView(view) => {
                self.settings.view = view;
                bridge::send(CalCmd::SaveSettings(self.settings.clone()));
                Task::none()
            }
            Msg::SelectDay(day) => {
                self.cursor = day;
                self.pane = Pane::Day(day);
                Task::none()
            }
            Msg::OpenEvent(id) => {
                if let Some(ev) = self.store.events.iter().find(|e| e.id == id).cloned() {
                    self.pane = Pane::Draft(draft_from_event(&ev));
                }
                Task::none()
            }
            Msg::NewEvent => {
                self.pane = Pane::Draft(self.new_draft(self.cursor));
                Task::none()
            }
            Msg::NewOn(day) => {
                self.cursor = day;
                self.pane = Pane::Draft(self.new_draft(day));
                Task::none()
            }
            Msg::ToggleCal(id) => {
                if let Some(cal) = self.store.calendars.iter_mut().find(|c| c.id == id) {
                    cal.visible = !cal.visible;
                    bridge::send(CalCmd::SetVisible {
                        id,
                        visible: cal.visible,
                    });
                }
                Task::none()
            }
            Msg::DefaultCal(id) => {
                self.settings.default_calendar = id;
                bridge::send(CalCmd::SaveSettings(self.settings.clone()));
                Task::none()
            }
            Msg::DraftTitle(s) => self.patch_draft(|d| d.title = s),
            Msg::DraftNotes(s) => self.patch_draft(|d| d.notes = s),
            Msg::DraftLocation(s) => self.patch_draft(|d| d.location = s),
            Msg::DraftDate(s) => self.patch_draft(|d| d.date = s),
            Msg::DraftStart(s) => self.patch_draft(|d| d.start_time = s),
            Msg::DraftEnd(s) => self.patch_draft(|d| d.end_time = s),
            Msg::DraftAllDay(v) => self.patch_draft(|d| d.all_day = v),
            Msg::DraftCalendar(id) => {
                self.cal_picker_open = false;
                self.patch_draft(|d| d.calendar_id = id)
            }
            Msg::ToggleCalPicker => {
                self.cal_picker_open = !self.cal_picker_open;
                Task::none()
            }
            Msg::DismissCalPicker => {
                self.cal_picker_open = false;
                Task::none()
            }
            Msg::SaveDraft => self.save_draft(),
            Msg::DeleteDraft => {
                if let Pane::Draft(d) = &self.pane {
                    if let Some(id) = &d.original_id {
                        bridge::send(CalCmd::DeleteEvent(id.clone()));
                    }
                }
                self.show_day(self.cursor);
                Task::none()
            }
            Msg::ClosePane => {
                self.dismiss_draft();
                Task::none()
            }
            Msg::OpenUrl(url) => {
                sola_core::open_url_logged(&url);
                Task::none()
            }
            Msg::Refresh => {
                self.sync_visible();
                Task::none()
            }
            Msg::DismissToast { generation } => {
                if generation == self.toast_gen {
                    self.toast = None;
                }
                Task::none()
            }
            Msg::KeyPressed(key, mods) => self.on_key(key, mods),
            Msg::MaybeMigrate => {
                if !self.got_config {
                    self.emit_config_from_store();
                    self.got_config = true;
                }
                Task::none()
            }
            Msg::SidebarPress => {
                self.dragging_sidebar = true;
                Task::none()
            }
            Msg::InspectorPress => {
                self.dragging_inspector = true;
                Task::none()
            }
            Msg::SidebarRelease => {
                if self.dragging_sidebar || self.dragging_inspector {
                    self.dragging_sidebar = false;
                    self.dragging_inspector = false;
                    bridge::send(CalCmd::SaveSettings(self.settings.clone()));
                }
                Task::none()
            }
            Msg::CursorMoved(x) => {
                if self.dragging_sidebar && self.window_w > 1.0 {
                    self.settings.sidebar_w = x.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                }
                if self.dragging_inspector && self.window_w > 1.0 {
                    self.settings.inspector_w =
                        (self.window_w - x).clamp(INSPECTOR_W_MIN, INSPECTOR_W_MAX);
                }
                Task::none()
            }
            Msg::WindowResized(w) => {
                self.window_w = w;
                Task::none()
            }
            Msg::RenameCal(id) => self.begin_rename(id),
            Msg::RenameSelectAll => {
                if self.renaming.is_none() {
                    return Task::none();
                }
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::text_input::select_all::<Msg>(
                        cal_rename_id(),
                    ),
                )
            }
            Msg::RenameInput(s) => {
                if let Some((_, draft)) = &mut self.renaming {
                    *draft = s;
                }
                Task::none()
            }
            Msg::RenameCommit => {
                if let Some((id, alias)) = self.renaming.take() {
                    self.color_picker = None;
                    bridge::send(CalCmd::SetAlias { id, alias });
                }
                Task::none()
            }
            Msg::HideCal(id) => {
                if let Some((rid, alias)) = self.renaming.take() {
                    if rid == id {
                        bridge::send(CalCmd::SetAlias {
                            id: rid,
                            alias,
                        });
                    }
                }
                self.color_picker = None;
                if let Some(cal) = self.store.calendars.iter_mut().find(|c| c.id == id) {
                    cal.hidden = true;
                    cal.visible = false;
                }
                let next = self.store.pick_default(&self.settings.default_calendar);
                if next != self.settings.default_calendar {
                    self.settings.default_calendar = next;
                    bridge::send(CalCmd::SaveSettings(self.settings.clone()));
                }
                bridge::send(CalCmd::SetHidden {
                    id,
                    hidden: true,
                });
                Task::none()
            }
            Msg::ToggleColor(id) => {
                if self.color_picker.as_ref().is_some_and(|(cid, _)| cid == &id) {
                    self.color_picker = None;
                } else {
                    let seed = self
                        .store
                        .calendar(&id)
                        .and_then(|c| parse_hex(c.display_color()))
                        .unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
                    self.color_picker = Some((id, ColorPicker::new(seed)));
                }
                Task::none()
            }
            Msg::ColorMsg(m) => {
                if let Some((id, picker)) = &mut self.color_picker {
                    picker.update(m);
                    let hex = color_to_hex(picker.color());
                    let id = id.clone();
                    if let Some(cal) = self.store.calendars.iter_mut().find(|c| c.id == id) {
                        cal.color_override = Some(hex.clone());
                    }
                    bridge::send(CalCmd::SetColor { id, color: hex });
                }
                Task::none()
            }
            Msg::ColorDismiss => {
                self.color_picker = None;
                Task::none()
            }
        }
    }

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            bridge::send(CalCmd::Shutdown);
            return iced::exit();
        }
        if let Some(Topic::MenuAction(p)) = Topic::parse(message) {
            if p.app_id == APP_ID {
                return self.on_menu(&p.action_id);
            }
        }
        if let Some(Topic::CalendarConfig(cfg)) = Topic::parse(message) {
            self.got_config = true;
            // Our own shelf publish (empty accounts) is already in the store.
            if cfg.accounts.is_empty() && !cfg.calendars.is_empty() {
                let fp = shelf_fingerprint_from_bus(&cfg.calendars);
                if self.last_shelf_fp.as_deref() == Some(fp.as_str()) {
                    return Task::none();
                }
            }
            bridge::send(CalCmd::ApplyConfig(cfg));
        }
        Task::none()
    }

    fn emit_config_from_store(&mut self) {
        let accounts: Vec<_> = self
            .store
            .accounts
            .iter()
            .filter_map(crate::worker::account_to_bus)
            .collect();
        let calendars: Vec<_> = self
            .store
            .calendars
            .iter()
            .map(Calendar::to_shelf)
            .collect();
        self.last_shelf_fp = Some(shelf_fingerprint(&self.store.calendars));
        let cfg = CalendarConfig {
            google_client_id: google_calendar_client_id(),
            accounts,
            calendars,
        };
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::CalendarConfig(cfg));
        }
    }

    /// Publish the calendar shelf so Settings can list hidden/alias/colour.
    /// Accounts stay empty: Settings keeps its own account list.
    fn maybe_emit_shelf(&mut self) {
        let fp = shelf_fingerprint(&self.store.calendars);
        if self.last_shelf_fp.as_deref() == Some(fp.as_str()) {
            return;
        }
        self.last_shelf_fp = Some(fp);
        let calendars = self
            .store
            .calendars
            .iter()
            .map(Calendar::to_shelf)
            .collect();
        let cfg = CalendarConfig {
            google_client_id: google_calendar_client_id(),
            accounts: Vec::new(),
            calendars,
        };
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::CalendarConfig(cfg));
        }
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "quit" => {
                bridge::send(CalCmd::Shutdown);
                iced::exit()
            }
            "new_event" => self.update(Msg::NewEvent),
            "refresh" => self.update(Msg::Refresh),
            "today" => self.update(Msg::Today),
            "view_month" => self.update(Msg::SetView(View::Month)),
            "view_week" => self.update(Msg::SetView(View::Week)),
            "view_day" => self.update(Msg::SetView(View::Day)),
            _ => Task::none(),
        }
    }

    fn on_key(&mut self, key: keyboard::Key, mods: keyboard::Modifiers) -> Task<Msg> {
        if mods.command() {
            return Task::none();
        }
        match key.as_ref() {
            keyboard::Key::Character("t") => self.update(Msg::Today),
            keyboard::Key::Character("n") => self.update(Msg::NewEvent),
            keyboard::Key::Character("1") => self.update(Msg::SetView(View::Month)),
            keyboard::Key::Character("2") => self.update(Msg::SetView(View::Week)),
            keyboard::Key::Character("3") => self.update(Msg::SetView(View::Day)),
            keyboard::Key::Named(NamedKey::ArrowLeft) => self.update(Msg::Prev),
            keyboard::Key::Named(NamedKey::ArrowRight) => self.update(Msg::Next),
            keyboard::Key::Named(NamedKey::Escape) => {
                if self.renaming.is_some() || self.color_picker.is_some() {
                    self.renaming = None;
                    self.color_picker = None;
                    Task::none()
                } else {
                    self.update(Msg::ClosePane)
                }
            }
            _ => Task::none(),
        }
    }

    fn on_worker(&mut self, ev: CalNotice) -> Task<Msg> {
        match ev {
            CalNotice::Snapshot(store) => {
                self.store = store;
                self.store.ensure_local();
                self.maybe_emit_shelf();
            }
            CalNotice::Settings(s) => {
                self.settings = s;
                self.settings.sidebar_w =
                    self.settings.sidebar_w.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                self.settings.inspector_w = self
                    .settings
                    .inspector_w
                    .clamp(INSPECTOR_W_MIN, INSPECTOR_W_MAX);
            }
            CalNotice::Status(s) => self.status = s,
            CalNotice::Error(s) | CalNotice::Toast(s) => return self.show_toast(s),
        }
        Task::none()
    }

    fn show_toast(&mut self, msg: String) -> Task<Msg> {
        self.toast_gen = self.toast_gen.saturating_add(1);
        self.toast = Some(msg);
        let generation = self.toast_gen;
        Task::perform(async move { tokio::time::sleep(TOAST_TTL).await }, move |_| {
            Msg::DismissToast { generation }
        })
    }

    fn patch_draft(&mut self, f: impl FnOnce(&mut Draft)) -> Task<Msg> {
        if let Pane::Draft(d) = &mut self.pane {
            f(d);
        }
        Task::none()
    }

    fn new_draft(&self, day: NaiveDate) -> Draft {
        let cal_id = if self.store.calendars.iter().any(|c| {
            c.id == self.settings.default_calendar && !c.read_only && !c.hidden
        }) {
            self.settings.default_calendar.clone()
        } else {
            self.store.pick_default(&self.settings.default_calendar)
        };
        let ev = CalEvent::new_local(&cal_id, day, Utc::now());
        draft_from_event(&ev)
            .tap_new()
    }

    fn save_draft(&mut self) -> Task<Msg> {
        let Pane::Draft(d) = &self.pane else {
            return Task::none();
        };
        if d.read_only {
            return self.show_toast("this occurrence is read-only".into());
        }
        match draft_to_event(d) {
            Ok(ev) => {
                let day = ev
                    .start_date
                    .unwrap_or_else(|| ev.start.with_timezone(&chrono::Local).date_naive());
                bridge::send(CalCmd::SaveEvent(ev));
                self.show_day(day);
            }
            Err(e) => return self.show_toast(e),
        }
        Task::none()
    }

    fn show_day(&mut self, day: NaiveDate) {
        self.cursor = day;
        self.pane = Pane::Day(day);
        self.cal_picker_open = false;
    }

    fn dismiss_draft(&mut self) {
        let day = match &self.pane {
            Pane::Draft(d) => parse_pretty_date(&d.date).unwrap_or(self.cursor),
            Pane::Day(d) => *d,
        };
        self.show_day(day);
    }

    fn begin_rename(&mut self, id: String) -> Task<Msg> {
        let draft = self
            .store
            .calendar(&id)
            .map(|c| c.display_name().to_string())
            .unwrap_or_default();
        self.renaming = Some((id, draft));
        self.color_picker = None;
        Task::batch([
            iced::widget::operation::focus(cal_rename_id()),
            Task::done(Msg::RenameSelectAll),
        ])
    }

    fn sync_visible(&self) {
        let (start, end) = match self.settings.view {
            View::Month => month_span(self.cursor),
            View::Week => {
                let w = week_of(self.cursor);
                (w[0], w[6])
            }
            View::Day => (self.cursor, self.cursor),
        };
        bridge::send(CalCmd::Sync { start, end });
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let board: Element<'_, Msg> = match self.settings.view {
            View::Month => self.view_month(),
            View::Week => self.view_week(),
            View::Day => self.view_day(),
        };
        let win = self.window_w.max(1.0);
        let rest_w = (win - self.settings.sidebar_w).max(1.0);
        let board_ratio = ((rest_w - self.settings.inspector_w) / rest_w).clamp(0.35, 0.88);
        let sidebar_ratio = (self.settings.sidebar_w / win).clamp(0.08, 0.45);
        let palette = self.theme.extended_palette();
        let line = palette.background.stronger.color;
        let chrome = palette.background.weakest.color;
        let canvas = palette.background.base.color;
        let main = split_with(
            SplitDir::Vertical,
            column![self.view_toolbar(), board]
                .width(Length::Fill)
                .height(Length::Fill),
            board_ratio,
            Msg::InspectorPress,
            self.view_inspector(),
            DividerColors {
                a: canvas,
                line,
                b: chrome,
            },
        );
        let body = split_with(
            SplitDir::Vertical,
            self.view_sidebar(),
            sidebar_ratio,
            Msg::SidebarPress,
            main,
            DividerColors {
                a: chrome,
                line,
                b: canvas,
            },
        );
        let mut col = column![body].width(Length::Fill).height(Length::Fill);
        if let Some(toast) = &self.toast {
            col = col.push(self.view_toast(toast));
        }
        let canvas = container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(canvas_style);
        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Calendar",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            canvas.into(),
        )
    }

    fn view_sidebar(&self) -> Element<'_, Msg> {
        let mut items = Vec::new();
        for cal in &self.store.calendars {
            if cal.hidden {
                continue;
            }
            let disc = cal_disc(cal.display_color(), cal.visible);
            let renaming = self
                .renaming
                .as_ref()
                .is_some_and(|(id, _)| id == &cal.id);
            let item = if renaming {
                let draft = self
                    .renaming
                    .as_ref()
                    .map(|(_, d)| d.as_str())
                    .unwrap_or("");
                let field = text_input("Calendar name", draft)
                    .id(cal_rename_id())
                    .size(12)
                    .font(fonts::ui_medium())
                    .line_height(iced::widget::text::LineHeight::Relative(1.2))
                    .on_input(Msg::RenameInput)
                    .on_submit(Msg::RenameCommit)
                    .style(kit_input::style)
                    .padding(Padding::from([1, 4]))
                    .width(Length::Fill);
                let fill = parse_hex(cal.display_color())
                    .unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
                let swatch = rename_color_swatch(fill, Msg::ToggleColor(cal.id.clone()));
                let trailing = row![
                    swatch,
                    hide_cal_button(Msg::HideCal(cal.id.clone())),
                    rename_commit_button(Msg::RenameCommit)
                ]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center);
                let trailing: Element<'_, Msg> =
                    match self.color_picker.as_ref().filter(|(id, _)| id == &cal.id) {
                        Some((_, picker)) => popover_anchored(
                            trailing,
                            popover(picker.view().map(Msg::ColorMsg)),
                            Msg::ColorDismiss,
                        )
                        .into(),
                        None => trailing.into(),
                    };
                let body = column![
                    row![field, trailing]
                        .spacing(SPACE_SM)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    kit_text::caption(cal.name.clone()).style(kit_text::muted),
                ]
                .spacing(2)
                .width(Length::Fill);
                SidebarItem::new(cal.display_name().to_string(), Msg::Tick)
                    .id(cal.id.clone())
                    .active(self.settings.default_calendar == cal.id)
                    .content(body)
            } else {
                SidebarItem::new(
                    cal.display_name().to_string(),
                    Msg::ToggleCal(cal.id.clone()),
                )
                .id(cal.id.clone())
                .leading(disc)
                .active(self.settings.default_calendar == cal.id)
                .on_double_click(Msg::DefaultCal(cal.id.clone()))
                .on_edit(Msg::RenameCal(cal.id.clone()))
            };
            items.push(item);
        }
        if items.is_empty() {
            items.push(
                SidebarItem::new("None shown", Msg::Tick)
                    .subtitle("Settings → Calendar"),
            );
        }
        let today = today();
        let tomorrow = today.succ_opt().unwrap_or(today);
        let sections = vec![
            SidebarSection::new("Calendars", items),
            self.quickview_section("Today", today),
            self.quickview_section("Tomorrow", tomorrow),
        ];
        let status = kit_text::caption(if self.status.is_empty() {
            " ".to_string()
        } else {
            self.status.clone()
        })
        .style(kit_text::muted);
        SidebarPanel::new(sections)
            .fill_width()
            .footer(status.into())
            .build()
    }

    fn quickview_section(&self, title: &'static str, day: NaiveDate) -> SidebarSection<'_, Msg> {
        let events = self.store.visible_events_on(day);
        let extra = events.len().saturating_sub(QUICKVIEW_CAP);
        let mut items = Vec::new();
        for ev in events.iter().take(QUICKVIEW_CAP) {
            let color = self
                .store
                .calendar(&ev.calendar_id)
                .map(|c| c.display_color().to_string())
                .unwrap_or_else(|| "#7aa2f7".into());
            items.push(
                SidebarItem::new(ev.display_title().to_string(), Msg::OpenEvent(ev.id.clone()))
                    .id(format!("qv-{}", ev.id))
                    .leading(cal_disc(&color, true))
                    .subtitle(ev.timed_label()),
            );
        }
        if extra > 0 {
            items.push(
                SidebarItem::new(format!("+{extra} more"), Msg::SelectDay(day))
                    .id(format!("qv-more-{day}")),
            );
        }
        if items.is_empty() {
            items.push(
                SidebarItem::new("Nothing scheduled", Msg::Tick)
                    .id(format!("qv-empty-{day}")),
            );
        }
        SidebarSection::new(title, items)
    }

    fn view_toolbar(&self) -> Element<'_, Msg> {
        let title = match self.settings.view {
            View::Month => month_title(self.cursor),
            View::Week => week_title(week_of(self.cursor)),
            View::Day => day_title(self.cursor),
        };
        let nav = row![
            kit_btn::labeled_sm("‹", kit_btn::ghost).on_press(Msg::Prev),
            kit_btn::labeled_sm("›", kit_btn::ghost).on_press(Msg::Next),
            kit_btn::labeled_sm("Today", kit_btn::secondary).on_press(Msg::Today),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        let views = row![
            view_chip("Month", self.settings.view == View::Month, View::Month),
            view_chip("Week", self.settings.view == View::Week, View::Week),
            view_chip("Day", self.settings.view == View::Day, View::Day),
        ]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center);
        container(
            row![
                nav,
                kit_text::subheading(title).width(Length::Fill),
                views,
                kit_btn::labeled_sm("New Event", kit_btn::primary).on_press(Msg::NewEvent),
            ]
            .spacing(SPACE_LG)
            .align_y(Alignment::Center)
            .padding(Padding::from([SPACE_SM, SPACE_LG]))
            .height(Length::Fixed(CHROME_H)),
        )
        .width(Length::Fill)
        .style(chrome_style)
        .into()
    }

    fn view_month(&self) -> Element<'_, Msg> {
        let weeks = month_weeks_filled(self.cursor);
        let this_month = first_of_month(self.cursor).month();
        let today = today();
        let selected = match self.pane {
            Pane::Day(d) => Some(d),
            Pane::Draft(_) => Some(self.cursor),
        };
        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        rows.push(
            row(WEEKDAYS
                .iter()
                .map(|d| {
                    container(kit_text::caption(*d).style(kit_text::muted))
                        .width(Length::Fill)
                        .padding(Padding::from([SPACE_SM, SPACE_SM]))
                        .into()
                })
                .collect::<Vec<_>>())
            .into(),
        );
        for week in weeks {
            let cells: Vec<Element<'_, Msg>> = week
                .iter()
                .map(|day| {
                    self.month_cell(*day, *day == today, selected == Some(*day), day.month() == this_month)
                })
                .collect();
            rows.push(row(cells).spacing(1).height(Length::Fill).into());
        }
        container(column(rows).spacing(1).padding(SPACE_MD))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn month_cell(
        &self,
        day: NaiveDate,
        is_today: bool,
        selected: bool,
        in_month: bool,
    ) -> Element<'_, Msg> {
        let events = self.store.visible_events_on(day);
        let chips: Vec<Element<'_, Msg>> = events
            .iter()
            .map(|ev| event_chip(ev, self.store.calendar(&ev.calendar_id)))
            .collect();
        let events_el: Element<'_, Msg> = if chips.is_empty() {
            Space::new().width(Length::Fill).height(Length::Fill).into()
        } else {
            crate::fit::stack(chips)
        };
        let mut num = container(
            text(format!("{}", day.day()))
                .size(12)
                .font(fonts::ui_medium()),
        )
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(22.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
        if is_today {
            num = num.style(today_disc);
        } else if !in_month {
            num = num.style(|theme: &Theme| container::Style {
                text_color: Some(theme.extended_palette().secondary.base.text),
                ..container::Style::default()
            });
        }
        let body = column![num, events_el]
            .spacing(SPACE_XS)
            .padding(Padding::from([SPACE_XS, SPACE_SM]))
            .align_x(Alignment::Start)
            .width(Length::Fill)
            .height(Length::Fill);
        mouse_area(
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .style(move |theme: &Theme| cell_style(theme, selected)),
        )
        .on_press(Msg::SelectDay(day))
        .on_double_click(Msg::NewOn(day))
        .into()
    }

    fn view_week(&self) -> Element<'_, Msg> {
        let week = week_of(self.cursor);
        let today = today();
        let headers: Vec<Element<'_, Msg>> = week
            .iter()
            .map(|d| {
                let label = format!("{} {}", WEEKDAYS[d.weekday().num_days_from_sunday() as usize], d.day());
                let mut t = kit_text::caption(label);
                if *d == today {
                    t = t.style(kit_text::accent);
                } else {
                    t = t.style(kit_text::muted);
                }
                mouse_area(
                    container(t)
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(SPACE_SM),
                )
                .on_press(Msg::SelectDay(*d))
                .into()
            })
            .collect();
        let cols: Vec<Element<'_, Msg>> = week
            .iter()
            .map(|d| self.agenda_column(*d, false))
            .collect();
        column![
            row(headers).padding(Padding::from([0.0, SPACE_MD])),
            h_hairline(),
            scrollable(row(cols).spacing(1).padding(SPACE_MD)).height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_day(&self) -> Element<'_, Msg> {
        scrollable(self.agenda_column(self.cursor, true))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn agenda_column(&self, day: NaiveDate, wide: bool) -> Element<'_, Msg> {
        let events = self.store.visible_events_on(day);
        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        for ev in events {
            let cal = self.store.calendar(&ev.calendar_id);
            let time = kit_text::caption(ev.timed_label()).style(kit_text::muted);
            let title = kit_text::body(ev.display_title().to_string());
            let disc = cal_disc(
                cal.map(|c| c.display_color()).unwrap_or("#7aa2f7"),
                true,
            );
            let row = row![disc, column![title, time].spacing(2)]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center)
                .padding(SPACE_SM);
            rows.push(
                button(row)
                    .style(kit_btn::list_item(false))
                    .width(Length::Fill)
                    .on_press(Msg::OpenEvent(ev.id.clone()))
                    .into(),
            );
        }
        if rows.is_empty() {
            rows.push(
                kit_text::caption("No events")
                    .style(kit_text::muted)
                    .into(),
            );
        }
        let inner = column(rows).spacing(SPACE_XS).padding(SPACE_SM);
        let _ = wide;
        mouse_area(container(inner).width(Length::Fill).padding(SPACE_XS))
            .on_press(Msg::SelectDay(day))
            .on_double_click(Msg::NewOn(day))
            .into()
    }

    fn view_inspector(&self) -> Element<'_, Msg> {
        let (body, actions): (Element<'_, Msg>, Option<Element<'_, Msg>>) = match &self.pane {
            Pane::Day(day) => (self.view_day_pane(*day), None),
            Pane::Draft(d) if d.read_only => (self.view_draft_read(d), None),
            Pane::Draft(d) => (self.view_draft_edit(d), Some(self.view_draft_actions(d))),
        };
        let mut col = column![scrollable(body).height(Length::Fill)]
            .width(Length::Fill)
            .height(Length::Fill);
        if let Some(actions) = actions {
            col = col.push(actions);
        }
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(chrome_style)
            .into()
    }

    fn view_day_pane(&self, day: NaiveDate) -> Element<'_, Msg> {
        let events = self.store.visible_events_on(day);
        let mut rows: Vec<Element<'_, Msg>> = vec![
            kit_text::subheading(day_title(day))
                .wrapping(Wrapping::Word)
                .into(),
            kit_btn::labeled("New Event", kit_btn::primary)
                .on_press(Msg::NewOn(day))
                .width(Length::Fill)
                .into(),
        ];
        if events.is_empty() {
            rows.push(
                kit_text::caption("Nothing on this day.")
                    .style(kit_text::muted)
                    .into(),
            );
        }
        for ev in events {
            rows.push(self.inspector_event_row(ev));
        }
        column(rows)
            .spacing(SPACE_MD)
            .padding(SPACE_LG)
            .width(Length::Fill)
            .into()
    }

    fn inspector_event_row<'a>(&'a self, ev: &'a CalEvent) -> Element<'a, Msg> {
        let cal = self.store.calendar(&ev.calendar_id);
        let disc = cal_disc(
            cal.map(|c| c.display_color()).unwrap_or("#7aa2f7"),
            true,
        );
        let when = kit_text::caption(ev.timed_label()).style(kit_text::muted);
        let title = kit_text::body(ev.display_title().to_string()).wrapping(Wrapping::Word);
        button(
            row![disc, column![title, when].spacing(2).width(Length::Fill)]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center)
                .padding(SPACE_SM),
        )
        .style(kit_btn::list_item(false))
        .width(Length::Fill)
        .on_press(Msg::OpenEvent(ev.id.clone()))
        .into()
    }

    fn view_draft_read<'a>(&'a self, d: &'a Draft) -> Element<'a, Msg> {
        let title = if d.title.trim().is_empty() {
            "Untitled"
        } else {
            d.title.trim()
        };
        let mut col = column![
            kit_text::subheading(title.to_string()).wrapping(Wrapping::Word),
            kit_text::caption(draft_when_line(d)).style(kit_text::muted),
        ]
        .spacing(SPACE_SM)
        .padding(SPACE_LG)
        .width(Length::Fill);
        if let Some(cal) = self.store.calendar(&d.calendar_id) {
            col = col.push(
                row![
                    cal_disc(cal.display_color(), true),
                    kit_text::body(cal.display_name().to_string()).wrapping(Wrapping::Word),
                ]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(Space::new().height(Length::Fixed(SPACE_MD)));
        if !d.location.trim().is_empty() {
            let blocks = crate::rich::to_blocks(&d.location);
            if !blocks.is_empty() {
                col = col.push(prose(blocks, &self.theme, Msg::OpenUrl));
            }
        }
        if !d.notes.trim().is_empty() {
            let blocks = crate::rich::to_blocks(&d.notes);
            if !blocks.is_empty() {
                col = col.push(prose(blocks, &self.theme, Msg::OpenUrl));
            }
        }
        if d.repeating {
            col = col.push(
                kit_text::caption("Repeating — editing the series comes later.")
                    .style(kit_text::muted),
            );
        }
        col.into()
    }

    fn view_draft_edit<'a>(&'a self, d: &'a Draft) -> Element<'a, Msg> {
        let title = text_input("New Event", &d.title)
            .size(18)
            .font(fonts::display())
            .line_height(iced::widget::text::LineHeight::Relative(1.2))
            .padding(Padding::from([2, 0]))
            .width(Length::Fill)
            .on_input(Msg::DraftTitle)
            .style(draft_plain_input);

        let mut when = column![draft_meta_row(
            "Date",
            draft_value_input("September 8, 2026", &d.date, Msg::DraftDate),
        )]
        .spacing(SPACE_MD)
        .width(Length::Fill);
        if !d.all_day {
            when = when.push(draft_meta_row(
                "Starts",
                draft_value_input("9:00 AM", &d.start_time, Msg::DraftStart),
            ));
            when = when.push(draft_meta_row(
                "Ends",
                draft_value_input("10:00 AM", &d.end_time, Msg::DraftEnd),
            ));
        }
        when = when.push(draft_meta_row(
            "All day",
            toggler(d.all_day)
                .on_toggle(Msg::DraftAllDay)
                .style(toggle_style),
        ));

        let details = column![
            draft_meta_row("Calendar", self.calendar_picker(d)),
            draft_meta_row(
                "Location",
                draft_value_input("Add a location", &d.location, Msg::DraftLocation),
            ),
            draft_meta_row(
                "Notes",
                draft_value_input("Add notes", &d.notes, Msg::DraftNotes),
            ),
        ]
        .spacing(SPACE_MD)
        .width(Length::Fill);

        let mut col = column![title, draft_hairline(), when, draft_hairline(), details]
            .spacing(SPACE_LG)
            .padding(SPACE_LG)
            .width(Length::Fill);
        if d.repeating {
            col = col.push(
                kit_text::caption("Repeating — editing the series comes later.")
                    .style(kit_text::muted),
            );
        }
        col.into()
    }

    fn calendar_picker<'a>(&'a self, d: &'a Draft) -> Element<'a, Msg> {
        let cal = self.store.calendar(&d.calendar_id);
        let name = cal
            .map(|c| c.display_name().to_string())
            .unwrap_or_else(|| "Calendar".into());
        let color = cal.map(|c| c.display_color()).unwrap_or("#7aa2f7");
        let chevron = icon_svg(
            icon_handle(if self.cal_picker_open {
                "lucide/chevron-up"
            } else {
                "lucide/chevron-down"
            }),
            12,
        );
        let trigger = button(
            row![
                cal_disc(color, true),
                kit_text::body(name)
                    .wrapping(Wrapping::Word)
                    .width(Length::Fill),
                chevron,
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center)
            .width(Length::Fill),
        )
        .padding(Padding::from([3, 0]))
        .style(kit_btn::ghost)
        .width(Length::Fill)
        .on_press(Msg::ToggleCalPicker);

        if !self.cal_picker_open {
            return trigger.into();
        }

        let rows: Vec<Element<'_, Msg>> = self
            .store
            .writable_calendars()
            .into_iter()
            .map(|c| {
                let selected = c.id == d.calendar_id;
                let check: Element<'_, Msg> = if selected {
                    icon_svg(icon_handle("lucide/check"), 12)
                } else {
                    Space::new().width(12).height(12).into()
                };
                button(
                    row![
                        cal_disc(c.display_color(), true),
                        kit_text::body(c.display_name().to_string())
                            .wrapping(Wrapping::Word)
                            .width(Length::Fill),
                        check,
                    ]
                    .spacing(SPACE_MD)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
                )
                .padding(Padding::from([6, 8]))
                .width(Length::Fill)
                .style(kit_btn::list_item(selected))
                .on_press(Msg::DraftCalendar(c.id.clone()))
                .into()
            })
            .collect();
        let menu = popover(column(rows).spacing(2))
            .padding(SPACE_SM)
            .width(Length::Fill);
        popover_anchored(trigger, menu, Msg::DismissCalPicker)
            .placement(sola_kit::components::popover::Placement::Below)
            .match_anchor_width()
            .into()
    }

    fn view_draft_actions(&self, d: &Draft) -> Element<'_, Msg> {
        let mut row = row![kit_btn::labeled("Save", kit_btn::primary)
            .on_press(Msg::SaveDraft)
            .width(Length::Fill)]
        .spacing(SPACE_SM)
        .width(Length::Fill);
        if d.original_id.is_some() && !d.repeating {
            row = row.push(kit_btn::labeled("Delete", kit_btn::danger).on_press(Msg::DeleteDraft));
        }
        column![
            draft_hairline(),
            container(row).padding(Padding::from([SPACE_MD, SPACE_LG])),
        ]
        .width(Length::Fill)
        .into()
    }

    fn view_toast(&self, toast: &str) -> Element<'_, Msg> {
        container(kit_text::body(toast.to_string()))
            .padding(Padding::from([SPACE_MD, SPACE_LG]))
            .width(Length::Fill)
            .style(toast_style)
            .into()
    }
}

impl Draft {
    fn tap_new(mut self) -> Self {
        self.original_id = None;
        self
    }
}

fn draft_from_event(ev: &CalEvent) -> Draft {
    let local_start = ev.start.with_timezone(&chrono::Local);
    let local_end = ev.end.with_timezone(&chrono::Local);
    let date = if ev.all_day {
        ev.start_date.unwrap_or_else(|| local_start.date_naive())
    } else {
        local_start.date_naive()
    };
    Draft {
        original_id: Some(ev.id.clone()),
        calendar_id: ev.calendar_id.clone(),
        title: ev.title.clone(),
        notes: ev.notes.clone(),
        location: ev.location.clone(),
        all_day: ev.all_day,
        date: pretty_date(date),
        start_time: format_hm(local_start.hour(), local_start.minute()),
        end_time: format_hm(local_end.hour(), local_end.minute()),
        read_only: ev.read_only,
        repeating: ev.master_id.is_some(),
        remote_id: ev.remote_id.clone(),
        href: ev.href.clone(),
        etag: ev.etag.clone(),
    }
}

fn draft_to_event(d: &Draft) -> Result<CalEvent, String> {
    let date = parse_pretty_date(&d.date)
        .ok_or_else(|| "date must be like September 8, 2026".to_string())?;
    let mut ev = if let Some(id) = &d.original_id {
        CalEvent {
            id: id.clone(),
            calendar_id: d.calendar_id.clone(),
            title: d.title.clone(),
            notes: d.notes.clone(),
            location: d.location.clone(),
            all_day: d.all_day,
            start: Utc::now(),
            end: Utc::now(),
            start_date: None,
            end_date: None,
            remote_id: d.remote_id.clone(),
            href: d.href.clone(),
            etag: d.etag.clone(),
            read_only: false,
            rrule: None,
            master_id: None,
        }
    } else {
        CalEvent::new_local(&d.calendar_id, date, Utc::now())
    };
    ev.calendar_id = d.calendar_id.clone();
    if d.original_id.is_some() {
        ev.remote_id = d.remote_id.clone();
        ev.href = d.href.clone();
        ev.etag = d.etag.clone();
    }
    ev.title = d.title.clone();
    ev.notes = d.notes.clone();
    ev.location = d.location.clone();
    ev.all_day = d.all_day;
    if d.all_day {
        ev.start_date = Some(date);
        ev.end_date = Some(date + chrono::Duration::days(1));
        ev.start = date
            .and_time(NaiveTime::MIN)
            .and_local_timezone(chrono::Local)
            .single()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| date.and_time(NaiveTime::MIN).and_utc());
        ev.end = ev.start + chrono::Duration::days(1);
    } else {
        let start_t = parse_time(&d.start_time)?;
        let end_t = parse_time(&d.end_time)?;
        let start_naive = date.and_time(start_t);
        let mut end_naive = date.and_time(end_t);
        if end_naive <= start_naive {
            end_naive += chrono::Duration::days(1);
        }
        ev.start = chrono::Local
            .from_local_datetime(&start_naive)
            .single()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| start_naive.and_utc());
        ev.end = chrono::Local
            .from_local_datetime(&end_naive)
            .single()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| end_naive.and_utc());
        ev.start_date = None;
        ev.end_date = None;
    }
    Ok(ev)
}

fn parse_time(s: &str) -> Result<NaiveTime, String> {
    let raw = s.trim().to_ascii_uppercase();
    let (body, pm) = if let Some(rest) = raw.strip_suffix("PM") {
        (rest.trim(), Some(true))
    } else if let Some(rest) = raw.strip_suffix("AM") {
        (rest.trim(), Some(false))
    } else {
        (raw.as_str(), None)
    };
    let (h, m) = if let Some((a, b)) = body.split_once(':') {
        (
            a.trim().parse::<u32>().map_err(|_| "bad time".to_string())?,
            b.trim().parse::<u32>().map_err(|_| "bad time".to_string())?,
        )
    } else {
        (
            body.parse::<u32>().map_err(|_| "bad time".to_string())?,
            0,
        )
    };
    let hour = match pm {
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        _ => h,
    };
    NaiveTime::from_hms_opt(hour, m, 0).ok_or_else(|| "bad time".into())
}

fn draft_when_line(d: &Draft) -> String {
    let date = parse_pretty_date(&d.date)
        .map(day_title)
        .unwrap_or_else(|| d.date.clone());
    if d.all_day {
        format!("{date} · All day")
    } else {
        format!("{date} · {} – {}", d.start_time, d.end_time)
    }
}

fn cal_rename_id() -> iced::widget::Id {
    iced::widget::Id::new("calendar-rename")
}

fn draft_meta_row<'a>(
    label: &'static str,
    control: impl Into<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    row![
        kit_text::caption(label)
            .style(kit_text::muted)
            .width(Length::Fixed(DRAFT_LABEL_W)),
        control.into(),
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn draft_value_input<'a>(
    placeholder: &'static str,
    value: &'a str,
    on_input: fn(String) -> Msg,
) -> Element<'a, Msg> {
    text_input(placeholder, value)
        .size(13)
        .font(fonts::ui())
        .padding(Padding::from([2, 0]))
        .width(Length::Fill)
        .on_input(on_input)
        .style(draft_plain_input)
        .into()
}

fn draft_plain_input(theme: &Theme, _status: kit_input::Status) -> kit_input::Style {
    let p = theme.extended_palette();
    kit_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        icon: p.secondary.base.text,
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.text
        },
        value: p.background.base.text,
        selection: mix_white(p.background.weaker.color, 0.16),
    }
}

fn draft_hairline<'a>() -> Element<'a, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(1)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(mix_white(
                    p.background.weakest.color,
                    HAIRLINE_A,
                ))),
                ..container::Style::default()
            }
        })
        .into()
}

fn shelf_fingerprint(cals: &[Calendar]) -> String {
    let mut parts: Vec<String> = cals
        .iter()
        .map(|c| {
            format!(
                "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
                c.id,
                c.hidden,
                c.alias.as_deref().unwrap_or(""),
                c.color_override.as_deref().unwrap_or(""),
                c.name
            )
        })
        .collect();
    parts.sort();
    parts.join("\u{1e}")
}

fn shelf_fingerprint_from_bus(cals: &[CalendarShelf]) -> String {
    let mut parts: Vec<String> = cals
        .iter()
        .map(|c| {
            format!(
                "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
                c.id,
                c.hidden,
                c.alias.as_deref().unwrap_or(""),
                c.color_override.as_deref().unwrap_or(""),
                c.name
            )
        })
        .collect();
    parts.sort();
    parts.join("\u{1e}")
}

fn rename_color_swatch<'a>(color: Color, on_press: Msg) -> Element<'a, Msg> {
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
        .interaction(mouse::Interaction::Pointer)
        .on_press(on_press)
        .into()
}

fn rename_commit_button<'a>(msg: Msg) -> Element<'a, Msg> {
    let handle = icon_handle("lucide/check");
    let glyph = icon_svg_colored(handle, 12, Color::from_rgb(0.85, 0.87, 0.90));
    button(glyph)
        .padding(Padding::from([2, 5]))
        .style(kit_btn::ghost)
        .on_press(msg)
        .into()
}

fn hide_cal_button<'a>(msg: Msg) -> Element<'a, Msg> {
    let handle = icon_handle("lucide/eye-off");
    let glyph = icon_svg_colored(handle, 12, Color::from_rgb(0.85, 0.87, 0.90));
    button(glyph)
        .padding(Padding::from([2, 5]))
        .style(kit_btn::ghost)
        .on_press(msg)
        .into()
}

fn view_chip<'a>(label: &'a str, active: bool, view: View) -> Element<'a, Msg> {
    let style = if active {
        kit_btn::secondary
    } else {
        kit_btn::ghost
    };
    kit_btn::labeled_sm(label, style)
        .on_press(Msg::SetView(view))
        .into()
}

fn event_chip<'a>(ev: &'a CalEvent, cal: Option<&'a Calendar>) -> Element<'a, Msg> {
    let color = cal
        .and_then(|c| parse_hex(c.display_color()))
        .unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
    let disc = container(Space::new().width(8).height(8))
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..container::Style::default()
        });
    let label = if ev.all_day {
        ev.display_title().to_string()
    } else {
        format!("{}  {}", ev.timed_label(), ev.display_title())
    };
    button(
        row![disc, kit_text::caption(label)]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
    )
    .style(kit_btn::ghost)
    .padding([2, 4])
    .on_press(Msg::OpenEvent(ev.id.clone()))
    .into()
}

fn cal_disc<'a>(hex: &str, visible: bool) -> Element<'a, Msg> {
    let color = parse_hex(hex).unwrap_or(Color::from_rgb(0.48, 0.64, 0.97));
    let fill = if visible {
        color
    } else {
        Color::TRANSPARENT
    };
    container(Space::new().width(10).height(10))
        .width(Length::Fixed(10.0))
        .height(Length::Fixed(10.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                color,
                width: 1.5,
                radius: 5.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn canvas_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        ..container::Style::default()
    }
}

fn chrome_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weakest.color)),
        ..container::Style::default()
    }
}

fn toast_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.strong.color)),
        text_color: Some(p.background.base.text),
        ..container::Style::default()
    }
}

fn cell_style(theme: &Theme, selected: bool) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(if selected {
            p.background.strong.color
        } else {
            mix_white(p.background.base.color, 0.02)
        })),
        border: Border {
            color: mix_white(p.background.base.color, HAIRLINE_A),
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
        ..container::Style::default()
    }
}

fn today_disc(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.primary.base.color)),
        text_color: Some(ON_FILL_DARK),
        border: Border {
            radius: 11.0.into(),
            ..Default::default()
        },
        ..container::Style::default()
    }
}

fn h_hairline() -> Element<'static, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(1)
        .style(hairline_style)
        .into()
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

