//! The shape every Settings page shares, so they read as one: a scrolling
//! column ([`page`]) of sections ([`section`]) — a heading with an action
//! on the right, a [`group`] (one raised, rounded surface of rows split by
//! hairlines) and a caption under it. A row ([`Item`]) opens its editor in
//! place, under it in the same group ([`editor`]), rather than in a detail
//! pane beside the list: the page is only as tall as what it holds.

use std::sync::OnceLock;

use iced::widget::{Column, Space, button, column, container, row, svg, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::style::{
    PAD_CONTROL_SM, RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, alpha,
    card_fill, hairline, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_kit::components::scroll;

/// The widest a page's column grows.
pub const PAGE_WIDTH: f32 = 600.0;
/// A group's inset around its rows, and a row's own padding.
const GROUP_PAD: f32 = 4.0;
const ROW_PAD: Padding = Padding {
    top: 8.0,
    bottom: 8.0,
    left: 10.0,
    right: 8.0,
};

/// A page: its sections in a column, scrolling when they outgrow the
/// window (room on the right for the scrollbar, and a gap to it).
pub fn page<'a, M: 'a>(sections: Vec<Element<'a, M>>) -> Element<'a, M> {
    let col = Column::from_vec(sections)
        .spacing(SPACE_XL + SPACE_LG)
        .width(Length::Fill)
        .max_width(PAGE_WIDTH);
    scroll::scrollable(container(col).padding(Padding {
        right: scroll::CLEARANCE + SPACE_XL,
        bottom: SPACE_XL,
        ..Padding::ZERO
    }))
    .height(Length::Fill)
    .into()
}

/// A section: `title` (and `action` on the right) over `body`, `caption`
/// under it.
pub fn section<'a, M: 'a>(
    title: &'a str,
    action: Option<Element<'a, M>>,
    body: Element<'a, M>,
    caption: Option<String>,
) -> Element<'a, M> {
    let mut head = row![
        kit_text::caption(title).style(kit_text::muted),
        Space::new().width(Length::Fill)
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .height(Length::Fixed(26.0))
    .padding(Padding {
        left: SPACE_XS,
        ..Padding::ZERO
    });
    if let Some(action) = action {
        head = head.push(action);
    }
    let mut col = column![head, body].spacing(SPACE_SM).width(Length::Fill);
    if let Some(caption) = caption {
        col = col.push(
            container(kit_text::caption(caption).style(kit_text::muted)).padding(Padding {
                left: SPACE_XS,
                right: SPACE_XS,
                ..Padding::ZERO
            }),
        );
    }
    col.into()
}

/// Rows on one raised surface, a hairline between each two.
pub fn group<'a, M: 'a>(rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    let mut col = column![].width(Length::Fill);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            col = col.push(rule());
        }
        col = col.push(r);
    }
    container(col)
        .padding(GROUP_PAD)
        .width(Length::Fill)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            container::Style {
                background: Some(card_fill(p.background.weaker.color)),
                border: hairline(p, RADIUS_LG),
                ..container::Style::default()
            }
        })
        .into()
}

fn rule<'a, M: 'a>() -> Element<'a, M> {
    container(
        container(Space::new().width(Length::Fill).height(1)).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(mix_white(
                    theme.extended_palette().background.weaker.color,
                    0.06,
                ))),
                ..container::Style::default()
            }
        }),
    )
    .padding(Padding {
        top: 1.0,
        bottom: 1.0,
        left: ROW_PAD.left,
        right: ROW_PAD.left,
    })
    .into()
}

/// A plain row in a group: text and controls, no press (a form row).
pub fn plain<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding(ROW_PAD)
        .width(Length::Fill)
        .into()
}

/// A setting in a group: its label, and its control on the right.
pub fn setting<'a, M: 'a>(
    label: impl Into<String>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    container(sola_kit::components::form::form_row(label, control))
        .padding(Padding {
            top: 2.0,
            bottom: 2.0,
            ..ROW_PAD
        })
        .width(Length::Fill)
        .into()
}

/// A row with nothing to list yet.
pub fn empty<'a, M: 'a>(message: impl Into<String>) -> Element<'a, M> {
    plain(kit_text::body(message.into()).style(kit_text::muted))
}

/// The editor of the row above it, in the same group.
pub fn editor<'a, M: 'a>(body: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(body)
        .padding(Padding {
            top: SPACE_MD,
            bottom: SPACE_LG,
            left: ROW_PAD.left,
            right: ROW_PAD.left,
        })
        .width(Length::Fill)
        .into()
}

/// An editor's buttons: `left` (Save, Discard…), `right` (Remove, Close).
pub fn footer<'a, M: 'a>(left: Vec<Element<'a, M>>, right: Vec<Element<'a, M>>) -> Element<'a, M> {
    let mut r = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    for b in left {
        r = r.push(b);
    }
    r = r.push(Space::new().width(Length::Fill));
    for b in right {
        r = r.push(b);
    }
    r.into()
}

/// A small labeled button for an editor's footer.
pub fn action<'a, M: Clone + 'a>(
    label: &'a str,
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
    on_press: M,
) -> Element<'a, M> {
    sola_kit::components::button::labeled_sm(label, style)
        .on_press(on_press)
        .into()
}

/// A section's add action: "+ label".
pub fn add<'a, M: Clone + 'a>(label: &'a str, on_press: M) -> Element<'a, M> {
    button(
        row![
            glyph(plus(), 12, 0.8),
            text(label).font(fonts::ui()).size(12)
        ]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center),
    )
    .padding(PAD_CONTROL_SM)
    .style(sola_kit::components::button::secondary)
    .on_press(on_press)
    .into()
}

/// A row that opens: a leading mark, title and subtitle, a trailing note,
/// and a chevron that turns down while open.
pub struct Item<'a, M> {
    title: String,
    subtitle: Option<String>,
    leading: Option<Element<'a, M>>,
    trailing: Option<Element<'a, M>>,
    open: bool,
    on_press: Option<M>,
}

pub fn item<'a, M: Clone + 'a>(title: impl Into<String>) -> Item<'a, M> {
    Item {
        title: title.into(),
        subtitle: None,
        leading: None,
        trailing: None,
        open: false,
        on_press: None,
    }
}

impl<'a, M: Clone + 'a> Item<'a, M> {
    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        self.subtitle = (!s.is_empty()).then_some(s);
        self
    }

    pub fn leading(mut self, e: impl Into<Element<'a, M>>) -> Self {
        self.leading = Some(e.into());
        self
    }

    pub fn trailing(mut self, e: impl Into<Element<'a, M>>) -> Self {
        self.trailing = Some(e.into());
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn on_press(mut self, m: M) -> Self {
        self.on_press = Some(m);
        self
    }
}

impl<'a, M: Clone + 'a> From<Item<'a, M>> for Element<'a, M> {
    fn from(item: Item<'a, M>) -> Self {
        let mut words = column![kit_text::body(item.title)].spacing(1).width(Length::Fill);
        if let Some(s) = item.subtitle {
            words = words.push(kit_text::caption(s).style(kit_text::muted));
        }
        let mut r = row![].spacing(SPACE_MD + SPACE_XS).align_y(Alignment::Center);
        if let Some(lead) = item.leading {
            r = r.push(container(lead).center_x(Length::Fixed(20.0)));
        }
        r = r.push(words);
        if let Some(trail) = item.trailing {
            r = r.push(trail);
        }
        if item.on_press.is_some() {
            let chevron = if item.open { chevron_down() } else { chevron_right() };
            r = r.push(glyph(chevron, 14, 0.45));
        }
        let open = item.open;
        button(r)
            .padding(ROW_PAD)
            .width(Length::Fill)
            .style(move |theme: &Theme, status| row_style(theme, status, open))
            .on_press_maybe(item.on_press)
            .into()
    }
}

fn row_style(theme: &Theme, status: button::Status, open: bool) -> button::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let fill = match status {
        _ if open => Some(mix_white(raised, 0.05)),
        button::Status::Hovered => Some(mix_white(raised, 0.035)),
        button::Status::Pressed => Some(mix_white(raised, 0.06)),
        _ => None,
    };
    button::Style {
        background: fill.map(Background::Color),
        text_color: p.background.base.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        ..button::Style::default()
    }
}

/// A pack icon in the text color at `strength`.
pub fn glyph<'a, M: 'a>(handle: svg::Handle, size: u16, strength: f32) -> Element<'a, M> {
    svg(handle)
        .width(Length::Fixed(f32::from(size)))
        .height(Length::Fixed(f32::from(size)))
        .style(move |theme: &Theme, _| svg::Style {
            color: Some(alpha(theme.extended_palette().background.base.text, strength)),
        })
        .into()
}

/// A named pack icon (`"lucide/mail"`), loaded once per name.
pub fn icon(name: &'static str) -> svg::Handle {
    static CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<&'static str, svg::Handle>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(name).or_insert_with(|| icon_handle(name)).clone()
}

fn plus() -> svg::Handle {
    icon("lucide/plus")
}

fn chevron_right() -> svg::Handle {
    icon("lucide/chevron-right")
}

fn chevron_down() -> svg::Handle {
    icon("lucide/chevron-down")
}

/// A small round mark of `color` (a calendar's, an account's).
pub fn dot<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new().width(10).height(10))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: alpha(Color::WHITE, 0.2),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}
