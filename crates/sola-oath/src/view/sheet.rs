//! One package, as far open as you want: the verdict and where it stands
//! first, the rest folded until asked.

use std::collections::BTreeSet;

use iced::widget::{Space, button, column, container, row};
use iced::{Alignment, Color, Element, Length, Padding};
use sola_kit::components::badge::{self, Tone as BadgeTone};
use sola_kit::components::scroll;

use super::ink::Ink;
use super::mark::{self, Kind};
use super::words::{self, Tone, short, size};
use super::{BODY, CAPTION, HEADING, PAGE, PROSE, SMALL, Section, display, medium, mono, ui};
use crate::Msg;
use crate::audit::{Security, Upstream, rank};
use crate::git::Pushed;
use crate::model::{Ecosystem, Package};
use crate::standing::Standing;
use crate::when;

pub fn badge_tone(s: Standing) -> BadgeTone {
    match s {
        Standing::NotPushed | Standing::Local => BadgeTone::Warning,
        Standing::Update | Standing::OriginOnly => BadgeTone::Accent,
        _ => BadgeTone::Neutral,
    }
}

pub fn standing_badge<'a>(s: Standing, ink: &Ink) -> Element<'a, Msg> {
    pill(mark::standing(s), s.label(), badge_tone(s), ink)
}

fn pill<'a>(m: Element<'a, Msg>, label: &'a str, tone: BadgeTone, ink: &Ink) -> Element<'a, Msg> {
    let color = match tone {
        BadgeTone::Warning => ink.warning,
        BadgeTone::Accent => ink.accent,
        BadgeTone::Danger => ink.danger,
        _ => ink.fg2,
    };
    container(row![m, medium(label, CAPTION, color)].spacing(7).align_y(Alignment::Center))
        .padding(Padding::from([4, 9]))
        .style(move |t| badge::style(t, tone))
        .into()
}

/// Advisories shown before "Show all".
const HITS: usize = 10;

pub fn view<'a>(
    e: &'a Ecosystem,
    p: &'a Package,
    ink: Ink,
    open: &BTreeSet<Section>,
    width: f32,
    all_hits: bool,
) -> Element<'a, Msg> {
    let mut badges = row![standing_badge(p.standing, &ink)].spacing(6);
    if p.advised() {
        badges = badges.push(pill(mark::mark(Kind::Advisory), "Advisories", BadgeTone::Danger, &ink));
    }
    let mut top = column![
        row![
            row![display(p.name.clone(), HEADING, ink.fg), mono(p.version.clone(), SMALL, ink.fg3)]
                .spacing(8)
                .align_y(Alignment::End),
            Space::new().width(Length::Fill),
            badges,
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(4);
    if !p.description.is_empty() {
        top = top.push(ui(p.description.clone(), BODY, ink.fg2));
    }
    if !p.home.is_empty() {
        top = top.push(link(p.home.trim_start_matches("https://").to_string(), p.home.clone(), &ink));
    }
    let (tone, said) = words::verdict(e, p);
    let mut page = column![top, verdict(tone, said, &ink)].spacing(18);

    let (below, above) = e.reach(&p.name);
    page = page.push(section(Section::Where, "Where it stands", String::new(), open, &ink, || places(e, p, &ink), None));
    page = page.push(section(
        Section::Tree,
        "Tree",
        format!("{below} below, {above} above"),
        open,
        &ink,
        || mini_tree(e, p, &ink),
        Some(("Open Tree", Msg::Show(super::View::Tree))),
    ));
    if !p.services.is_empty() {
        page = page.push(section(
            Section::Restarts,
            "Moving it restarts",
            words_n(p.services.len(), "service", "services"),
            open,
            &ink,
            || restarts(p, &ink),
            None,
        ));
    }
    page = page.push(section(Section::Security, "Security", words::security_short(p), open, &ink, || security(e, p, &ink, all_hits), None));
    if !p.links.is_empty() {
        page = page.push(section(
            Section::Programs,
            "Programs",
            format!("{} in /bin", p.links.len()),
            open,
            &ink,
            || chips(p.links.iter().map(|l| chip(l.clone(), None, true, &ink)).collect()),
            None,
        ));
    }
    if !p.versions.is_empty() {
        page = page.push(section(
            Section::Versions,
            "Versions on disk",
            format!("{}, {}", p.versions.len(), size(p.on_disk())),
            open,
            &ink,
            || versions(p, &ink),
            None,
        ));
    }
    if let Some(s) = &p.source {
        page = page.push(section(Section::Source, "Where it came from", s.short(), open, &ink, || source(p, &ink), None));
    }
    if !p.raw.is_null() {
        page = page.push(section(
            Section::Raw,
            "Everything",
            format!("oath get pkg:{}", p.name),
            open,
            &ink,
            || mono(serde_json::to_string_pretty(&p.raw).unwrap_or_default(), CAPTION, ink.fg3).into(),
            None,
        ));
    }
    let bg = ink.raised;
    container(
        scroll::scrollable(container(page).padding(Padding { top: 22.0, right: PAGE, bottom: 40.0, left: PAGE }))
            .height(Length::Fill),
    )
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(move |_| container::Style { background: Some(bg.into()), ..Default::default() })
    .into()
}

fn words_n(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn link<'a>(label: String, url: String, ink: &Ink) -> Element<'a, Msg> {
    button(ui(label, SMALL, ink.accent)).padding(0).style(super::card::bare).on_press(Msg::Open(url)).into()
}

fn verdict<'a>(tone: Tone, said: String, ink: &Ink) -> Element<'a, Msg> {
    let (fill, edge) = match tone {
        Tone::Plain => (ink.face2, ink.rule2),
        Tone::Warn => (sola_kit::components::style::mix(ink.warning, ink.raised, 0.09), Color { a: 0.28, ..ink.warning }),
        Tone::Danger => (sola_kit::components::style::mix(ink.danger, ink.raised, 0.09), Color { a: 0.28, ..ink.danger }),
    };
    container(ui(said, PROSE, ink.fg).line_height(1.55))
        .padding(Padding::from([12, 14]))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(fill.into()),
            border: iced::Border { color: edge, width: 1.0, radius: 7.0.into() },
            ..Default::default()
        })
        .into()
}

fn section<'a>(
    id: Section,
    title: &'a str,
    sub: String,
    open: &BTreeSet<Section>,
    ink: &Ink,
    body: impl FnOnce() -> Element<'a, Msg>,
    act: Option<(&'a str, Msg)>,
) -> Element<'a, Msg> {
    let is_open = open.contains(&id);
    let mut head = row![
        ui(if is_open { "▾" } else { "▸" }, SMALL, ink.fg3),
        medium(title, SMALL, ink.fg),
        ui(sub, SMALL, ink.fg3),
        Space::new().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some((label, msg)) = act {
        head = head.push(button(ui(label, SMALL, ink.accent)).padding(0).style(super::card::bare).on_press(msg));
    }
    let mut col = column![
        button(container(head).height(Length::Fixed(22.0)).align_y(Alignment::Center))
            .padding(0)
            .width(Length::Fill)
            .style(super::card::bare)
            .on_press(Msg::ToggleSection(id))
    ];
    if is_open {
        col = col.push(container(body()).padding(Padding { top: 10.0, right: 0.0, bottom: 2.0, left: 22.0 }));
    }
    let rule = ink.rule;
    column![
        container(Space::new()).width(Length::Fill).height(Length::Fixed(1.0)).style(move |_| container::Style {
            background: Some(rule.into()),
            ..Default::default()
        }),
        col.padding(Padding { top: 12.0, ..Padding::ZERO }),
    ]
    .into()
}

fn note<'a>(s: impl Into<String>, ink: &Ink) -> Element<'a, Msg> {
    ui(s.into(), SMALL, ink.fg3).line_height(1.5).into()
}

/// A row of the places grid: name, hash, where from.
fn place<'a>(name: Element<'a, Msg>, value: Element<'a, Msg>, from: String, ink: &Ink) -> Element<'a, Msg> {
    row![
        container(name).width(Length::Fixed(150.0)),
        container(value).width(Length::Fixed(150.0)),
        container(mono(from, SMALL, ink.fg3)).width(Length::Fill).clip(true),
    ]
    .spacing(12)
    .height(Length::Fixed(26.0))
    .align_y(Alignment::Center)
    .into()
}

fn quiet<'a>(s: &'a str, ink: &Ink) -> Element<'a, Msg> {
    ui(s, CAPTION, ink.fg4).into()
}

fn places<'a>(e: &'a Ecosystem, p: &'a Package, ink: &Ink) -> Element<'a, Msg> {
    let name = |b: String, i: String| -> Element<'a, Msg> {
        row![medium(b, SMALL, ink.fg), ui(i, CAPTION, ink.fg3)].spacing(6).align_y(Alignment::Center).into()
    };
    let local = matches!(p.standing, Standing::Local | Standing::NotPushed);
    let here_val: Element<'a, Msg> = match p.standing {
        Standing::OriginOnly => quiet("not in the catalog", ink),
        Standing::Off => quiet("not linked", ink),
        _ => mono(short(&p.hash).to_string(), SMALL, if local { ink.warning } else { ink.fg2 }).into(),
    };
    let here_from = if !p.present {
        String::new()
    } else if p.is_release_build() {
        "the release".into()
    } else {
        p.source.as_ref().map(|s| s.short()).unwrap_or_else(|| "no record".into())
    };
    let mut col = column![place(name(e.host.clone(), "here".into()), here_val, here_from, ink), rule(ink)];
    let rel_id = e.release.as_ref().map(|r| r.output.clone()).unwrap_or_default();
    let rel_val: Element<'a, Msg> = match p.release.as_deref() {
        None => quiet("not in it", ink),
        Some(h) if p.present && h == p.hash => quiet("same as here", ink),
        Some(h) => mono(short(h).to_string(), SMALL, ink.fg2).into(),
    };
    let rel_from = if p.release.is_some() && p.name.starts_with("sola") {
        // A release records each Sola pack's source beside its pin.
        p.source.as_ref().filter(|_| p.is_release_build()).map(|s| s.short()).unwrap_or_default()
    } else {
        String::new()
    };
    col = col.push(place(name("Release".into(), rel_id), rel_val, rel_from, ink));
    let (newest_name, newest_val): (String, Element<'a, Msg>) = match &e.check {
        None => (String::new(), quiet("not checked", ink)),
        Some(c) if c.error.is_some() => (String::new(), quiet("the check could not say", ink)),
        Some(c) => {
            let id = c.newest.clone().unwrap_or_default();
            let v = match &p.newest {
                Some(m) if p.moves() => mono(short(&m.to_hash).to_string(), SMALL, ink.accent).into(),
                _ if p.in_release() => quiet("same as here", ink),
                _ => quiet("not in it", ink),
            };
            (id, v)
        }
    };
    let checked = e.check.as_ref().map(|c| format!("checked {}", when::ago(&c.at))).unwrap_or_default();
    col = col.push(place(name("Newest".into(), newest_name), newest_val, checked, ink));
    if p.here {
        col = col.push(rule(ink));
        let up: Element<'a, Msg> = match &p.upstream {
            Some(Upstream::Newer(v)) => mono(v.clone(), SMALL, ink.fg2).into(),
            Some(Upstream::Current) => quiet("nothing newer known", ink),
            Some(Upstream::Unread(_)) => quiet("could not tell", ink),
            None => quiet("not audited", ink),
        };
        let from = e.report.as_ref().filter(|_| p.upstream.is_some()).map(|r| format!("audit {}", &r.generated.get(..10).unwrap_or(&r.generated))).unwrap_or_default();
        col = col.push(place(name("Upstream".into(), String::new()), up, from, ink));
    }
    let mut lines = Vec::new();
    if p.versions.is_empty() {
        if p.here {
            lines.push("Nothing on disk.".to_string());
        }
    } else {
        lines.push(format!("On disk: {}, {}.", words_n(p.versions.len(), "version", "versions"), size(p.on_disk())));
    }
    if let Some((at, ok)) = &p.verified {
        lines.push(format!("Verified {} {}.", if *ok { "good" } else { "damaged" }, at.get(..10).unwrap_or(at)));
    }
    if !p.removable {
        lines.push("Part of the fixed base: Oath will not remove it.".into());
    }
    if p.status != "in-sync" && !p.status.is_empty() {
        lines.push(format!("Oath says {}: the catalog and the machine differ until the next apply.", p.status));
    }
    if !lines.is_empty() {
        col = col.push(container(note(lines.join(" "), ink)).padding(Padding { top: 6.0, ..Padding::ZERO }));
    }
    col.into()
}

fn rule<'a>(ink: &Ink) -> Element<'a, Msg> {
    let c = ink.rule;
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_| container::Style { background: Some(c.into()), ..Default::default() })
        .into()
}

pub fn chip<'a>(label: String, to: Option<(Standing, bool)>, code: bool, ink: &Ink) -> Element<'a, Msg> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    if let Some((s, _)) = to {
        r = r.push(mark::standing(s));
    }
    let t = if code { mono(label.clone(), SMALL, ink.fg2) } else { ui(label.clone(), SMALL, ink.fg2) };
    r = r.push(t);
    if to.is_some_and(|(_, adv)| adv) {
        r = r.push(mark::mark(Kind::Advisory));
    }
    let face = if code { ink.face2 } else { ink.face3 };
    let hover = sola_kit::components::style::mix_white(ink.bg, 0.16);
    let b = button(container(r).height(Length::Fixed(22.0)).align_y(Alignment::Center))
        .padding(Padding::from([0, 8]))
        .style(move |_, status| iced::widget::button::Style {
            background: Some(if matches!(status, iced::widget::button::Status::Hovered) { hover } else { face }.into()),
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        });
    match to {
        Some(_) => b.on_press(Msg::Select(label)).into(),
        None => b.into(),
    }
}

fn chips<'a>(items: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    row(items).spacing(4).wrap().vertical_spacing(4).into()
}

fn pkg_chips<'a>(e: &'a Ecosystem, names: &[String], ink: &Ink, cap: usize) -> Element<'a, Msg> {
    let mut items: Vec<Element<'a, Msg>> = names
        .iter()
        .take(cap)
        .map(|n| {
            let to = e.get(n).map(|p| (p.standing, p.advised()));
            chip(n.clone(), Some(to.unwrap_or((Standing::Current, false))), false, ink)
        })
        .collect();
    if names.len() > cap {
        items.push(ui(format!("+{}", names.len() - cap), SMALL, ink.fg3).into());
    }
    chips(items)
}

fn mini_tree<'a>(e: &'a Ecosystem, p: &'a Package, ink: &Ink) -> Element<'a, Msg> {
    let fold = |ls: Vec<Vec<String>>| -> Vec<Vec<String>> {
        if ls.len() > 2 {
            let mut it = ls.into_iter();
            let first = it.next().unwrap_or_default();
            let mut rest: Vec<String> = it.flatten().collect();
            rest.sort();
            vec![first, rest]
        } else {
            ls
        }
    };
    let up = fold(e.graph.levels_up(&p.name));
    let down = fold(e.graph.levels_down(&p.name));
    let level = |tag: &'a str, names: &[String]| -> Element<'a, Msg> {
        row![container(ui(tag, CAPTION, ink.fg3)).width(Length::Fixed(52.0)).align_x(Alignment::End), pkg_chips(e, names, ink, 18)]
            .spacing(8)
            .into()
    };
    let mut col = column![].spacing(6);
    if up.is_empty() {
        col = col.push(row![Space::new().width(Length::Fixed(60.0)), note("Nothing needs it.", ink)]);
    }
    for (i, names) in up.iter().enumerate().rev() {
        col = col.push(level(if i == 0 { "direct" } else { "further" }, names));
    }
    let accent = ink.accent;
    col = col.push(
        row![
            Space::new().width(Length::Fixed(60.0)),
            container(row![mark::standing(p.standing), medium(p.name.clone(), BODY, ink.fg)].spacing(8).align_y(Alignment::Center))
                .padding(Padding::from([0, 10]))
                .height(Length::Fixed(26.0))
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Color { a: 0.14, ..accent }.into()),
                    border: iced::Border { color: Color { a: 0.28, ..accent }, width: 1.0, radius: 4.0.into() },
                    ..Default::default()
                }),
        ]
        .align_y(Alignment::Center),
    );
    for (i, names) in down.iter().enumerate() {
        col = col.push(level(if i == 0 { "direct" } else { "further" }, names));
    }
    if down.is_empty() {
        col = col.push(row![Space::new().width(Length::Fixed(60.0)), note("Needs nothing.", ink)]);
    }
    if let Some(base) = crate::graph::implicit_base(&p.name).filter(|b| e.get(b).is_some()) {
        col = col.push(row![
            Space::new().width(Length::Fixed(60.0)),
            note(format!("It stands on {base} by rule: Oath does not list the C library as a need."), ink)
        ]);
    }
    col.into()
}

fn restarts<'a>(p: &'a Package, ink: &Ink) -> Element<'a, Msg> {
    let items = p
        .services
        .iter()
        .map(|s| {
            let seat = s.seat.as_deref().map(|x| format!(" ({x})")).unwrap_or_default();
            chip(format!("{}{seat}", s.id.trim_start_matches("svc:")), None, true, ink)
        })
        .collect();
    let mut col = column![chips(items)].spacing(8);
    let words = match p.name.as_str() {
        "sola" => Some("The desk restarts. Running apps and games keep going."),
        "rivulet" => Some("This ends the session: the compositor goes with it."),
        _ => None,
    };
    if let Some(w) = words {
        col = col.push(note(w, ink));
    }
    col.into()
}

fn severity_pill<'a>(sev: Option<&str>, ink: &Ink) -> Element<'a, Msg> {
    let (color, fill) = match sev {
        Some("CRITICAL") | Some("HIGH") => (ink.danger, Color { a: 0.14, ..ink.danger }),
        Some("MEDIUM") => (ink.warning, Color { a: 0.14, ..ink.warning }),
        _ => (ink.fg2, ink.face3),
    };
    let label = sev.map(|s| s.to_lowercase()).unwrap_or_else(|| "severity unknown".into());
    container(medium(label, CAPTION, color))
        .padding(Padding::from([3, 7]))
        .style(move |_| container::Style {
            background: Some(fill.into()),
            border: iced::Border { radius: 9.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

fn security<'a>(e: &'a Ecosystem, p: &'a Package, ink: &Ink, all_hits: bool) -> Element<'a, Msg> {
    let mut col = column![].spacing(8);
    let report_line = e.report.as_ref().map(|r| {
        format!(
            "From the audit of {} (Oath {}), signed by {}.",
            r.generated.get(..10).unwrap_or(&r.generated),
            r.commit.get(..10).unwrap_or(&r.commit),
            if r.signer.is_empty() { "an unnamed key" } else { &r.signer }
        )
    });
    match &p.security {
        Security::Assessed { hits, suppressed, same_build } => {
            let mut hits: Vec<_> = hits.iter().collect();
            hits.sort_by_key(|h| rank(h));
            let more = hits.len().saturating_sub(HITS);
            if !all_hits {
                hits.truncate(HITS);
            }
            for h in hits {
                let mut top = row![mark::mark(Kind::Advisory), medium(h.id.clone(), SMALL, ink.fg), severity_pill(h.severity.as_deref(), ink)]
                    .spacing(8)
                    .align_y(Alignment::Center);
                if h.kev {
                    top = top.push(medium("known exploited", CAPTION, ink.danger));
                }
                if h.status != "affected" {
                    top = top.push(ui(format!("status {}", h.status), CAPTION, ink.fg3));
                }
                let mut meta = row![].spacing(16).align_y(Alignment::Center);
                if let Some(x) = h.epss {
                    meta = meta.push(ui(format!("EPSS {:.2}%", x * 100.0), SMALL, ink.fg3));
                }
                if !h.sources.is_empty() {
                    meta = meta.push(ui(h.sources.join(", "), SMALL, ink.fg3));
                }
                if !h.url.is_empty() {
                    meta = meta.push(link("Read it".into(), h.url.clone(), ink));
                }
                let face = ink.face2;
                col = col.push(
                    container(column![top, ui(h.summary.clone(), BODY, ink.fg).line_height(1.45), meta].spacing(8))
                        .padding(Padding::from([12, 14]))
                        .width(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(face.into()),
                            border: iced::Border { radius: 7.0.into(), ..Default::default() },
                            ..Default::default()
                        }),
                );
            }
            if more > 0 && !all_hits {
                col = col.push(
                    button(ui(format!("Show all {}, {more} more", more + HITS), SMALL, ink.accent))
                        .padding(0)
                        .style(super::card::bare)
                        .on_press(Msg::AllHits),
                );
            }
            if p.advised() {
                let above = e.graph.above(&p.name);
                let running = above.iter().filter(|n| e.get(n).is_some_and(|x| !x.services.is_empty())).count();
                col = col.push(note(
                    format!(
                        "Reaches {} through what uses it, {} of them running as services.",
                        words_n(above.len(), "package", "packages"),
                        running
                    ),
                    ink,
                ));
            } else {
                col = col.push(note("None listed for this version. An empty list is not proof that it is safe.", ink));
            }
            if !same_build {
                col = col.push(note(
                    "The report read another build of this version; its findings are by version, so they hold here.",
                    ink,
                ));
            }
            if !suppressed.is_empty() {
                col = col.push(medium(format!("Set aside by Oath ({})", suppressed.len()), SMALL, ink.fg2));
                for (id, why) in suppressed {
                    col = col.push(
                        row![container(mono(id.clone(), SMALL, ink.fg2)).width(Length::Fixed(150.0)), ui(why.clone(), SMALL, ink.fg3)]
                            .spacing(12),
                    );
                }
            }
        }
        Security::OtherVersion(v) => col = col.push(note(
            format!("Not assessed: the report read version {v}, and this is {}. Unknown, not safe.", p.version),
            ink,
        )),
        Security::Skipped(why) => col = col.push(note(format!("The audit leaves it out: {why}."), ink)),
        Security::Source => col = col.push(note("A source tree: nothing runs from it, so there is nothing to audit.", ink)),
        Security::Absent => col = col.push(note(
            if p.standing == Standing::Local || p.standing == Standing::NotPushed {
                "Not in the report: the audit reads the release's tree, and this build is not in it. Unknown, not safe."
            } else {
                "Not in the report. Unknown, not safe."
            },
            ink,
        )),
        Security::NoReport => col = col.push(note("No audit report was read, so nothing is known.", ink)),
    }
    if let Some(r) = &e.report {
        let failed = r.failed_feeds();
        if !failed.is_empty() {
            let names: Vec<String> = failed.iter().map(|(k, v)| format!("{k} ({v})")).collect();
            col = col.push(ui(format!("Feeds that failed, so their findings are missing: {}.", names.join(", ")), SMALL, ink.warning));
        }
    }
    if let Some(l) = report_line
        && !matches!(p.security, Security::NoReport | Security::Source)
    {
        col = col.push(note(l, ink));
    }
    col.into()
}

fn versions<'a>(p: &'a Package, ink: &Ink) -> Element<'a, Msg> {
    let mut col = column![];
    for v in &p.versions {
        let live = v.hash == p.hash && p.present;
        let held = if v.held.is_empty() {
            ("the next reap removes it".to_string(), ink.fg3)
        } else if live {
            ("live".to_string(), ink.success)
        } else {
            (held_words(&v.held), ink.fg3)
        };
        col = col.push(
            row![
                container(mono(short(&v.hash).to_string(), SMALL, ink.fg2)).width(Length::Fixed(100.0)),
                container(ui(v.arrived.get(..10).unwrap_or(&v.arrived).to_string(), SMALL, ink.fg2)).width(Length::Fixed(96.0)),
                container(ui(held.0, SMALL, held.1)).width(Length::Fill).clip(true),
                container(mono(size(v.size.unwrap_or(0)), SMALL, ink.fg2)).width(Length::Fixed(70.0)).align_x(Alignment::End),
            ]
            .spacing(12)
            .height(Length::Fixed(22.0))
            .align_y(Alignment::Center),
        );
    }
    col.into()
}

/// What keeps an old version, in a few words.
fn held_words(held: &[String]) -> String {
    let gens: Vec<&str> = held.iter().filter_map(|h| h.strip_prefix("generation:")).collect();
    let users = held.iter().filter(|h| h.starts_with("needs:")).count();
    let plans = held.iter().filter(|h| h.starts_with("plan:")).count();
    let mut out = Vec::new();
    if held.iter().any(|h| h == "release") {
        out.push("the release".to_string());
    }
    if held.iter().any(|h| h == "pin") {
        out.push("a pin".to_string());
    }
    if let (Some(a), Some(b)) = (gens.first(), gens.last()) {
        out.push(if a == b { format!("generation {a}") } else { format!("generations {a}–{b}") });
    }
    if users > 0 {
        out.push(words_n(users, "package's need", "packages' needs"));
    }
    if plans > 0 {
        out.push(words_n(plans, "plan", "plans"));
    }
    if held.iter().any(|h| h == "arrived") {
        out.push("arrived today".to_string());
    }
    if out.is_empty() {
        return held.join(", ");
    }
    format!("kept by {}", out.join(", "))
}

fn source<'a>(p: &'a Package, ink: &Ink) -> Element<'a, Msg> {
    let Some(s) = &p.source else {
        return Space::new().into();
    };
    let pushed = match &p.pushed {
        Some(Pushed::Yes(b)) => format!("yes, {b} has it"),
        Some(Pushed::No) => "no: no remote branch has the commit".into(),
        Some(Pushed::Dirty) => "no: built from uncommitted changes".into(),
        Some(Pushed::Unknown(why)) => format!("unknown: {why}"),
        None if p.is_release_build() => "yes: a release is built from a pushed master".into(),
        None => "not checked".into(),
    };
    let kv = |k: &'a str, v: String| -> Element<'a, Msg> {
        row![container(ui(k, SMALL, ink.fg3)).width(Length::Fixed(96.0)), mono(v, SMALL, ink.fg2)].spacing(12).into()
    };
    column![
        kv("Repository", s.repo.clone()),
        kv("Commit", s.commit.clone()),
        kv("Branch", s.branch.clone()),
        kv("Dirty", if s.dirty { "yes".into() } else { "no".into() }),
        kv("Pushed", pushed),
        kv("Built by", s.built_by.clone()),
        kv("Built at", s.built_at.clone()),
    ]
    .spacing(4)
    .into()
}

#[cfg(test)]
mod tests {
    use super::held_words;

    #[test]
    fn what_keeps_a_version() {
        let held: Vec<String> = ["generation:88", "generation:92", "needs:pkg:cef", "release"].iter().map(|s| s.to_string()).collect();
        assert_eq!(held_words(&held), "kept by the release, generations 88–92, 1 package's need");
    }
}
