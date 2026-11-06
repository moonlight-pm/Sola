//! The card over the map: the lit package, its verdict and its counts,
//! and the way to its sheet or its tree.

use iced::widget::{Space, button, column, container, row};
use iced::{Alignment, Color, Element, Length, Padding, Shadow, Theme, Vector};
use sola_kit::components::button::{labeled_sm, secondary};

use super::ink::Ink;
use super::words::{self, size};
use super::{CAPTION, SMALL, TITLE, View, display, mono, sheet, ui};
use crate::Msg;
use crate::model::{Ecosystem, Package};

/// A press target with no chrome of its own.
pub fn bare(_: &Theme, _: button::Status) -> button::Style {
    button::Style { background: None, ..Default::default() }
}

pub fn view<'a>(e: &'a Ecosystem, p: &'a Package, ink: Ink) -> Element<'a, Msg> {
    let (below, above) = e.reach(&p.name);
    let count = |n: String, label: &'a str| -> Element<'a, Msg> {
        column![mono(n, TITLE, ink.fg), ui(label, CAPTION, ink.fg3)].spacing(3).width(Length::Fill).into()
    };
    let weight = if p.here { p.in_use() } else { p.listed.as_ref().and_then(|l| l.size).unwrap_or(0) };
    let body = column![
        row![
            display(p.name.clone(), TITLE, ink.fg),
            mono(p.version.clone(), SMALL, ink.fg3),
            Space::new().width(Length::Fill),
            sheet::standing_badge(p.standing, &ink),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        ui(words::verdict(e, p).1, SMALL, ink.fg2).line_height(1.5),
        row![
            count(below.to_string(), "below"),
            count(above.to_string(), "above"),
            count(size(weight), "in use"),
            count(size(p.on_disk()), "on disk"),
        ]
        .spacing(8),
        row![
            labeled_sm("Details", secondary).on_press(Msg::Show(View::Packages)),
            labeled_sm("Open Tree", secondary).on_press(Msg::Show(View::Tree)),
        ]
        .spacing(6),
    ]
    .spacing(10);
    let (bg, edge) = (ink.raised, ink.rule2);
    container(body)
        .width(Length::Fixed(430.0))
        .padding(Padding::from([14, 16]))
        .style(move |_| container::Style {
            background: Some(bg.into()),
            border: iced::Border { color: edge, width: 1.0, radius: 10.0.into() },
            shadow: Shadow { color: Color { a: 0.45, ..Color::BLACK }, offset: Vector::new(0.0, 2.0), blur_radius: 12.0 },
            ..Default::default()
        })
        .into()
}
