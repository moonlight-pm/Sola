//! Map / Strata: each column a category, each row how far a package
//! stands from the ground (what it needs, all the way down). Brighter
//! blocks hold up more. Hover or click a package and its tree lights: up,
//! what uses it, in white; down, what it needs, in cyan; everything else
//! steps back. Packages keep their place between visits, so the picture
//! can stick: only a new need moves one.

use std::collections::{BTreeMap, BTreeSet};

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use sola_kit::components::style::{alpha, mix_white};
use sola_kit::fonts;

use super::ink::Ink;
use super::mark::{self, Kind};
use super::paint::{dashed, fit, label, line, rounded, vcurve};
use super::words::size;
use super::{CAPTION, SMALL};
use crate::Msg;
use crate::model::{Ecosystem, Package};
use crate::standing::Standing;

const LV_W: f32 = 54.0;
const GAP: f32 = 6.0;
const ROW_PAD: f32 = 14.0;
const GROUND_H: f32 = 44.0;
const TRAY_W: f32 = 128.0;

pub struct Strata<'a> {
    pub e: &'a Ecosystem,
    pub sel: Option<&'a str>,
}

struct Layout {
    blocks: Vec<(String, Rectangle)>,
    /// `(depth, top y)` for each row's rule and number.
    rows: Vec<(usize, f32)>,
    ground: f32,
    columns: Vec<(String, f32, f32)>,
    trays: Vec<(String, f32)>,
    block_h: f32,
}

/// Where each package stands: its depth row, unless it stands alone (no
/// needs, no users) or is only on the origin, which go in the trays.
fn layout(e: &Ecosystem, bounds: Size) -> Layout {
    let alone = |p: &Package| e.graph.needs(&p.name).is_empty() && e.graph.users(&p.name).is_empty();
    let placed = |p: &Package| p.standing != Standing::OriginOnly && !alone(p);
    // Only the categories something here stands in; the rest are trays.
    let cats: Vec<&String> = e.categories.iter().filter(|c| e.pkgs.values().any(|p| &p.category == *c && placed(p))).collect();
    let n = cats.len().max(1) as f32;
    let col_w = ((bounds.width - LV_W - GAP * n) / n).max(20.0);
    let col_x = |i: usize| LV_W + GAP + i as f32 * (col_w + GAP);
    let mut cells: BTreeMap<(usize, usize), Vec<&Package>> = BTreeMap::new();
    let mut lone: Vec<&Package> = Vec::new();
    let mut origin: Vec<&Package> = Vec::new();
    for p in e.pkgs.values() {
        if p.standing == Standing::OriginOnly {
            origin.push(p);
        } else if alone(p) {
            lone.push(p);
        } else {
            let c = cats.iter().position(|c| **c == p.category).unwrap_or(0);
            cells.entry((e.graph.depth(&p.name), c)).or_default().push(p);
        }
    }
    for ps in cells.values_mut() {
        ps.sort_by(|a, b| e.reach(&b.name).1.cmp(&e.reach(&a.name).1).then(a.name.cmp(&b.name)));
    }
    let max_d = cells.keys().map(|(d, _)| *d).max().unwrap_or(0);
    let tall: Vec<usize> =
        (0..=max_d).map(|d| cells.iter().filter(|((dd, _), _)| *dd == d).map(|(_, v)| v.len()).max().unwrap_or(0)).collect();
    let per_line = (((bounds.width - LV_W - GAP) / (TRAY_W + 2.0)).floor() as usize).max(1);
    let tray_lines: usize = [lone.len(), origin.len()].iter().filter(|n| **n > 0).map(|n| n.div_ceil(per_line)).sum();
    let tray_caps = [lone.len(), origin.len()].iter().filter(|n| **n > 0).count();
    // The block height that fits it all, between legible and roomy.
    let stacks: usize = tall.iter().sum::<usize>() + tray_lines;
    let fixed = GROUND_H + ROW_PAD * (max_d + 1) as f32 + 22.0 * tray_caps as f32 + 12.0;
    let block_h = ((bounds.height - fixed) / stacks.max(1) as f32 - 3.0).clamp(14.0, 28.0);
    let step = block_h + 3.0;

    let rows_h: f32 = tall.iter().map(|t| *t as f32 * step + ROW_PAD).sum();
    let trays_h = tray_lines as f32 * (block_h + 2.0) + 22.0 * tray_caps as f32;
    let top = (bounds.height - rows_h - GROUND_H - trays_h - 12.0).max(0.0);
    let mut y = top;
    let mut rows = Vec::new();
    let mut blocks = Vec::new();
    for d in (0..=max_d).rev() {
        rows.push((d, y));
        for c in 0..cats.len() {
            for (i, p) in cells.get(&(d, c)).into_iter().flatten().enumerate() {
                let r = Rectangle::new(Point::new(col_x(c), y + ROW_PAD / 2.0 + i as f32 * step), Size::new(col_w, block_h));
                blocks.push((p.name.clone(), r));
            }
        }
        y += tall[d] as f32 * step + ROW_PAD;
    }
    let ground = y;
    let columns = cats.iter().enumerate().map(|(i, c)| ((*c).clone(), col_x(i), col_w)).collect();
    let mut ty = ground + GROUND_H + 12.0;
    let mut trays = Vec::new();
    for (title, ps) in [("Stand alone", lone), ("On origin only", origin)] {
        if ps.is_empty() {
            continue;
        }
        trays.push((format!("{title} {}", ps.len()), ty));
        ty += 22.0;
        for (i, p) in ps.iter().enumerate() {
            let (line_i, k) = (i / per_line, i % per_line);
            let r = Rectangle::new(
                Point::new(LV_W + GAP + k as f32 * (TRAY_W + 2.0), ty + line_i as f32 * (block_h + 2.0)),
                Size::new(TRAY_W, block_h),
            );
            blocks.push((p.name.clone(), r));
        }
        ty += ps.len().div_ceil(per_line) as f32 * (block_h + 2.0);
    }
    Layout { blocks, rows, ground, columns, trays, block_h }
}

impl Layout {
    fn at(&self, p: Point) -> Option<&str> {
        self.blocks.iter().find(|(_, r)| r.contains(p)).map(|(n, _)| n.as_str())
    }
}

#[derive(Default)]
pub struct Hover(Option<String>);

impl canvas::Program<Msg> for Strata<'_> {
    type State = Hover;

    fn update(&self, state: &mut Hover, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Msg>> {
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let now = cursor.position_in(bounds).and_then(|p| layout(self.e, bounds.size()).at(p).map(str::to_string));
                if now != state.0 {
                    state.0 = now;
                    return Some(canvas::Action::request_redraw());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => {
                state.0 = None;
                Some(canvas::Action::request_redraw())
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let msg = match layout(self.e, bounds.size()).at(p) {
                    Some(n) => Msg::Select(n.to_string()),
                    None => Msg::Deselect,
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &Hover, renderer: &Renderer, theme: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let ink = Ink::of(theme);
        let mut f = Frame::new(renderer, bounds.size());
        let l = layout(self.e, bounds.size());
        let row_rule = mix_white(ink.bg, 0.04);
        for (d, y) in &l.rows {
            f.stroke(&canvas::Path::line(Point::new(0.0, *y), Point::new(bounds.width, *y)), line(row_rule, 1.0));
            label(&mut f, d.to_string(), Point::new(0.0, y + ROW_PAD / 2.0 + l.block_h / 2.0), CAPTION, ink.fg4, fonts::mono());
        }
        f.stroke(&canvas::Path::line(Point::new(0.0, l.ground), Point::new(bounds.width, l.ground)), line(ink.rule2, 2.0));
        for (c, x, w) in &l.columns {
            let ps: Vec<&Package> = self.e.pkgs.values().filter(|p| &p.category == c).collect();
            let weight: u64 = ps.iter().map(|p| p.in_use()).sum();
            label(&mut f, fit(c, *w, SMALL), Point::new(*x, l.ground + 15.0), SMALL, ink.fg, fonts::ui_medium());
            label(&mut f, fit(&format!("{} · {}", ps.len(), size(weight)), *w, CAPTION), Point::new(*x, l.ground + 31.0), CAPTION, ink.fg3, fonts::mono());
        }
        for (t, y) in &l.trays {
            label(&mut f, t.clone(), Point::new(LV_W + GAP, y + 9.0), CAPTION, ink.fg3, fonts::ui());
        }

        let focus = state.0.as_deref().or(self.sel).filter(|n| self.e.get(n).is_some());
        let lit = focus.map(|id| {
            let up = self.e.graph.above(id);
            let down = self.e.graph.below(id);
            let direct: BTreeSet<String> =
                self.e.graph.users(id).iter().chain(self.e.graph.needs(id)).cloned().collect();
            (id, up, down, direct)
        });
        let rect: BTreeMap<&str, Rectangle> = l.blocks.iter().map(|(n, r)| (n.as_str(), *r)).collect();
        if let Some((id, up, down, _)) = &lit {
            for (cone, color) in [(up, ink.up), (down, ink.accent)] {
                for u in cone.iter().map(String::as_str).chain([*id]) {
                    for n in self.e.graph.needs(u) {
                        if !(cone.contains(n) || n == id) || !self.e.graph.drawn(u, n) {
                            continue;
                        }
                        let (Some(a), Some(b)) = (rect.get(u), rect.get(n.as_str())) else { continue };
                        if let Some(path) = vcurve(*a, *b) {
                            let hot = u == *id || n == id;
                            f.stroke(&path, line(if hot { color } else { alpha(color, 0.24) }, if hot { 1.4 } else { 1.0 }));
                        }
                    }
                }
            }
        }
        for (name, r) in &l.blocks {
            let Some(p) = self.e.get(name) else { continue };
            let state = match &lit {
                None => Lit::Rest,
                Some((id, ..)) if *id == name => Lit::Selfish,
                Some((_, _, _, direct)) if direct.contains(name) => Lit::Direct,
                Some((_, up, down, _)) if up.contains(name) || down.contains(name) => Lit::Cone,
                Some(_) => Lit::Dim,
            };
            block(&mut f, &ink, self.e, p, *r, state);
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &Hover, _: Rectangle, _: mouse::Cursor) -> mouse::Interaction {
        if state.0.is_some() { mouse::Interaction::Pointer } else { mouse::Interaction::default() }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Lit {
    Rest,
    Selfish,
    Direct,
    Cone,
    Dim,
}

/// How much a package holds up: brighter the more there is above it.
fn load(e: &Ecosystem, p: &Package) -> u8 {
    match e.reach(&p.name).1 {
        u if u >= 30 => 3,
        u if u >= 10 => 2,
        u if u >= 2 => 1,
        _ => 0,
    }
}

fn block(f: &mut Frame, ink: &Ink, e: &Ecosystem, p: &Package, r: Rectangle, lit: Lit) {
    let fade = |c: Color| if lit == Lit::Dim { alpha(c, c.a * 0.4) } else { c };
    let (mut fill, mut text) = match load(e, p) {
        3 => (mix_white(ink.bg, 0.15), ink.fg),
        2 => (ink.face3, ink.fg),
        1 => (ink.face2, ink.fg2),
        _ => (ink.face2, ink.fg3),
    };
    match lit {
        Lit::Cone => (fill, text) = (ink.face3, ink.fg),
        Lit::Direct => (fill, text) = (mix_white(ink.bg, 0.18), ink.fg),
        _ => {}
    }
    let shape = rounded(r, 3.0);
    if lit == Lit::Selfish {
        f.fill(&rounded(Rectangle { x: r.x - 3.0, y: r.y - 3.0, width: r.width + 6.0, height: r.height + 6.0 }, 5.0), alpha(ink.accent, 0.25));
        f.fill(&shape, ink.accent);
        let on = Color { a: 1.0, ..ink.bg };
        label(f, fit(&p.name, r.width - 16.0, SMALL), Point::new(r.x + 8.0, r.center_y()), SMALL, on, fonts::ui_medium());
        return;
    }
    match p.standing {
        Standing::Off => f.stroke(&shape, line(fade(ink.rule2), 1.0)),
        Standing::OriginOnly => f.stroke(&shape, dashed(fade(alpha(ink.accent, 0.4)), 1.0)),
        Standing::NotPushed => {
            f.fill(&shape, fade(fill));
            f.stroke(&shape, line(fade(ink.warning), 1.0));
            text = ink.warning;
        }
        _ => f.fill(&shape, fade(fill)),
    }
    let bar = if p.advised() {
        Some(ink.danger)
    } else {
        match p.standing {
            Standing::Local => Some(ink.warning),
            Standing::Update => Some(ink.accent),
            _ => None,
        }
    };
    if let Some(c) = bar {
        f.fill(&canvas::Path::rectangle(Point::new(r.x, r.y), Size::new(2.0, r.height)), fade(c));
    }
    let mut x = r.x + 8.0;
    let size = if r.height < 18.0 { CAPTION } else { SMALL };
    if p.standing != Standing::Current {
        mark::draw(f, ink, Kind::Standing(p.standing), Point::new(x + 4.0, r.center_y()), 0.85);
        x += 14.0;
    }
    let room = r.x + r.width - x - if p.advised() { 16.0 } else { 6.0 };
    let name = fit(&p.name, room, size);
    let shown = name.chars().count() as f32 * size * 0.56;
    label(f, name, Point::new(x, r.center_y()), size, fade(text), fonts::ui());
    if p.advised() {
        mark::draw(f, ink, Kind::Advisory, Point::new((x + shown + 8.0).min(r.x + r.width - 8.0), r.center_y()), 0.8);
    }
}
