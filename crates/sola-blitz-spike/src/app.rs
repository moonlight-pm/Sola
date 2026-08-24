//! Sidebar view — port of Scratch `sidebar.js` behavior, Rust events, no JS.

use std::time::Instant;

use dioxus_native::prelude::dioxus_elements::input_data::MouseButton;
use dioxus_native::prelude::*;

use crate::iced_label::{self, LABEL_H, LABEL_W};

use crate::strip::{
    ANIM_MS, Kind, LeafMeta, Rect, Slot, WELL_PAD_V, dest_from_pointer, dest_index, drop_from_slot,
    ease_out, extra_at, pointer_in_origin_group, rest_rects, slot_eq,
};
use crate::tabs::{
    Event as TabEvent, Leaf, Snapshot, Store, apply_event, create_store, group_has_selected,
    snapshot,
};

const CSS: &str = include_str!("../assets/sidebar.css");

const IDLE: [u8; 4] = [0xa1, 0xad, 0xc7, 0xff];
const HEADER: [u8; 4] = [0x9a, 0xa3, 0xb8, 0xff];
const FG: [u8; 4] = [0xe9, 0xec, 0xf2, 0xff];


#[derive(Clone, Copy)]
struct Press {
    origin: usize,
    grab: i32,
    press_y: i32,
}

#[derive(Clone, Copy)]
struct Drag {
    press: Option<Press>,
    dragging: bool,
    pointer_y: i32,
    dest: Slot,
    t_from: f32,
    t_to: f32,
    t_start: Instant,
    h: i32,
}

impl Drag {
    fn idle() -> Self {
        Self {
            press: None,
            dragging: false,
            pointer_y: 0,
            dest: Slot::Origin,
            t_from: 1.0,
            t_to: 1.0,
            t_start: Instant::now(),
            h: 32,
        }
    }

    fn t_now(&self) -> f32 {
        if (self.t_from - self.t_to).abs() < f32::EPSILON {
            return self.t_to;
        }
        let p = ease_out(self.t_start.elapsed().as_secs_f32() / (ANIM_MS as f32 / 1000.0));
        self.t_from + (self.t_to - self.t_from) * p
    }

    fn t_animating(&self) -> bool {
        (self.t_from - self.t_to).abs() > f32::EPSILON
            && self.t_start.elapsed().as_millis() < ANIM_MS as u128
    }

    fn t_go(&mut self, next: f32) {
        self.t_from = self.t_now();
        self.t_to = next;
        self.t_start = Instant::now();
    }
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

fn leaving_origin_group(snap: &Snapshot, rest: &[Rect], drag: &Drag) -> bool {
    let Some(press) = drag.press else {
        return false;
    };
    if !drag.dragging {
        return false;
    }
    let meta = leaf_meta(snap);
    let Some(g) = meta.get(press.origin).and_then(|m| m.group.as_deref()) else {
        return false;
    };
    !pointer_in_origin_group(g, drag.pointer_y, &meta, rest)
}

fn well_rects(snap: &Snapshot, rest: &[Rect], drag: &Drag) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let leaving = leaving_origin_group(snap, rest, drag);
    for span in &snap.spans {
        if !span.grouped || span.len == 0 {
            continue;
        }
        let last_i = span.start + span.len - 1;
        let (Some(first), Some(last)) = (rest.get(span.start), rest.get(last_i)) else {
            continue;
        };
        let extra = |i: usize| {
            if !drag.dragging || drag.press.is_none() {
                return 0;
            }
            extra_at(i, drag.press.unwrap().origin, drag.t_now(), drag.h)
        };
        let fy = first.y + extra(span.start);
        let mut bot = last.y + extra(last_i) + last.h;
        let origin_in = drag
            .press
            .is_some_and(|p| p.origin >= span.start && p.origin < span.start + span.len);
        if drag.dragging && origin_in && !leaving {
            let slot_at = match drag.dest {
                Slot::End => rest.len(),
                Slot::Before(i) => i,
                Slot::Origin => drag.press.map(|p| p.origin + 1).unwrap_or(0),
            };
            if slot_at >= span.start + span.len {
                bot = bot.max(last.y + last.h);
            }
        }
        let top = fy - WELL_PAD_V;
        let height = (bot - fy + WELL_PAD_V * 2).max(0);
        out.push((top, height));
    }
    out
}

fn row_class(
    leaf: &Leaf,
    span_start: bool,
    span_end: bool,
    active: bool,
    is_origin: bool,
) -> String {
    let mut c = String::from("row");
    match leaf.kind {
        Kind::Header => c.push_str(" is-header"),
        Kind::Item => c.push_str(" is-item"),
    }
    if active {
        c.push_str(" is-active");
    }
    if leaf.kind == Kind::Header && leaf.collapsed {
        c.push_str(" is-collapsed");
    }
    if span_start {
        c.push_str(" span-start");
    }
    if span_end {
        c.push_str(" span-end");
    }
    if leaf.group.is_none() && leaf.kind == Kind::Item {
        c.push_str(" is-loose");
    }
    if is_origin {
        c.push_str(" is-origin");
    }
    c
}

fn local_y(evt: &Event<MouseData>) -> i32 {
    evt.data().client_coordinates().y.round() as i32
}

struct RowView {
    i: usize,
    class: String,
    style: String,
    is_header: bool,
    src: String,
}

pub fn app() -> Element {
    let store = use_signal(create_store);
    let drag = use_signal(Drag::idle);
    let mut frame = use_signal(|| 0u32);

    use_effect(move || {
        let _ = frame();
        let d = drag();
        if d.dragging && d.t_animating() {
            spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                frame += 1;
            });
        }
    });

    let snap = snapshot(&store());
    let rest = rest_from_snap(&snap);
    let d = drag();
    let _ = frame();
    let wells = well_rects(&snap, &rest, &d);
    let origin = d.press.map(|p| p.origin);
    let t = d.t_now();

    let selected_label = snap
        .leaves
        .iter()
        .find(|l| l.kind == Kind::Item && l.id == snap.selected_id)
        .map(|l| l.label.clone())
        .unwrap_or_else(|| snap.selected_id.clone());

    let mut rows = Vec::with_capacity(snap.leaves.len());
    for (i, leaf) in snap.leaves.iter().enumerate() {
        let span = snap
            .spans
            .iter()
            .find(|s| i >= s.start && i < s.start + s.len);
        let span_start = span.is_some_and(|s| s.grouped && i == s.start);
        let span_end = span.is_some_and(|s| s.grouped && i == s.start + s.len - 1);
        let active = (leaf.kind == Kind::Item && leaf.id == snap.selected_id)
            || (leaf.kind == Kind::Header
                && leaf.collapsed
                && group_has_selected(&store(), &leaf.id, &snap.selected_id));
        let is_origin = d.dragging && origin == Some(i);
        let dy = if d.dragging && d.press.is_some() {
            extra_at(i, d.press.unwrap().origin, t, d.h)
        } else {
            0
        };
        let rgba = if active {
            FG
        } else if leaf.kind == Kind::Header {
            HEADER
        } else {
            IDLE
        };
        let weight = if active || leaf.kind == Kind::Header {
            500
        } else {
            400
        };
        rows.push(RowView {
            i,
            class: row_class(leaf, span_start, span_end, active, is_origin),
            style: if dy != 0 {
                format!("transform: translateY({dy}px)")
            } else {
                String::new()
            },
            is_header: leaf.kind == Kind::Header,
            src: iced_label::label_data_url(&leaf.label, rgba, weight),
        });
    }

    let ghost = d.press.and_then(|press| {
        if !d.dragging {
            return None;
        }
        snap.leaves.get(press.origin).map(|leaf| {
            let rgba = IDLE;
            let weight = if leaf.kind == Kind::Header { 500 } else { 400 };
            (
                d.pointer_y - press.grab,
                d.h,
                leaf.kind == Kind::Header,
                iced_label::label_data_url(&leaf.label, rgba, weight),
            )
        })
    });

    let strip_class = if d.dragging {
        "strip is-dragging"
    } else {
        "strip"
    };

    rsx! {
        style { "{CSS}" }
        div { class: "app",
            aside { class: "sidebar", "aria-label": "Sidebar",
                div {
                    class: "{strip_class}",
                    onmousemove: move |evt| on_move(store, drag, evt),
                    onmouseup: move |_| on_up(store, drag),
                    onmouseleave: move |_| {
                        if drag().dragging {
                            on_up(store, drag);
                        }
                    },
                    div { class: "wells", "aria-hidden": "true",
                        for (top, height) in wells.iter().copied() {
                            div {
                                class: "well",
                                style: "top: {top}px; height: {height}px;",
                            }
                        }
                    }
                    div { class: "leaves",
                        for row in rows {
                            div {
                                class: "{row.class}",
                                style: "{row.style}",
                                onmousedown: move |evt| {
                                    on_down(store, drag, row.i, evt);
                                },
                                div { class: "etch",
                                    if row.is_header {
                                        span { class: "chevron" }
                                    }
                                    img {
                                        class: "label",
                                        src: "{row.src}",
                                        width: LABEL_W,
                                        height: LABEL_H,
                                    }
                                }
                            }
                        }
                    }
                    if let Some((top, h, is_header, src)) = ghost {
                        div {
                            class: "ghost",
                            style: "top: {top}px; height: {h}px;",
                            div { class: "row",
                                div { class: "etch",
                                    if is_header {
                                        span { class: "chevron" }
                                    }
                                    img {
                                        class: "label",
                                        src: "{src}",
                                        width: LABEL_W,
                                        height: LABEL_H,
                                    }
                                }
                            }
                        }
                    }
                }
            }
            main { class: "stage", "aria-label": "Content",
                div { class: "stage-title", "{selected_label}" }
            }
        }
    }
}

fn live_rest(store: Signal<Store>) -> (Snapshot, Vec<Rect>) {
    let snap = snapshot(&store());
    let rest = rest_from_snap(&snap);
    (snap, rest)
}

fn on_down(store: Signal<Store>, mut drag: Signal<Drag>, i: usize, evt: Event<MouseData>) {
    if evt.data().trigger_button() != Some(MouseButton::Primary) {
        return;
    }
    let (_, rest) = live_rest(store);
    let Some(r) = rest.get(i).copied() else {
        return;
    };
    let y = local_y(&evt);
    evt.prevent_default();
    drag.with_mut(|d| {
        d.pointer_y = y;
        d.press = Some(Press {
            origin: i,
            grab: y - r.y,
            press_y: y,
        });
        d.dragging = false;
        d.dest = Slot::Origin;
        d.h = r.h.max(1);
        d.t_from = (i + 1) as f32;
        d.t_to = (i + 1) as f32;
    });
}

fn on_move(store: Signal<Store>, mut drag: Signal<Drag>, evt: Event<MouseData>) {
    let y = local_y(&evt);
    let (snap, rest) = live_rest(store);
    let meta = leaf_meta(&snap);
    drag.with_mut(|d| {
        let Some(press) = d.press else {
            return;
        };
        d.pointer_y = y;
        if !d.dragging && (y - press.press_y).abs() >= crate::strip::THRESHOLD {
            if meta
                .get(press.origin)
                .is_some_and(|m| m.kind == Kind::Header)
            {
                return;
            }
            d.dragging = true;
            d.h = rest.get(press.origin).map(|r| r.h.max(1)).unwrap_or(d.h);
            d.dest = Slot::Origin;
            d.t_from = (press.origin + 1) as f32;
            d.t_to = (press.origin + 1) as f32;
        }
        if d.dragging {
            let next = dest_from_pointer(y, press.origin, &rest);
            if !slot_eq(next, d.dest) {
                d.dest = next;
                d.t_go(dest_index(press.origin, next, rest.len()) as f32);
            }
        }
    });
}

fn on_up(mut store: Signal<Store>, mut drag: Signal<Drag>) {
    let d = drag();
    let Some(press) = d.press else {
        return;
    };
    let origin = press.origin;
    let was = d.dragging;
    let y = d.pointer_y;
    let slot = d.dest;
    drag.set(Drag::idle());
    let (snap, rest) = live_rest(store);
    let meta = leaf_meta(&snap);
    if !was {
        let Some(m) = meta.get(origin) else {
            return;
        };
        let ev = if m.kind == Kind::Header {
            TabEvent::Toggle { id: m.id.clone() }
        } else {
            TabEvent::Activate { id: m.id.clone() }
        };
        store.with_mut(|s| apply_event(s, ev));
        return;
    }
    if let Some(drop) = drop_from_slot(origin, slot, &meta, y, &rest) {
        store.with_mut(|s| {
            apply_event(
                s,
                TabEvent::Drop {
                    id: drop.id,
                    dest: drop.dest,
                },
            );
        });
    }
}
