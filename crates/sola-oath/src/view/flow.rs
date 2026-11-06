//! Tree / Flow: one package in the middle, what uses it to the left and
//! what it needs to the right, a column per step away (three and more
//! folded into one). Each column uses the one to its right. Hover a
//! package to light the paths between it and the middle; click one to
//! move there.

use std::collections::{BTreeMap, BTreeSet};

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Point, Rectangle, Renderer, Size, Theme};
use sola_kit::components::style::alpha;
use sola_kit::fonts;

use super::ink::Ink;
use super::mark::{self, Kind};
use super::paint::{fit, hcurve, label, line, rounded};
use super::{CAPTION, SMALL};
use crate::Msg;
use crate::model::Ecosystem;

const CAP_H: f32 = 38.0;
const NODE_H: f32 = 24.0;
const SELF_H: f32 = 34.0;
const STEP: f32 = 27.0;
const COL_GAP: f32 = 56.0;

pub struct Flow<'a> {
    pub e: &'a Ecosystem,
    pub id: &'a str,
}

struct Column {
    title: String,
    count: usize,
    names: Vec<String>,
}

fn fold(ls: Vec<Vec<String>>, keep: usize) -> Vec<Vec<String>> {
    if ls.len() <= keep {
        return ls;
    }
    let mut out: Vec<Vec<String>> = ls[..keep - 1].to_vec();
    let mut rest: Vec<String> = ls[keep - 1..].iter().flatten().cloned().collect();
    rest.sort();
    out.push(rest);
    out
}

fn away(i: usize) -> &'static str {
    match i {
        0 => "direct",
        1 => "2 away",
        _ => "3 or more",
    }
}

impl Flow<'_> {
    fn columns(&self) -> Vec<Column> {
        let up = fold(self.e.graph.levels_up(self.id), 3);
        let down = fold(self.e.graph.levels_down(self.id), 3);
        let mut cols: Vec<Column> = up
            .into_iter()
            .enumerate()
            .rev()
            .map(|(i, names)| Column { title: format!("Uses it, {}", away(i)), count: names.len(), names })
            .collect();
        cols.push(Column { title: "This package".into(), count: 0, names: vec![self.id.to_string()] });
        cols.extend(
            down.into_iter()
                .enumerate()
                .map(|(i, names)| Column { title: format!("Needs, {}", away(i)), count: names.len(), names }),
        );
        cols
    }

    /// The canvas height the tallest column needs.
    pub fn height(&self) -> f32 {
        let tallest = self.columns().iter().map(|c| c.names.len()).max().unwrap_or(1);
        CAP_H + tallest as f32 * STEP + SELF_H
    }

    fn layout(&self, bounds: Size) -> (Vec<(String, usize, f32)>, BTreeMap<String, Rectangle>) {
        let cols = self.columns();
        let n = cols.len() as f32;
        let w = ((bounds.width - COL_GAP * (n - 1.0)) / n).max(40.0);
        let mut caps = Vec::new();
        let mut rects = BTreeMap::new();
        for (i, c) in cols.iter().enumerate() {
            let x = i as f32 * (w + COL_GAP);
            caps.push((c.title.clone(), c.count, x));
            if c.names.len() == 1 && c.names[0] == self.id {
                let y = (CAP_H + (bounds.height - CAP_H) / 2.0 - SELF_H / 2.0).max(CAP_H);
                rects.insert(self.id.to_string(), Rectangle::new(Point::new(x, y), Size::new(w, SELF_H)));
                continue;
            }
            for (k, name) in c.names.iter().enumerate() {
                rects.insert(name.clone(), Rectangle::new(Point::new(x, CAP_H + k as f32 * STEP), Size::new(w, NODE_H)));
            }
        }
        (caps, rects)
    }
}

#[derive(Default)]
pub struct Hover(Option<String>);

impl canvas::Program<Msg> for Flow<'_> {
    type State = Hover;

    fn update(&self, state: &mut Hover, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Msg>> {
        let at = |p: Point| {
            let (_, rects) = self.layout(bounds.size());
            rects.into_iter().find(|(_, r)| r.contains(p)).map(|(n, _)| n)
        };
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let now = cursor.position_in(bounds).and_then(at);
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
                let name = at(cursor.position_in(bounds)?)?;
                (name != self.id).then(|| canvas::Action::publish(Msg::TreeTo(name)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &Hover, renderer: &Renderer, theme: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let ink = Ink::of(theme);
        let mut f = Frame::new(renderer, bounds.size());
        let (caps, rects) = self.layout(bounds.size());
        for (t, n, x) in &caps {
            label(&mut f, t.clone(), Point::new(*x, 7.0), CAPTION, ink.fg2, fonts::ui_medium());
            if *n > 0 {
                label(&mut f, n.to_string(), Point::new(*x, 22.0), CAPTION, ink.fg3, fonts::ui());
            }
        }
        let hot: Option<BTreeSet<String>> = state.0.as_deref().and_then(|h| self.e.graph.between(self.id, h));
        let up = self.e.graph.above(self.id);
        let down = self.e.graph.below(self.id);
        let side = |x: &str, cone: &BTreeSet<String>| x == self.id || cone.contains(x);
        for (u, a) in &rects {
            for n in self.e.graph.needs(u) {
                let Some(b) = rects.get(n) else { continue };
                // An edge within one side; one that skips past the middle
                // (a user that needs one of its needs too) is left out.
                let within = (side(u, &up) && side(n, &up)) || (side(u, &down) && side(n, &down));
                if !within || !self.e.graph.drawn(u, n) {
                    continue;
                }
                let Some(path) = hcurve(*a, *b) else { continue };
                // Left of the middle is what uses it; right, what it needs.
                let color = if up.contains(u) { ink.up } else { ink.accent };
                let touches = u == self.id || n == self.id;
                let (c, w) = match &hot {
                    Some(h) if h.contains(u) && h.contains(n) => (color, 1.4),
                    Some(_) => (alpha(color, 0.03), 1.0),
                    None if touches => (color, 1.4),
                    None => (alpha(color, 0.24), 1.0),
                };
                f.stroke(&path, line(c, w));
            }
        }
        for (name, r) in &rects {
            let me = name == self.id;
            let dim = hot.as_ref().is_some_and(|h| !h.contains(name));
            let lit = hot.as_ref().is_some_and(|h| h.contains(name)) && !me;
            let shape = rounded(*r, 4.0);
            if me {
                f.fill(&shape, alpha(ink.accent, 0.14));
                f.stroke(&shape, line(alpha(ink.accent, 0.28), 1.0));
            } else {
                f.fill(&shape, if lit || state.0.as_deref() == Some(name) { ink.face3 } else { ink.face2 });
                if lit {
                    f.stroke(&shape, line(ink.rule2, 1.0));
                }
            }
            let fade = |c: iced::Color| if dim { alpha(c, c.a * 0.22) } else { c };
            let mut x = r.x + 8.0;
            if let Some(p) = self.e.get(name) {
                if !dim {
                    mark::draw(&mut f, &ink, Kind::Standing(p.standing), Point::new(x + 4.0, r.center_y()), 0.85);
                }
                x += 16.0;
                let size = if me { super::BODY } else { SMALL };
                let room = r.x + r.width - x - if p.advised() { 16.0 } else { 6.0 };
                let shown = fit(name, room, size);
                let wid = shown.chars().count() as f32 * size * 0.56;
                label(&mut f, shown, Point::new(x, r.center_y()), size, fade(if me || lit { ink.fg } else { ink.fg2 }), if me { fonts::ui_medium() } else { fonts::ui() });
                if p.advised() && !dim {
                    mark::draw(&mut f, &ink, Kind::Advisory, Point::new(x + wid + 8.0, r.center_y()), 0.8);
                }
            } else {
                label(&mut f, fit(name, r.width - 16.0, SMALL), Point::new(x, r.center_y()), SMALL, fade(ink.fg3), fonts::ui());
            }
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &Hover, _: Rectangle, _: mouse::Cursor) -> mouse::Interaction {
        match state.0.as_deref() {
            Some(n) if n != self.id => mouse::Interaction::Pointer,
            _ => mouse::Interaction::default(),
        }
    }
}
