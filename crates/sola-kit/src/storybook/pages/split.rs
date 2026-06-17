//! Split showcase — orientation-parameterized two-pane split with the
//! kit's draggable divider in between. The ratio is static in the
//! showcase (a real consumer manages drag state and tracks the cursor
//! to update the ratio); the visual + both orientations are what
//! matter here.

use iced::widget::{column, container};
use iced::{Element, Length};

use sola_bus::topics::SplitDir;
use sola_kit::components::card::style as card_style;
use sola_kit::components::split;
use sola_kit::components::text::{body, code, heading, muted};

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    let left = container(body("Pane A — 60%").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);
    let right = container(body("Pane B — fills remainder").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);

    let vertical = container(split(SplitDir::Vertical, left, 0.6, Msg::Noop, right))
        .style(card_style)
        .height(Length::Fixed(200.0))
        .width(Length::Fill);

    let top = container(body("Pane A — 40%").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);
    let bottom = container(body("Pane B — fills remainder").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);

    let horizontal = container(split(SplitDir::Horizontal, top, 0.4, Msg::Noop, bottom))
        .style(card_style)
        .height(Length::Fixed(200.0))
        .width(Length::Fill);

    column![
        heading("Split"),
        body("Two-pane split, side-by-side or stacked. The divider emits on_drag; the consumer tracks the cursor to update the ratio.")
            .style(muted),
        body("Vertical — side-by-side (new pane on the right, ⌘⇧→)").style(muted),
        vertical,
        body("Horizontal — stacked (new pane below, ⌘⇧↓)").style(muted),
        horizontal,
        code("split(dir, a, ratio, on_drag, b)").style(muted),
    ]
    .spacing(16)
    .into()
}
