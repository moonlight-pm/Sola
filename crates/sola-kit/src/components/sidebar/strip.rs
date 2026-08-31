//! Morph2 reorder: one flat list, one hole, FLIP after each hole move.
//!
//! Port of `~/Workspace/Scratch/morph2.js`. Dest uses rest layout
//! (`offsetTop`), never transformed rects. One hole move per frame.

use std::collections::HashMap;

use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::mouse;
use iced::time::Instant;
use iced::window;
use iced::{
    Animation, Background, Color, Element, Event, Length, Point, Rectangle, Shadow, Size,
    Transformation, Vector, animation::Easing,
};

use crate::components::style::{CHROME_SURFACE, alpha};

use super::gesture::{Dest, Drop, Event as SidebarEvent, Msg};
use super::{PANEL_REORDER_ANIM_MS, PANEL_REORDER_THRESHOLD, group_well_style, px};

const WELL_PAD: i32 = 3;
const ROW_GAP: i32 = 3;
const GROUP_END_GAP: i32 = 6;
const PAD_TOP: i32 = 6;
const PAD_BOT: i32 = 12;
const TITLE_INSET: f32 = 3.0;
const ROW_INSET: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafKind {
    Header,
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Group,
    Item,
    Loose,
}

#[derive(Clone)]
pub struct LeafMeta {
    pub id: String,
    pub kind: LeafKind,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionSpan {
    pub grouped: bool,
    pub start: usize,
    pub len: usize,
}

#[derive(Debug, Clone)]
struct ViewRow {
    id: String,
    kind: Kind,
    group: Option<String>,
    hole: bool,
    leaf: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct Rest {
    y: i32,
    h: i32,
}

struct Slot {
    slot: usize,
    absorb: Option<String>,
}

fn kind_of(m: &LeafMeta) -> Kind {
    match m.kind {
        LeafKind::Header => Kind::Group,
        LeafKind::Item if m.group.is_some() => Kind::Item,
        LeafKind::Item => Kind::Loose,
    }
}

fn is_group_like(row: &ViewRow) -> bool {
    row.kind == Kind::Group
}

fn skip_empty(view: &[ViewRow], mut i: i32, dir: i32) -> i32 {
    let n = view.len() as i32;
    while i >= 0 && i < n && view[i as usize].hole {
        i += dir;
    }
    i
}

fn group_id_above(view: &[ViewRow], i: usize) -> Option<String> {
    for j in (0..=i).rev() {
        if view[j].hole {
            continue;
        }
        match view[j].kind {
            Kind::Group => return view[j].group.clone(),
            Kind::Loose => return None,
            Kind::Item => {}
        }
    }
    None
}

fn is_last_member(view: &[ViewRow], i: usize) -> bool {
    let it = &view[i];
    if it.hole {
        return false;
    }
    if it.kind != Kind::Item && it.kind != Kind::Group {
        return false;
    }
    let n = skip_empty(view, i as i32 + 1, 1);
    n >= view.len() as i32
        || view[n as usize].kind == Kind::Loose
        || view[n as usize].kind == Kind::Group
}

fn is_first_after_group(view: &[ViewRow], i: usize) -> bool {
    let it = &view[i];
    if it.hole {
        return false;
    }
    if it.kind != Kind::Loose && it.kind != Kind::Group {
        return false;
    }
    let p = skip_empty(view, i as i32 - 1, -1);
    p >= 0 && (view[p as usize].kind == Kind::Item || view[p as usize].kind == Kind::Group)
}

/// C5 bottom half → absorb. U1 top half coming down → leave.
/// Coming *up* at U1 uses the whole row (see `slot_at`).
fn seam_absorb(y: i32, view: &[ViewRow], rects: &[Rest]) -> Option<String> {
    for i in 0..view.len() {
        if !is_last_member(view, i) {
            continue;
        }
        let a = rects.get(i)?;
        let next = skip_empty(view, i as i32 + 1, 1);
        let b = if next >= 0 && (next as usize) < rects.len() {
            Some(rects[next as usize])
        } else {
            None
        };
        let a_mid = a.y + a.h / 2;
        let a_bot = a.y + a.h;
        if y >= a_mid && y < a_bot {
            return group_id_above(view, i);
        }
        if let Some(b) = b {
            let b_mid = b.y + b.h / 2;
            if y >= b.y && y < b_mid {
                return None;
            }
            if y >= a_bot && y < b.y {
                return if y < (a_bot + b.y) / 2 {
                    group_id_above(view, i)
                } else {
                    None
                };
            }
        }
    }
    None
}

struct Block {
    start: usize,
    end: usize,
    y: i32,
    h: i32,
}

fn dest_blocks(view: &[ViewRow], rects: &[Rest]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < view.len() {
        if view[i].hole || view[i].kind != Kind::Group {
            blocks.push(Block {
                start: i,
                end: i + 1,
                y: rects[i].y,
                h: rects[i].h,
            });
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < view.len() && view[end].kind == Kind::Item {
            end += 1;
        }
        let y = rects[i].y;
        let h = rects[end - 1].y + rects[end - 1].h - y;
        blocks.push(Block {
            start: i,
            end,
            y,
            h,
        });
        i = end;
    }
    blocks
}

fn slot_at_group(y: i32, empty_at: usize, rects: &[Rest], view: &[ViewRow]) -> Slot {
    let blocks = dest_blocks(view, rects);
    if let Some(self_b) = blocks.iter().find(|b| b.start == empty_at) {
        if y >= self_b.y && y < self_b.y + self_b.h {
            return Slot {
                slot: empty_at + 1,
                absorb: None,
            };
        }
    }
    let hit = blocks
        .iter()
        .position(|b| b.start != empty_at && y >= b.y && y < b.y + b.h);
    let mut to = if let Some(hi) = hit {
        let b = &blocks[hi];
        if y < b.y + b.h / 2 { b.start } else { b.end }
    } else {
        blocks
            .iter()
            .position(|b| y < b.y)
            .map(|j| blocks[j].start)
            .unwrap_or(view.len())
    };
    if to == empty_at || to == empty_at + 1 {
        to = empty_at + 1;
    }
    Slot {
        slot: to,
        absorb: None,
    }
}

/// Insert index for the pointer. `origin + 1` means the hole stays put.
fn slot_at(y: i32, origin: usize, rects: &[Rest], view: &[ViewRow], as_group: bool) -> Slot {
    if as_group {
        return slot_at_group(y, origin, rects, view);
    }
    let n = rects.len();
    if let Some(o) = rects.get(origin) {
        if y >= o.y && y < o.y + o.h {
            return Slot {
                slot: origin + 1,
                absorb: seam_absorb(y, view, rects),
            };
        }
    }
    let hit = rects.iter().position(|r| y >= r.y && y < r.y + r.h);
    let mut to = if let Some(hit) = hit {
        let r = &rects[hit];
        let top_half = y < r.y + r.h / 2;
        if (is_first_after_group(view, hit) || is_last_member(view, hit)) && hit < origin {
            hit
        } else if is_last_member(view, hit) || is_first_after_group(view, hit) {
            if top_half { hit } else { hit + 1 }
        } else if hit < origin {
            hit
        } else {
            hit + 1
        }
    } else {
        rects.iter().position(|r| y < r.y).unwrap_or(n)
    };
    if to == origin || to == origin + 1 {
        to = origin + 1;
    }
    Slot {
        slot: to,
        absorb: seam_absorb(y, view, rects),
    }
}

fn move_hole(from: usize, slot: usize) -> Option<usize> {
    if slot == from || slot == from + 1 {
        return None;
    }
    Some(if slot > from { slot - 1 } else { slot })
}

fn after_group_like(view: &[ViewRow], from: usize) -> bool {
    let mut i = from;
    while i < view.len() && view[i].hole && !is_group_like(&view[i]) {
        i += 1;
    }
    i < view.len() && is_group_like(&view[i])
}

fn group_spans(view: &[ViewRow], absorb: Option<&str>) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < view.len() {
        if view[i].kind != Kind::Group || view[i].hole {
            i += 1;
            continue;
        }
        let gid = view[i].group.clone();
        let mut end = i + 1;
        while end < view.len() {
            let k = &view[end];
            if k.hole {
                let after = view.get(end + 1);
                let seam = after.is_none()
                    || matches!(after.map(|a| a.kind), Some(Kind::Loose | Kind::Group));
                if seam {
                    if absorb.is_some() && gid.as_deref() == absorb {
                        end += 1;
                    }
                    break;
                }
                end += 1;
                continue;
            }
            if k.kind == Kind::Item {
                end += 1;
                continue;
            }
            break;
        }
        spans.push((i, end));
        i = end;
    }
    spans
}

fn group_end(view: &[ViewRow], i: usize) -> bool {
    let spans = group_spans(view, None);
    for &(start, end) in &spans {
        let mut last = end as i32 - 1;
        while last >= start as i32 && view[last as usize].hole {
            last -= 1;
        }
        if last >= start as i32 && last as usize == i && after_group_like(view, end) {
            return true;
        }
    }
    let row = &view[i];
    row.hole && is_group_like(row) && after_group_like(view, i + 1)
}

fn gap_after(view: &[ViewRow], i: usize) -> i32 {
    if group_end(view, i) {
        GROUP_END_GAP
    } else {
        ROW_GAP
    }
}

fn span_of(meta: &[LeafMeta], origin: usize) -> (usize, usize) {
    if meta.get(origin).is_none_or(|m| m.kind != LeafKind::Header) {
        return (origin, origin + 1);
    }
    let gid = meta[origin].group.as_deref();
    let mut end = origin + 1;
    while end < meta.len() && meta[end].kind == LeafKind::Item && meta[end].group.as_deref() == gid
    {
        end += 1;
    }
    (origin, end)
}

fn occupied_h(view: &[ViewRow], rest: &[Rest], start_view: usize, span_len: usize) -> i32 {
    if rest.is_empty() {
        return 32;
    }
    let mut last = start_view;
    let mut seen = 0usize;
    for (i, row) in view.iter().enumerate().skip(start_view) {
        if row.hole {
            continue;
        }
        seen += 1;
        last = i;
        if seen >= span_len {
            break;
        }
    }
    let a = rest.get(start_view).map(|r| r.y).unwrap_or(0);
    let b = rest.get(last).map(|r| r.y + r.h).unwrap_or(32);
    (b - a).max(1)
}

fn snapshot_visual(st: &StripState, now: Instant) -> HashMap<String, f32> {
    let mut m = HashMap::new();
    for (row, r) in st.view.iter().zip(st.rest.iter()) {
        if row.hole {
            continue;
        }
        let dy = st
            .flip
            .get(&row.id)
            .map(|a| a.interpolate_with(|v| v, now))
            .unwrap_or(0.0);
        m.insert(row.id.clone(), r.y as f32 + dy);
    }
    m
}

fn apply_dest(st: &mut StripState) -> bool {
    let Some(held) = st.held.as_ref() else {
        return false;
    };
    if st.rest.len() != st.view.len() {
        return false;
    }
    let next = slot_at(st.pointer, st.hole_at, &st.rest, &st.view, held.as_group);
    st.absorb = next.absorb;
    let Some(new_at) = move_hole(st.hole_at, next.slot) else {
        return false;
    };
    st.pending_visual = Some(snapshot_visual(st, Instant::now()));
    st.hole_at = new_at;
    true
}

fn gap_anim(v: f32) -> Animation<f32> {
    Animation::new(v)
        .duration(std::time::Duration::from_millis(PANEL_REORDER_ANIM_MS))
        .easing(Easing::EaseOut)
}

fn row_visual_y(st: &StripState, vi: usize, now: Instant) -> f32 {
    let y = st.rest.get(vi).map(|r| r.y as f32).unwrap_or(0.0);
    let Some(row) = st.view.get(vi) else {
        return y;
    };
    if row.hole {
        return y;
    }
    y + st
        .flip
        .get(&row.id)
        .map(|a| a.interpolate_with(|v| v, now))
        .unwrap_or(0.0)
}

fn rest_y_by_id(st: &StripState) -> HashMap<String, i32> {
    st.view
        .iter()
        .zip(st.rest.iter())
        .filter(|(row, _)| !row.hole)
        .map(|(row, r)| (row.id.clone(), r.y))
        .collect()
}

struct Held {
    start: usize,
    end: usize,
    ids: Vec<String>,
    as_group: bool,
    h: i32,
    grab: i32,
}

struct StripState {
    rest: Vec<Rest>,
    view: Vec<ViewRow>,
    press: Option<usize>,
    press_y: i32,
    press_grab: i32,
    dragging: bool,
    pointer: i32,
    held: Option<Held>,
    hole_at: usize,
    absorb: Option<String>,
    flip: HashMap<String, Animation<f32>>,
    pending_visual: Option<HashMap<String, f32>>,
    settling: bool,
    ids_at_release: Vec<String>,
    hole_origin: usize,
    /// Visual Y of each held id at release; FLIP into the committed slot.
    fly_from: HashMap<String, f32>,
}

impl Default for StripState {
    fn default() -> Self {
        Self {
            rest: Vec::new(),
            view: Vec::new(),
            press: None,
            press_y: 0,
            press_grab: 0,
            dragging: false,
            pointer: 0,
            held: None,
            hole_at: 0,
            absorb: None,
            flip: HashMap::new(),
            pending_visual: None,
            settling: false,
            ids_at_release: Vec::new(),
            hole_origin: 0,
            fly_from: HashMap::new(),
        }
    }
}

fn build_view(meta: &[LeafMeta], st: &StripState) -> Vec<ViewRow> {
    let held = st.held.as_ref();
    let mut rows = Vec::new();
    for (i, m) in meta.iter().enumerate() {
        if held.is_some_and(|h| i >= h.start && i < h.end) {
            continue;
        }
        rows.push(ViewRow {
            id: m.id.clone(),
            kind: kind_of(m),
            group: m.group.clone(),
            hole: false,
            leaf: Some(i),
        });
    }
    if let Some(h) = held {
        let hole = ViewRow {
            id: "empty".into(),
            kind: if h.as_group { Kind::Group } else { Kind::Item },
            group: None,
            hole: true,
            leaf: None,
        };
        let at = st.hole_at.min(rows.len());
        rows.insert(at, hole);
    }
    rows
}

fn drop_of(held: &Held, view: &[ViewRow], hole_at: usize, absorb: Option<String>) -> Option<Drop> {
    let id = held.ids.first()?.clone();
    if held.as_group {
        let before = view
            .iter()
            .skip(hole_at + 1)
            .find(|r| !r.hole)
            .map(|r| r.id.clone());
        return Some(Drop {
            id,
            dest: Dest::BlockBefore { before },
        });
    }
    if let Some(g) = absorb {
        let before = view.iter().skip(hole_at + 1).find_map(|r| {
            if !r.hole && r.kind == Kind::Item && r.group.as_deref() == Some(g.as_str()) {
                Some(r.id.clone())
            } else {
                None
            }
        });
        return Some(Drop {
            id,
            dest: Dest::Join { section: g, before },
        });
    }
    let next = view.iter().skip(hole_at + 1).find(|r| !r.hole);
    let dest = match next {
        Some(r) if r.kind == Kind::Group => Dest::BeforeGroup { id: r.id.clone() },
        Some(r) if r.kind == Kind::Item => match r.group.clone() {
            Some(section) => Dest::Join {
                section,
                before: Some(r.id.clone()),
            },
            None => Dest::Loose {
                before: Some(r.id.clone()),
            },
        },
        Some(r) => Dest::Loose {
            before: Some(r.id.clone()),
        },
        None => Dest::Loose { before: None },
    };
    Some(Drop { id, dest })
}

pub struct ReorderStrip<'a, Message> {
    leaves: Vec<Element<'a, Message>>,
    meta: Vec<LeafMeta>,
    on_action: std::rc::Rc<dyn Fn(Msg) -> Message + 'a>,
}

impl<'a, Message> ReorderStrip<'a, Message> {
    pub fn new(
        leaves: Vec<Element<'a, Message>>,
        meta: Vec<LeafMeta>,
        _spans: Vec<SectionSpan>,
        _item_spacing: f32,
        on_action: std::rc::Rc<dyn Fn(Msg) -> Message + 'a>,
    ) -> Self {
        Self {
            leaves,
            meta,
            on_action,
        }
    }
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for ReorderStrip<'_, Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<StripState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(StripState::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.leaves.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.leaves);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if tree.children.len() != self.leaves.len() {
            tree.diff_children(&self.leaves);
        }
        let width = px(limits.max().width);
        let st = tree.state.downcast_mut::<StripState>();
        if st.settling && !st.ids_at_release.is_empty() {
            let ids: Vec<String> = self.meta.iter().map(|m| m.id.clone()).collect();
            if ids != st.ids_at_release {
                st.held = None;
                st.settling = false;
                st.hole_at = 0;
                st.absorb = None;
                st.pending_visual = None;
            }
        }
        let fly_from = if st.held.is_none() {
            std::mem::take(&mut st.fly_from)
        } else {
            HashMap::new()
        };
        if !fly_from.is_empty() {
            st.flip.clear();
        }
        let skip_others = !fly_from.is_empty();
        let view = build_view(&self.meta, st);
        let now = Instant::now();
        let pending = if skip_others {
            None
        } else {
            st.pending_visual.take()
        };
        let old_y = if skip_others {
            HashMap::new()
        } else {
            rest_y_by_id(st)
        };
        let old_visual = if skip_others {
            HashMap::new()
        } else {
            snapshot_visual(st, now)
        };

        let mut y = PAD_TOP;
        let mut rest = Vec::with_capacity(view.len());
        let mut pos: Vec<Option<(i32, i32, f32)>> = vec![None; self.leaves.len()];
        let mut nodes: Vec<layout::Node> = vec![layout::Node::new(Size::ZERO); self.leaves.len()];

        for (vi, row) in view.iter().enumerate() {
            if row.hole {
                let h = st.held.as_ref().map(|h| h.h).unwrap_or(32);
                rest.push(Rest { y, h });
                y += h + gap_after(&view, vi);
                continue;
            }
            let i = row.leaf.expect("leaf");
            let inset = if row.kind == Kind::Group {
                TITLE_INSET
            } else {
                ROW_INSET
            };
            let child_limits = limits.loose().max_width((width - inset * 2.0).max(0.0));
            let child = self.leaves[i].as_widget_mut().layout(
                &mut tree.children[i],
                renderer,
                &child_limits,
            );
            let h = px(child.size().height).max(1.0) as i32;
            rest.push(Rest { y, h });
            pos[i] = Some((y, h, inset));
            nodes[i] = child;
            y += h + gap_after(&view, vi);
        }
        y += PAD_BOT;

        if let Some(held) = st.held.as_ref() {
            let ghost_y = if st.dragging {
                st.pointer - held.grab - if held.as_group { WELL_PAD } else { 0 }
            } else {
                st.rest.get(st.hole_at).map(|r| r.y).unwrap_or(PAD_TOP)
            };
            let mut gy = ghost_y;
            for i in held.start..held.end {
                let inset = if self.meta[i].kind == LeafKind::Header {
                    TITLE_INSET
                } else {
                    ROW_INSET
                };
                let child_limits = limits.loose().max_width((width - inset * 2.0).max(0.0));
                let child = self.leaves[i].as_widget_mut().layout(
                    &mut tree.children[i],
                    renderer,
                    &child_limits,
                );
                let h = px(child.size().height).max(1.0) as i32;
                pos[i] = Some((gy, h, inset));
                nodes[i] = child;
                gy += h + ROW_GAP;
            }
        }

        for row in &view {
            if row.hole {
                continue;
            }
            let Some(i) = row.leaf else { continue };
            let Some((ny, _, _)) = pos[i] else { continue };
            let y0 = pending
                .as_ref()
                .and_then(|v| v.get(&row.id).copied())
                .or_else(|| {
                    let old = *old_y.get(&row.id)?;
                    if old == ny {
                        None
                    } else {
                        Some(old_visual.get(&row.id).copied().unwrap_or(old as f32))
                    }
                });
            if let Some(y0) = y0 {
                let dy = y0 - ny as f32;
                if dy.abs() >= 0.5 {
                    let mut a = gap_anim(dy);
                    a.go_mut(0.0, now);
                    st.flip.insert(row.id.clone(), a);
                } else {
                    st.flip.remove(&row.id);
                }
            }
        }
        for (id, from_y) in fly_from {
            let Some(row) = view.iter().find(|r| r.id == id) else {
                continue;
            };
            let Some(i) = row.leaf else { continue };
            let Some((ny, _, _)) = pos[i] else { continue };
            let dy = from_y - ny as f32;
            if dy.abs() >= 0.5 {
                let mut a = gap_anim(dy);
                a.go_mut(0.0, now);
                st.flip.insert(id, a);
            }
        }

        for (i, node) in nodes.iter_mut().enumerate() {
            if let Some((py, _, inset)) = pos[i] {
                *node = std::mem::replace(node, layout::Node::new(Size::ZERO))
                    .move_to(Point::new(inset, py as f32));
            }
        }

        st.rest = rest;
        st.view = view;
        layout::Node::with_children(Size::new(width, y.max(0) as f32), nodes)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let st = tree.state.downcast_ref::<StripState>();
        if !st.dragging {
            for (i, ((child_layout, child_tree), child)) in layout
                .children()
                .zip(tree.children.iter_mut())
                .zip(self.leaves.iter_mut())
                .enumerate()
            {
                let _ = i;
                child.as_widget_mut().update(
                    child_tree,
                    event,
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                );
            }
            if shell.is_event_captured() {
                return;
            }
        }

        let bounds = layout.bounds();
        let st = tree.state.downcast_mut::<StripState>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(pos) = cursor.position_in(bounds) else {
                    return;
                };
                let y = px(pos.y).round() as i32;
                let Some(vi) = st
                    .rest
                    .iter()
                    .position(|r| y >= r.y && y < r.y + r.h.max(1))
                else {
                    return;
                };
                let Some(leaf) = st.view.get(vi).and_then(|r| r.leaf) else {
                    return;
                };
                if st.view[vi].hole {
                    return;
                }
                st.press = Some(leaf);
                st.press_y = y;
                st.press_grab = y - st.rest[vi].y;
                st.pointer = y;
                st.dragging = false;
                st.held = None;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(origin) = st.press else {
                    return;
                };
                let Some(abs) = cursor.land().position() else {
                    return;
                };
                let y = px(abs.y - bounds.y).round() as i32;
                st.pointer = y;
                if !st.dragging && (y - st.press_y).abs() < PANEL_REORDER_THRESHOLD as i32 {
                    return;
                }
                if !st.dragging {
                    let (start, end) = span_of(&self.meta, origin);
                    let vi = st
                        .view
                        .iter()
                        .position(|r| r.leaf == Some(start))
                        .unwrap_or(0);
                    let h = occupied_h(&st.view, &st.rest, vi, end - start);
                    let grab = st.rest.get(vi).map(|r| y - r.y).unwrap_or(st.press_grab);
                    let ids: Vec<String> = (start..end.min(self.meta.len()))
                        .map(|i| self.meta[i].id.clone())
                        .collect();
                    st.held = Some(Held {
                        start,
                        end,
                        ids,
                        as_group: self
                            .meta
                            .get(origin)
                            .is_some_and(|m| m.kind == LeafKind::Header),
                        h: h.max(1),
                        grab,
                    });
                    st.hole_at = vi;
                    st.hole_origin = vi;
                    st.settling = false;
                    st.dragging = true;
                    st.flip.clear();
                }
                // Ghost Y is applied in `layout` from `pointer`. Without a
                // relayout, draw keeps the last ghost and the drag stutters.
                // (Idle chrome must not vsync-present; this is drag-only.)
                shell.invalidate_layout();
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let Some(origin) = st.press.take() else {
                    return;
                };
                let was = st.dragging;
                st.dragging = false;
                if !was {
                    st.held = None;
                    if let Some(m) = self.meta.get(origin) {
                        let ev = match m.kind {
                            LeafKind::Header => SidebarEvent::ToggleSection { id: m.id.clone() },
                            LeafKind::Item => SidebarEvent::Activate { id: m.id.clone() },
                        };
                        shell.publish((self.on_action)(Msg::Outcome(ev)));
                    }
                } else if let Some(held) = st.held.as_ref() {
                    let dest = drop_of(held, &st.view, st.hole_at, st.absorb.clone());
                    let pad = if held.as_group { WELL_PAD } else { 0 };
                    let from_y = st.pointer - held.grab - pad;
                    let mut fly = HashMap::new();
                    let mut y = from_y as f32;
                    for i in held.start..held.end.min(self.meta.len()) {
                        fly.insert(self.meta[i].id.clone(), y);
                        let h = 32.0;
                        y += h + ROW_GAP as f32;
                    }
                    st.flip.clear();
                    st.fly_from = fly;
                    if st.hole_at == st.hole_origin {
                        st.held = None;
                        st.hole_at = 0;
                        st.absorb = None;
                    } else {
                        st.settling = true;
                        st.ids_at_release = self.meta.iter().map(|m| m.id.clone()).collect();
                        if let Some(drop) = dest {
                            shell.publish((self.on_action)(Msg::Outcome(SidebarEvent::Drop(drop))));
                        }
                    }
                }
                shell.invalidate_layout();
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if st.dragging && !st.settling && apply_dest(st) {
                    shell.invalidate_layout();
                }
                let flipping = st.flip.values().any(|a| a.is_animating(*now));
                if flipping {
                    shell.invalidate_layout();
                }
                // Morph2: one hole hop per frame. Keep vsync only while the
                // gesture or FLIP is live — not as an idle chrome pump.
                if st.dragging || st.settling || flipping {
                    shell.request_redraw();
                }
            }
            _ => {}
        }
        let _ = renderer;
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let st = tree.state.downcast_ref::<StripState>();
        let now = Instant::now();
        let children: Vec<Layout<'_>> = layout.children().collect();
        let well = group_well_style();
        for (start, end) in group_spans(&st.view, st.absorb.as_deref()) {
            if start >= st.rest.len() || end == 0 || end - 1 >= st.rest.len() {
                continue;
            }
            let first_y = row_visual_y(st, start, now);
            let last = st.rest[end - 1];
            let last_bottom = row_visual_y(st, end - 1, now) + last.h as f32;
            let top = first_y - WELL_PAD as f32;
            let bottom = if end < st.rest.len() {
                last_bottom.min(row_visual_y(st, end, now) - WELL_PAD as f32)
            } else {
                last_bottom + WELL_PAD as f32
            };
            let bounds = Rectangle {
                x: layout.bounds().x + 2.0,
                y: layout.bounds().y + top,
                width: (layout.bounds().width - 4.0).max(0.0),
                height: (bottom - top).max(0.0),
            };
            if let Some(bg) = well.background {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: well.border,
                        shadow: well.shadow,
                        snap: well.snap,
                    },
                    bg,
                );
            }
        }

        let held = st.held.as_ref();
        for (i, ((child_layout, child_tree), child)) in children
            .iter()
            .zip(tree.children.iter())
            .zip(self.leaves.iter())
            .enumerate()
        {
            let is_held = held.is_some_and(|h| i >= h.start && i < h.end);
            if is_held {
                continue;
            }
            let id = self.meta.get(i).map(|m| m.id.as_str()).unwrap_or("");
            let dy = st
                .flip
                .get(id)
                .map(|a| a.interpolate_with(|v| v, now))
                .unwrap_or(0.0);
            if dy.abs() >= 0.5 {
                renderer.with_transformation(Transformation::translate(0.0, dy), |renderer| {
                    child.as_widget().draw(
                        child_tree,
                        renderer,
                        theme,
                        style,
                        *child_layout,
                        cursor,
                        viewport,
                    );
                });
            } else {
                child.as_widget().draw(
                    child_tree,
                    renderer,
                    theme,
                    style,
                    *child_layout,
                    cursor,
                    viewport,
                );
            }
        }

        if let Some(held) = held {
            if st.dragging || st.settling {
                renderer.with_layer(*viewport, |renderer| {
                    let mut plate: Option<Rectangle> = None;
                    let show_plate = st.dragging;
                    for i in held.start..held.end {
                        if let Some(child_layout) = children.get(i) {
                            let b = child_layout.bounds();
                            plate = Some(match plate {
                                None => b,
                                Some(p) => {
                                    let x = p.x.min(b.x);
                                    let y = p.y.min(b.y);
                                    let r = (p.x + p.width).max(b.x + b.width);
                                    let bot = (p.y + p.height).max(b.y + b.height);
                                    Rectangle {
                                        x,
                                        y,
                                        width: r - x,
                                        height: bot - y,
                                    }
                                }
                            });
                        }
                    }
                    if show_plate {
                        if let Some(b) = plate {
                            if held.as_group {
                                let bounds = Rectangle {
                                    x: layout.bounds().x + 2.0,
                                    y: b.y - WELL_PAD as f32,
                                    width: (layout.bounds().width - 4.0).max(0.0),
                                    height: b.height + 2.0 * WELL_PAD as f32,
                                };
                                if let Some(bg) = well.background {
                                    let bg = match bg {
                                        Background::Color(c) => Background::Color(alpha(c, 0.8)),
                                        other => other,
                                    };
                                    let mut border = well.border;
                                    border.color = alpha(border.color, 0.8);
                                    renderer.fill_quad(
                                        renderer::Quad {
                                            bounds,
                                            border,
                                            shadow: Shadow {
                                                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                                                offset: Vector::new(0.0, 2.0),
                                                blur_radius: 8.0,
                                            },
                                            snap: well.snap,
                                        },
                                        bg,
                                    );
                                }
                            } else {
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle {
                                            x: layout.bounds().x + 1.0,
                                            y: b.y,
                                            width: (layout.bounds().width - 2.0).max(0.0),
                                            height: b.height,
                                        },
                                        border: iced::Border {
                                            radius: 4.0.into(),
                                            color: Color::TRANSPARENT,
                                            width: 0.0,
                                        },
                                        shadow: Shadow {
                                            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                                            offset: Vector::new(0.0, 2.0),
                                            blur_radius: 8.0,
                                        },
                                        snap: true,
                                    },
                                    Background::Color(Color {
                                        a: 0.5,
                                        ..CHROME_SURFACE
                                    }),
                                );
                            }
                        }
                    }
                    for i in held.start..held.end {
                        if let (Some(child_layout), Some(child_tree), Some(child)) =
                            (children.get(i), tree.children.get(i), self.leaves.get(i))
                        {
                            child.as_widget().draw(
                                child_tree,
                                renderer,
                                theme,
                                style,
                                *child_layout,
                                cursor,
                                viewport,
                            );
                        }
                    }
                });
            }
        }
        let _ = style;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        // Without this, focus / select-all never reach an inline header
        // field (the default Widget::operate is a no-op).
        if tree.children.len() != self.leaves.len() {
            tree.diff_children(&self.leaves);
        }
        for ((child, child_tree), child_layout) in self
            .leaves
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(child_tree, child_layout, renderer, operation);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let st = tree.state.downcast_ref::<StripState>();
        if st.dragging {
            return mouse::Interaction::Grabbing;
        }
        self.leaves
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
            .map(|((child, child_tree), child_layout)| {
                child.as_widget().mouse_interaction(
                    child_tree,
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or(mouse::Interaction::None)
    }
}

impl<'a, Message: Clone + 'a> From<ReorderStrip<'a, Message>> for Element<'a, Message> {
    fn from(value: ReorderStrip<'a, Message>) -> Self {
        Element::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, kind: Kind, y: i32) -> (ViewRow, Rest) {
        (
            ViewRow {
                id: id.into(),
                kind,
                group: match kind {
                    Kind::Group => Some(id.into()),
                    Kind::Item => Some("c".into()),
                    Kind::Loose => None,
                },
                hole: id == "empty",
                leaf: None,
            },
            Rest { y, h: 32 },
        )
    }

    fn split(rows: Vec<(ViewRow, Rest)>) -> (Vec<ViewRow>, Vec<Rest>) {
        rows.into_iter().unzip()
    }

    #[test]
    fn hole_stay_is_origin_plus_one() {
        assert_eq!(move_hole(4, 4), None);
        assert_eq!(move_hole(4, 5), None);
        assert_eq!(move_hole(4, 6), Some(5));
        assert_eq!(move_hole(4, 2), Some(2));
    }

    #[test]
    fn interior_yields_immediately_down() {
        // H C1 C2 empty(C3) C4 C5  — wait C3 held, hole at 3, C4 at 4
        let (view, rects) = split(vec![
            row("c", Kind::Group, 0),
            row("c1", Kind::Item, 32),
            row("c2", Kind::Item, 64),
            row("empty", Kind::Item, 96),
            row("c4", Kind::Item, 128),
            row("c5", Kind::Item, 160),
            row("u1", Kind::Loose, 192),
        ]);
        // pointer on C4 (below hole): yield after C4
        let s = slot_at(140, 3, &rects, &view, false);
        assert_eq!(s.slot, 5);
    }

    #[test]
    fn interior_yields_immediately_up() {
        let (view, rects) = split(vec![
            row("c", Kind::Group, 0),
            row("c1", Kind::Item, 32),
            row("c2", Kind::Item, 64),
            row("c3", Kind::Item, 96),
            row("empty", Kind::Item, 128),
            row("c5", Kind::Item, 160),
        ]);
        // pointer on C3: insert before C3
        let s = slot_at(110, 4, &rects, &view, false);
        assert_eq!(s.slot, 3);
    }

    #[test]
    fn last_member_bottom_half_absorbs() {
        let (view, rects) = split(vec![
            row("c", Kind::Group, 0),
            row("c5", Kind::Item, 32),
            row("empty", Kind::Item, 64),
            row("u1", Kind::Loose, 96),
        ]);
        let s = slot_at(32 + 20, 2, &rects, &view, false);
        assert_eq!(s.absorb.as_deref(), Some("c"));
    }

    #[test]
    fn first_after_top_half_leaves() {
        let (view, rects) = split(vec![
            row("c", Kind::Group, 0),
            row("c5", Kind::Item, 32),
            row("empty", Kind::Item, 64),
            row("u1", Kind::Loose, 96),
        ]);
        let s = slot_at(96 + 4, 2, &rects, &view, false);
        assert_eq!(s.absorb, None);
    }
}
