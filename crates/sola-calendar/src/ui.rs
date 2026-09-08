//! Kit UI: calendars rail, month/week/day board, inspector.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::{
    Space, button, checkbox, column, container, mouse_area, row, scrollable, text,
};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Subscription, Task,
    Theme,
};
use sola_bus::Message;
use sola_bus::topics::{CalendarConfig, Topic};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::style::{
    HAIRLINE_A, ON_FILL_DARK, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    SelectOption, SidebarItem, SidebarSection, button as kit_btn, field, form_row, select, sidebar,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::bridge;
use crate::model::{CalEvent, Calendar, Settings, Store, View, LOCAL_CAL_ID, parse_hex};
use crate::timeutil::{
    WEEKDAYS, day_title, first_of_month, month_span, month_title, month_weeks_filled, next_month,
    next_week, prev_month, prev_week, today, week_of, week_title,
};
use crate::worker::{CalCmd, CalNotice};

const APP_ID: &str = "sola-calendar";
const SIDEBAR_W: f32 = 200.0;
const INSPECTOR_W: f32 = 300.0;
const CHROME_H: f32 = 44.0;
const TOAST_TTL: Duration = Duration::from_secs(4);

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
}

#[derive(Debug, Clone)]
enum Pane {
    Idle,
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
    Refresh,
    DismissToast { generation: u64 },
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    Tick,
    MaybeMigrate,
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
            pane: Pane::Idle,
            status: String::new(),
            toast: None,
            toast_gen: 0,
            cal_picker_open: false,
            got_config: false,
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
                Task::none()
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
                self.sync_visible();
                Task::none()
            }
            Msg::Next => {
                self.cursor = match self.settings.view {
                    View::Month => next_month(self.cursor),
                    View::Week => next_week(self.cursor),
                    View::Day => self.cursor + chrono::Duration::days(1),
                };
                self.sync_visible();
                Task::none()
            }
            Msg::Today => {
                self.cursor = today();
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
                self.pane = Pane::Idle;
                Task::none()
            }
            Msg::ClosePane => {
                self.pane = Pane::Idle;
                self.cal_picker_open = false;
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
            bridge::send(CalCmd::ApplyConfig(cfg));
        }
        Task::none()
    }

    fn emit_config_from_store(&self) {
        let accounts: Vec<_> = self
            .store
            .accounts
            .iter()
            .filter_map(crate::worker::account_to_bus)
            .collect();
        if accounts.is_empty() && self.settings.google_client_id.is_empty() {
            return;
        }
        let cfg = CalendarConfig {
            google_client_id: self.settings.google_client_id.clone(),
            accounts,
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
            keyboard::Key::Named(NamedKey::Escape) => self.update(Msg::ClosePane),
            _ => Task::none(),
        }
    }

    fn on_worker(&mut self, ev: CalNotice) -> Task<Msg> {
        match ev {
            CalNotice::Snapshot(store) => {
                self.store = store;
                self.store.ensure_local();
            }
            CalNotice::Settings(s) => {
                self.settings = s;
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
        let cal_id = if self
            .store
            .calendars
            .iter()
            .any(|c| c.id == self.settings.default_calendar && !c.read_only)
        {
            self.settings.default_calendar.clone()
        } else {
            LOCAL_CAL_ID.to_string()
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
                bridge::send(CalCmd::SaveEvent(ev));
                self.pane = Pane::Idle;
            }
            Err(e) => return self.show_toast(e),
        }
        Task::none()
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
        let mut body = row![
            self.view_sidebar(),
            v_hairline(),
            column![self.view_toolbar(), board]
                .width(Length::Fill)
                .height(Length::Fill),
        ];
        if !matches!(self.pane, Pane::Idle) {
            body = body.push(v_hairline()).push(self.view_inspector());
        }
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
            let disc = cal_disc(&cal.color, cal.visible);
            let mut item = SidebarItem::new(cal.name.clone(), Msg::ToggleCal(cal.id.clone()))
                .id(cal.id.clone())
                .leading(disc)
                .active(self.settings.default_calendar == cal.id)
                .on_double_click(Msg::DefaultCal(cal.id.clone()));
            if cal.read_only {
                item = item.secondary("view");
            }
            items.push(item);
        }
        let mut sections = vec![SidebarSection::new("Calendars", items).fill()];
        let mut account_items = Vec::new();
        if self.store.accounts.is_empty() {
            account_items.push(
                SidebarItem::new("Settings → Calendar", Msg::Tick)
                    .subtitle("Google, iCloud, URL, CalDAV"),
            );
        } else {
            for acc in &self.store.accounts {
                account_items.push(
                    SidebarItem::new(acc.label.clone(), Msg::Tick)
                        .subtitle(acc.kind.label()),
                );
            }
        }
        sections.push(SidebarSection::new("Accounts", account_items));
        let status = kit_text::caption(if self.status.is_empty() {
            " ".to_string()
        } else {
            self.status.clone()
        })
        .style(kit_text::muted);
        container(column![
            sidebar(sections).height(Length::Fill),
            container(status).padding(Padding::from([SPACE_SM, SPACE_LG])),
        ])
        .width(Length::Fixed(SIDEBAR_W))
        .height(Length::Fill)
        .style(chrome_style)
        .into()
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
            _ => None,
        };
        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        rows.push(
            row(WEEKDAYS
                .iter()
                .map(|d| {
                    container(kit_text::caption(*d).style(kit_text::muted))
                        .width(Length::Fill)
                        .center_x(Length::Fill)
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
        let extra = events.len().saturating_sub(3);
        let mut chips: Vec<Element<'_, Msg>> = events
            .iter()
            .take(3)
            .map(|ev| event_chip(ev, self.store.calendar(&ev.calendar_id)))
            .collect();
        if extra > 0 {
            chips.push(
                kit_text::caption(format!("+{extra} more"))
                    .style(kit_text::muted)
                    .into(),
            );
        }
        let mut num = container(text(format!("{}", day.day())).size(12).font(fonts::ui_medium()))
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0))
            .center_x(Length::Fill)
            .center_y(Length::Fill);
        if is_today {
            num = num.style(today_disc);
        } else if !in_month {
            num = num.style(|theme: &Theme| container::Style {
                text_color: Some(theme.extended_palette().secondary.base.text),
                ..container::Style::default()
            });
        }
        let body = column![num, column(chips).spacing(1)]
            .spacing(SPACE_XS)
            .padding(Padding::from([SPACE_XS, SPACE_SM]))
            .width(Length::Fill)
            .height(Length::Fill);
        mouse_area(
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
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
            let disc = cal_disc(cal.map(|c| c.color.as_str()).unwrap_or("#7aa2f7"), true);
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
        let content: Element<'_, Msg> = match &self.pane {
            Pane::Idle => Space::new().into(),
            Pane::Day(day) => self.view_day_pane(*day),
            Pane::Draft(d) => self.view_draft(d),
        };
        container(scrollable(content).height(Length::Fill))
            .width(Length::Fixed(INSPECTOR_W))
            .height(Length::Fill)
            .style(chrome_style)
            .into()
    }

    fn view_day_pane(&self, day: NaiveDate) -> Element<'_, Msg> {
        let events = self.store.visible_events_on(day);
        let mut rows: Vec<Element<'_, Msg>> = vec![
            inspector_header(day_title(day)),
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
            let cal = self.store.calendar(&ev.calendar_id);
            rows.push(event_chip(ev, cal));
        }
        column(rows)
            .spacing(SPACE_MD)
            .padding(SPACE_LG)
            .into()
    }

    fn view_draft(&self, d: &Draft) -> Element<'_, Msg> {
        let cal_label = self
            .store
            .calendar(&d.calendar_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "Calendar".into());
        let cal_opts = self.store.writable_calendars().into_iter().map(|c| {
            SelectOption::new(
                c.name.clone(),
                c.id == d.calendar_id,
                Msg::DraftCalendar(c.id.clone()),
            )
            .mark(c.id.clone())
        });
        let mut col = column![
            inspector_header(if d.original_id.is_some() {
                "Event"
            } else {
                "New Event"
            }),
            field(
                "Title",
                text_input("Event title", &d.title).on_input(Msg::DraftTitle),
                None,
                None,
            ),
            field(
                "Date",
                text_input("YYYY-MM-DD", &d.date).on_input(Msg::DraftDate),
                None,
                None,
            ),
        ]
        .spacing(SPACE_LG)
        .padding(SPACE_LG);
        if !d.all_day {
            col = col.push(field(
                "Starts",
                text_input("9:00 AM or 09:00", &d.start_time).on_input(Msg::DraftStart),
                None,
                None,
            ));
            col = col.push(field(
                "Ends",
                text_input("10:00 AM or 10:00", &d.end_time).on_input(Msg::DraftEnd),
                None,
                None,
            ));
        }
        col = col.push(form_row(
            "All day",
            checkbox(d.all_day)
                .style(sola_kit::components::checkbox_style)
                .on_toggle(Msg::DraftAllDay),
        ));
        if !d.read_only {
            col = col.push(field(
                "Calendar",
                select(
                    cal_label,
                    cal_opts,
                    self.cal_picker_open,
                    Msg::ToggleCalPicker,
                    Msg::DismissCalPicker,
                ),
                None,
                None,
            ));
        }
        col = col.push(field(
            "Location",
            text_input("Optional", &d.location).on_input(Msg::DraftLocation),
            None,
            None,
        ));
        col = col.push(field(
            "Notes",
            text_input("Optional", &d.notes).on_input(Msg::DraftNotes),
            None,
            None,
        ));
        if d.repeating {
            col = col.push(
                kit_text::caption("Repeating iCloud event — editing the series comes later.")
                    .style(kit_text::muted),
            );
        }
        if d.read_only {
            col = col.push(kit_btn::labeled("Close", kit_btn::secondary).on_press(Msg::ClosePane));
        } else {
            col = col.push(
                row![
                    kit_btn::labeled("Save", kit_btn::primary)
                        .on_press(Msg::SaveDraft)
                        .width(Length::Fill),
                    kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::ClosePane),
                ]
                .spacing(SPACE_SM),
            );
            if d.original_id.is_some() && !d.repeating {
                col = col.push(
                    kit_btn::labeled("Delete", kit_btn::danger).on_press(Msg::DeleteDraft),
                );
            }
        }
        col.into()
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
        date: date.to_string(),
        start_time: format!("{:02}:{:02}", local_start.hour(), local_start.minute()),
        end_time: format!("{:02}:{:02}", local_end.hour(), local_end.minute()),
        read_only: ev.read_only,
        repeating: ev.master_id.is_some(),
        remote_id: ev.remote_id.clone(),
        href: ev.href.clone(),
        etag: ev.etag.clone(),
    }
}

fn draft_to_event(d: &Draft) -> Result<CalEvent, String> {
    let date = NaiveDate::parse_from_str(d.date.trim(), "%Y-%m-%d")
        .map_err(|_| "date must be YYYY-MM-DD".to_string())?;
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

fn inspector_header<'a>(title: impl Into<String>) -> Element<'a, Msg> {
    row![
        kit_text::subheading(title.into()).width(Length::Fill),
        kit_btn::labeled_sm("Close", kit_btn::ghost).on_press(Msg::ClosePane),
    ]
    .align_y(Alignment::Center)
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
        .and_then(|c| parse_hex(&c.color))
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

