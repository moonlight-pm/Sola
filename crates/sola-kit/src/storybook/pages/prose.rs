//! Prose — letter reading: paragraphs, quotes, inline links, rules and
//! pictures.

use iced::widget::column;
use iced::{Element, Length};

use std::collections::HashMap;
use std::sync::Arc;

use sola_kit::components::prose::{
    ImagePixels, ProseBlock, ProseImage, ProseRun, parse_markdown, parse_plain, prose, prose_letter,
};
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

/// A letter with a rule and a picture (an accent-to-ink gradient the
/// page paints itself; a letter's would come from the message or the web).
fn letter(theme: &iced::Theme) -> Element<'static, Msg> {
    let (w, h) = (480u32, 120u32);
    let p = theme.extended_palette();
    let (a, b) = (p.primary.base.color, p.background.base.text);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            let mix = |a: f32, b: f32| ((a + (b - a) * t) * 255.0) as u8;
            rgba.extend([mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), 255]);
        }
    }
    let images = Arc::new(HashMap::from([(
        "gradient".to_string(),
        ImagePixels {
            handle: iced::advanced::image::Handle::from_rgba(w, h, rgba),
            width: w,
            height: h,
            plate: false,
        },
    )]));
    let blocks = vec![
        ProseBlock::Paragraph(vec![ProseRun::text("A picture keeps its proportions at the width the letter asked for, inside the column:")]),
        ProseBlock::Images(vec![ProseImage {
            key: "gradient".into(),
            alt: "Gradient".into(),
            width: Some(w),
            height: Some(h),
            url: None,
        }]),
        ProseBlock::Paragraph(vec![ProseRun::text("Pictures side by side (a row of icons) share the line:")]),
        ProseBlock::Images(
            (0..3)
                .map(|n| ProseImage {
                    key: "gradient".into(),
                    alt: format!("Icon {n}"),
                    width: Some(48),
                    height: None,
                    url: None,
                })
                .collect(),
        ),
        ProseBlock::Paragraph(vec![
            ProseRun::text("A run can be struck through, as an old price: "),
            ProseRun {
                strike: true,
                ..ProseRun::text("$14.99")
            },
            ProseRun::text("  $9.74"),
        ]),
        ProseBlock::Rule,
        ProseBlock::Paragraph(vec![ProseRun::text("A rule sits between sections. A picture the app has not supplied takes no room.")]),
        ProseBlock::Images(vec![ProseImage {
            key: "missing".into(),
            alt: "Not loaded".into(),
            width: Some(600),
            height: Some(200),
            url: None,
        }]),
    ];
    prose_letter(
        blocks,
        theme,
        0,
        images,
        |_| Msg::Select(crate::storybook::Page::Prose),
        |_| Msg::Select(crate::storybook::Page::Prose),
    )
}

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
                    letter(theme),
                    body("HTML mail (`prose_letter`): pictures by key, rules.").style(muted),
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
