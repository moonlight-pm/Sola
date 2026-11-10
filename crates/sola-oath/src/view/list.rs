//! The package list: a sentence first (if it says nothing is wrong, you
//! are done reading), one LED per package, then the table. Columns go
//! quiet (`·`) when they agree with this machine, so the hashes you can
//! read are the ones that differ.

use std::collections::BTreeSet;

use iced::widget::text::Span;
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, rich_text, row, span, text};
use iced::{Alignment, Color, Element, Length, Padding};
use sola_kit::components::button::list_item;
use sola_kit::components::scroll;
use sola_kit::components::text_input::{self, text_input};
use sola_kit::fonts;

use super::ink::Ink;
use super::mark::{self, Kind};
use super::words::{self, Part, short, size};
use super::{CAPTION, Filter, PAGE, SMALL, medium, mono, ui};
use crate::Msg;
use crate::audit::Upstream;
use crate::model::{Ecosystem, Package};
use crate::standing::Standing;

const ROW_H: f32 = 28.0;
const COLS: [f32; 9] = [90.0, 120.0, 120.0, 120.0, 90.0, 210.0, 56.0, 62.0, 70.0];

pub struct Args<'a> {
    pub e: &'a Ecosystem,
    pub ink: Ink,
    pub sel: Option<&'a str>,
    pub filter: Filter,
    pub query: &'a str,
    pub open: &'a BTreeSet<String>,
    pub checking: bool,
    /// The list's width, for which columns fit.
    pub room: f32,
}

/// Which of [`COLS`] fit `room` beside a readable name: the counts go
/// first, then where it was built, the version, upstream, newest, release.
fn fitting(room: f32) -> [bool; 9] {
    const DROP: [usize; 8] = [7, 6, 8, 5, 0, 4, 3, 2];
    let mut shown = [true; 9];
    let used = |shown: &[bool; 9]| COLS.iter().zip(shown).filter(|(_, s)| **s).map(|(w, _)| w + 8.0).sum::<f32>();
    for i in DROP {
        if room - 20.0 - used(&shown) >= 200.0 {
            break;
        }
        shown[i] = false;
    }
    shown
}

pub fn view<'a>(a: Args<'a>) -> Element<'a, Msg> {
    let ink = a.ink;
    let cols = fitting(a.room - 2.0 * PAGE);
    let mut page = column![summary(a.e, &ink), leds(a.e, &ink), tools(&a)].spacing(18);
    page = page.push(head(&ink, cols));
    for g in groups(&a, cols) {
        page = page.push(g);
    }
    if !a.e.problems.is_empty() {
        let mut p = column![medium("What could not be read", SMALL, ink.fg2)].spacing(4);
        for line in &a.e.problems {
            p = p.push(ui(line.clone(), SMALL, ink.danger));
        }
        page = page.push(p);
    }
    scroll::scrollable(container(page).padding(Padding { top: 22.0, right: PAGE, bottom: 40.0, left: PAGE }))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn summary<'a>(e: &'a Ecosystem, ink: &Ink) -> Element<'a, Msg> {
    let spans: Vec<Span<'a, (), iced::Font>> = words::summary(e)
        .into_iter()
        .map(|(part, s)| {
            let (color, font) = match part {
                Part::Quiet => (ink.fg2, fonts::display()),
                Part::Mono => (ink.fg, fonts::mono()),
                Part::Warn => (ink.warning, fonts::display()),
                Part::Accent => (ink.accent, fonts::display()),
            };
            span(s).color(color).font(font)
        })
        .collect();
    let place = format!(
        "{} · {}",
        e.host.to_uppercase(),
        if e.profile.is_empty() { "NO PROFILE".into() } else { format!("{} PROFILE", e.profile.to_uppercase()) }
    );
    column![medium(place, CAPTION, ink.fg3), rich_text(spans).size(22).line_height(1.36).width(Length::Fixed(1180.0))]
        .spacing(10)
        .into()
}

/// Worst first: what needs a look sorts to the front of the strip.
fn order(p: &Package) -> (u8, u8) {
    let s = match p.standing {
        Standing::NotPushed => 0,
        Standing::Update => 1,
        Standing::Local => 2,
        Standing::OriginOnly => 3,
        Standing::Off => 4,
        Standing::Current => 5,
    };
    (u8::from(!p.advised()), s)
}

fn leds<'a>(e: &'a Ecosystem, ink: &Ink) -> Element<'a, Msg> {
    let mut all: Vec<&Package> = e.pkgs.values().collect();
    all.sort_by_key(|p| order(p));
    let strip = row(all.into_iter().map(|p| {
        let (fill, edge) = ink.led(p);
        mark::led(fill, edge, 6.0, 12.0)
    })).spacing(2).wrap().vertical_spacing(2);
    let mut tally = row![].spacing(20).align_y(Alignment::Center);
    for s in [Standing::Local, Standing::NotPushed, Standing::Update, Standing::Off, Standing::OriginOnly, Standing::Current] {
        let n = e.count(|p| p.standing == s);
        if n > 0 {
            tally = tally.push(tally_item(mark::standing(s), s.label(), n, ink));
        }
    }
    tally = tally.push(tally_item(mark::mark(Kind::Advisory), "Advisories", e.count(Package::advised), ink));
    column![strip, tally.wrap().vertical_spacing(6)].spacing(10).into()
}

fn tally_item<'a>(m: Element<'a, Msg>, label: &'a str, n: usize, ink: &Ink) -> Element<'a, Msg> {
    row![m, ui(label, SMALL, ink.fg2), mono(n.to_string(), SMALL, ink.fg)].spacing(7).align_y(Alignment::Center).into()
}

fn tools<'a>(a: &Args<'a>) -> Element<'a, Msg> {
    let ink = a.ink;
    let label = if a.checking { "Checking…" } else { "Check for a newer release" };
    let mut check = button(text(label).size(SMALL).wrapping(Wrapping::None))
        .padding(sola_kit::components::style::PAD_CONTROL_SM)
        .style(sola_kit::components::button::secondary);
    if !a.checking {
        check = check.on_press(Msg::Check);
    }
    let mut r = row![
        container(
            text_input("Filter by name, program, version, need", a.query)
                .on_input(Msg::Query)
                .style(text_input::style)
                .size(SMALL)
        )
        .width(Length::Fill)
        .max_width(340.0),
        check,
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if a.room > 1100.0 {
        r = r.push(Space::new().width(Length::Fill));
        r = r.push(ui("·  agrees with this machine    –  nothing to compare", SMALL, ink.fg3).wrapping(Wrapping::None));
    }
    r.into()
}

fn cell<'a>(content: impl Into<Element<'a, Msg>>, w: f32, right: bool) -> Element<'a, Msg> {
    let c = container(content).width(Length::Fixed(w)).height(Length::Fixed(ROW_H)).align_y(Alignment::Center).clip(true);
    if right { c.align_x(Alignment::End).into() } else { c.into() }
}

fn head<'a>(ink: &Ink, cols: [bool; 9]) -> Element<'a, Msg> {
    let names = ["Version", "Here", "Release", "Newest", "Upstream", "Built from", "Needs", "Used by", "Size"];
    let mut r = row![container(medium("Package", CAPTION, ink.fg3)).width(Length::Fill)].spacing(8).align_y(Alignment::Center);
    for (i, (n, w)) in names.iter().zip(COLS).enumerate() {
        if cols[i] {
            r = r.push(cell(medium(*n, CAPTION, ink.fg3), w, i >= 6));
        }
    }
    let rule = ink.rule;
    container(r.height(Length::Fixed(26.0)))
        .padding(Padding::from([0, 10]))
        .style(move |_| container::Style {
            border: iced::Border { color: rule, width: 0.0, ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// A hash column: `·` when it agrees with here, `–` with nothing to say.
fn hcell<'a>(v: Option<&str>, here: &str, color: Color, ink: &Ink) -> Element<'a, Msg> {
    match v {
        None | Some("") => mono("–", SMALL, ink.fg4).into(),
        Some(h) if h == here => mono("·", SMALL, ink.fg4).into(),
        Some(h) => mono(short(h).to_string(), SMALL, color).into(),
    }
}

pub fn row_of<'a>(e: &'a Ecosystem, p: &'a Package, ink: &Ink, selected: bool, cols: [bool; 9]) -> Element<'a, Msg> {
    let (below, above) = e.reach(&p.name);
    let off = |s: &'a str| -> Element<'a, Msg> { ui(s, CAPTION, ink.fg3).into() };
    let local = matches!(p.standing, Standing::Local | Standing::NotPushed);
    let here: Element<'a, Msg> = match p.standing {
        Standing::OriginOnly => off("not here"),
        Standing::Off => off("off"),
        _ => mono(short(&p.hash).to_string(), SMALL, if local { ink.warning } else { ink.fg2 }).into(),
    };
    let release: Element<'a, Msg> = if !p.here || !p.present {
        mono("–", SMALL, ink.fg4).into()
    } else if p.release.is_none() {
        off("not in it")
    } else {
        hcell(p.release.as_deref(), &p.hash, ink.fg2, ink)
    };
    let newest: Element<'a, Msg> = match (&e.check, &p.newest) {
        (Some(c), _) if c.error.is_some() => mono("–", SMALL, ink.fg4).into(),
        (Some(_), Some(m)) if p.present && p.moves() => mono(short(&m.to_hash).to_string(), SMALL, ink.accent).into(),
        (Some(_), _) if p.present && p.published() => mono("·", SMALL, ink.fg4).into(),
        _ => mono("–", SMALL, ink.fg4).into(),
    };
    let upstream: Element<'a, Msg> = match &p.upstream {
        Some(Upstream::Newer(v)) => mono(v.clone(), SMALL, ink.fg2).into(),
        Some(Upstream::Current) => mono("·", SMALL, ink.fg4).into(),
        Some(Upstream::Unread(_)) => mono("?", SMALL, ink.fg3).into(),
        None => mono("–", SMALL, ink.fg4).into(),
    };
    let from: Element<'a, Msg> = match (&p.source, p.standing) {
        (Some(s), Standing::NotPushed) => {
            mono(format!("{}{}", s.short(), if s.dirty { " dirty" } else { "" }), SMALL, ink.warning).into()
        }
        (Some(s), Standing::Local) => mono(s.short(), SMALL, ink.fg2).into(),
        _ => Space::new().into(),
    };
    let weight = if p.here { p.in_use() } else { p.listed.as_ref().and_then(|l| l.size).unwrap_or(0) };
    let mut name = row![mark::standing(p.standing), ui(p.name.clone(), super::BODY, ink.fg)]
        .spacing(9)
        .align_y(Alignment::Center);
    if p.advised() {
        name = name.push(mark::mark(Kind::Advisory));
    }
    let cells: [Element<'a, Msg>; 9] = [
        mono(p.version.clone(), SMALL, ink.fg3).into(),
        here,
        release,
        newest,
        upstream,
        from,
        mono(below.to_string(), SMALL, ink.fg3).into(),
        mono(above.to_string(), SMALL, ink.fg3).into(),
        mono(size(weight), SMALL, ink.fg3).into(),
    ];
    let mut r = row![container(name).width(Length::Fill).height(Length::Fixed(ROW_H)).align_y(Alignment::Center).clip(true)]
        .spacing(8)
        .align_y(Alignment::Center);
    for (i, c) in cells.into_iter().enumerate() {
        if cols[i] {
            r = r.push(cell(c, COLS[i], i >= 6));
        }
    }
    button(r)
        .padding(Padding::from([0, 10]))
        .width(Length::Fill)
        .style(list_item(selected))
        .on_press(Msg::Select(p.name.clone()))
        .into()
}

fn caption<'a>(t: String, n: usize, ink: &Ink) -> Element<'a, Msg> {
    container(
        row![medium(t, CAPTION, ink.fg2), mono(n.to_string(), CAPTION, ink.fg3)].spacing(8).align_y(Alignment::Center),
    )
    .height(Length::Fixed(26.0))
    .padding(Padding::from([0, 10]))
    .align_y(Alignment::Center)
    .into()
}

fn matches(p: &Package, q: &str) -> bool {
    let q = q.to_lowercase();
    p.name.to_lowercase().contains(&q)
        || p.version.to_lowercase().contains(&q)
        || p.links.iter().any(|l| l.to_lowercase().contains(&q))
        || p.description.to_lowercase().contains(&q)
}

fn groups<'a>(a: &Args<'a>, cols: [bool; 9]) -> Vec<Element<'a, Msg>> {
    let ink = a.ink;
    let e = a.e;
    let mut out: Vec<Element<'a, Msg>> = Vec::new();
    let query = a.query.trim();
    if a.filter != Filter::All || !query.is_empty() {
        let mut ps: Vec<&Package> =
            e.pkgs.values().filter(|p| a.filter.keeps(p) && (query.is_empty() || matches(p, query))).collect();
        ps.sort_by(|x, y| x.category.cmp(&y.category).then(x.name.cmp(&y.name)));
        let title = if query.is_empty() { a.filter.label().to_string() } else { format!("{} matching “{query}”", a.filter.label()) };
        let mut g = column![caption(title, ps.len(), &ink)];
        if ps.is_empty() {
            g = g.push(container(ui(empty_words(a.filter), SMALL, ink.fg3)).padding(Padding::from([4, 10])));
        }
        for p in ps {
            g = g.push(row_of(e, p, &ink, a.sel == Some(p.name.as_str()), cols));
        }
        out.push(g.into());
        return out;
    }
    let look: [(String, Box<dyn Fn(&Package) -> bool>); 4] = [
        ("Not pushed".into(), Box::new(|p: &Package| p.standing == Standing::NotPushed)),
        ("Advisories".into(), Box::new(Package::advised)),
        ("Updates".into(), Box::new(|p: &Package| p.standing == Standing::Update && !p.advised())),
        ("Local builds, pushed or unknown".into(), Box::new(|p: &Package| p.standing == Standing::Local && !p.advised())),
    ];
    for (t, f) in look {
        let ps: Vec<&Package> = e.pkgs.values().filter(|p| f(p)).collect();
        if ps.is_empty() {
            continue;
        }
        let mut g = column![caption(t, ps.len(), &ink)];
        for p in ps {
            g = g.push(row_of(e, p, &ink, a.sel == Some(p.name.as_str()), cols));
        }
        out.push(g.into());
    }
    let mut all = column![caption("Everything, by category".into(), e.pkgs.len(), &ink)];
    for cat in &e.categories {
        let mut ps: Vec<&Package> = e.pkgs.values().filter(|p| &p.category == cat).collect();
        ps.sort_by(|x, y| order(x).cmp(&order(y)).then(x.name.cmp(&y.name)));
        let open = a.open.contains(cat);
        all = all.push(category_row(cat, &ps, open, &ink));
        if open {
            let mut rows = column![].padding(Padding { top: 2.0, bottom: 8.0, ..Padding::ZERO });
            for p in ps {
                rows = rows.push(row_of(e, p, &ink, a.sel == Some(p.name.as_str()), cols));
            }
            all = all.push(rows);
        }
    }
    out.push(all.into());
    out
}

fn empty_words(f: Filter) -> &'static str {
    match f {
        Filter::Is(Standing::NotPushed) => "Every build here is the release's or its commit is on a remote.",
        Filter::Advisories => "The audit lists no advisories on the versions here.",
        _ => "None.",
    }
}

fn category_row<'a>(cat: &str, ps: &[&'a Package], open: bool, ink: &Ink) -> Element<'a, Msg> {
    let caret = if open { "▾" } else { "▸" };
    let weight: u64 = ps.iter().map(|p| p.in_use()).sum();
    let strip = row(ps.iter().map(|p| {
        let (fill, edge) = ink.led(p);
        mark::led(fill, edge, 5.0, 10.0)
    })).spacing(2).wrap().vertical_spacing(2);
    let r = row![
        container(ui(caret, SMALL, ink.fg3)).width(Length::Fixed(14.0)),
        container(ui(cat.to_string(), super::BODY, ink.fg)).width(Length::Fixed(190.0)),
        container(mono(ps.len().to_string(), SMALL, ink.fg3)).width(Length::Fixed(74.0)),
        container(strip).width(Length::Fill),
        container(mono(size(weight), SMALL, ink.fg3)).width(Length::Fixed(70.0)).align_x(Alignment::End),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let rule = ink.rule;
    container(
        button(container(r).height(Length::Fixed(30.0)).align_y(Alignment::Center))
            .padding(Padding::from([0, 10]))
            .width(Length::Fill)
            .style(list_item(false))
            .on_press(Msg::ToggleCategory(cat.to_string())),
    )
    .style(move |_| container::Style {
        border: iced::Border { color: rule, width: 0.0, ..Default::default() },
        ..Default::default()
    })
    .into()
}
