//! Monitor state, update, and view.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::widget::Id as ScrollId;
use iced::widget::operation;
use iced::widget::scrollable::{RelativeOffset, Viewport};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, stack, text};
use iced::{Alignment, Element, Event, Length, Padding, Subscription, Task, Theme, event, mouse};

use sola_bus::Message;
use sola_bus::topics::{Topic, TopicKind};
use sola_kit::app::{apply_theme_update, is_self_quit};
use sola_kit::components::button as kit_button;
use sola_kit::components::divider::horizontal_divider_drag_with;
use sola_kit::components::json::{line as json_line, pretty as json_pretty};
use sola_kit::components::select::{SelectOption, select_sized};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    DividerColors, SidebarItem, SidebarPanel, SidebarSection, Tone, badge, sidebar, toolbar_button,
    vertical_divider_with,
};
use sola_kit::fonts;
use sola_kit::sola_call::{ObserveEvent, OwnerCatalog, TraceEvent, TraceKind};
use sola_kit::theme::default_theme;
use sola_kit::theme_for;

const APP_ID: &str = "sola-monitor";
const MAX_MESSAGES: usize = 5_000;
const RAIL_W_DEFAULT: f32 = 260.0;
const RAIL_W_MIN: f32 = 160.0;
const RAIL_W_MAX: f32 = 560.0;
const INSPECTOR_H_DEFAULT: f32 = 220.0;
const INSPECTOR_H_MIN: f32 = 96.0;
const INSPECTOR_H_MAX: f32 = 640.0;
const TIME_COL_W: f32 = 96.0;
const TOPIC_COL_W: f32 = 168.0;
const SOURCE_COL_W: f32 = 112.0;
const CALL_COL_W: f32 = 220.0;
const CALLER_COL_W: f32 = 120.0;
const STATUS_COL_W: f32 = 88.0;
const SELECT_W: f32 = 180.0;
const FILTER_W: f32 = 220.0;

fn log_scroll_id() -> ScrollId {
    ScrollId::new("monitor-log")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plane {
    Bus,
    Call,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    None,
    Bus(u64),
    Sticky(String),
    Call(String),
    Catalog {
        owner: String,
        method: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Rail,
    Inspector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallStatus {
    Pending,
    Ok,
    Error,
    Timeout,
    Up,
    Down,
}

struct BusEntry {
    seq: u64,
    timestamp: f64,
    topic: String,
    source: String,
    payload_preview: String,
    payload_pretty: String,
    is_sticky: bool,
}

struct CallEntry {
    key: String,
    timestamp: f64,
    owner: String,
    method: String,
    caller: String,
    status: CallStatus,
    duration_ms: Option<u64>,
    params_pretty: String,
    params_preview: String,
    result_pretty: String,
}

pub struct App {
    plane: Plane,
    bus_log: Vec<BusEntry>,
    bus_pause: Vec<BusEntry>,
    sticky: BTreeMap<String, BusEntry>,
    call_log: Vec<CallEntry>,
    call_pause: Vec<CallEntry>,
    catalog: Vec<OwnerCatalog>,
    call_up: bool,
    filter: String,
    topic_filter: Option<String>,
    owner_filter: Option<String>,
    filter_open: bool,
    paused: bool,
    follow: bool,
    selection: Selection,
    rail_w: f32,
    inspector_h: f32,
    dragging: Drag,
    last_cursor: Option<(f32, f32)>,
    drag_anchor: Option<(f32, f32)>,
    next_seq: u64,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            plane: Plane::Bus,
            bus_log: Vec::new(),
            bus_pause: Vec::new(),
            sticky: BTreeMap::new(),
            call_log: Vec::new(),
            call_pause: Vec::new(),
            catalog: Vec::new(),
            call_up: false,
            filter: String::new(),
            topic_filter: None,
            owner_filter: None,
            filter_open: false,
            paused: false,
            follow: true,
            selection: Selection::None,
            rail_w: RAIL_W_DEFAULT,
            inspector_h: INSPECTOR_H_DEFAULT,
            dragging: Drag::None,
            last_cursor: None,
            drag_anchor: None,
            next_seq: 0,
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Observe(ObserveEvent),
    Plane(Plane),
    FilterChanged(String),
    TopicFilter(String),
    OwnerFilter(String),
    ToggleFilterOpen,
    DismissFilter,
    TogglePause,
    Clear,
    Select(Selection),
    LogScrolled(Viewport),
    RailPress,
    InspectorPress,
    CursorMoved(f32, f32),
    CursorReleased,
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        (
            Self::default(),
            sola_kit::window_ready_task(Msg::WindowReady),
        )
    }

    pub fn title(&self) -> String {
        "Monitor".into()
    }

    pub fn theme(&self) -> Theme {
        theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            sola_kit::app::bus_subscription().map(Msg::Bus),
            sola_kit::observe_subscription().map(Msg::Observe),
            event::listen_with(|event, _, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x, position.y))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                let our_quit = is_self_quit(&message, APP_ID);
                self.push_bus(&message);
                if our_quit {
                    return iced::exit();
                }
                return self.maybe_follow();
            }
            Msg::Observe(ev) => {
                self.on_observe(ev);
                return self.maybe_follow();
            }
            Msg::Plane(p) => {
                self.plane = p;
                self.filter_open = false;
                self.selection = Selection::None;
            }
            Msg::FilterChanged(s) => self.filter = s,
            Msg::TopicFilter(t) => {
                self.topic_filter = if t.is_empty() { None } else { Some(t) };
                self.filter_open = false;
            }
            Msg::OwnerFilter(t) => {
                self.owner_filter = if t.is_empty() { None } else { Some(t) };
                self.filter_open = false;
            }
            Msg::ToggleFilterOpen => self.filter_open = !self.filter_open,
            Msg::DismissFilter => self.filter_open = false,
            Msg::TogglePause => {
                self.paused = !self.paused;
                if !self.paused {
                    let bus: Vec<_> = self.bus_pause.drain(..).collect();
                    for e in bus {
                        self.bus_log.push(e);
                    }
                    let calls: Vec<_> = self.call_pause.drain(..).collect();
                    for e in calls {
                        self.call_log.push(e);
                    }
                    trim(&mut self.bus_log, MAX_MESSAGES);
                    trim(&mut self.call_log, MAX_MESSAGES);
                    return self.maybe_follow();
                }
            }
            Msg::Clear => match self.plane {
                Plane::Bus => {
                    self.bus_log.clear();
                    self.bus_pause.clear();
                    if matches!(self.selection, Selection::Bus(_)) {
                        self.selection = Selection::None;
                    }
                }
                Plane::Call => {
                    self.call_log.clear();
                    self.call_pause.clear();
                    if matches!(self.selection, Selection::Call(_)) {
                        self.selection = Selection::None;
                    }
                }
            },
            Msg::Select(sel) => {
                self.selection = if self.selection == sel {
                    Selection::None
                } else {
                    sel
                };
            }
            Msg::LogScrolled(vp) => {
                self.follow = vp.relative_offset().y >= 0.97;
            }
            Msg::RailPress => {
                self.dragging = Drag::Rail;
                if let Some((x, _)) = self.last_cursor {
                    self.drag_anchor = Some((x, self.rail_w));
                }
            }
            Msg::InspectorPress => {
                self.dragging = Drag::Inspector;
                if let Some((_, y)) = self.last_cursor {
                    self.drag_anchor = Some((y, self.inspector_h));
                }
            }
            Msg::CursorMoved(x, y) => {
                self.last_cursor = Some((x, y));
                match (self.dragging, self.drag_anchor) {
                    (Drag::Rail, Some((anchor, size))) => {
                        self.rail_w = (size + (anchor - x)).clamp(RAIL_W_MIN, RAIL_W_MAX);
                    }
                    (Drag::Inspector, Some((anchor, size))) => {
                        self.inspector_h =
                            (size + (anchor - y)).clamp(INSPECTOR_H_MIN, INSPECTOR_H_MAX);
                    }
                    _ => {}
                }
            }
            Msg::CursorReleased => {
                self.dragging = Drag::None;
                self.drag_anchor = None;
            }
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
        }
        Task::none()
    }

    fn maybe_follow(&self) -> Task<Msg> {
        if self.follow && !self.paused {
            operation::snap_to(log_scroll_id(), RelativeOffset::END)
        } else {
            Task::none()
        }
    }

    fn push_bus(&mut self, message: &Message) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let entry = BusEntry::from_message(message, seq);
        if entry.is_sticky {
            self.sticky
                .insert(entry.topic.clone(), BusEntry::from_message(message, seq));
        }
        if self.paused {
            self.bus_pause.push(entry);
        } else {
            self.bus_log.push(entry);
            trim(&mut self.bus_log, MAX_MESSAGES);
        }
    }

    fn on_observe(&mut self, ev: ObserveEvent) {
        match ev {
            ObserveEvent::Down => {
                self.call_up = false;
                self.catalog.clear();
            }
            ObserveEvent::Catalog(owners) => {
                self.call_up = true;
                self.catalog = owners;
            }
            ObserveEvent::Trace(tr) => self.push_trace(tr),
        }
    }

    fn push_trace(&mut self, tr: TraceEvent) {
        match tr.kind {
            TraceKind::Invoke => {
                let key = match tr.id {
                    Some(id) => id,
                    None => self.lifecycle_key("invoke"),
                };
                let (preview, pretty) =
                    json_pair(tr.params.as_ref().unwrap_or(&serde_json::Value::Null));
                let entry = CallEntry {
                    key: key.clone(),
                    timestamp: now_secs(),
                    owner: tr.owner.unwrap_or_default(),
                    method: tr.method.unwrap_or_default(),
                    caller: tr.caller.unwrap_or_default(),
                    status: CallStatus::Pending,
                    duration_ms: None,
                    params_pretty: pretty,
                    params_preview: preview,
                    result_pretty: String::new(),
                };
                self.push_call(entry);
            }
            TraceKind::Reply | TraceKind::Timeout => {
                let key = match tr.id.clone() {
                    Some(id) => id,
                    None => self.lifecycle_key("reply"),
                };
                let status = if tr.kind == TraceKind::Timeout {
                    CallStatus::Timeout
                } else if tr.ok == Some(true) {
                    CallStatus::Ok
                } else {
                    CallStatus::Error
                };
                let result = reply_pretty(&tr);
                if let Some(e) = self
                    .call_log
                    .iter_mut()
                    .chain(self.call_pause.iter_mut())
                    .find(|e| e.key == key)
                {
                    e.status = status;
                    e.duration_ms = tr.duration_ms;
                    e.result_pretty = result;
                } else {
                    let (preview, pretty) =
                        json_pair(tr.params.as_ref().unwrap_or(&serde_json::Value::Null));
                    self.push_call(CallEntry {
                        key,
                        timestamp: now_secs(),
                        owner: tr.owner.unwrap_or_default(),
                        method: tr.method.unwrap_or_default(),
                        caller: tr.caller.unwrap_or_default(),
                        status,
                        duration_ms: tr.duration_ms,
                        params_pretty: pretty,
                        params_preview: preview,
                        result_pretty: result,
                    });
                }
            }
            TraceKind::Advertise | TraceKind::Unregister => {
                let up = tr.kind == TraceKind::Advertise;
                let owner = tr.owner.unwrap_or_default();
                let (preview, pretty) =
                    json_pair(tr.data.as_ref().unwrap_or(&serde_json::Value::Null));
                let key = self.lifecycle_key(if up { "up" } else { "down" });
                self.push_call(CallEntry {
                    key,
                    timestamp: now_secs(),
                    owner,
                    method: if up {
                        "advertise".into()
                    } else {
                        "unregister".into()
                    },
                    caller: String::new(),
                    status: if up { CallStatus::Up } else { CallStatus::Down },
                    duration_ms: None,
                    params_pretty: pretty,
                    params_preview: preview,
                    result_pretty: String::new(),
                });
            }
        }
    }

    fn lifecycle_key(&mut self, kind: &str) -> String {
        let n = self.next_seq;
        self.next_seq += 1;
        format!("{kind}-{n}")
    }

    fn push_call(&mut self, entry: CallEntry) {
        if self.paused {
            self.call_pause.push(entry);
        } else {
            self.call_log.push(entry);
            trim(&mut self.call_log, MAX_MESSAGES);
        }
    }

    fn pause_count(&self) -> usize {
        self.bus_pause.len() + self.call_pause.len()
    }

    fn bus_visible(&self) -> Vec<&BusEntry> {
        let q = self.filter.to_lowercase();
        self.bus_log
            .iter()
            .filter(|e| {
                if let Some(t) = &self.topic_filter {
                    if &e.topic != t {
                        return false;
                    }
                }
                if q.is_empty() {
                    return true;
                }
                let hay = format!("{} {} {}", e.topic, e.source, e.payload_preview).to_lowercase();
                hay.contains(&q)
            })
            .collect()
    }

    fn call_visible(&self) -> Vec<&CallEntry> {
        let q = self.filter.to_lowercase();
        self.call_log
            .iter()
            .filter(|e| {
                if let Some(o) = &self.owner_filter {
                    if &e.owner != o {
                        return false;
                    }
                }
                if q.is_empty() {
                    return true;
                }
                let hay = format!(
                    "{} {} {} {} {}",
                    e.owner, e.method, e.caller, e.params_preview, e.result_pretty
                )
                .to_lowercase();
                hay.contains(&q)
            })
            .collect()
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let nav = sidebar(vec![SidebarSection::unlabeled(vec![
            SidebarItem::new("Bus", Msg::Plane(Plane::Bus))
                .active(self.plane == Plane::Bus)
                .subtitle("Fan-out facts"),
            SidebarItem::new("Call", Msg::Plane(Plane::Call))
                .active(self.plane == Plane::Call)
                .subtitle(if self.call_up {
                    "Request / reply"
                } else {
                    "Host down"
                }),
        ])]);

        let toolbar = self.view_toolbar();
        let log = self.view_log();
        let inspector = self.view_inspector();
        let rail = self.view_rail();

        let log_col = column![
            log,
            horizontal_divider_drag_with(
                Msg::InspectorPress,
                DividerColors::from_theme(&self.theme),
            ),
            container(inspector)
                .width(Length::Fill)
                .height(Length::Fixed(self.inspector_h)),
        ]
        .height(Length::Fill);

        let work = row![
            container(log_col).width(Length::Fill).height(Length::Fill),
            vertical_divider_with(Msg::RailPress, DividerColors::from_theme(&self.theme),),
            container(rail)
                .width(Length::Fixed(self.rail_w))
                .height(Length::Fill),
        ]
        .height(Length::Fill);

        let main = column![toolbar, work].height(Length::Fill);
        let body: Element<'_, Msg> = row![nav, main]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let content = if self.dragging != Drag::None {
            let interaction = match self.dragging {
                Drag::Rail => mouse::Interaction::ResizingColumn,
                Drag::Inspector => mouse::Interaction::ResizingRow,
                Drag::None => mouse::Interaction::Idle,
            };
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill),)
                    .interaction(interaction),
            ]
            .into()
        } else {
            body
        };

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Monitor",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content,
        )
    }

    fn view_toolbar(&self) -> Element<'_, Msg> {
        let filter = text_input("Filter", &self.filter)
            .on_input(Msg::FilterChanged)
            .font(fonts::ui())
            .size(13)
            .width(Length::Fixed(FILTER_W));

        let picker = match self.plane {
            Plane::Bus => {
                let selected = self.topic_filter.clone().unwrap_or_default();
                let label = self
                    .topic_filter
                    .clone()
                    .unwrap_or_else(|| "All topics".into());
                let mut opts = vec![SelectOption::new(
                    "All topics",
                    self.topic_filter.is_none(),
                    Msg::TopicFilter(String::new()),
                )];
                for name in topic_names() {
                    let active = selected.as_str() == name;
                    opts.push(SelectOption::new(
                        name,
                        active,
                        Msg::TopicFilter(name.to_string()),
                    ));
                }
                select_sized(
                    label,
                    opts,
                    self.filter_open,
                    Msg::ToggleFilterOpen,
                    Msg::DismissFilter,
                    SELECT_W,
                )
            }
            Plane::Call => {
                let selected = self.owner_filter.clone().unwrap_or_default();
                let label = self
                    .owner_filter
                    .clone()
                    .unwrap_or_else(|| "All owners".into());
                let mut opts = vec![SelectOption::new(
                    "All owners",
                    self.owner_filter.is_none(),
                    Msg::OwnerFilter(String::new()),
                )];
                for o in &self.catalog {
                    let active = selected == o.owner;
                    opts.push(SelectOption::new(
                        o.owner.clone(),
                        active,
                        Msg::OwnerFilter(o.owner.clone()),
                    ));
                }
                select_sized(
                    label,
                    opts,
                    self.filter_open,
                    Msg::ToggleFilterOpen,
                    Msg::DismissFilter,
                    SELECT_W,
                )
            }
        };

        let pause = if self.paused {
            let n = self.pause_count();
            format!("Resume ({n})")
        } else {
            "Pause".into()
        };

        let count = match self.plane {
            Plane::Bus => self.bus_log.len(),
            Plane::Call => self.call_log.len(),
        };

        container(
            row![
                filter,
                container(picker).width(Length::Fixed(SELECT_W)),
                kit_text::caption(format!("{count}")).style(kit_text::muted),
                Space::new().width(Length::Fill),
                toolbar_button(pause).on_press(Msg::TogglePause),
                toolbar_button("Clear").on_press(Msg::Clear),
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center)
            .padding(Padding::from([SPACE_MD, SPACE_LG])),
        )
        .width(Length::Fill)
        .style(toolbar_style)
        .into()
    }

    fn view_log(&self) -> Element<'_, Msg> {
        match self.plane {
            Plane::Bus => self.view_bus_log(),
            Plane::Call => self.view_call_log(),
        }
    }

    fn view_bus_log(&self) -> Element<'_, Msg> {
        let header = log_header(vec![
            ("Time", TIME_COL_W),
            ("Topic", TOPIC_COL_W),
            ("Source", SOURCE_COL_W),
        ]);
        let rows = self.bus_visible();
        let mut list: Vec<Element<'_, Msg>> = Vec::new();
        if rows.is_empty() {
            list.push(empty_copy(if self.bus_log.is_empty() {
                "Waiting for bus traffic"
            } else {
                "No matching traffic"
            }));
        }
        for e in rows {
            let selected = self.selection == Selection::Bus(e.seq);
            let line = row![
                col_time(e.timestamp),
                col_text(&e.topic, TOPIC_COL_W),
                col_text(&e.source, SOURCE_COL_W),
                json_line(&e.payload_preview, &self.theme),
            ]
            .spacing(SPACE_XL)
            .align_y(Alignment::Center)
            .width(Length::Fill);
            list.push(log_row(line, selected, Msg::Select(Selection::Bus(e.seq))));
        }
        log_scroll(header, list)
    }

    fn view_call_log(&self) -> Element<'_, Msg> {
        let header = log_header(vec![
            ("Time", TIME_COL_W),
            ("Call", CALL_COL_W),
            ("Caller", CALLER_COL_W),
            ("Status", STATUS_COL_W),
        ]);
        let rows = self.call_visible();
        let mut list: Vec<Element<'_, Msg>> = Vec::new();
        if rows.is_empty() {
            let copy = if !self.call_up {
                "Call host is not running"
            } else if self.call_log.is_empty() {
                "No calls yet"
            } else {
                "No matching traffic"
            };
            list.push(empty_copy(copy));
        }
        for e in rows {
            let selected = self.selection == Selection::Call(e.key.clone());
            let call = if e.method.is_empty() {
                e.owner.clone()
            } else {
                format!("{}.{}", e.owner, e.method)
            };
            let mut status_cell = row![status_badge(e.status)].spacing(SPACE_SM);
            if let Some(ms) = e.duration_ms {
                status_cell =
                    status_cell.push(kit_text::caption(format!("{ms} ms")).style(kit_text::muted));
            }
            let line = row![
                col_time(e.timestamp),
                col_text(&call, CALL_COL_W),
                col_text(&e.caller, CALLER_COL_W),
                container(status_cell).width(Length::Fixed(STATUS_COL_W + 48.0)),
                json_line(&e.params_preview, &self.theme),
            ]
            .spacing(SPACE_XL)
            .align_y(Alignment::Center)
            .width(Length::Fill);
            list.push(log_row(
                line,
                selected,
                Msg::Select(Selection::Call(e.key.clone())),
            ));
        }
        log_scroll(header, list)
    }

    fn view_inspector(&self) -> Element<'_, Msg> {
        let (title, body): (String, Element<'_, Msg>) = match &self.selection {
            Selection::None => (
                "Inspector".into(),
                kit_text::caption("Select a row")
                    .style(kit_text::muted)
                    .into(),
            ),
            Selection::Bus(seq) => {
                let e = self.bus_log.iter().find(|e| e.seq == *seq);
                match e {
                    Some(e) => (
                        format!("{} · {}", e.topic, e.source),
                        json_pretty(&e.payload_pretty, &self.theme),
                    ),
                    None => (
                        "Inspector".into(),
                        kit_text::caption("Message dropped from the buffer")
                            .style(kit_text::muted)
                            .into(),
                    ),
                }
            }
            Selection::Sticky(topic) => match self.sticky.get(topic) {
                Some(e) => (
                    format!("{} · last known", e.topic),
                    json_pretty(&e.payload_pretty, &self.theme),
                ),
                None => (
                    "Inspector".into(),
                    kit_text::caption("No sticky").style(kit_text::muted).into(),
                ),
            },
            Selection::Call(key) => {
                let e = self
                    .call_log
                    .iter()
                    .chain(self.call_pause.iter())
                    .find(|e| e.key == *key);
                match e {
                    Some(e) => (
                        call_title(e),
                        json_pretty(&call_inspect_json(e), &self.theme),
                    ),
                    None => (
                        "Inspector".into(),
                        kit_text::caption("Call dropped from the buffer")
                            .style(kit_text::muted)
                            .into(),
                    ),
                }
            }
            Selection::Catalog { owner, method } => {
                let pretty = catalog_pretty(&self.catalog, owner, method.as_deref());
                let title = match method {
                    Some(m) => format!("{owner}.{m}"),
                    None => owner.clone(),
                };
                (title, json_pretty(&pretty, &self.theme))
            }
        };

        column![
            container(kit_text::caption(title).style(kit_text::muted))
                .padding(Padding::from([SPACE_SM, SPACE_LG]))
                .width(Length::Fill)
                .style(header_style),
            scrollable(container(body).padding(Padding::from([SPACE_MD, SPACE_LG])))
                .height(Length::Fill)
                .width(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    fn view_rail(&self) -> Element<'_, Msg> {
        match self.plane {
            Plane::Bus => {
                let items: Vec<_> = self
                    .sticky
                    .values()
                    .map(|e| {
                        let sel = matches!(&self.selection, Selection::Sticky(t) if t == &e.topic);
                        SidebarItem::new(
                            e.topic.clone(),
                            Msg::Select(Selection::Sticky(e.topic.clone())),
                        )
                        .subtitle(e.source.clone())
                        .active(sel)
                        .id(e.topic.clone())
                    })
                    .collect();
                let empty = items.is_empty();
                let section = SidebarSection::new("Last known", items).fill();
                if empty {
                    column![
                        rail_heading("Last known"),
                        empty_copy("No sticky topics yet"),
                    ]
                    .into()
                } else {
                    SidebarPanel::new(vec![section]).fill_width().build()
                }
            }
            Plane::Call => {
                if !self.call_up {
                    return column![
                        rail_heading("Owners"),
                        empty_copy("Call host is not running"),
                    ]
                    .into();
                }
                let mut items = Vec::new();
                for o in &self.catalog {
                    for m in &o.methods {
                        let id = format!("{}.{}", o.owner, m.name);
                        let sel = matches!(
                            &self.selection,
                            Selection::Catalog { owner, method }
                                if owner == &o.owner && method.as_deref() == Some(m.name.as_str())
                        );
                        let mut item = SidebarItem::new(
                            id.clone(),
                            Msg::Select(Selection::Catalog {
                                owner: o.owner.clone(),
                                method: Some(m.name.clone()),
                            }),
                        )
                        .active(sel)
                        .id(id);
                        if !m.summary.is_empty() {
                            item = item.subtitle(m.summary.clone());
                        }
                        items.push(item);
                    }
                }
                if items.is_empty() {
                    column![rail_heading("Owners"), empty_copy("No owners advertised"),].into()
                } else {
                    SidebarPanel::new(vec![SidebarSection::new("Owners", items).fill()])
                        .fill_width()
                        .build()
                }
            }
        }
    }
}

impl BusEntry {
    fn from_message(msg: &Message, seq: u64) -> Self {
        let kind = TopicKind::from_str(&msg.topic);
        let is_sticky = kind.map(|k| k.behavior().is_sticky()).unwrap_or(false);
        let (payload_preview, payload_pretty) = match Topic::parse(msg) {
            Some(topic) => json_pair(&topic.to_json_value()),
            None => match &msg.payload {
                None => (String::new(), String::new()),
                Some(bytes) if bytes.is_empty() => (String::new(), String::new()),
                Some(bytes) => {
                    let s = format!("<{} bytes, unparsed>", bytes.len());
                    (s.clone(), s)
                }
            },
        };
        Self {
            seq,
            timestamp: now_secs(),
            topic: msg.topic.clone(),
            source: msg.source.clone(),
            payload_preview,
            payload_pretty,
            is_sticky,
        }
    }
}

fn trim<T>(v: &mut Vec<T>, max: usize) {
    if v.len() > max {
        let drop = v.len() - max;
        v.drain(0..drop);
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn json_pair(v: &serde_json::Value) -> (String, String) {
    if v.is_null() {
        return (String::new(), String::new());
    }
    let compact = v.to_string();
    let preview = truncate_preview(&compact, 240);
    let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
    (preview, pretty)
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn reply_pretty(tr: &TraceEvent) -> String {
    let mut obj = serde_json::Map::new();
    if let Some(ok) = tr.ok {
        obj.insert("ok".into(), serde_json::Value::Bool(ok));
    }
    if let Some(err) = &tr.error {
        obj.insert("error".into(), serde_json::Value::String(err.clone()));
    }
    if let Some(data) = &tr.data {
        obj.insert("data".into(), data.clone());
    }
    if let Some(ms) = tr.duration_ms {
        obj.insert("duration_ms".into(), serde_json::json!(ms));
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default()
}

fn call_inspect_json(e: &CallEntry) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), serde_json::Value::String(e.key.clone()));
    obj.insert("owner".into(), serde_json::Value::String(e.owner.clone()));
    obj.insert("method".into(), serde_json::Value::String(e.method.clone()));
    if !e.caller.is_empty() {
        obj.insert("caller".into(), serde_json::Value::String(e.caller.clone()));
    }
    obj.insert(
        "status".into(),
        serde_json::Value::String(
            match e.status {
                CallStatus::Pending => "pending",
                CallStatus::Ok => "ok",
                CallStatus::Error => "error",
                CallStatus::Timeout => "timeout",
                CallStatus::Up => "up",
                CallStatus::Down => "down",
            }
            .into(),
        ),
    );
    if let Some(ms) = e.duration_ms {
        obj.insert("duration_ms".into(), serde_json::json!(ms));
    }
    if !e.params_pretty.is_empty() {
        if let Ok(v) = serde_json::from_str(&e.params_pretty) {
            obj.insert("params".into(), v);
        }
    }
    if !e.result_pretty.is_empty() {
        if let Ok(v) = serde_json::from_str(&e.result_pretty) {
            obj.insert("result".into(), v);
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default()
}

fn call_title(e: &CallEntry) -> String {
    let call = if e.method.is_empty() {
        e.owner.clone()
    } else {
        format!("{}.{}", e.owner, e.method)
    };
    match e.duration_ms {
        Some(ms) => format!("{call} · {ms} ms"),
        None => call,
    }
}

fn catalog_pretty(catalog: &[OwnerCatalog], owner: &str, method: Option<&str>) -> String {
    let Some(o) = catalog.iter().find(|o| o.owner == owner) else {
        return String::new();
    };
    if let Some(name) = method {
        if let Some(m) = o.methods.iter().find(|m| m.name == name) {
            return serde_json::to_string_pretty(m).unwrap_or_default();
        }
    }
    serde_json::to_string_pretty(o).unwrap_or_default()
}

fn topic_names() -> Vec<&'static str> {
    let mut v: Vec<_> = TopicKind::ALL.iter().map(|k| k.as_str()).collect();
    v.sort();
    v
}

fn format_clock(unix_secs: f64) -> String {
    let total_ms = (unix_secs * 1000.0) as i64;
    let seconds_today = (total_ms / 1000).rem_euclid(86400);
    let ms = total_ms.rem_euclid(1000);
    let h = seconds_today / 3600;
    let m = (seconds_today % 3600) / 60;
    let s = seconds_today % 60;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

fn col_time<'a>(ts: f64) -> Element<'a, Msg> {
    text(format_clock(ts))
        .font(fonts::mono())
        .size(12)
        .style(kit_text::muted)
        .width(Length::Fixed(TIME_COL_W))
        .into()
}

fn col_text<'a>(s: &str, w: f32) -> Element<'a, Msg> {
    text(s.to_string())
        .font(fonts::ui())
        .size(13)
        .width(Length::Fixed(w))
        .wrapping(Wrapping::None)
        .into()
}

fn log_header<'a>(cols: Vec<(&'static str, f32)>) -> Element<'a, Msg> {
    let mut cells: Vec<Element<'a, Msg>> = cols
        .into_iter()
        .map(|(label, w)| {
            kit_text::caption(label)
                .style(kit_text::muted)
                .width(Length::Fixed(w))
                .into()
        })
        .collect();
    cells.push(
        kit_text::caption("Payload")
            .style(kit_text::muted)
            .width(Length::Fill)
            .into(),
    );
    container(row(cells).spacing(SPACE_XL))
        .padding(Padding::from([SPACE_SM, SPACE_LG]))
        .style(header_style)
        .width(Length::Fill)
        .into()
}

fn log_row<'a>(
    line: impl Into<Element<'a, Msg>>,
    selected: bool,
    on_press: Msg,
) -> Element<'a, Msg> {
    button(
        container(line.into())
            .padding(Padding {
                top: SPACE_XS + 2.0,
                bottom: SPACE_XS + 2.0,
                left: SPACE_LG,
                right: SPACE_LG,
            })
            .width(Length::Fill),
    )
    .on_press(on_press)
    .padding(0)
    .width(Length::Fill)
    .style(kit_button::list_item(selected))
    .into()
}

fn log_scroll<'a>(header: Element<'a, Msg>, rows: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let body = scrollable(column(rows).spacing(0).width(Length::Fill))
        .id(log_scroll_id())
        .on_scroll(Msg::LogScrolled)
        .height(Length::Fill)
        .width(Length::Fill);
    column![header, body].height(Length::Fill).into()
}

fn empty_copy<'a>(s: &'static str) -> Element<'a, Msg> {
    container(kit_text::caption(s).style(kit_text::muted))
        .padding(Padding::from([SPACE_XL, SPACE_LG]))
        .width(Length::Fill)
        .into()
}

fn rail_heading<'a>(s: &'static str) -> Element<'a, Msg> {
    container(kit_text::caption(s).style(kit_text::muted))
        .padding(Padding::from([SPACE_SM, SPACE_LG]))
        .width(Length::Fill)
        .style(header_style)
        .into()
}

fn status_badge<'a>(status: CallStatus) -> Element<'a, Msg> {
    let (label, tone) = match status {
        CallStatus::Pending => ("pending", Tone::Neutral),
        CallStatus::Ok => ("ok", Tone::Success),
        CallStatus::Error => ("error", Tone::Danger),
        CallStatus::Timeout => ("timeout", Tone::Warning),
        CallStatus::Up => ("up", Tone::Accent),
        CallStatus::Down => ("down", Tone::Neutral),
    };
    badge(label, tone)
}

fn toolbar_style(t: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(
            t.extended_palette().background.weaker.color,
        )),
        ..container::Style::default()
    }
}

fn header_style(t: &Theme) -> container::Style {
    let p = t.extended_palette();
    container::Style {
        background: Some(iced::Background::Color(p.background.weaker.color)),
        text_color: Some(p.secondary.base.text),
        ..container::Style::default()
    }
}
