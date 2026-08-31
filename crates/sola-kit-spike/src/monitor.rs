//! HTML/CSS Monitor (lab). Distinct from iced `sola-monitor`.
//!
//! Identity: `sola-monitor-lab` / `Monitor (lab)`. Same planes as iced:
//! bus fan-out + call observer (`Role::Observer` + `Wire::Trace`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
    Window as BusWindow, WindowFloating,
};
use sola_bus::{BusClient, Message};
use sola_call::{ObserveEvent, OwnerCatalog, TraceEvent, TraceKind};
use sola_core::KeyCode;

use crate::app::Click;
use crate::components::button::Kind as Btn;
use crate::components::{
    Sidebar, SidebarItem, badge, button, field, json_pretty, json_preview, pane, select, split,
    text, titlebar, toolbar,
};
use crate::css::{Sheet, parse_sheet};
use crate::dom::{Elem, parse_html};
use crate::gpu::Quad;
use crate::host::Surface;
use crate::icons::Icons;
use crate::layout::{
    PaintItem, append_scrollbars, apply_pointer_hover, hit_test, hover_at, layout_tree,
    point_in_item, scrollbar_thumb,
};
use crate::markup::{self};
use crate::paint::{Fonts, PaintPass, paint_glyphs};

pub const APP_ID: &str = "sola-monitor-lab";
pub const WINDOW_TITLE: &str = "Monitor (lab)";

const CSS: &str = include_str!("../assets/kit.css");
const HTML: &str = include_str!("../assets/monitor.html");

const MAX_MESSAGES: usize = 5_000;
const SIDEBAR_W: f32 = 220.0;
const TITLEBAR_H: f32 = 38.0;
const TOOLBAR_H: f32 = 44.0;
const HEAD_H: f32 = 28.0;
const ROW_H: f32 = 28.0;
const RULE: f32 = 8.0;
const COL_TIME: f32 = 96.0;
const COL_TOPIC: f32 = 168.0;
const COL_SOURCE: f32 = 112.0;
const COL_CALL: f32 = 220.0;
const COL_CALLER: f32 = 120.0;
const COL_STATUS: f32 = 88.0;
const COL_GAP: f32 = 16.0;
const LOG_PAD_X: f32 = 32.0;
const RAIL_W_DEFAULT: f32 = 260.0;
const RAIL_W_MIN: f32 = 160.0;
const RAIL_W_MAX: f32 = 560.0;
const INSPECTOR_H_DEFAULT: f32 = 220.0;
const INSPECTOR_H_MIN: f32 = 96.0;
const INSPECTOR_H_MAX: f32 = 640.0;

#[derive(Clone, Copy)]
struct Metrics {
    main_w: f32,
    rail_w: f32,
    log_h: f32,
    inspect_h: f32,
    payload_w: f32,
}

fn metrics(
    css_w: f32,
    css_h: f32,
    floating: bool,
    plane: Plane,
    rail_w: f32,
    inspector_h: f32,
) -> Metrics {
    let stage_h = if floating {
        (css_h - TITLEBAR_H).max(200.0)
    } else {
        css_h.max(200.0)
    };
    let stage_w = (css_w - SIDEBAR_W).max(400.0);
    let work_h = (stage_h - TOOLBAR_H).max(160.0);
    let rail_w = rail_w
        .clamp(RAIL_W_MIN, RAIL_W_MAX)
        .min((stage_w * 0.45).max(RAIL_W_MIN));
    let main_w = (stage_w - rail_w - RULE).max(280.0);
    let inspect_h = inspector_h
        .clamp(INSPECTOR_H_MIN, INSPECTOR_H_MAX)
        .min((work_h * 0.55).max(INSPECTOR_H_MIN));
    let log_h = (work_h - inspect_h - RULE).max(80.0);
    let cols = match plane {
        Plane::Bus => COL_TIME + COL_TOPIC + COL_SOURCE + COL_GAP * 3.0,
        Plane::Call => COL_TIME + COL_CALL + COL_CALLER + COL_STATUS + COL_GAP * 4.0,
    };
    let payload_w = (main_w - cols - LOG_PAD_X).max(80.0);
    Metrics {
        main_w,
        rail_w,
        log_h,
        inspect_h,
        payload_w,
    }
}

fn apply_sizes(root: &mut Elem, m: Metrics) {
    let h = m.inspect_h.round();
    let w = m.rail_w.round();
    markup::set_style(
        root,
        "inspect-pane",
        &format!("height:{h}px;min-height:{h}px;max-height:{h}px;flex-grow:0;flex-shrink:0"),
    );
    markup::set_style(
        root,
        "rail-pane",
        &format!(
            "width:{w}px;min-width:{w}px;max-width:{w}px;height:100%;flex-grow:0;flex-shrink:0"
        ),
    );
}

fn box_overlaps(x: f32, y: f32, w: f32, h: f32, pane: (f32, f32, f32, f32)) -> bool {
    x < pane.0 + pane.2 && x + w > pane.0 && y < pane.1 + pane.3 && y + h > pane.1
}

fn is_log_chrome(item: &crate::layout::PaintItem) -> bool {
    item.classes
        .iter()
        .any(|c| matches!(c.as_str(), "log-row" | "log-spacer" | "log-empty"))
}

fn is_inspect_chrome(item: &crate::layout::PaintItem) -> bool {
    item.classes.iter().any(|c| {
        matches!(
            c.as_str(),
            "json-line"
                | "inspect-head"
                | "pane-head"
                | "inspect-body"
                | "inspect"
                | "split-rule"
                | "split-rule-h"
                | "split-line"
                | "split-line-h"
        )
    }) || matches!(
        item.data_id.as_deref(),
        Some("inspect-scroll" | "inspect-pane" | "inspect-head" | "rule-h" | "rule-v")
    )
}

fn clip_to_pane(items: &mut [crate::layout::PaintItem], id: &str) {
    let Some(pane) = items
        .iter()
        .find(|i| i.data_id.as_deref() == Some(id) && i.overflow_scroll)
        .or_else(|| items.iter().find(|i| i.data_id.as_deref() == Some(id)))
        .map(|i| (i.x, i.y, i.w, i.h))
    else {
        return;
    };
    let inspect_pane = items
        .iter()
        .find(|i| i.data_id.as_deref() == Some("inspect-pane"))
        .map(|i| (i.x, i.y, i.w, i.h));
    let mut leaked_rows: Vec<(f32, f32, f32, f32)> = Vec::new();
    for item in items.iter_mut() {
        if item.z >= 8 {
            continue;
        }
        if id == "log-scroll"
            && (is_inspect_chrome(item)
                || item.data_id.as_deref() == Some("log-head")
                || item.classes.iter().any(|c| c == "log-head")
                || item.y + 1.0 < pane.1)
        {
            continue;
        }
        if id == "log-scroll"
            && inspect_pane.is_some_and(|p| box_overlaps(item.x, item.y, item.w, item.h, p))
            && !is_log_chrome(item)
        {
            continue;
        }
        let overlaps = box_overlaps(item.x, item.y, item.w, item.h, pane);
        if overlaps {
            if let Some(c) =
                crate::layout::intersect_clip(item.clip, pane.0, pane.1, pane.2, pane.3)
            {
                item.clip = Some(c);
            }
            continue;
        }
        if id == "log-scroll" && is_log_chrome(item) {
            leaked_rows.push((item.x, item.y, item.w, item.h));
            item.hidden = true;
        }
    }
    if id != "log-scroll" || leaked_rows.is_empty() {
        return;
    }
    for item in items.iter_mut() {
        if item.hidden || item.z >= 8 || is_inspect_chrome(item) {
            continue;
        }
        if inspect_pane.is_some_and(|p| box_overlaps(item.x, item.y, item.w, item.h, p))
            && !is_log_chrome(item)
        {
            continue;
        }
        if leaked_rows
            .iter()
            .any(|r| box_overlaps(item.x, item.y, item.w, item.h, *r))
        {
            item.hidden = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Plane {
    Bus,
    Call,
}

#[derive(Clone, PartialEq, Eq)]
enum Selection {
    None,
    Bus(u64),
    Sticky(String),
    Call(String),
    Catalog { owner: String, method: String },
}

#[derive(Clone, PartialEq)]
enum Drag {
    None,
    Split,
    SplitH,
    Scroll {
        id: String,
        origin_y: f32,
        origin_scroll: f32,
        max: f32,
        track_h: f32,
        thumb_h: f32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

pub struct Monitor {
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
    plane: Plane,
    bus_log: Vec<BusEntry>,
    bus_pause: Vec<BusEntry>,
    sticky: BTreeMap<String, BusEntry>,
    call_log: Vec<CallEntry>,
    call_pause: Vec<CallEntry>,
    catalog: Vec<OwnerCatalog>,
    call_up: bool,
    topic_filter: Option<String>,
    owner_filter: Option<String>,
    select_open: bool,
    paused: bool,
    follow: bool,
    selection: Selection,
    rail_w: f32,
    inspector_h: f32,
    drag: Drag,
    next_seq: u64,
    bus: BusClient,
    observe: Receiver<ObserveEvent>,
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
    last_ui: Instant,
    log_dirty: bool,
    last_metrics: Option<Metrics>,
    layout_dirty: bool,
    hover_dirty: bool,
}

pub fn run() {
    crate::host::run_with(
        APP_ID,
        WINDOW_TITLE,
        Box::new(Monitor::new(1100.0, 720.0, 1.0)),
    );
}

impl Monitor {
    fn new(css_w: f32, css_h: f32, scale: f32) -> Self {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let css_path = assets.join("kit.css");
        let html_path = assets.join("monitor.html");
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
            menus: vec![MenuDefinition {
                label: "Monitor".into(),
                items: vec![MenuItem::Action {
                    id: "quit".into(),
                    label: "Quit Monitor".into(),
                    shortcut: Some(KeyCode::Q.meta()),
                    disabled: false,
                    checked: false,
                }],
            }],
        }));
        let observe = sola_call::start_observer(APP_ID);
        tracing::info!(app_id = APP_ID, "bus connected; call observer started");
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
            fields: HashMap::from([("filter".into(), String::new())]),
            time: 0.0,
            plane: Plane::Bus,
            bus_log: Vec::new(),
            bus_pause: Vec::new(),
            sticky: BTreeMap::new(),
            call_log: Vec::new(),
            call_pause: Vec::new(),
            catalog: Vec::new(),
            call_up: false,
            topic_filter: None,
            owner_filter: None,
            select_open: false,
            paused: false,
            follow: true,
            selection: Selection::None,
            rail_w: RAIL_W_DEFAULT,
            inspector_h: INSPECTOR_H_DEFAULT,
            drag: Drag::None,
            next_seq: 0,
            bus,
            observe,
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
            last_ui: Instant::now(),
            log_dirty: false,
            last_metrics: None,
            layout_dirty: true,
            hover_dirty: false,
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

    fn begin_scroll_drag(&mut self, id: Option<&str>, y: f32, jump_track: bool) {
        let Some(id) = id else {
            return;
        };
        let Some(pane) = self
            .last_items
            .iter()
            .find(|i| i.data_id.as_deref() == Some(id) && i.overflow_scroll)
        else {
            return;
        };
        let max = (pane.content_h - pane.h).max(0.0);
        let Some((_, thumb_h, _)) = scrollbar_thumb(pane.h, pane.content_h, 0.0) else {
            return;
        };
        let track_h = pane.h;
        let mut scroll = if id == "log-scroll" && self.follow && !self.paused {
            max
        } else {
            self.scrolls.get(id).copied().unwrap_or(0.0)
        };
        if jump_track && max > 0.5 {
            let travel = (track_h - thumb_h).max(1.0);
            let t = ((y - pane.y - thumb_h * 0.5) / travel).clamp(0.0, 1.0);
            scroll = t * max;
            self.scrolls.insert(id.to_string(), scroll);
            if id == "log-scroll" {
                self.follow = scroll >= max - 0.5;
            }
        }
        self.drag = Drag::Scroll {
            id: id.to_string(),
            origin_y: y,
            origin_scroll: scroll,
            max,
            track_h,
            thumb_h,
        };
    }

    fn pointer_in_log(&self, x: f32, y: f32) -> bool {
        self.last_items.iter().any(|i| {
            (i.data_id.as_deref() == Some("log-scroll") || i.classes.iter().any(|c| c == "log-row"))
                && point_in_item(i, x, y)
        })
    }

    fn hit_id(&self, id: &str, x: f32, y: f32) -> bool {
        self.last_items
            .iter()
            .any(|i| i.data_id.as_deref() == Some(id) && point_in_item(i, x, y))
    }

    fn chrome_top(&self) -> f32 {
        if self.is_floating() { TITLEBAR_H } else { 0.0 }
    }

    fn log_view_h(&self) -> f32 {
        self.last_items
            .iter()
            .find(|i| i.data_id.as_deref() == Some("log-scroll"))
            .map(|i| i.h)
            .or_else(|| self.last_metrics.map(|m| (m.log_h - HEAD_H).max(40.0)))
            .unwrap_or(200.0)
    }

    fn scroll_log(&mut self, dy: f32, max: f32) -> bool {
        let cur = if self.follow && !self.paused {
            max
        } else {
            self.scrolls.get("log-scroll").copied().unwrap_or(0.0)
        };
        let next = (cur + dy).clamp(0.0, max);
        if (next - cur).abs() < 0.01 {
            return false;
        }
        self.follow = max <= 0.5 || next >= max - 0.5;
        self.scrolls.insert("log-scroll".into(), next);
        self.bump_layout();
        true
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

    /// `true` = chrome (theme/float) needs an immediate redraw.
    fn handle_bus(&mut self, msg: Message) -> bool {
        self.push_bus(&msg);
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
                self.push_call(CallEntry {
                    key,
                    timestamp: now_secs(),
                    owner: tr.owner.unwrap_or_default(),
                    method: tr.method.unwrap_or_default(),
                    caller: tr.caller.unwrap_or_default(),
                    status: CallStatus::Pending,
                    duration_ms: None,
                    params_pretty: pretty,
                    params_preview: preview,
                    result_pretty: String::new(),
                });
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

    fn filter_q(&self) -> String {
        self.fields
            .get("filter")
            .map(|s| s.to_lowercase())
            .unwrap_or_default()
    }

    fn bus_visible(&self) -> Vec<&BusEntry> {
        let q = self.filter_q();
        self.bus_log
            .iter()
            .filter(|e| {
                if self.topic_filter.as_ref().is_some_and(|t| t != &e.topic) {
                    return false;
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
        let q = self.filter_q();
        self.call_log
            .iter()
            .filter(|e| {
                if self.owner_filter.as_ref().is_some_and(|o| o != &e.owner) {
                    return false;
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

    fn toggle_pause(&mut self) {
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
        }
    }

    fn clear_plane(&mut self) {
        match self.plane {
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
        }
    }

    fn set_selection(&mut self, sel: Selection) {
        self.selection = if self.selection == sel {
            Selection::None
        } else {
            sel
        };
    }

    fn sync_fields(&mut self) {
        let (select_label, count) = match self.plane {
            Plane::Bus => (
                self.topic_filter
                    .clone()
                    .unwrap_or_else(|| "All topics".into()),
                self.bus_log.len(),
            ),
            Plane::Call => (
                self.owner_filter
                    .clone()
                    .unwrap_or_else(|| "All owners".into()),
                self.call_log.len(),
            ),
        };
        self.fields.insert("select-label".into(), select_label);
        self.fields.insert("count".into(), format!("{count}"));
        let pause = if self.paused {
            format!("Resume ({})", self.pause_count())
        } else {
            "Pause".into()
        };
        self.fields.insert("pause-label".into(), pause);
        let inspect = match &self.selection {
            Selection::None => "Inspector".to_string(),
            Selection::Bus(seq) => self
                .bus_log
                .iter()
                .find(|e| e.seq == *seq)
                .map(|e| format!("{} · {}", e.topic, e.source))
                .unwrap_or_else(|| "Inspector".into()),
            Selection::Sticky(topic) => format!("{topic} · last known"),
            Selection::Call(key) => self
                .call_log
                .iter()
                .chain(self.call_pause.iter())
                .find(|e| e.key == *key)
                .map(call_title)
                .unwrap_or_else(|| "Inspector".into()),
            Selection::Catalog { owner, method } => format!("{owner}.{method}"),
        };
        self.fields.insert("inspect-title".into(), inspect);
    }

    fn fill_sidebar(&self, root: &mut Elem) {
        let mut next = markup::next_uid(root);
        let call_sub = if self.call_up {
            "Request / reply"
        } else {
            "Host down"
        };
        let sb = Sidebar::new([
            SidebarItem::new("bus", "Bus")
                .action("plane")
                .subtitle("Fan-out facts")
                .active(self.plane == Plane::Bus),
            SidebarItem::new("call", "Call")
                .action("plane")
                .subtitle(call_sub)
                .active(self.plane == Plane::Call),
        ])
        .data_id("sidebar")
        .build(&mut next);
        markup::replace_slot(root, "sidebar", sb);
    }

    fn rebuild(&mut self) {
        self.sync_fields();
        let mut root = self.html_root.clone();
        self.fill_chrome(&mut root);
        if !self.is_floating() {
            markup::hide_if(&mut root, |el| {
                el.data_id.as_deref() == Some("csd") || el.classes.iter().any(|c| c == "titlebar")
            });
        }
        self.fill_sidebar(&mut root);
        let m = metrics(
            self.css_w,
            self.css_h,
            self.is_floating(),
            self.plane,
            self.rail_w,
            self.inspector_h,
        );
        self.last_metrics = Some(m);
        self.fill_toolbar(&mut root);
        self.fill_select(&mut root);
        self.fill_log(&mut root, m);
        self.fill_inspect(&mut root);
        self.fill_rail(&mut root);
        if !self.select_open {
            markup::hide_slot(&mut root, "select-menu", true);
        }
        markup::walk_mut(&mut root, &mut |el| {
            if self.is_floating() && el.classes.iter().any(|c| c == "app") {
                markup::add_class(el, "is-float");
            }
            if el.data_action.as_deref() == Some("select-toggle") && self.select_open {
                markup::add_class(el, "is-open");
            }
        });
        apply_sizes(&mut root, m);
        markup::apply_fields(&mut root, &self.fields);
        markup::apply_placeholder(
            &mut root,
            "filter",
            field_empty(&self.fields, "filter"),
            "Filter",
        );
        markup::apply_focus(&mut root, self.focused.as_deref());
        self.last_items = layout_tree(
            &root,
            &self.sheet,
            None,
            self.css_w,
            self.css_h,
            &mut self.fonts,
            &self.scrolls,
        );
        if self.snap_follow() {
            self.last_items = layout_tree(
                &root,
                &self.sheet,
                None,
                self.css_w,
                self.css_h,
                &mut self.fonts,
                &self.scrolls,
            );
        }
        clip_to_pane(&mut self.last_items, "log-scroll");
        clip_to_pane(&mut self.last_items, "inspect-scroll");
        clip_to_pane(&mut self.last_items, "rail-scroll");
        self.patch_log_content_h();
        apply_pointer_hover(&mut self.last_items, self.hover);
        append_scrollbars(&mut self.last_items, &self.scrolls);
        apply_pointer_hover(&mut self.last_items, self.hover);
    }

    fn patch_log_content_h(&mut self) {
        let Some(m) = self.last_metrics else {
            return;
        };
        let view_h = self
            .last_items
            .iter()
            .find(|i| i.data_id.as_deref() == Some("log-scroll"))
            .map(|i| i.h)
            .unwrap_or_else(|| (m.log_h - HEAD_H).max(40.0));
        let n = match self.plane {
            Plane::Bus => self.bus_visible().len(),
            Plane::Call => self.call_visible().len(),
        };
        let content = (n as f32 * ROW_H).max(view_h);
        if let Some(item) = self
            .last_items
            .iter_mut()
            .find(|i| i.data_id.as_deref() == Some("log-scroll"))
        {
            item.content_h = content;
            item.overflow_scroll = true;
        }
    }

    fn snap_follow(&mut self) -> bool {
        if !self.follow || self.paused {
            return false;
        }
        let Some(item) = self
            .last_items
            .iter()
            .find(|i| i.data_id.as_deref() == Some("log-scroll"))
        else {
            return false;
        };
        let max = (item.content_h - item.h).max(0.0);
        let cur = self.scrolls.get("log-scroll").copied().unwrap_or(0.0);
        if (max - cur).abs() < 0.5 {
            return false;
        }
        self.scrolls.insert("log-scroll".into(), max);
        true
    }

    fn fill_chrome(&self, root: &mut Elem) {
        let mut next = markup::next_uid(root);
        markup::replace_slot(root, "titlebar", titlebar(&mut next, WINDOW_TITLE));
        markup::replace_slot(
            root,
            "split-h",
            split::horizontal(&mut next, "split-drag-h", "rule-h"),
        );
        markup::replace_slot(
            root,
            "split-v",
            split::vertical(&mut next, "split-drag", "rule-v"),
        );
        markup::replace_slot(
            root,
            "inspect-head",
            pane::head(&mut next, "inspect-head", "inspect-title", "Inspector"),
        );
    }

    fn fill_toolbar(&self, root: &mut Elem) {
        let mut next = 12_000u32;
        let mut filter = field::input(&mut next, "filter");
        markup::add_class(&mut filter, "monitor-filter");
        filter.style_attr =
            Some("width:220px;min-width:220px;max-width:220px;flex-grow:0;flex-shrink:0".into());
        let mut sel = select::select(
            &mut next,
            "filter-select",
            Some("monitor-select"),
            "select-label",
        );
        sel.style_attr =
            Some("width:180px;min-width:180px;max-width:180px;flex-grow:0;flex-shrink:0".into());
        let mut count = text::bind_caption(&mut next, "count");
        markup::add_class(&mut count, "toolbar-count");
        let mut pause = button(&mut next, Btn::Toolbar, false, "pause", None, "Pause");
        pause.data_bind = Some("pause-label".into());
        let clear = button(&mut next, Btn::Toolbar, false, "clear", None, "Clear");
        let spacer = markup::node(&mut next, &["spacer"], None, None, "");
        markup::replace_slot(
            root,
            "toolbar",
            toolbar::bar(
                &mut next,
                "monitor-toolbar",
                &["monitor-toolbar"],
                vec![filter, sel, count, spacer, pause, clear],
            ),
        );
    }

    fn fill_select(&self, root: &mut Elem) {
        let mut next = 20_000u32;
        let mut items = Vec::new();
        match self.plane {
            Plane::Bus => {
                items.push(select::menu_item(
                    &mut next,
                    "pick-filter",
                    "*",
                    "All topics",
                    self.topic_filter.is_none(),
                ));
                for name in topic_names() {
                    let active = self.topic_filter.as_deref() == Some(name);
                    items.push(select::menu_item(
                        &mut next,
                        "pick-filter",
                        name,
                        name,
                        active,
                    ));
                }
            }
            Plane::Call => {
                items.push(select::menu_item(
                    &mut next,
                    "pick-filter",
                    "*",
                    "All owners",
                    self.owner_filter.is_none(),
                ));
                for o in &self.catalog {
                    let active = self.owner_filter.as_deref() == Some(o.owner.as_str());
                    items.push(select::menu_item(
                        &mut next,
                        "pick-filter",
                        &o.owner,
                        &o.owner,
                        active,
                    ));
                }
            }
        }
        markup::fill_slot(root, "select-menu", items);
    }

    fn fill_log(&mut self, root: &mut Elem, m: Metrics) {
        let mut next = 30_000u32;
        let view_h = (m.log_h - HEAD_H).max(40.0);
        match self.plane {
            Plane::Bus => {
                markup::fill_slot(
                    root,
                    "log-head",
                    pane::column_labels(
                        &mut next,
                        &[
                            ("col-time", "Time"),
                            ("col-topic", "Topic"),
                            ("col-source", "Source"),
                            ("col-payload", "Payload"),
                        ],
                    ),
                );
                markup::set_style(
                    root,
                    "log-head",
                    "height:28px;min-height:28px;max-height:28px;flex-grow:0;flex-shrink:0",
                );
                let vis = self.bus_visible();
                let n = vis.len();
                let (start, end, top, bot, scroll) = self.virt_window(n, view_h);
                let mut rows = Vec::new();
                if n == 0 {
                    rows.push(empty_row(
                        &mut next,
                        if self.bus_log.is_empty() {
                            "Waiting for bus traffic"
                        } else {
                            "No matching traffic"
                        },
                    ));
                } else {
                    if top > 0.5 {
                        rows.push(log_spacer(&mut next, top));
                    }
                    for e in &vis[start..end] {
                        let selected = self.selection == Selection::Bus(e.seq);
                        let id = e.seq.to_string();
                        rows.push(bus_row(&mut next, e, selected, &id, m));
                    }
                    if bot > 0.5 {
                        rows.push(log_spacer(&mut next, bot));
                    }
                }
                markup::fill_slot(root, "log-rows", rows);
                self.scrolls.insert("log-scroll".into(), scroll);
            }
            Plane::Call => {
                markup::fill_slot(
                    root,
                    "log-head",
                    pane::column_labels(
                        &mut next,
                        &[
                            ("col-time", "Time"),
                            ("col-call", "Call"),
                            ("col-caller", "Caller"),
                            ("col-status", "Status"),
                            ("col-payload", "Payload"),
                        ],
                    ),
                );
                markup::set_style(
                    root,
                    "log-head",
                    "height:28px;min-height:28px;max-height:28px;flex-grow:0;flex-shrink:0",
                );
                let vis = self.call_visible();
                let n = vis.len();
                let (start, end, top, bot, scroll) = self.virt_window(n, view_h);
                let mut rows = Vec::new();
                if n == 0 {
                    let copy = if !self.call_up {
                        "Call host is not running"
                    } else if self.call_log.is_empty() {
                        "No calls yet"
                    } else {
                        "No matching traffic"
                    };
                    rows.push(empty_row(&mut next, copy));
                } else {
                    if top > 0.5 {
                        rows.push(log_spacer(&mut next, top));
                    }
                    for e in &vis[start..end] {
                        let selected = matches!(&self.selection, Selection::Call(k) if k == &e.key);
                        rows.push(call_row(&mut next, e, selected, m));
                    }
                    if bot > 0.5 {
                        rows.push(log_spacer(&mut next, bot));
                    }
                }
                markup::fill_slot(root, "log-rows", rows);
                self.scrolls.insert("log-scroll".into(), scroll);
            }
        }
    }

    fn virt_window(&self, n: usize, view_h: f32) -> (usize, usize, f32, f32, f32) {
        if n == 0 {
            return (0, 0, 0.0, 0.0, 0.0);
        }
        let max_scroll = log_view_max(n, view_h);
        let scroll = if self.follow && !self.paused {
            max_scroll
        } else {
            self.scrolls
                .get("log-scroll")
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, max_scroll)
        };
        let start = ((scroll / ROW_H).floor() as usize).min(n - 1);
        let count = ((view_h / ROW_H).ceil() as usize + 1).max(1);
        let end = (start + count).min(n);
        let top = start as f32 * ROW_H;
        let bot = (n - end) as f32 * ROW_H;
        (start, end, top, bot, scroll)
    }

    fn fill_inspect(&self, root: &mut Elem) {
        let mut next = 40_000u32;
        let kids = match &self.selection {
            Selection::None => vec![markup::node(
                &mut next,
                &["t-caption"],
                None,
                None,
                "Select a row",
            )],
            Selection::Bus(seq) => match self.bus_log.iter().find(|e| e.seq == *seq) {
                Some(e) => json_pretty(&mut next, &e.payload_pretty),
                None => vec![markup::node(
                    &mut next,
                    &["t-caption"],
                    None,
                    None,
                    "Message dropped from the buffer",
                )],
            },
            Selection::Sticky(topic) => match self.sticky.get(topic) {
                Some(e) => json_pretty(&mut next, &e.payload_pretty),
                None => vec![markup::node(
                    &mut next,
                    &["t-caption"],
                    None,
                    None,
                    "No sticky",
                )],
            },
            Selection::Call(key) => {
                let e = self
                    .call_log
                    .iter()
                    .chain(self.call_pause.iter())
                    .find(|e| e.key == *key);
                match e {
                    Some(e) => json_pretty(&mut next, &call_inspect_json(e)),
                    None => vec![markup::node(
                        &mut next,
                        &["t-caption"],
                        None,
                        None,
                        "Call dropped from the buffer",
                    )],
                }
            }
            Selection::Catalog { owner, method } => {
                json_pretty(&mut next, &catalog_pretty(&self.catalog, owner, method))
            }
        };
        markup::fill_slot(root, "inspect-body", kids);
    }

    fn fill_rail(&self, root: &mut Elem) {
        let mut next = 50_000u32;
        let (title, items, empty) = match self.plane {
            Plane::Bus => {
                let items: Vec<SidebarItem> = self
                    .sticky
                    .values()
                    .map(|e| {
                        let sel = matches!(&self.selection, Selection::Sticky(t) if t == &e.topic);
                        SidebarItem::new(&e.topic, &e.topic)
                            .action("select-sticky")
                            .subtitle(e.source.clone())
                            .active(sel)
                    })
                    .collect();
                let empty = if items.is_empty() {
                    Some("No sticky topics yet")
                } else {
                    None
                };
                ("Last known", items, empty)
            }
            Plane::Call => {
                if !self.call_up {
                    ("Owners", Vec::new(), Some("Call host is not running"))
                } else {
                    let mut items = Vec::new();
                    for o in &self.catalog {
                        for m in &o.methods {
                            let id = format!("{}::{}", o.owner, m.name);
                            let sel = matches!(
                                &self.selection,
                                Selection::Catalog { owner, method }
                                    if owner == &o.owner && method == &m.name
                            );
                            let mut item = SidebarItem::new(id, format!("{}.{}", o.owner, m.name))
                                .action("select-catalog")
                                .active(sel);
                            if !m.summary.is_empty() {
                                item = item.subtitle(m.summary.clone());
                            }
                            items.push(item);
                        }
                    }
                    let empty = if items.is_empty() {
                        Some("No owners advertised")
                    } else {
                        None
                    };
                    ("Owners", items, empty)
                }
            }
        };
        let mut rows = vec![SidebarItem::header(title)];
        rows.extend(items);
        let mut sb = Sidebar::new(rows)
            .fill()
            .nav_id("rail-scroll")
            .build(&mut next);
        if let Some(copy) = empty
            && let Some(nav) = sb.children.first_mut()
        {
            nav.children.push(empty_row(&mut next, copy));
        }
        markup::replace_slot(root, "rail", sb);
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

impl Surface for Monitor {
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
    }
    fn time(&self) -> f32 {
        self.time
    }
    fn needs_frame(&self) -> bool {
        self.focused.is_some() || self.drag != Drag::None
    }
    fn has_overlay(&self) -> bool {
        self.select_open
    }
    fn has_focus(&self) -> bool {
        self.focused.is_some()
    }
    fn blur(&mut self) {
        self.set_focus(None);
    }
    fn dismiss_overlays(&mut self) -> bool {
        let any = self.select_open;
        self.select_open = false;
        if any {
            self.bump_layout();
        }
        any
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
        self.bump_layout();
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
    fn tab(&mut self, _back: bool) {
        self.set_focus(Some("filter".into()));
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
    }
    fn enter(&mut self) {}
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
                    self.html = s;
                    self.html_root = parse_html(&self.html);
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
        let sel = self.sel_px();
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
        if dy.abs() < 0.01 {
            return false;
        }
        if self.pointer_in_log(x, y) {
            let view_h = self.log_view_h();
            let n = match self.plane {
                Plane::Bus => self.bus_visible().len(),
                Plane::Call => self.call_visible().len(),
            };
            return self.scroll_log(dy, log_view_max(n, view_h));
        }
        let mut found = None;
        for item in &self.last_items {
            if item.overflow_scroll
                && point_in_item(item, x, y)
                && let Some(id) = item.data_id.as_deref()
            {
                found = Some((id.to_string(), (item.content_h - item.h).max(0.0)));
            }
        }
        let Some((id, max)) = found else {
            return false;
        };
        if id == "log-scroll" {
            return self.scroll_log(dy, max);
        }
        let cur = self.scrolls.get(&id).copied().unwrap_or(0.0);
        let next = (cur + dy).clamp(0.0, max);
        if (next - cur).abs() < 0.5 {
            return false;
        }
        self.scrolls.insert(id, next);
        self.bump_layout();
        true
    }
    fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let mut dirty = false;
        match &self.drag {
            Drag::Split => {
                let rail = (self.css_w - x).clamp(RAIL_W_MIN, RAIL_W_MAX);
                if (rail - self.rail_w).abs() > 0.5 {
                    self.rail_w = rail;
                    self.bump_layout();
                    dirty = true;
                }
            }
            Drag::SplitH => {
                let top = self.chrome_top() + TOOLBAR_H;
                let bottom = self.css_h;
                let avail = (bottom - top - RULE).max(80.0 + INSPECTOR_H_MIN);
                let log_h = (y - top).clamp(80.0, avail - INSPECTOR_H_MIN);
                let insp = (avail - log_h).clamp(INSPECTOR_H_MIN, INSPECTOR_H_MAX);
                if (insp - self.inspector_h).abs() > 0.5 {
                    self.inspector_h = insp;
                    self.bump_layout();
                    dirty = true;
                }
            }
            Drag::Scroll {
                id,
                origin_y,
                origin_scroll,
                max,
                track_h,
                thumb_h,
            } => {
                let travel = (*track_h - *thumb_h).max(1.0);
                let delta = (y - *origin_y) / travel * *max;
                let next = (*origin_scroll + delta).clamp(0.0, *max);
                let cur = self.scrolls.get(id).copied().unwrap_or(0.0);
                if (next - cur).abs() > 0.5 {
                    if id == "log-scroll" {
                        self.follow = *max <= 0.5 || next >= *max - 0.5;
                    }
                    self.scrolls.insert(id.clone(), next);
                    self.bump_layout();
                    dirty = true;
                }
            }
            Drag::None => {}
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
        if self.last_items.iter().rev().any(|i| {
            point_in_item(i, x, y) && i.classes.iter().any(|c| c == "sb-thumb" || c == "sb-track")
        }) {
            return crate::host::CursorKind::Pointer;
        }
        if self.hit_id("rule-h", x, y) {
            return crate::host::CursorKind::NsResize;
        }
        if self.hit_id("rule-v", x, y) {
            return crate::host::CursorKind::EwResize;
        }
        let hit = self
            .last_items
            .iter()
            .rev()
            .find(|i| point_in_item(i, x, y));
        let Some(hit) = hit else {
            return crate::host::CursorKind::Default;
        };
        if hit.classes.iter().any(|c| c == "input") {
            crate::host::CursorKind::Text
        } else if hit.classes.iter().any(|c| {
            c == "toolbar-btn"
                || c == "btn"
                || c == "row"
                || c == "select"
                || c == "log-row"
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
            self.select_open = false;
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
        self.set_focus(None);
        if let Some(hit) = self.last_items.iter().rev().find(|i| {
            point_in_item(i, x, y) && i.classes.iter().any(|c| c == "sb-thumb" || c == "sb-track")
        }) {
            let action = hit.data_action.clone();
            let id = hit.data_id.clone();
            self.begin_scroll_drag(id.as_deref(), y, action.as_deref() == Some("scroll-track"));
            return self.picked();
        }
        if self.hit_id("rule-h", x, y) {
            self.drag = Drag::SplitH;
            return self.picked();
        }
        if self.hit_id("rule-v", x, y) {
            self.drag = Drag::Split;
            return self.picked();
        }
        let Some(hit) = hit_test(&self.last_items, x, y) else {
            if self.dismiss_overlays() {
                return self.picked();
            }
            return Click::None;
        };
        let action = hit.data_action.clone();
        let id = hit.data_id.clone();
        if self.select_open
            && action.as_deref() != Some("select-toggle")
            && action.as_deref() != Some("pick-filter")
        {
            self.select_open = false;
        }
        match action.as_deref() {
            Some("close") => return Click::Close,
            Some("drag") => return Click::Drag,
            Some("split-drag") => {
                self.drag = Drag::Split;
                return self.picked();
            }
            Some("split-drag-h") => {
                self.drag = Drag::SplitH;
                return self.picked();
            }
            Some("scroll-thumb") | Some("scroll-track") => {
                self.begin_scroll_drag(id.as_deref(), y, action.as_deref() == Some("scroll-track"));
                return self.picked();
            }
            Some("plane") => {
                self.plane = if id.as_deref() == Some("call") {
                    Plane::Call
                } else {
                    Plane::Bus
                };
                self.select_open = false;
                self.selection = Selection::None;
                return self.picked();
            }
            Some("select-toggle") => {
                self.select_open = !self.select_open;
                return self.picked();
            }
            Some("pick-filter") => {
                let v = id.unwrap_or_default();
                match self.plane {
                    Plane::Bus => {
                        self.topic_filter = if v.is_empty() || v == "*" {
                            None
                        } else {
                            Some(v)
                        };
                    }
                    Plane::Call => {
                        self.owner_filter = if v.is_empty() || v == "*" {
                            None
                        } else {
                            Some(v)
                        };
                    }
                }
                self.select_open = false;
                return self.picked();
            }
            Some("pause") => {
                self.toggle_pause();
                return self.picked();
            }
            Some("clear") => {
                self.clear_plane();
                return self.picked();
            }
            Some("select-bus") => {
                if let Some(seq) = id.and_then(|s| s.parse().ok()) {
                    self.set_selection(Selection::Bus(seq));
                }
                return self.picked();
            }
            Some("select-call") => {
                if let Some(key) = id {
                    self.set_selection(Selection::Call(key));
                }
                return self.picked();
            }
            Some("select-sticky") => {
                if let Some(topic) = id {
                    self.set_selection(Selection::Sticky(topic));
                }
                return self.picked();
            }
            Some("select-catalog") => {
                if let Some(raw) = id
                    && let Some((owner, method)) = raw.split_once("::")
                {
                    self.set_selection(Selection::Catalog {
                        owner: owner.to_string(),
                        method: method.to_string(),
                    });
                }
                return self.picked();
            }
            _ => {}
        }
        Click::None
    }
    fn poll(&mut self) -> bool {
        let mut chrome = false;
        let mut log = false;
        let mut n = 0;
        while let Some(msg) = self.bus.try_recv() {
            if self.handle_bus(msg) {
                chrome = true;
            } else {
                log = true;
            }
            n += 1;
            if n >= 512 {
                log = true;
                break;
            }
        }
        while let Ok(ev) = self.observe.try_recv() {
            self.on_observe(ev);
            log = true;
            n += 1;
            if n >= 512 {
                break;
            }
        }
        if chrome {
            self.log_dirty = false;
            self.last_ui = Instant::now();
            self.bump_layout();
            return true;
        }
        if log {
            self.log_dirty = true;
        }
        if self.log_dirty && self.last_ui.elapsed() >= Duration::from_millis(200) {
            self.log_dirty = false;
            self.last_ui = Instant::now();
            self.bump_layout();
            return true;
        }
        false
    }
}

fn log_view_max(n: usize, view_h: f32) -> f32 {
    (n as f32 * ROW_H - view_h).max(0.0)
}

fn log_spacer(next: &mut u32, h: f32) -> Elem {
    let mut e = markup::node(next, &["log-spacer"], None, None, "");
    e.style_attr = Some(format!(
        "width:100%;height:{}px;flex-shrink:0",
        h.round().max(0.0)
    ));
    e
}

fn empty_row(next: &mut u32, copy: &str) -> Elem {
    let mut wrap = markup::node(next, &["log-empty"], None, None, "");
    wrap.children
        .push(markup::node(next, &["t-caption"], None, None, copy));
    wrap
}

fn bus_row(next: &mut u32, e: &BusEntry, selected: bool, id: &str, m: Metrics) -> Elem {
    let classes: &[&str] = if selected {
        &["log-row", "is-active"]
    } else {
        &["log-row"]
    };
    let mut row = markup::node(next, classes, Some("select-bus"), Some(id), "");
    row.style_attr = Some(format!(
        "width:{}px;min-width:0px;max-width:{}px;overflow:hidden",
        m.main_w.round(),
        m.main_w.round()
    ));
    row.children.push(markup::node(
        next,
        &["col-time"],
        None,
        None,
        &format_clock(e.timestamp),
    ));
    row.children
        .push(markup::node(next, &["col-topic"], None, None, &e.topic));
    row.children
        .push(markup::node(next, &["col-source"], None, None, &e.source));
    row.children
        .push(json_preview(next, &e.payload_preview, m.payload_w));
    row
}

fn call_row(next: &mut u32, e: &CallEntry, selected: bool, m: Metrics) -> Elem {
    let classes: &[&str] = if selected {
        &["log-row", "is-active"]
    } else {
        &["log-row"]
    };
    let mut row = markup::node(next, classes, Some("select-call"), Some(&e.key), "");
    row.style_attr = Some(format!(
        "width:{}px;min-width:0px;max-width:{}px;overflow:hidden",
        m.main_w.round(),
        m.main_w.round()
    ));
    row.children.push(markup::node(
        next,
        &["col-time"],
        None,
        None,
        &format_clock(e.timestamp),
    ));
    let call = if e.method.is_empty() {
        e.owner.clone()
    } else {
        format!("{}.{}", e.owner, e.method)
    };
    row.children
        .push(markup::node(next, &["col-call"], None, None, &call));
    row.children
        .push(markup::node(next, &["col-caller"], None, None, &e.caller));
    let mut status = markup::node(next, &["col-status"], None, None, "");
    let (label, tone) = match e.status {
        CallStatus::Pending => ("pending", "badge-neutral"),
        CallStatus::Ok => ("ok", "badge-success"),
        CallStatus::Error => ("error", "badge-danger"),
        CallStatus::Timeout => ("timeout", "badge-warning"),
        CallStatus::Up => ("up", "badge-accent"),
        CallStatus::Down => ("down", "badge-neutral"),
    };
    status.children.push(badge(next, tone, label));
    if let Some(ms) = e.duration_ms {
        status.children.push(markup::node(
            next,
            &["t-caption"],
            None,
            None,
            &format!("{ms} ms"),
        ));
    }
    row.children.push(status);
    row.children
        .push(json_preview(next, &e.params_preview, m.payload_w));
    row
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
    if !e.params_pretty.is_empty()
        && let Ok(v) = serde_json::from_str(&e.params_pretty)
    {
        obj.insert("params".into(), v);
    }
    if !e.result_pretty.is_empty()
        && let Ok(v) = serde_json::from_str(&e.result_pretty)
    {
        obj.insert("result".into(), v);
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

fn catalog_pretty(catalog: &[OwnerCatalog], owner: &str, method: &str) -> String {
    let Some(o) = catalog.iter().find(|o| o.owner == owner) else {
        return String::new();
    };
    if let Some(m) = o.methods.iter().find(|m| m.name == method) {
        return serde_json::to_string_pretty(m).unwrap_or_default();
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

fn field_empty(fields: &HashMap<String, String>, id: &str) -> bool {
    fields.get(id).map(|s| s.is_empty()).unwrap_or(true)
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

#[cfg(test)]
mod chrome_layout {
    use super::*;
    use crate::css::parse_sheet;
    use crate::dom::parse_html;
    use crate::layout::layout_tree;
    use crate::paint::Fonts;

    fn id<'a>(items: &'a [PaintItem], name: &str) -> &'a PaintItem {
        items
            .iter()
            .find(|i| i.data_id.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    }

    fn text_of<'a>(items: &'a [PaintItem], needle: &str) -> &'a PaintItem {
        items
            .iter()
            .find(|i| i.text.as_ref().is_some_and(|t| t.text == needle))
            .unwrap_or_else(|| panic!("missing text {needle}"))
    }

    fn layout_chrome() -> (Vec<PaintItem>, Metrics) {
        let sheet = parse_sheet(include_str!("../assets/kit.css"));
        let mut root = parse_html(include_str!("../assets/monitor.html"));
        markup::hide_if(&mut root, |el| {
            el.data_id.as_deref() == Some("csd") || el.classes.iter().any(|c| c == "titlebar")
        });
        let mut next = markup::next_uid(&root);
        markup::replace_slot(
            &mut root,
            "sidebar",
            Sidebar::new([
                SidebarItem::new("bus", "Bus")
                    .action("plane")
                    .subtitle("Fan-out facts")
                    .active(true),
                SidebarItem::new("call", "Call")
                    .action("plane")
                    .subtitle("Request / reply"),
            ])
            .data_id("sidebar")
            .build(&mut next),
        );
        let mut filter = field::input(&mut next, "filter");
        markup::add_class(&mut filter, "monitor-filter");
        let sel = select::select(
            &mut next,
            "filter-select",
            Some("monitor-select"),
            "select-label",
        );
        let count = text::bind_caption(&mut next, "count");
        let pause = button(&mut next, Btn::Toolbar, false, "pause", None, "Pause");
        let clear = button(&mut next, Btn::Toolbar, false, "clear", None, "Clear");
        let spacer = markup::node(&mut next, &["spacer"], None, None, "");
        markup::replace_slot(
            &mut root,
            "toolbar",
            toolbar::bar(
                &mut next,
                "monitor-toolbar",
                &["monitor-toolbar"],
                vec![filter, sel, count, spacer, pause, clear],
            ),
        );
        markup::fill_slot(
            &mut root,
            "log-head",
            pane::column_labels(
                &mut next,
                &[
                    ("col-time", "Time"),
                    ("col-topic", "Topic"),
                    ("col-source", "Source"),
                    ("col-payload", "Payload"),
                ],
            ),
        );
        markup::set_style(
            &mut root,
            "log-head",
            "height:28px;min-height:28px;max-height:28px;flex-grow:0;flex-shrink:0",
        );
        markup::replace_slot(
            &mut root,
            "inspect-head",
            pane::head(&mut next, "inspect-head", "inspect-title", "Inspector"),
        );
        markup::replace_slot(
            &mut root,
            "split-h",
            split::horizontal(&mut next, "split-drag-h", "rule-h"),
        );
        markup::replace_slot(
            &mut root,
            "split-v",
            split::vertical(&mut next, "split-drag", "rule-v"),
        );
        let mut rail_items = vec![SidebarItem::header("Last known")];
        rail_items.push(
            SidebarItem::new("Session", "Session")
                .action("select-sticky")
                .subtitle("Session manager"),
        );
        markup::replace_slot(
            &mut root,
            "rail",
            Sidebar::new(rail_items)
                .fill()
                .nav_id("rail-scroll")
                .build(&mut next),
        );
        let m = metrics(
            1434.0,
            900.0,
            false,
            Plane::Bus,
            RAIL_W_DEFAULT,
            INSPECTOR_H_DEFAULT,
        );
        apply_sizes(&mut root, m);
        markup::apply_placeholder(&mut root, "filter", true, "Filter");
        let mut fields = std::collections::HashMap::new();
        fields.insert("select-label".into(), "All topics".into());
        fields.insert("count".into(), "8".into());
        markup::apply_fields(&mut root, &fields);
        let e = BusEntry {
            seq: 1,
            timestamp: 0.0,
            topic: "Frame".into(),
            source: "sola-shell".into(),
            payload_preview: r#"{"fullscreen":false,"height":2132,"width":1434,"window_id":1}"#
                .repeat(4),
            payload_pretty: "{}".into(),
            is_sticky: false,
        };
        markup::fill_slot(
            &mut root,
            "log-rows",
            vec![bus_row(&mut next, &e, false, "1", m)],
        );
        let mut fonts = Fonts::new();
        let mut items = layout_tree(
            &root,
            &sheet,
            None,
            1434.0,
            900.0,
            &mut fonts,
            &Default::default(),
        );
        clip_to_pane(&mut items, "log-scroll");
        (items, m)
    }

    #[test]
    fn chrome_matches_iced_tree() {
        let (items, m) = layout_chrome();
        let sidebar = id(&items, "sidebar");
        let toolbar = id(&items, "monitor-toolbar");
        let filter = id(&items, "filter");
        let log = id(&items, "log-scroll");
        let head = id(&items, "log-head");
        let inspect = id(&items, "inspect-pane");
        let rail = id(&items, "rail-pane");
        let time = text_of(&items, "Time");
        let topic = text_of(&items, "Topic");
        let payload = text_of(&items, "Payload");
        let pause = text_of(&items, "Pause");
        let clear = text_of(&items, "Clear");
        let last_known = text_of(&items, "Last known");

        assert!(
            toolbar.h >= 36.0,
            "toolbar height collapsed to {}",
            toolbar.h
        );
        assert!(
            (toolbar.x - SIDEBAR_W).abs() < 2.0,
            "toolbar x={} should start at the sidebar edge {}",
            toolbar.x,
            SIDEBAR_W
        );
        assert!(
            toolbar.x + toolbar.w <= rail.x + 1.5,
            "toolbar right {} should stop at the rail {}",
            toolbar.x + toolbar.w,
            rail.x
        );
        assert!(
            filter.x + 1.0 >= SIDEBAR_W,
            "Filter x={} is under the sidebar ({})",
            filter.x,
            SIDEBAR_W
        );
        assert!(
            filter.w >= 160.0 && filter.h >= 24.0,
            "Filter collapsed to {}x{}",
            filter.w,
            filter.h
        );
        assert!(
            filter.y + filter.h <= toolbar.y + toolbar.h + 1.0,
            "Filter y={} is not in the toolbar",
            filter.y
        );
        assert!(
            (pause.y - toolbar.y).abs() < 20.0,
            "Pause y={} is not in the toolbar {}",
            pause.y,
            toolbar.y
        );
        assert!(
            (clear.y - toolbar.y).abs() < 20.0,
            "Clear y={} is not in the toolbar {}",
            clear.y,
            toolbar.y
        );
        assert!(head.h >= 20.0, "log-head collapsed to height {}", head.h);
        assert!(
            !time.hidden && time.h >= 8.0 && time.w >= 20.0,
            "Time header hidden={} {}x{} at {},{}",
            time.hidden,
            time.w,
            time.h,
            time.x,
            time.y
        );
        assert!(
            time.y + 4.0 <= log.y + 1.0,
            "Time y={} must sit above log-scroll y={}",
            time.y,
            log.y
        );
        assert!(
            head.y + 0.5 >= toolbar.y + toolbar.h,
            "column headers y={} overlap toolbar bottom {}",
            head.y,
            toolbar.y + toolbar.h
        );
        assert!(
            time.y + 1.0 >= toolbar.y + toolbar.h,
            "Time header y={} is not below the toolbar",
            time.y
        );
        assert!(
            topic.x > time.x,
            "Topic x={} should sit right of Time {}",
            topic.x,
            time.x
        );
        assert!(
            payload.x > topic.x,
            "Payload x={} should sit right of Topic {}",
            payload.x,
            topic.x
        );
        assert!(
            log.y + 0.5 >= head.y + head.h - 1.0,
            "log y={} overlaps headers {}",
            log.y,
            head.y + head.h
        );
        assert!(
            rail.x + 1.0 >= log.x + log.w,
            "last-known x={} should sit right of the log {}",
            rail.x,
            log.x + log.w
        );
        assert!(
            rail.y < 2.0,
            "last-known y={} should meet the top of the window",
            rail.y
        );
        assert!(
            rail.h + rail.y >= 890.0,
            "last-known height {} should fill the window",
            rail.h
        );
        assert!(
            pause.x + 8.0 < rail.x && clear.x + 8.0 < rail.x,
            "Pause/Clear should sit on the log toolbar, not over the rail (pause.x={} rail.x={})",
            pause.x,
            rail.x
        );
        assert!(
            last_known.x + 1.0 >= rail.x,
            "Last known heading x={} is not in the right rail {}",
            last_known.x,
            rail.x
        );
        assert!(
            inspect.y + 0.5 >= log.y + 40.0,
            "inspector y={} should sit under the log {}",
            inspect.y,
            log.y
        );
        assert!(
            inspect.x + 40.0 < rail.x,
            "inspector x={} should not be the right pane (rail x={})",
            inspect.x,
            rail.x
        );
        assert!(
            (sidebar.w - SIDEBAR_W).abs() < 2.0,
            "sidebar width {}",
            sidebar.w
        );
        let row_payload = items
            .iter()
            .find(|i| {
                i.classes.iter().any(|c| c == "col-payload")
                    && i.text.as_ref().is_none_or(|t| t.text != "Payload")
            })
            .expect("payload cell");
        assert!(
            row_payload.w <= m.payload_w + 4.0,
            "payload width {} > cell {}",
            row_payload.w,
            m.payload_w
        );
        let log_right = log.x + log.w;
        for item in &items {
            if !item
                .classes
                .iter()
                .any(|c| matches!(c.as_str(), "jk" | "js" | "jn" | "jl" | "jp"))
            {
                continue;
            }
            let clip_right = item
                .clip
                .map(|(x, _, w, _)| x + w)
                .unwrap_or(item.x + item.w);
            assert!(
                clip_right <= log_right + 1.5,
                "token clip right {} past log {}",
                clip_right,
                log_right
            );
        }
    }

    #[test]
    fn log_buffer_is_taller_than_the_viewport() {
        assert!(
            log_view_max(80, 280.0) > 1000.0,
            "80 rows must scroll in a 280px pane"
        );
        assert_eq!(log_view_max(5, 280.0), 0.0);
    }

    #[test]
    fn one_wheel_notch_unpins_follow_from_tail() {
        let max = 10_000.0_f32;
        let next = (max - 140.0).clamp(0.0, max);
        assert!(
            next < max - 0.5,
            "a 5-row notch must leave the tail pin, got {next} vs {max}"
        );
    }
}
