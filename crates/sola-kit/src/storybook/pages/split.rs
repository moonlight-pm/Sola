//! Split showcase — orientation-parameterized two-pane split with the
//! kit's draggable divider in between. The ratio is static in the
//! showcase (a real consumer manages drag state and tracks the cursor
//! to update the ratio); the visual + both orientations are what
//! matter here.

use iced::widget::{column, container};
use iced::{Element, Length};

use sola_bus::topics::SplitDir;
use sola_kit::components::card::style as card_style;
use sola_kit::components::split_with;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::DividerColors;

use crate::storybook::Msg;

pub fn view() -> Element<'static, Msg> {
    // Card-raised panels sit on canvas — divider sides must match the
    // raised fill or the hit strip paints a canvas gutter through the card.
    // Storybook pages don't hold Theme in view state; seed snapshot is fine.
    let chrome = DividerColors::raised(&sola_kit::theme::default_theme());

    let left = container(body("Pane A — 60%").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);
    let right = container(body("Pane B — fills remainder").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill);

    let vertical = container(split_with(
        SplitDir::Vertical,
        left,
        0.6,
        Msg::Noop,
        right,
        chrome,
    ))
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

    let horizontal = container(split_with(
        SplitDir::Horizontal,
        top,
        0.4,
        Msg::Noop,
        bottom,
        chrome,
    ))
    .style(card_style)
    .height(Length::Fixed(200.0))
    .width(Length::Fill);

    column![
        heading("Split"),
        body(
            "Two-pane split. Divider is a | line | b bands (1px hairline in an 8px hit \
             strip). Pass DividerColors so side bands match the panes being split."
        )
        .style(muted),
        body("Vertical — side-by-side (new pane on the right, ⌘⇧→)").style(muted),
        vertical,
        body("Horizontal — stacked (new pane below, ⌘⇧↓)").style(muted),
        horizontal,
        code("split_with(dir, a, ratio, on_drag, b, DividerColors::raised(theme))").style(muted),
    ]
    .spacing(16)
    .into()
}
