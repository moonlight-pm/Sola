//! Reading pane: the open letter, or why there is none. Drawn as Next's
//! `.mail__read`: the subject, one line of who and when, the actions, then
//! the letter, on the window's face.

use iced::font::Weight;
use iced::widget::{Space, column, container, rich_text, row, span, text};
use iced::{Alignment, Element, Font, Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon_svg;
use sola_kit::components::prose::{ImagePixels, ProseImages, prose_letter};
use sola_kit::components::readable;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use super::format::split_address;
use super::widgets::{icon, tool};
use super::{App, Msg, style};
use crate::protocol::date::{letter_date, letter_time};
use crate::protocol::{LetterImage, MailAttachment, MessageBody, MessageSummary, attachments};
use sola_kit::components::scroll;

/// Comfortable reading measure (~65ch at 14px prose).
const READ_MAX_W: f32 = 640.0;
/// Next's `.mail__read` padding: 20 over the subject, 26 at the sides.
const PAD_TOP: f32 = 20.0;
const PAD_SIDE: f32 = 26.0;

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
    /// The pictures the letter shows, by key (its own now, the web's
    /// once asked for).
    pub images: ProseImages,
    pub web: Web,
}

/// The letter's pictures from the web.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Web {
    /// It has none.
    None,
    /// Not fetched: nothing is asked of the web until the reader says.
    Hidden,
    Loading,
}

impl Letter {
    /// The pictures move out of the body into the view's map.
    pub fn new(mut body: MessageBody) -> Self {
        let images = std::mem::take(&mut body.images)
            .into_iter()
            .map(|i| (i.key.clone(), pixels(i)))
            .collect();
        let web = if body.remote_images.is_empty() {
            Web::None
        } else {
            Web::Hidden
        };
        Self {
            body,
            selection: None,
            select_all: 0,
            images: std::sync::Arc::new(images),
            web,
        }
    }

    /// A web picture arrived.
    pub fn add_image(&mut self, image: LetterImage) {
        std::sync::Arc::make_mut(&mut self.images).insert(image.key.clone(), pixels(image));
    }
}

fn pixels(image: LetterImage) -> ImagePixels {
    ImagePixels {
        handle: iced::advanced::image::Handle::from_rgba(image.width, image.height, image.rgba),
        width: image.width,
        height: image.height,
        plate: image.plate,
    }
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
            .align_x(Alignment::Center)
            .into(),
            (Some(_), Reader::Failed(message)) => column![
                kit_text::body("Couldn’t open this message").style(kit_text::muted),
                kit_text::caption(super::format::truncate(message, 90)).style(kit_text::muted),
                kit_btn::labeled_sm("Try Again", kit_btn::secondary).on_press(Msg::RetryBody),
            ]
            .spacing(SPACE_SM)
            .align_x(Alignment::Center)
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
            .style(style::canvas)
            .into()
    }

    fn view_letter<'a>(&'a self, letter: &'a Letter) -> Element<'a, Msg> {
        let body = &letter.body;
        let row_msg = self.list.rows.iter().find(|m| m.id == body.id);
        let mut col = column![
            self.letter_header(body),
            container(self.letter_actions(body, row_msg)).padding(Padding {
                top: 14.0,
                right: 0.0,
                bottom: 18.0,
                left: 0.0,
            }),
        ]
        .width(Length::Fill);
        if letter.web != Web::None {
            col = col.push(container(web_strip(letter.web)).padding(Padding::ZERO.bottom(18.0)));
        }
        if !body.attachments.is_empty() {
            col = col.push(
                container(received_attachments(&body.attachments))
                    .padding(Padding::ZERO.bottom(18.0)),
            );
        }
        col = col.push(prose_letter(
            body.blocks.clone(),
            &self.theme,
            letter.select_all,
            letter.images.clone(),
            Msg::OpenUrl,
            Msg::BodySelect,
        ));
        let page = readable(
            col.padding(Padding {
                top: PAD_TOP,
                right: PAD_SIDE,
                bottom: 40.0,
                left: PAD_SIDE,
            }),
            READ_MAX_W + PAD_SIDE * 2.0,
        )
        .width(Length::Fill);
        container(scroll::scrollable(page).height(Length::Fill).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::canvas)
            .into()
    }

    /// The subject; who sent it, their address and when on one line; who
    /// else it went to under that.
    fn letter_header(&self, body: &MessageBody) -> Element<'static, Msg> {
        let ink = self.theme.extended_palette().background.base.text;
        let dim = self.theme.extended_palette().secondary.base.text;
        let subject = if body.subject.is_empty() {
            "(no subject)".to_string()
        } else {
            body.subject.clone()
        };
        let subject = text(subject)
            .font(Font {
                weight: Weight::Semibold,
                ..fonts::display()
            })
            .size(18)
            .width(Length::Fill);

        let (name, addr) = split_address(&body.from);
        let mut rest = String::new();
        if let Some(addr) = addr {
            rest.push_str(&format!("  ·  {addr}"));
        }
        let when = when(body.date);
        if !when.is_empty() {
            rest.push_str(&format!("  ·  {when}"));
        }
        let from = rich_text([
            span(name).font(fonts::ui_medium()).size(13).color(ink),
            span(rest).font(fonts::ui()).size(12).color(dim),
        ])
        .on_link_click(iced::never)
        .width(Length::Fill);

        let mut head = column![subject, from].spacing(6).width(Length::Fill);
        let to = people_line(&body.to, &body.cc);
        if !to.is_empty() {
            head = head.push(text(to).size(12).color(dim).width(Length::Fill));
        }
        head.into()
    }

    /// Next's `.acts`: Reply first and filled, Reply All and Forward; the
    /// moves out of this mailbox; read and flag at the right. In Drafts,
    /// Edit Draft takes the replies' place.
    fn letter_actions(
        &self,
        body: &MessageBody,
        row_msg: Option<&MessageSummary>,
    ) -> Element<'static, Msg> {
        let folder = body.id.folder.as_str();
        let mut bar = row![].spacing(SPACE_SM).align_y(Alignment::Center);
        if folder == "Drafts" {
            bar = bar.push(
                kit_btn::labeled_sm("Edit Draft", kit_btn::secondary).on_press(Msg::EditDraft),
            );
        } else {
            bar = bar
                .push(
                    kit_btn::labeled_sm("Reply", kit_btn::secondary)
                        .on_press(Msg::Reply { all: false }),
                )
                .push(
                    kit_btn::labeled_sm("Reply All", kit_btn::ghost)
                        .on_press(Msg::Reply { all: true }),
                )
                .push(kit_btn::labeled_sm("Forward", kit_btn::ghost).on_press(Msg::Forward));
        }

        let mut moves = row![].spacing(SPACE_XS).align_y(Alignment::Center);
        // Back to Inbox is the useful move out of Junk / Trash / Archive.
        if matches!(folder, "Junk" | "Trash" | "Archive") {
            moves = moves.push(tool(
                "lucide/inbox",
                "Move to Inbox",
                Some(Msg::MoveSelected("INBOX".into())),
            ));
        }
        for (name, icon, tip) in [
            ("Archive", "lucide/archive", "Archive"),
            ("Junk", "lucide/ban", "Move to Junk"),
            ("Trash", "lucide/trash", "Move to Trash"),
        ] {
            if folder != name {
                moves = moves.push(tool(icon, tip, Some(Msg::MoveSelected(name.into()))));
            }
        }
        bar = bar
            .push(Space::new().width(SPACE_MD))
            .push(moves)
            .push(Space::new().width(Length::Fill));

        if let Some(m) = row_msg {
            bar = bar
                .push(if m.seen {
                    tool("lucide/mail", "Mark as unread", Some(Msg::ToggleSeen))
                } else {
                    tool("lucide/mail-open", "Mark as read", Some(Msg::ToggleSeen))
                })
                .push(tool(
                    "lucide/flag",
                    if m.flagged { "Unflag" } else { "Flag" },
                    Some(Msg::ToggleFlag),
                ));
        }
        bar.width(Length::Fill).into()
    }
}

/// Pictures from the web are not fetched until asked (a fetch tells the
/// sender the letter was opened, and from where).
fn web_strip(web: Web) -> Element<'static, Msg> {
    let line = match web {
        Web::Loading => row![
            kit_text::caption("Loading pictures from the web…").style(kit_text::muted),
            Space::new().width(Length::Fill),
        ],
        _ => row![
            kit_text::caption("Pictures from the web are hidden").style(kit_text::muted),
            Space::new().width(Length::Fill),
            kit_btn::labeled_sm("Load Pictures", kit_btn::secondary).on_press(Msg::LoadImages),
        ],
    };
    attachment_well(line.spacing(SPACE_MD).align_y(Alignment::Center))
}

/// `26 Oct 2026, 13:40`.
fn when(ts: i64) -> String {
    let date = letter_date(ts);
    let time = letter_time(ts);
    match (date.is_empty(), time.is_empty()) {
        (true, _) => String::new(),
        (false, true) => date,
        (false, false) => format!("{date}, {time}"),
    }
}

/// `To Ana, bob@x.com  ·  Cc Carol`: names where there are names.
fn people_line(to: &[String], cc: &[String]) -> String {
    let names = |list: &[String]| {
        list.iter()
            .map(|p| split_address(p).0)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut parts = Vec::new();
    if !to.is_empty() {
        parts.push(format!("To {}", names(to)));
    }
    if !cc.is_empty() {
        parts.push(format!("Cc {}", names(cc)));
    }
    parts.join("  ·  ")
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
        .align_y(Alignment::Center)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_name_who_has_a_name() {
        assert_eq!(people_line(&[], &[]), "");
        assert_eq!(
            people_line(&["Ana Lopez <ana@x.com>".into(), "bob@x.com".into()], &[]),
            "To Ana Lopez, bob@x.com"
        );
        assert_eq!(
            people_line(&["ana@x.com".into()], &["Carol <c@x.com>".into()]),
            "To ana@x.com  ·  Cc Carol"
        );
    }

    #[test]
    fn when_needs_a_date() {
        assert_eq!(when(0), "");
    }
}
