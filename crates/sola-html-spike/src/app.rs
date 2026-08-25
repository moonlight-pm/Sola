//! Store → HTML/CSS → layout → paint. Rust events, no JS.

use crate::css::{Rgba, Sheet, parse_color, parse_sheet};
use crate::gpu::Gpu;
use crate::layout::{PaintItem, hit_test, hover_at, layout_tree};
use crate::markup::{self, RowSpec};
use crate::gpu::Quad;
use crate::paint::{Fonts, draw_label, paint, paint_glyphs};
use crate::strip::{
    ANIM_MS, Kind, LeafMeta, Rect, Slot, THRESHOLD, WELL_PAD_V, dest_from_pointer, dest_index,
    drop_from_slot, ease_out, extra_at, rest_rects, slot_eq,
};
use crate::tabs::{
    Event as TabEvent, Snapshot, Store, apply_event, create_store, group_has_selected, snapshot,
};

const CSS: &str = include_str!("../assets/sidebar.css");
const HTML: &str = include_str!("../assets/sidebar.html");

const PALETTES: &[(&str, &[(&str, &str)])] = &[
    (
        "graphite",
        &[
            ("--bg", "#0c0e12"),
            ("--chrome", "#121722"),
            ("--raised", "#151922"),
            ("--well", "#10141e"),
            ("--selected", "#0e121a"),
            ("--lip", "#20252f"),
            ("--hairline", "#21252e"),
            ("--hover", "#1e2533"),
            ("--fg", "#e9ecf2"),
            ("--idle", "#a1adc7"),
            ("--header", "#9aa3b8"),
            ("--accent", "#3dd6f5"),
        ],
    ),
    (
        "warm",
        &[
            ("--bg", "#140e0c"),
            ("--chrome", "#1c1612"),
            ("--raised", "#221c16"),
            ("--well", "#18120e"),
            ("--selected", "#1a1410"),
            ("--lip", "#2a221c"),
            ("--hairline", "#3a3228"),
            ("--hover", "#2e241c"),
            ("--fg", "#f4ece4"),
            ("--idle", "#c4b4a4"),
            ("--header", "#b8a898"),
            ("--accent", "#f0a050"),
        ],
    ),
    (
        "green",
        &[
            ("--bg", "#0c1210"),
            ("--chrome", "#121a16"),
            ("--raised", "#151e1a"),
            ("--well", "#101814"),
            ("--selected", "#0e1612"),
            ("--lip", "#1c2a22"),
            ("--hairline", "#24382c"),
            ("--hover", "#1a2a20"),
            ("--fg", "#e8f2ec"),
            ("--idle", "#a8c4b4"),
            ("--header", "#98b8a8"),
            ("--accent", "#3ddf8a"),
        ],
    ),
];

fn apply_palette(sheet: &mut Sheet, pal: (&str, &[(&str, &str)])) {
    for (k, v) in pal.1 {
        sheet.vars.insert((*k).to_string(), (*v).to_string());
    }
}

pub struct App {
    pub store: Store,
    pub sheet: Sheet,
    pub fonts: Fonts,
    pub hover: Option<u32>,
    pub press: Option<Press>,
    pub dragging: bool,
    pub dest: Slot,
    pub pointer_y: f32,
    pub grab: f32,
    t_from: f32,
    t_to: f32,
    t_start: std::time::Instant,
    /// Layout size in CSS pixels.
    pub css_w: f32,
    pub css_h: f32,
    pub scale: f32,
    pub scroll_y: f32,
    scroll_max: f32,
    pub input: String,
    pub input_focused: bool,
    preedit: String,
    preedit_cursor: Option<(usize, usize)>,
    css_path: std::path::PathBuf,
    css_mtime: Option<std::time::SystemTime>,
    html_path: std::path::PathBuf,
    html_mtime: Option<std::time::SystemTime>,
    html: String,
    last_items: Vec<PaintItem>,
    tick: f32,
    theme_i: usize,
    gpu: Option<Gpu>,
    scroll_drag: Option<ScrollDrag>,
}

#[derive(Clone, Copy)]
struct ScrollDrag {
    grab: f32,
    strip_y: f32,
    view_h: f32,
    thumb_h: f32,
    max_scroll: f32,
}

#[derive(Clone, Copy)]
pub struct Press {
    pub origin: usize,
    pub press_y: f32,
}

impl App {
    pub fn new(css_w: f32, css_h: f32, scale: f32) -> Self {
        Self::create(css_w, css_h, scale, true)
    }

    /// Live window: CSS chrome only; GPU present draws the hole (no readback).
    pub fn for_present(css_w: f32, css_h: f32, scale: f32) -> Self {
        Self::create(css_w, css_h, scale, false)
    }

    fn create(css_w: f32, css_h: f32, scale: f32, readback_hole: bool) -> Self {
        let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let css_path = assets.join("sidebar.css");
        let html_path = assets.join("sidebar.html");
        let (sheet, css_mtime) = load_sheet(&css_path);
        let (html, html_mtime) = load_html(&html_path);
        Self {
            store: create_store(),
            sheet,
            fonts: Fonts::new(),
            hover: None,
            press: None,
            dragging: false,
            dest: Slot::Origin,
            pointer_y: 0.0,
            grab: 0.0,
            t_from: 1.0,
            t_to: 1.0,
            t_start: std::time::Instant::now(),
            css_w,
            css_h,
            scale: scale.max(0.01),
            scroll_y: 0.0,
            scroll_max: 0.0,
            input: String::new(),
            input_focused: false,
            preedit: String::new(),
            preedit_cursor: None,
            css_path,
            css_mtime,
            html_path,
            html_mtime,
            html,
            last_items: Vec::new(),
            tick: 0.0,
            theme_i: 0,
            gpu: if readback_hole { Gpu::new() } else { None },
            scroll_drag: None,
        }
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        (
            (self.css_w * self.scale).round().max(1.0) as u32,
            (self.css_h * self.scale).round().max(1.0) as u32,
        )
    }

    pub fn reload_css_if_changed(&mut self) -> bool {
        self.reload_assets()
    }

    fn reload_assets(&mut self) -> bool {
        let mut changed = false;
        if let Some((sheet, mtime)) = reload_if_newer(&self.css_path, self.css_mtime) {
            self.sheet = parse_sheet(&sheet);
            self.css_mtime = mtime;
            tracing::info!(path = %self.css_path.display(), "reloaded CSS");
            changed = true;
        }
        if let Some((html, mtime)) = reload_if_newer(&self.html_path, self.html_mtime) {
            self.html = html;
            self.html_mtime = mtime;
            tracing::info!(path = %self.html_path.display(), "reloaded HTML");
            changed = true;
        }
        changed
    }

    fn rebuild_items(&mut self) {
        self.reload_assets();
        let snap = snapshot(&self.store);
        let query = format!("{}{}", self.input, self.preedit);
        let title = snap
            .leaves
            .iter()
            .find(|l| l.kind == Kind::Item && l.id == snap.selected_id)
            .map(|l| l.label.as_str())
            .unwrap_or("");
        let rows = row_specs(
            &self.store,
            &snap,
            self.dragging,
            self.press.map(|p| p.origin),
            self.t_now(),
        );
        let root = markup::expand(
            &self.html,
            &rows,
            title,
            &query,
            self.input_focused,
            self.dragging,
        );
        let mut items = layout_tree(
            &root,
            &self.sheet,
            self.hover,
            self.css_w,
            self.css_h,
        );
        insert_wells(
            &snap,
            &mut items,
            self.dragging,
            self.press.map(|p| p.origin),
            self.t_now(),
        );
        self.scroll_max = apply_scroll(&mut items, &mut self.scroll_y);
        insert_scroll_thumb(&mut items, self.scroll_y, self.scroll_max);
        insert_preedit_mark(
            &mut items,
            &mut self.fonts,
            &self.input,
            &self.preedit,
            accent_color(&self.sheet),
        );
        self.last_items = items;
    }

    pub fn frame(&mut self) -> Vec<u32> {
        self.rebuild_items();
        let mut pix = paint(
            &self.last_items,
            &mut self.fonts,
            self.css_w,
            self.css_h,
            self.scale,
        );
        let (bw, bh) = self.buffer_size();
        if let Some(gpu) = self.gpu.as_ref() {
            blit_surface(
                &self.last_items,
                &mut pix,
                bw,
                bh,
                self.scale,
                self.tick,
                Some(gpu),
            );
        }
        let caret = self.caret_prefix();
        blit_caret(
            &self.last_items,
            &mut self.fonts,
            &mut pix,
            bw,
            bh,
            self.scale,
            caret.as_deref(),
            self.tick,
        );
        if self.dragging {
            if let Some(press) = self.press {
                blit_ghost(
                    &snapshot(&self.store),
                    press.origin,
                    strip_origin_x(&self.last_items),
                    self.pointer_y - self.grab,
                    strip_width(&self.last_items),
                    &mut self.fonts,
                    &mut pix,
                    bw,
                    bh,
                    self.scale,
                );
            }
        }
        pix
    }

    /// GPU boxes + glyph overlay for the live wgpu present path.
    pub fn live_layers(&mut self) -> (Vec<Quad>, Vec<u32>) {
        self.rebuild_items();
        let (bw, bh) = self.buffer_size();
        let quads = chrome_quads(&self.last_items, self.scale, bw, bh);
        let mut pix = paint_glyphs(
            &self.last_items,
            &mut self.fonts,
            self.css_w,
            self.css_h,
            self.scale,
        );
        let caret = self.caret_prefix();
        blit_caret(
            &self.last_items,
            &mut self.fonts,
            &mut pix,
            bw,
            bh,
            self.scale,
            caret.as_deref(),
            self.tick,
        );
        if self.dragging {
            if let Some(press) = self.press {
                blit_ghost(
                    &snapshot(&self.store),
                    press.origin,
                    strip_origin_x(&self.last_items),
                    self.pointer_y - self.grab,
                    strip_width(&self.last_items),
                    &mut self.fonts,
                    &mut pix,
                    bw,
                    bh,
                    self.scale,
                );
            }
        }
        (quads, pix)
    }

    /// Returns true if the frame should redraw.
    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        self.pointer_y = y;
        if let Some(drag) = self.scroll_drag {
            let travel = (drag.view_h - drag.thumb_h).max(1.0);
            let t = ((y - drag.strip_y - drag.grab) / travel).clamp(0.0, 1.0);
            self.scroll_y = t * drag.max_scroll;
            return true;
        }
        let new_hover = hover_at(&self.last_items, x, y);
        let hover_changed = new_hover != self.hover;
        self.hover = new_hover;
        let snap = snapshot(&self.store);
        if let Some(press) = self.press {
            if !self.dragging && (y - press.press_y).abs() >= THRESHOLD as f32 {
                let meta = leaf_meta(&snap);
                if meta
                    .get(press.origin)
                    .is_some_and(|m| m.kind != Kind::Header)
                {
                    self.dragging = true;
                    self.dest = Slot::Origin;
                    let t = (press.origin + 1) as f32;
                    self.t_from = t;
                    self.t_to = t;
                    self.t_start = std::time::Instant::now();
                }
            }
            if self.dragging {
                let rest = rest_from_snap(&snap);
                let local_y = strip_local_y(&self.last_items, y, self.scroll_y);
                let next = dest_from_pointer(local_y.round() as i32, press.origin, &rest);
                if !slot_eq(next, self.dest) {
                    self.dest = next;
                    self.t_go(dest_index(press.origin, next, rest.len()) as f32);
                }
            }
        }
        hover_changed
            || self.dragging
            || (self.t_from - self.t_to).abs() > f32::EPSILON
                && self.t_start.elapsed().as_millis() < ANIM_MS as u128
    }

    pub fn mouse_down(&mut self, x: f32, y: f32) {
        let focused = self.last_items.iter().rev().any(|i| {
            i.data_input.is_some()
                && x >= i.x
                && x < i.x + i.w
                && y >= i.y
                && y < i.y + i.h
        });
        if self.input_focused && !focused {
            self.ime_disable();
        }
        self.input_focused = focused;
        if let Some(drag) = thumb_drag_from_hit(&self.last_items, x, y, self.scroll_max) {
            self.scroll_drag = Some(drag);
            self.press = None;
            return;
        }
        if let Some(hit) = hit_test(&self.last_items, x, y) {
            if let Some(id) = &hit.data_id {
                let snap = snapshot(&self.store);
                let origin = snap.leaves.iter().position(|l| l.id == *id).unwrap_or(0);
                self.press = Some(Press { origin, press_y: y });
                self.pointer_y = y;
                self.dragging = false;
                self.dest = Slot::Origin;
                self.t_from = (origin + 1) as f32;
                self.t_to = (origin + 1) as f32;
                let rest = rest_from_snap(&snap);
                if let Some(r) = rest.get(origin) {
                    self.grab = y - (strip_origin_y(&self.last_items) + r.y as f32);
                }
            }
        }
    }

    pub fn wheel(&mut self, dy: f32) -> bool {
        let before = self.scroll_y;
        self.scroll_y = (self.scroll_y - dy).max(0.0);
        (self.scroll_y - before).abs() > 0.1
    }

    pub fn type_text(&mut self, s: &str) {
        if self.input_focused && self.preedit.is_empty() {
            self.input.push_str(s);
            self.tick = 0.0;
        }
    }

    pub fn backspace(&mut self) {
        if self.input_focused && self.preedit.is_empty() {
            self.input.pop();
            self.tick = 0.0;
        }
    }

    pub fn set_preedit(&mut self, text: String, cursor: Option<(usize, usize)>) {
        if text != self.preedit {
            tracing::info!(preedit = %text, "IME preedit");
        }
        self.preedit = text;
        self.preedit_cursor = cursor;
        self.tick = 0.0;
    }

    pub fn ime_commit(&mut self, s: &str) {
        self.preedit.clear();
        self.preedit_cursor = None;
        if self.input_focused {
            self.input.push_str(s);
            self.tick = 0.0;
        }
        tracing::info!(commit = %s, "IME commit");
    }

    pub fn ime_disable(&mut self) {
        self.preedit.clear();
        self.preedit_cursor = None;
    }

    pub fn ime_on(&self) -> bool {
        self.input_focused
    }

    /// Caret box in CSS pixels for `set_ime_cursor_area`.
    pub fn ime_area(&self) -> Option<(f32, f32, f32, f32)> {
        if !self.input_focused {
            return None;
        }
        let item = self.last_items.iter().find(|i| i.data_input.is_some())?;
        Some((item.x + 10.0, item.y, item.w.max(8.0) - 12.0, item.h))
    }

    fn caret_prefix(&self) -> Option<String> {
        if !self.input_focused {
            return None;
        }
        if !self.preedit.is_empty() {
            return match self.preedit_cursor {
                None => None,
                Some((start, _)) => {
                    let start = start.min(self.preedit.len());
                    let mut s = self.input.clone();
                    if self.preedit.is_char_boundary(start) {
                        s.push_str(&self.preedit[..start]);
                    } else {
                        s.push_str(&self.preedit);
                    }
                    Some(s)
                }
            };
        }
        Some(self.input.clone())
    }

    pub fn tick(&mut self, dt: f32) {
        self.tick += dt;
    }

    pub fn time(&self) -> f32 {
        self.tick
    }

    pub fn gpu_live(&self) -> bool {
        self.gpu.is_some()
    }

    /// Device-pixel rect of the CSS `data-surface` hole after the last `frame`.
    pub fn surface_device_rect(&self) -> Option<(u32, u32, u32, u32)> {
        let item = self.last_items.iter().find(|i| i.data_surface.is_some())?;
        let s = self.scale.max(0.01);
        Some((
            (item.x * s).round().max(0.0) as u32,
            (item.y * s).round().max(0.0) as u32,
            (item.w * s).round().max(1.0) as u32,
            (item.h * s).round().max(1.0) as u32,
        ))
    }

    /// CSS-pixel rect of the hole (parent surface coordinates).
    pub fn surface_css_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let item = self.last_items.iter().find(|i| i.data_surface.is_some())?;
        Some((item.x, item.y, item.w, item.h))
    }

    /// Cycle :root tokens in memory (no bus). File reload still wins if CSS changes.
    pub fn cycle_theme(&mut self) {
        self.theme_i = (self.theme_i + 1) % PALETTES.len();
        apply_palette(&mut self.sheet, PALETTES[self.theme_i]);
        tracing::info!(theme = PALETTES[self.theme_i].0, "live CSS vars updated");
    }

    fn t_now(&self) -> f32 {
        if (self.t_from - self.t_to).abs() < f32::EPSILON {
            return self.t_to;
        }
        let p = ease_out(self.t_start.elapsed().as_secs_f32() / (ANIM_MS as f32 / 1000.0));
        self.t_from + (self.t_to - self.t_from) * p.min(1.0)
    }

    fn t_go(&mut self, next: f32) {
        self.t_from = self.t_now();
        self.t_to = next;
        self.t_start = std::time::Instant::now();
    }

    pub fn label_metrics(&self) -> Option<(f32, f32, f32)> {
        let item = self.last_items.iter().find(|i| {
            i.text
                .as_ref()
                .is_some_and(|t| t.size > 10.0 && t.size < 14.0)
        })?;
        Some((item.w, item.h, item.text.as_ref()?.size))
    }

    pub fn mouse_up(&mut self, _x: f32, y: f32) {
        if self.scroll_drag.take().is_some() {
            return;
        }
        let Some(press) = self.press.take() else {
            return;
        };
        let snap = snapshot(&self.store);
        let was = self.dragging;
        self.dragging = false;
        let rest = rest_from_snap(&snap);
        let meta = leaf_meta(&snap);
        if !was {
            if let Some(m) = meta.get(press.origin) {
                let ev = if m.kind == Kind::Header {
                    TabEvent::Toggle { id: m.id.clone() }
                } else {
                    TabEvent::Activate { id: m.id.clone() }
                };
                apply_event(&mut self.store, ev);
            }
            return;
        }
        let query = format!("{}{}", self.input, self.preedit);
        let title = snap
            .leaves
            .iter()
            .find(|l| l.kind == Kind::Item && l.id == snap.selected_id)
            .map(|l| l.label.as_str())
            .unwrap_or("");
        let rows = row_specs(&self.store, &snap, false, None, 1.0);
        let root = markup::expand(
            &self.html,
            &rows,
            title,
            &query,
            self.input_focused,
            false,
        );
        let items = layout_tree(&root, &self.sheet, None, self.css_w, self.css_h);
        let local_y = strip_local_y(&items, y, self.scroll_y);
        if let Some(drop) = drop_from_slot(
            press.origin,
            self.dest,
            &meta,
            local_y.round() as i32,
            &rest,
        ) {
            apply_event(
                &mut self.store,
                TabEvent::Drop {
                    id: drop.id,
                    dest: drop.dest,
                },
            );
        }
        let _ = dest_index(press.origin, self.dest, rest.len());
    }
}

fn leaf_meta(snap: &Snapshot) -> Vec<LeafMeta> {
    snap.leaves
        .iter()
        .map(|l| LeafMeta {
            id: l.id.clone(),
            kind: l.kind,
            group: l.group.clone(),
        })
        .collect()
}

fn rest_from_snap(snap: &Snapshot) -> Vec<Rect> {
    let n = snap.leaves.len();
    let mut start = vec![false; n];
    let mut end = vec![false; n];
    for span in &snap.spans {
        if span.grouped && span.len > 0 {
            start[span.start] = true;
            end[span.start + span.len - 1] = true;
        }
    }
    let kinds: Vec<Kind> = snap.leaves.iter().map(|l| l.kind).collect();
    rest_rects(&kinds, &start, &end)
}

fn strip_origin_y(items: &[PaintItem]) -> f32 {
    items
        .iter()
        .find(|i| i.classes.iter().any(|c| c == "strip"))
        .map(|i| i.y)
        .unwrap_or(0.0)
}

fn strip_origin_x(items: &[PaintItem]) -> f32 {
    items
        .iter()
        .find(|i| i.classes.iter().any(|c| c == "strip"))
        .map(|i| i.x)
        .unwrap_or(0.0)
}

fn strip_width(items: &[PaintItem]) -> f32 {
    items
        .iter()
        .find(|i| i.classes.iter().any(|c| c == "strip"))
        .map(|i| i.w)
        .unwrap_or(220.0)
}

fn strip_local_y(items: &[PaintItem], y: f32, scroll_y: f32) -> f32 {
    y - strip_origin_y(items) + scroll_y
}

fn insert_wells(
    snap: &Snapshot,
    items: &mut Vec<PaintItem>,
    dragging: bool,
    origin: Option<usize>,
    t: f32,
) {
    let rest = rest_from_snap(snap);
    let strip = items
        .iter()
        .find(|i| i.classes.iter().any(|c| c == "strip"))
        .map(|i| (i.x, i.y, i.w));
    let Some((sx, sy, sw)) = strip else {
        return;
    };
    let mut wells = Vec::new();
    for span in &snap.spans {
        if !span.grouped || span.len == 0 {
            continue;
        }
        let last = span.start + span.len - 1;
        let (Some(first), Some(last_r)) = (rest.get(span.start), rest.get(last)) else {
            continue;
        };
        let extra = |i: usize| {
            if !dragging {
                return 0;
            }
            extra_at(i, origin.unwrap_or(0), t, 32)
        };
        let fy = first.y + extra(span.start);
        let bot = last_r.y + extra(last) + last_r.h;
        let top = fy - WELL_PAD_V;
        let height = (bot - fy + WELL_PAD_V * 2).max(0);
        wells.push(PaintItem {
            uid: 0,
            x: sx,
            y: sy + top as f32,
            w: sw,
            h: height as f32,
            bg: Some(Rgba::rgb(0x10, 0x14, 0x1e)),
            border: Some((1.0, Rgba::rgb(0x21, 0x25, 0x2e))),
            radius: 5.0,
            text: None,
            data_id: None,
            data_kind: None,
            data_surface: None,
            data_input: None,
            classes: vec!["well".into()],
            clip: items
                .iter()
                .find(|i| i.classes.iter().any(|c| c == "strip"))
                .and_then(|s| s.clip),
            overflow_scroll: false,
            hidden: false,
            pad: [0.0; 4],
        });
    }
    let insert_at = items
        .iter()
        .position(|i| i.classes.iter().any(|c| c == "leaves"))
        .unwrap_or(0);
    for (i, well) in wells.into_iter().enumerate() {
        items.insert(insert_at + i, well);
    }
}

fn row_specs(
    store: &Store,
    snap: &Snapshot,
    dragging: bool,
    origin: Option<usize>,
    t: f32,
) -> Vec<RowSpec> {
    let rest = rest_from_snap(snap);
    snap.leaves
        .iter()
        .enumerate()
        .map(|(i, leaf)| {
            let span = snap
                .spans
                .iter()
                .find(|s| i >= s.start && i < s.start + s.len);
            let span_start = span.is_some_and(|s| s.grouped && i == s.start);
            let span_end = span.is_some_and(|s| s.grouped && i == s.start + s.len - 1);
            let active = (leaf.kind == Kind::Item && leaf.id == snap.selected_id)
                || (leaf.kind == Kind::Header
                    && leaf.collapsed
                    && group_has_selected(store, &leaf.id, &snap.selected_id));
            let mut classes = Vec::new();
            if leaf.kind == Kind::Header {
                classes.push("is-header".into());
            }
            if active {
                classes.push("is-active".into());
            }
            if span_start {
                classes.push("span-start".into());
            }
            if span_end {
                classes.push("span-end".into());
            }
            if dragging && origin == Some(i) {
                classes.push("is-origin".into());
            }
            let dy = if dragging {
                extra_at(
                    i,
                    origin.unwrap_or(0),
                    t,
                    rest.get(i).map(|r| r.h).unwrap_or(32),
                )
            } else {
                0
            };
            RowSpec {
                id: leaf.id.clone(),
                kind: if leaf.kind == Kind::Header {
                    "header".into()
                } else {
                    "item".into()
                },
                label: leaf.label.clone(),
                classes,
                translate_y: (dy != 0).then_some(dy),
            }
        })
        .collect()
}

fn load_sheet(path: &std::path::Path) -> (Sheet, Option<std::time::SystemTime>) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| CSS.to_string());
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok());
    (parse_sheet(&text), mtime)
}

fn load_html(path: &std::path::Path) -> (String, Option<std::time::SystemTime>) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| HTML.to_string());
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok());
    (text, mtime)
}

fn reload_if_newer(
    path: &std::path::Path,
    prev: Option<std::time::SystemTime>,
) -> Option<(String, Option<std::time::SystemTime>)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    if prev == Some(mtime) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some((text, Some(mtime)))
}

fn apply_scroll(items: &mut [PaintItem], scroll_y: &mut f32) -> f32 {
    let Some(strip) = items.iter().find(|i| i.overflow_scroll) else {
        return 0.0;
    };
    let (sx, sy, sw, sh) = (strip.x, strip.y, strip.w, strip.h);
    let mut bottom = sy;
    for i in items.iter() {
        if i.data_id.is_some() {
            bottom = bottom.max(i.y + i.h);
        }
    }
    let max_scroll = (bottom - (sy + sh) + 8.0).max(0.0);
    *scroll_y = scroll_y.clamp(0.0, max_scroll);
    let sy_scroll = *scroll_y;
    for i in items.iter_mut() {
        if i.classes.iter().any(|c| c == "app" || c == "sidebar" || c == "strip") {
            continue;
        }
        if i.x + i.w < sx || i.x > sx + sw {
            continue;
        }
        i.y -= sy_scroll;
        i.clip = Some((sx, sy, sw, sh));
    }
    max_scroll
}

fn accent_color(sheet: &Sheet) -> Rgba {
    sheet
        .vars
        .get("--accent")
        .and_then(|v| parse_color(v))
        .unwrap_or(Rgba::rgb(0x3d, 0xd6, 0xf5))
}

fn insert_preedit_mark(
    items: &mut Vec<PaintItem>,
    fonts: &mut Fonts,
    committed: &str,
    preedit: &str,
    accent: Rgba,
) {
    if preedit.is_empty() {
        return;
    }
    let Some(item) = items.iter().find(|i| i.data_input.is_some()) else {
        return;
    };
    let (x, y, pad_l, pad_b, h) = (item.x, item.y, item.pad[3], item.pad[2], item.h);
    let left = fonts.measure_width(committed, 12.0, 400, "SF Pro Text");
    let width = fonts.measure_width(preedit, 12.0, 400, "SF Pro Text").max(4.0);
    items.push(PaintItem {
        uid: 0,
        x: x + pad_l + left,
        y: y + h - pad_b - 5.0,
        w: width,
        h: 1.0,
        bg: Some(accent),
        border: None,
        radius: 0.0,
        text: None,
        data_id: None,
        data_kind: None,
        data_surface: None,
        data_input: None,
        classes: vec!["preedit-mark".into()],
        clip: None,
        overflow_scroll: false,
        hidden: false,
        pad: [0.0; 4],
    });
}

fn insert_scroll_thumb(items: &mut Vec<PaintItem>, scroll_y: f32, max_scroll: f32) {
    if max_scroll < 1.0 {
        return;
    }
    let Some(strip) = items.iter().find(|i| i.overflow_scroll) else {
        return;
    };
    let (sx, sy, sw, sh) = (strip.x, strip.y, strip.w, strip.h);
    let content_h = sh + max_scroll;
    let thumb_h = (sh * sh / content_h).clamp(24.0, sh);
    let travel = (sh - thumb_h).max(0.0);
    let t = (scroll_y / max_scroll).clamp(0.0, 1.0);
    let y = sy + t * travel;
    let w = 3.0;
    let x = sx + sw - w - 4.0;
    items.push(PaintItem {
        uid: 0,
        x,
        y,
        w,
        h: thumb_h,
        bg: Some(Rgba {
            r: 0x9a,
            g: 0xa3,
            b: 0xb8,
            a: 110,
        }),
        border: None,
        radius: 1.5,
        text: None,
        data_id: None,
        data_kind: None,
        data_surface: None,
        data_input: None,
        classes: vec!["scroll-thumb".into()],
        clip: Some((sx, sy, sw, sh)),
        overflow_scroll: false,
        hidden: false,
        pad: [0.0; 4],
    });
}

fn thumb_drag_from_hit(items: &[PaintItem], x: f32, y: f32, max_scroll: f32) -> Option<ScrollDrag> {
    if max_scroll < 1.0 {
        return None;
    }
    let thumb = items.iter().rev().find(|i| {
        i.classes.iter().any(|c| c == "scroll-thumb")
            && x >= i.x - 4.0
            && x < i.x + i.w + 4.0
            && y >= i.y
            && y < i.y + i.h
    })?;
    let strip = items.iter().find(|i| i.overflow_scroll)?;
    Some(ScrollDrag {
        grab: y - thumb.y,
        strip_y: strip.y,
        view_h: strip.h,
        thumb_h: thumb.h,
        max_scroll,
    })
}

fn blit_surface(
    items: &[PaintItem],
    pix: &mut [u32],
    bw: u32,
    bh: u32,
    scale: f32,
    tick: f32,
    gpu: Option<&Gpu>,
) {
    let Some(item) = items.iter().find(|i| i.data_surface.is_some()) else {
        return;
    };
    let s = scale.max(0.01);
    let x0 = (item.x * s).round() as i32;
    let y0 = (item.y * s).round() as i32;
    let w = (item.w * s).round() as i32;
    let h = (item.h * s).round() as i32;
    let ww = w.max(1) as u32;
    let hh = h.max(1) as u32;
    if let Some(gpu) = gpu {
        if let Some(frame) = gpu.render(ww, hh, tick) {
            for yy in 0..hh as i32 {
                for xx in 0..ww as i32 {
                    let px = x0 + xx;
                    let py = y0 + yy;
                    if px < 0 || py < 0 {
                        continue;
                    }
                    let ux = px as u32;
                    let uy = py as u32;
                    if ux >= bw || uy >= bh {
                        continue;
                    }
                    pix[(uy * bw + ux) as usize] = frame[(yy as u32 * ww + xx as u32) as usize];
                }
            }
            return;
        }
    }
    let well = 0x0010141eu32;
    for yy in 0..h.max(0) {
        for xx in 0..w.max(0) {
            let px = x0 + xx;
            let py = y0 + yy;
            if px < 0 || py < 0 {
                continue;
            }
            let ux = px as u32;
            let uy = py as u32;
            if ux >= bw || uy >= bh {
                continue;
            }
            let checker = ((xx / 12) + (yy / 12)) % 2 == 0;
            pix[(uy * bw + ux) as usize] = if checker { well } else { 0x00151822 };
        }
    }
}

fn blit_caret(
    items: &[PaintItem],
    fonts: &mut Fonts,
    pix: &mut [u32],
    bw: u32,
    bh: u32,
    scale: f32,
    prefix: Option<&str>,
    tick: f32,
) {
    let Some(text) = prefix else {
        return;
    };
    if (tick * 1.6) as i32 % 2 == 1 {
        return;
    }
    let Some(item) = items.iter().find(|i| i.data_input.is_some()) else {
        return;
    };
    let s = scale.max(0.01);
    let tw = fonts.measure_width(text, 12.0, 400, "SF Pro Text");
    let cx = ((item.x + item.pad[3] + tw + 1.0) * s).round() as i32;
    let y0 = ((item.y + item.pad[0] + 4.0) * s).round() as i32;
    let y1 = ((item.y + item.h - item.pad[2] - 4.0) * s).round() as i32;
    let color = 0xFF3dd6f5u32;
    for y in y0..y1 {
        if y < 0 {
            continue;
        }
        let ux = cx as u32;
        let uy = y as u32;
        if ux >= bw || uy >= bh {
            continue;
        }
        pix[(uy * bw + ux) as usize] = color;
    }
}

fn blit_ghost(
    snap: &Snapshot,
    origin: usize,
    strip_x: f32,
    top: f32,
    strip_w: f32,
    fonts: &mut Fonts,
    pix: &mut [u32],
    bw: u32,
    bh: u32,
    scale: f32,
) {
    let Some(leaf) = snap.leaves.get(origin) else {
        return;
    };
    let s = scale.max(0.01);
    let x = strip_x + 1.0;
    let w = strip_w - 2.0;
    let h = 32.0;
    let lip = Rgba::rgb(0x20, 0x25, 0x2f);
    let fg = Rgba::rgb(0xe9, 0xec, 0xf2);
    // Reuse fill via a tiny PaintItem path: draw_label after a crude rect.
    let x0 = (x * s).round() as i32;
    let y0 = (top * s).round() as i32;
    let rw = (w * s).round() as i32;
    let rh = (h * s).round() as i32;
    for yy in 0..rh.max(0) {
        for xx in 0..rw.max(0) {
            let px = x0 + xx;
            let py = y0 + yy;
            if px < 0 || py < 0 {
                continue;
            }
            let ux = px as u32;
            let uy = py as u32;
            if ux >= bw || uy >= bh {
                continue;
            }
            pix[(uy * bw + ux) as usize] = 0xFF000000 | lip.to_u32();
        }
    }
    draw_label(
        pix,
        bw,
        bh,
        fonts,
        &leaf.label,
        (x + 10.0) * s,
        (top + 7.0) * s,
        (w - 16.0) * s,
        16.0 * s,
        fg,
        12.0 * s,
        if leaf.kind == Kind::Header { 500 } else { 400 },
        "SF Pro Text",
        None,
    );
}

fn chrome_quads(items: &[PaintItem], scale: f32, sw: u32, sh: u32) -> Vec<Quad> {
    let s = scale.max(0.01);
    let screen = [0.0, 0.0, sw as f32, sh as f32];
    let mut out = Vec::new();
    for item in items {
        if item.hidden || item.w < 0.5 || item.h < 0.5 {
            continue;
        }
        let clip = item
            .clip
            .map(|(x, y, w, h)| [x * s, y * s, w * s, h * s])
            .unwrap_or(screen);
        let xywh = [item.x * s, item.y * s, item.w * s, item.h * s];
        if let Some(bg) = item.bg {
            out.push(Quad {
                xywh,
                color: [
                    bg.r as f32 / 255.0,
                    bg.g as f32 / 255.0,
                    bg.b as f32 / 255.0,
                    bg.a as f32 / 255.0,
                ],
                clip,
                radius: item.radius * s,
                _pad: [0.0; 3],
            });
        }
        if let Some((bw, col)) = item.border {
            let t = (bw * s).max(1.0);
            let [x, y, w, h] = xywh;
            let c = [
                col.r as f32 / 255.0,
                col.g as f32 / 255.0,
                col.b as f32 / 255.0,
                1.0,
            ];
            let stroke = |xywh: [f32; 4]| Quad {
                xywh,
                color: c,
                clip,
                radius: 0.0,
                _pad: [0.0; 3],
            };
            out.push(stroke([x, y, w, t]));
            out.push(stroke([x, y + h - t, w, t]));
            out.push(stroke([x, y, t, h]));
            out.push(stroke([x + w - t, y, t, h]));
        }
    }
    out
}


