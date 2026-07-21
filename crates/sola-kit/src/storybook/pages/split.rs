//! Split showcase — interactive two-pane split with live drag.
//!
//! Stateful so the divider actually resizes the panes (same consumer
//! pattern as terminal/monitor: press on the divider, track global
//! cursor motion / release for ratio). Demo containers have fixed
//! logical sizes so cursor position maps cleanly to a ratio without
//! layout probes.

use iced::widget::{column, container, mouse_area, stack, Space};
use iced::{Element, Length, Theme, mouse};

use sola_bus::topics::SplitDir;
use sola_kit::components::card::style as card_style;
use sola_kit::components::split_with;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::DividerColors;

/// Logical size of each demo card's content area. Fixed so drag→ratio
/// math does not need widget layout geometry.
const DEMO_W: f32 = 560.0;
const DEMO_H: f32 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Divider pressed on the vertical (side-by-side) demo.
    VerticalPress,
    /// Divider pressed on the horizontal (stacked) demo.
    HorizontalPress,
    /// Global cursor moved while a drag is live (window-logical coords).
    CursorMoved { x: f32, y: f32 },
    /// Mouse released — end any active drag.
    Release,
}

pub struct State {
    pub vertical_ratio: f32,
    pub horizontal_ratio: f32,
    /// Which demo is mid-drag, if any.
    pub dragging: Option<Axis>,
    /// Demo origin in window coords, captured on the first cursor sample
    /// after press (anchor-on-first-move — we don't know layout origin
    /// until the pointer moves over the demo).
    pub origin: Option<(f32, f32)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            vertical_ratio: 0.6,
            horizontal_ratio: 0.4,
            dragging: None,
            origin: None,
        }
    }
}

impl State {
    pub fn needs_cursor_subscription(&self) -> bool {
        self.dragging.is_some()
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::VerticalPress => {
                self.dragging = Some(Axis::Vertical);
                self.origin = None;
            }
            Msg::HorizontalPress => {
                self.dragging = Some(Axis::Horizontal);
                self.origin = None;
            }
            Msg::CursorMoved { x, y } => {
                let Some(axis) = self.dragging else { return };
                // First sample: treat the cursor as sitting on the divider
                // and back-compute the demo origin from the current ratio.
                // That way we never need an absolute layout rect.
                if self.origin.is_none() {
                    let (ox, oy) = match axis {
                        Axis::Vertical => (x - self.vertical_ratio * DEMO_W, y),
                        Axis::Horizontal => (x, y - self.horizontal_ratio * DEMO_H),
                    };
                    self.origin = Some((ox, oy));
                }
                let Some((ox, oy)) = self.origin else { return };
                match axis {
                    Axis::Vertical => {
                        let r = ((x - ox) / DEMO_W).clamp(0.1, 0.9);
                        self.vertical_ratio = r;
                    }
                    Axis::Horizontal => {
                        let r = ((y - oy) / DEMO_H).clamp(0.1, 0.9);
                        self.horizontal_ratio = r;
                    }
                }
            }
            Msg::Release => {
                self.dragging = None;
                self.origin = None;
            }
        }
    }
}

pub fn view<'a>(state: &'a State, theme: &Theme) -> Element<'a, Msg> {
    let chrome = DividerColors::raised(theme);

    let left = pane("Pane A");
    let right = pane("Pane B");
    let vertical = container(split_with(
        SplitDir::Vertical,
        left,
        state.vertical_ratio,
        Msg::VerticalPress,
        right,
        chrome,
    ))
    .style(card_style)
    .height(Length::Fixed(DEMO_H))
    .width(Length::Fixed(DEMO_W));

    let top = pane("Pane A");
    let bottom = pane("Pane B");
    let horizontal = container(split_with(
        SplitDir::Horizontal,
        top,
        state.horizontal_ratio,
        Msg::HorizontalPress,
        bottom,
        chrome,
    ))
    .style(card_style)
    .height(Length::Fixed(DEMO_H))
    .width(Length::Fixed(DEMO_W));

    // While dragging, a transparent overlay over the demo keeps the
    // resize cursor even if the pointer races past the hairline (same
    // idea as SidebarPanel / terminal).
    let vertical = with_drag_overlay(vertical, state.dragging == Some(Axis::Vertical), true);
    let horizontal =
        with_drag_overlay(horizontal, state.dragging == Some(Axis::Horizontal), false);

    column![
        heading("Split"),
        body(
            "Two-pane split with a live-draggable divider. Side bands use \
             DividerColors::raised so the hit strip matches the card; only \
             the 1px hairline shows. Drag either divider to resize."
        )
        .style(muted),
        body("Vertical — side-by-side (drag the centre line)").style(muted),
        vertical,
        body(format!(
            "ratio = {:.0}% / {:.0}%",
            state.vertical_ratio * 100.0,
            (1.0 - state.vertical_ratio) * 100.0
        ))
        .style(muted),
        body("Horizontal — stacked (drag the centre line)").style(muted),
        horizontal,
        body(format!(
            "ratio = {:.0}% / {:.0}%",
            state.horizontal_ratio * 100.0,
            (1.0 - state.horizontal_ratio) * 100.0
        ))
        .style(muted),
        code("split_with(dir, a, ratio, on_drag, b, DividerColors::raised(theme))").style(muted),
    ]
    .spacing(16)
    .into()
}

fn pane(label: &str) -> Element<'static, Msg> {
    container(body(label.to_string()).style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn with_drag_overlay<'a>(
    demo: impl Into<Element<'a, Msg>>,
    dragging: bool,
    vertical: bool,
) -> Element<'a, Msg> {
    let demo = demo.into();
    if !dragging {
        return demo;
    }
    let interaction = if vertical {
        mouse::Interaction::ResizingColumn
    } else {
        mouse::Interaction::ResizingRow
    };
    stack![
        demo,
        mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .interaction(interaction),
    ]
    .into()
}
