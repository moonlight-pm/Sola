//! Stat detail dropdown panels, rendered in the Menu window.

use iced::widget::{column, container, mouse_area, stack, text};
use iced::{Element, Length, Padding};

use crate::app::{Msg, Shell};
use crate::stats::Metric;
use sola_kit::components::popover;

pub const CARD_WIDTH: f32 = 320.0;

/// Lower-contrast label text. We deliberately do NOT use
/// `sola_kit::components::text::muted` here — on the dropdown card it resolves
/// to a colour that renders invisible (the same trap the menu accelerators
/// hit). Deriving from `palette().text` keeps it visible. Mirrors
/// `crate::calendar::dim`.
fn dim(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(iced::Color {
            a: 0.55,
            ..theme.palette().text
        }),
    }
}

/// Build the right-anchored panel for `metric`, over a dismiss backdrop.
/// Mirrors `crate::menu::view::calendar_panel`.
pub fn panel(shell: &Shell, metric: Metric) -> Element<'_, Msg> {
    let card = match metric {
        Metric::Cpu => cpu_card(shell),
        Metric::Gpu => placeholder("GPU"),
        Metric::Mem => placeholder("Memory"),
        Metric::Net => placeholder("Network"),
    };

    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = (output_w - CARD_WIDTH - 8.0).max(0.0);

    let positioned: Element<'_, Msg> = container(card)
        .padding(Padding {
            top: 0.0,
            left,
            right: 0.0,
            bottom: 0.0,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Top)
        .into();

    let backdrop: Element<'_, Msg> = mouse_area(
        container(text("")).width(Length::Fill).height(Length::Fill),
    )
    .on_press(Msg::CloseMenu)
    .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn placeholder(label: &str) -> Element<'static, Msg> {
    popover(column![text(label.to_string()).size(14)].padding(4))
        .padding(Padding::new(8.0))
        .width(Length::Fixed(CARD_WIDTH))
        .into()
}

/// Minimal CPU card (header only) — fleshed out in Phase 3.
fn cpu_card(shell: &Shell) -> Element<'_, Msg> {
    let pct = shell.stats.cpu_pct;
    popover(
        column![
            text("CPU").size(11).style(dim),
            text(format!("{:.0}%", pct))
                .font(sola_kit::fonts::MONO)
                .size(28),
        ]
        .spacing(4)
        .padding(4),
    )
    .padding(Padding::new(8.0))
    .width(Length::Fixed(CARD_WIDTH))
    .into()
}
