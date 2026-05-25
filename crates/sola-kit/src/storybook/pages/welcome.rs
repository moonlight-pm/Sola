//! Welcome / index page — first thing the storybook shows on launch.
//!
//! Doubles as a font pre-warm sheet: every role font is exercised on
//! this page with a hex-digit + alphabet sample so cosmic-text shapes
//! the glyphs the Theme page later needs (display, mono, code in
//! particular) before the user clicks through. Without this the Theme
//! page's first paint pays the rasterisation tax for fonts no earlier
//! page touched.

use iced::widget::{column, row};
use iced::{Element, Length};

use sola_kit::components::text::{body, caption, code, heading, muted, subheading};

use crate::storybook::Msg;

const SAMPLE: &str = "AaBbCc 0123456789  the quick brown fox";

pub fn view() -> Element<'static, Msg> {
    column![
        heading("Welcome"),
        body(
            "sola-kit ships reusable iced widgets, named style fns, the \
             canonical palette, and the boilerplate every iced app would \
             otherwise repeat (bus connect, app menu, font registration, \
             window settings)."
        ),
        body(
            "The storybook dogfoods every component. Pick a page on the \
             left to see it rendered against the current theme."
        )
        .style(muted),

        subheading("Font roles"),
        body("Each row is rendered in one of the kit's semantic font roles.")
            .style(muted),
        font_sample("display", iced::widget::text(SAMPLE).size(18).font(sola_kit::fonts::display())),
        font_sample("ui_medium", iced::widget::text(SAMPLE).size(14).font(sola_kit::fonts::ui_medium())),
        font_sample("ui",       iced::widget::text(SAMPLE).size(14).font(sola_kit::fonts::ui())),
        font_sample("chrome",   iced::widget::text(SAMPLE).size(12).font(sola_kit::fonts::chrome())),
        font_sample("mono",     code(SAMPLE)),
    ]
    .spacing(12)
    .into()
}

fn font_sample<'a>(role: &'static str, sample: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    row![
        iced::widget::container(caption(role).style(muted))
            .width(Length::Fixed(96.0)),
        sample.into(),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}
