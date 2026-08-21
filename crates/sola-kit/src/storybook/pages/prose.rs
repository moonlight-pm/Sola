//! Prose — letter reading: paragraphs, quotes, inline links.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::prose::{parse_plain, prose};
use sola_kit::components::readable;
use sola_kit::components::text::{body, muted};

use crate::storybook::Msg;
use crate::storybook::pages::chrome::{lede, panel};

const SAMPLE: &str = "\
Hi Joshua,

Confirm the sign-in at https://auth.example.com/login/magic/verify?token=abc \
if this was you.

> On 12 Aug Joshua wrote:
> Does the reading pane still look like a form field?

Best,
Mail
";

pub fn view(theme: &iced::Theme) -> Element<'static, Msg> {
    column![
        lede(
            "Prose",
            "Letter measure: paragraphs, quoted replies, inline links. Drag to select; click a link. I-bar only over the letter — sibling chrome stays the default pointer.",
        ),
        readable(
            panel(
                column![
                    prose(parse_plain(SAMPLE), theme, |_| Msg::Select(
                        crate::storybook::Page::Prose
                    )),
                    body("Drag to select. Click a link — it should feel like mail, not a chip row.")
                        .style(muted),
                ]
                .spacing(16),
            )
            .width(Length::Fill),
            560.0,
        ),
    ]
    .spacing(16)
    .into()
}
