//! Map / Weight: area is what each package takes, categories first. On
//! disk, the solid part of a tile is the build in use and the hatching is
//! the older versions the store still holds; in use, only the builds that
//! are linked count.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use sola_kit::components::style::{alpha, mix};
use sola_kit::fonts;

use super::ink::Ink;
use super::paint::{fit, hatch, label, line, rounded};
use super::squarify::squarify;
use super::words::size;
use super::{CAPTION, SMALL};
use crate::Msg;
use crate::model::{Ecosystem, Package};
use crate::standing::Standing;

pub struct Weight<'a> {
    pub e: &'a Ecosystem,
    pub sel: Option<&'a str>,
    /// On disk (every version) or in use (the linked build).
    pub disk: bool,
}

struct Tile {
    name: String,
    rect: Rectangle,
    value: u64,
}

struct Group {
    category: String,
    rect: Rectangle,
    value: u64,
}

impl Weight<'_> {
    fn value(&self, p: &Package) -> u64 {
        if self.disk { p.on_disk() } else { p.in_use() }
    }

    fn layout(&self, bounds: Size) -> (Vec<Group>, Vec<Tile>) {
        let here: Vec<&Package> =
            self.e.pkgs.values().filter(|p| p.here && (self.disk || p.present)).collect();
        let total: u64 = here.iter().map(|p| self.value(p)).sum();
        // Every package keeps a sliver, so the smallest still shows.
        let floor = (total as f64 * 0.00035).max(1.0);
        let shown = |v: u64| (v as f64).max(floor);
        let mut groups: Vec<(String, Vec<&Package>, f64, u64)> = self
            .e
            .categories
            .iter()
            .map(|c| {
                let ps: Vec<&Package> = here.iter().copied().filter(|p| &p.category == c).collect();
                let v = ps.iter().map(|p| shown(self.value(p))).sum();
                let real = ps.iter().map(|p| self.value(p)).sum();
                (c.clone(), ps, v, real)
            })
            .filter(|(_, ps, ..)| !ps.is_empty())
            .collect();
        groups.sort_by(|a, b| b.2.total_cmp(&a.2));
        let rects = squarify(&groups.iter().map(|g| g.2).collect::<Vec<_>>(), Rectangle::new(Point::ORIGIN, bounds));
        let mut out_g = Vec::new();
        let mut tiles = Vec::new();
        for ((category, mut ps, _, real), r) in groups.into_iter().zip(rects) {
            let inner = Rectangle { x: r.x + 1.0, y: r.y + 24.0, width: (r.width - 6.0).max(0.0), height: (r.height - 29.0).max(0.0) };
            ps.sort_by(|a, b| self.value(b).cmp(&self.value(a)).then(a.name.cmp(&b.name)));
            let trs = squarify(&ps.iter().map(|p| shown(self.value(p))).collect::<Vec<_>>(), inner);
            for (p, t) in ps.iter().zip(trs) {
                tiles.push(Tile { name: p.name.clone(), rect: t, value: self.value(p) });
            }
            out_g.push(Group { category, rect: r, value: real });
        }
        (out_g, tiles)
    }
}

#[derive(Default)]
pub struct Hover(Option<String>);

impl canvas::Program<Msg> for Weight<'_> {
    type State = Hover;

    fn update(&self, state: &mut Hover, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Msg>> {
        let at = |p: Point| {
            let (_, tiles) = self.layout(bounds.size());
            tiles.into_iter().find(|t| t.rect.contains(p)).map(|t| t.name)
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
                let p = cursor.position_in(bounds)?;
                Some(canvas::Action::publish(at(p).map_or(Msg::Deselect, Msg::Select)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &Hover, renderer: &Renderer, theme: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let ink = Ink::of(theme);
        let mut f = Frame::new(renderer, bounds.size());
        let (groups, tiles) = self.layout(bounds.size());
        for g in &groups {
            let r = Rectangle { width: g.rect.width - 4.0, height: g.rect.height - 4.0, ..g.rect };
            f.fill(&rounded(r, 2.0), ink.bg);
            f.stroke(&rounded(r, 2.0), line(ink.rule2, 1.0));
        }
        let base = Color::from_rgb8(0x24, 0x24, 0x24);
        for t in &tiles {
            let Some(p) = self.e.get(&t.name) else { continue };
            let r = Rectangle { width: (t.rect.width - 1.0).max(0.0), height: (t.rect.height - 1.0).max(0.0), ..t.rect };
            if r.width < 0.5 || r.height < 0.5 {
                continue;
            }
            let hovered = state.0.as_deref() == Some(t.name.as_str());
            f.fill(&canvas::Path::rectangle(r.position(), r.size()), base);
            let live = live_color(&ink, p, hovered);
            let frac = if self.disk && t.value > 0 { (p.in_use() as f32 / t.value as f32).clamp(0.02, 1.0) } else { 1.0 };
            let live_h = r.height * frac;
            if frac < 1.0 {
                let old = Rectangle { height: r.height - live_h, ..r };
                f.fill(&canvas::Path::rectangle(old.position(), old.size()), Color::from_rgb8(0x16, 0x16, 0x16));
                hatch(&mut f, old, 7.0, Color::from_rgb8(0x2a, 0x2a, 0x2a));
            }
            f.fill(&canvas::Path::rectangle(Point::new(r.x, r.y + r.height - live_h), Size::new(r.width, live_h)), live);
            if self.sel == Some(t.name.as_str()) {
                f.stroke(&canvas::Path::rectangle(Point::new(r.x + 1.0, r.y + 1.0), Size::new(r.width - 2.0, r.height - 2.0)), line(ink.accent, 2.0));
            }
            if r.width > 58.0 && r.height > 36.0 {
                label(&mut f, fit(&p.name, r.width - 16.0, SMALL), Point::new(r.x + 8.0, r.y + 14.0), SMALL, ink.fg, fonts::ui());
                let versions = if self.disk && p.versions.len() > 1 { format!(" · {} versions", p.versions.len()) } else { String::new() };
                label(&mut f, fit(&format!("{}{versions}", size(t.value)), r.width - 16.0, CAPTION), Point::new(r.x + 8.0, r.y + 30.0), CAPTION, ink.fg2, fonts::mono());
            }
        }
        for g in &groups {
            label(&mut f, fit(&g.category, g.rect.width - 80.0, SMALL), Point::new(g.rect.x + 7.0, g.rect.y + 12.0), SMALL, ink.fg, fonts::ui_medium());
            let shown = fit(&g.category, g.rect.width - 80.0, SMALL).chars().count() as f32 * SMALL * 0.56;
            label(&mut f, size(g.value), Point::new(g.rect.x + 13.0 + shown, g.rect.y + 12.0), CAPTION, ink.fg2, fonts::mono());
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &Hover, _: Rectangle, _: mouse::Cursor) -> mouse::Interaction {
        if state.0.is_some() { mouse::Interaction::Pointer } else { mouse::Interaction::default() }
    }
}

/// The solid part of a tile: graphite for current, the standing's color
/// otherwise, red over all for advisories.
fn live_color(ink: &Ink, p: &Package, hovered: bool) -> Color {
    let dark = Color::from_rgb8(0x1a, 0x1a, 0x1a);
    let c = if p.advised() {
        mix(ink.danger, dark, 0.55)
    } else {
        match p.standing {
            Standing::NotPushed => ink.warning,
            Standing::Local => mix(ink.warning, dark, 0.46),
            Standing::Update => mix(ink.accent, dark, 0.44),
            Standing::Off => dark,
            _ => Color::from_rgb8(0x36, 0x36, 0x36),
        }
    };
    if hovered { mix(Color::WHITE, c, 0.12) } else { alpha(c, 1.0) }
}
