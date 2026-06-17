//! Stat detail dropdown panels, rendered in the Menu window.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{column, container, mouse_area, stack, text};
use iced::{mouse, Color, Element, Length, Padding, Point, Rectangle, Renderer, Theme};

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

// ---------------------------------------------------------------------------
// History graph widget
// ---------------------------------------------------------------------------

/// A 60-sample area+line history chart. `max` is the value mapped to the top
/// (e.g. 100.0 for percentages, or the buffer peak for rates).
pub struct Graph {
    pub samples: Vec<f32>,
    pub max: f32,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for Graph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let n = self.samples.len();
        if n < 2 || self.max <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let w = bounds.width;
        let h = bounds.height;
        let x = |i: usize| (i as f32 / (n - 1) as f32) * w;
        let y = |v: f32| h - (v / self.max).clamp(0.0, 1.0) * h;

        let line = Path::new(|p: &mut canvas::path::Builder| {
            p.move_to(Point::new(x(0), y(self.samples[0])));
            for i in 1..n {
                p.line_to(Point::new(x(i), y(self.samples[i])));
            }
        });
        let area = Path::new(|p: &mut canvas::path::Builder| {
            p.move_to(Point::new(x(0), h));
            for i in 0..n {
                p.line_to(Point::new(x(i), y(self.samples[i])));
            }
            p.line_to(Point::new(x(n - 1), h));
            p.close();
        });
        frame.fill(&area, Color { a: 0.25, ..self.color });
        frame.stroke(
            &line,
            Stroke::default().with_color(self.color).with_width(1.5),
        );
        vec![frame.into_geometry()]
    }
}

/// Convenience: a fixed-height graph element from samples.
pub fn history_graph<'a, Message: 'a>(
    samples: Vec<f32>,
    max: f32,
    color: Color,
) -> Element<'a, Message> {
    Canvas::new(Graph { samples, max, color })
        .width(Length::Fill)
        .height(Length::Fixed(58.0))
        .into()
}
