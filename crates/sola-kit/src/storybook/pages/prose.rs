//! Prose — letter reading: paragraphs, quotes, inline links.

use iced::widget::column;
use iced::{Element, Length};

use sola_kit::components::prose::{parse_markdown, parse_plain, prose};
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

const MARKDOWN: &str = "\
# Chorus

**Bold** and *italic* stay.

```verse
Hello darkness, my old friend
I've come to talk with you again
```

- one
- two
";

pub fn view(theme: &iced::Theme) -> Element<'static, Msg> {
    column![
        lede(
            "Prose",
            "Letter measure for mail (`parse_plain`). Chat markdown (`parse_markdown`) keeps line breaks, headings, emphasis, lists, and `verse` fences for lyrics.",
        ),
        readable(
            panel(
                column![
                    prose(parse_plain(SAMPLE), theme, |_| Msg::Select(
                        crate::storybook::Page::Prose
                    )),
                    body("Mail: soft-wrapped paragraphs, quoted replies, links.")
                        .style(muted),
                    prose(parse_markdown(MARKDOWN), theme, |_| Msg::Select(
                        crate::storybook::Page::Prose
                    )),
                    body("Chat: hard line breaks. Verse fences keep stanza lines.")
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
