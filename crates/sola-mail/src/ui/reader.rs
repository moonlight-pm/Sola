//! Reading pane: the open letter, or why there is none.

use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon_svg;
use sola_kit::components::prose::prose_selectable;
use sola_kit::components::readable;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use super::format::split_address;
use super::widgets::{h_hairline, icon};
use super::{App, Msg, style};
use crate::protocol::date::{letter_date, letter_time};
use crate::protocol::{MailAttachment, MessageBody, attachments};

/// Comfortable reading measure (~65ch at 14px prose).
const READ_MAX_W: f32 = 640.0;

pub enum Reader {
    /// Nothing requested yet (or the selection cleared).
    Empty,
    /// The body FETCH is in flight: show a blank well, never the
    /// previous letter under a different row.
    Loading,
    Open(Box<Letter>),
    Failed(String),
}

pub struct Letter {
    pub body: MessageBody,
    /// Visible in-body selection (drag or Select All). Copy uses this.
    pub selection: Option<String>,
    /// Bump to make the letter widget select everything.
    pub select_all: u64,
}

impl Reader {
    pub fn letter(&self) -> Option<&Letter> {
        match self {
            Reader::Open(l) => Some(l),
            _ => None,
        }
    }

    pub fn letter_mut(&mut self) -> Option<&mut Letter> {
        match self {
            Reader::Open(l) => Some(l),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&MessageBody> {
        self.letter().map(|l| &l.body)
    }
}

impl App {
    pub(super) fn view_reader(&self) -> Element<'_, Msg> {
        let inner: Element<'_, Msg> = match (&self.selected, &self.reader) {
            (None, _) => column![
                kit_text::body("No message selected").style(kit_text::muted),
                kit_text::caption("Pick one from the list, or press ↓").style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .align_x(iced::Alignment::Center)
            .into(),
            (Some(_), Reader::Failed(message)) => column![
                kit_text::body("Couldn’t open this message").style(kit_text::muted),
                kit_text::caption(super::format::truncate(message, 90)).style(kit_text::muted),
                kit_btn::labeled_sm("Try Again", kit_btn::secondary).on_press(Msg::RetryBody),
            ]
            .spacing(SPACE_SM)
            .align_x(iced::Alignment::Center)
            .into(),
            (Some(id), Reader::Open(letter)) if letter.body.id == *id => {
                return self.view_letter(letter);
            }
            _ => Space::new().width(Length::Fill).height(Length::Fill).into(),
        };
        container(inner)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(style::read_pane)
            .into()
    }

    fn view_letter<'a>(&'a self, letter: &'a Letter) -> Element<'a, Msg> {
        let body = &letter.body;
        let mut col = column![letter_header(body)]
            .spacing(20.0)
            .width(Length::Fill);
        if !body.attachments.is_empty() {
            col = col.push(received_attachments(&body.attachments));
        }
        col = col.push(h_hairline()).push(prose_selectable(
            body.blocks.clone(),
            &self.theme,
            letter.select_all,
            Msg::OpenUrl,
            Msg::BodySelect,
        ));
        let page = readable(
            col.padding(Padding {
                top: 28.0,
                right: 8.0,
                bottom: 40.0,
                left: 8.0,
            }),
            READ_MAX_W,
        )
        .width(Length::Fill);
        container(scrollable(page).height(Length::Fill).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::read_pane)
            .into()
    }
}

fn letter_header(body: &MessageBody) -> Element<'static, Msg> {
    let subject = if body.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        body.subject.clone()
    };
    let (name, addr) = split_address(&body.from);
    let mut from = column![text(name).font(fonts::ui_medium()).size(14)]
        .spacing(2)
        .width(Length::Fill);
    if let Some(addr) = addr {
        from = from.push(kit_text::caption(addr).style(kit_text::muted));
    }
    let mut date = column![kit_text::caption(letter_date(body.date)).style(kit_text::muted)]
        .spacing(2)
        .width(Length::Fill);
    let time = letter_time(body.date);
    if !time.is_empty() {
        date = date.push(kit_text::caption(time).style(kit_text::muted));
    }
    let mut meta = column![meta_row("From", from.into()), meta_row("Date", date.into())]
        .spacing(SPACE_LG)
        .width(Length::Fill);
    for (label, list) in [("To", &body.to), ("Cc", &body.cc)] {
        if !list.is_empty() {
            meta = meta.push(meta_row(
                label,
                kit_text::caption(list.join(", "))
                    .style(kit_text::muted)
                    .into(),
            ));
        }
    }
    column![
        text(subject)
            .font(fonts::ui_medium())
            .size(22)
            .width(Length::Fill),
        meta,
    ]
    .spacing(SPACE_XL)
    .width(Length::Fill)
    .into()
}

fn meta_row(label: &'static str, value: Element<'static, Msg>) -> Element<'static, Msg> {
    row![
        kit_text::caption(label)
            .style(kit_text::muted)
            .width(Length::Fixed(44.0)),
        value,
    ]
    .spacing(SPACE_LG)
    .align_y(iced::Alignment::Start)
    .width(Length::Fill)
    .into()
}

fn received_attachments(atts: &[MailAttachment]) -> Element<'static, Msg> {
    let rows = atts.iter().enumerate().map(|(i, att)| {
        row![
            icon_svg(icon("lucide/file"), 14),
            kit_text::body(att.filename.clone()).width(Length::Fill),
            kit_text::caption(attachments::human_size(att.size)).style(kit_text::muted),
            kit_btn::labeled_sm("Open", kit_btn::secondary).on_press(Msg::OpenAttachment(i)),
            kit_btn::labeled_sm("Save", kit_btn::ghost).on_press(Msg::SaveAttachment(i)),
        ]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
    });
    attachment_well(column(rows).spacing(SPACE_SM).width(Length::Fill))
}

pub fn attachment_well<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    container(content)
        .padding(Padding::from([SPACE_MD, SPACE_LG]))
        .width(Length::Fill)
        .style(style::well)
        .into()
}
