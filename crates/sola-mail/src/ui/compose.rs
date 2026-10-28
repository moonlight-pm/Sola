//! Compose pane: the draft, how replies / forwards are built, the form.

use iced::advanced::widget::operation::{Focusable, Outcome};
use iced::advanced::widget::{Id, Operation};
use iced::widget::text_editor;
use iced::widget::{
    Space, button, column, container, mouse_area, row, stack, text, text_editor as editor,
};
use iced::{Background, Color, Element, Length, Padding, Rectangle, Task};
use sola_bus::topics::mail_addr_key;
use sola_kit::components::button as kit_btn;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::contacts::Contacts;
use super::format::{short_name, split_address};
use super::widgets::{h_hairline, icon};
use super::{Msg, style};
use crate::protocol::date::{attribution, letter_date, letter_time};
use crate::protocol::recipients;
use crate::protocol::sender::Outgoing;
use crate::protocol::{MailAttachment, MailId, MessageBody, attachments};

/// Label column — wide enough for “Subject”.
const LABEL_W: f32 = 56.0;
/// Header fields and body share the kit body size.
const FIELD_SIZE: f32 = 13.0;
/// Suggestions shown under To / Cc.
const SUGGESTIONS: usize = 5;

pub struct Draft {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: text_editor::Content,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<MailAttachment>,
    /// The saved draft this one was reopened from.
    pub replaces: Option<MailId>,
}

/// Focusable compose fields, in Tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    To,
    Cc,
    Subject,
    Body,
}

impl Field {
    const ORDER: [Field; 4] = [Field::To, Field::Cc, Field::Subject, Field::Body];

    pub fn id(self) -> Id {
        Id::new(match self {
            Field::To => "compose-to",
            Field::Cc => "compose-cc",
            Field::Subject => "compose-subject",
            Field::Body => "compose-body",
        })
    }

    pub fn from_id(id: &Id) -> Option<Self> {
        Self::ORDER.into_iter().find(|f| f.id() == *id)
    }

    /// Tab / Shift+Tab neighbour (wraps).
    pub fn step(self, back: bool) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        let n = Self::ORDER.len();
        Self::ORDER[if back { (i + n - 1) % n } else { (i + 1) % n }]
    }

    pub fn focus<T>(self) -> Task<T> {
        iced::widget::operation::focus(self.id())
    }
}

/// Which compose field has keyboard focus (`None`: none of them —
/// iced's own `find_focused` reports nothing in that case).
pub fn focused_field() -> Task<Option<Field>> {
    struct Find(Option<Field>);
    impl Operation<Option<Field>> for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Field>>)) {
            operate(self);
        }
        fn focusable(&mut self, id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
            if state.is_focused()
                && let Some(field) = id.and_then(Field::from_id)
            {
                self.0 = Some(field);
            }
        }
        fn finish(&self) -> Outcome<Option<Field>> {
            Outcome::Some(self.0)
        }
    }
    iced::advanced::widget::operate(Find(None))
}

#[derive(Debug, Clone)]
pub enum Edit {
    From(String),
    ToggleFrom,
    DismissFrom,
    To(String),
    Cc(String),
    Subject(String),
    Body(text_editor::Action),
}

/// Send / Save Draft in flight (buttons disabled meanwhile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Sending,
    Saving,
}

pub struct Compose {
    pub draft: Draft,
    pub from_open: bool,
    pub busy: Option<Busy>,
    /// "Discard this draft?" card is showing.
    pub confirm_discard: bool,
    /// Letter to open once the draft is discarded / saved.
    pub then_open: Option<MailId>,
    /// To / Cc last typed in: where suggestions show.
    pub active: Option<Field>,
    /// A Send found bad addresses: mark them until fixed.
    pub show_errors: bool,
    edited: bool,
}

impl Compose {
    pub fn new(draft: Draft) -> Self {
        Self {
            draft,
            from_open: false,
            busy: None,
            confirm_discard: false,
            then_open: None,
            active: None,
            show_errors: false,
            edited: false,
        }
    }

    /// Where the cursor should start: To while it is empty, else the body.
    pub fn first_field(&self) -> Field {
        if self.draft.to.trim().is_empty() {
            Field::To
        } else {
            Field::Body
        }
    }

    /// Anything the user typed or attached (a fresh reply with only the
    /// quote is not worth asking about).
    pub fn dirty(&self) -> bool {
        self.edited
    }

    pub fn touch(&mut self) {
        self.edited = true;
    }

    pub fn apply(&mut self, edit: Edit) {
        let d = &mut self.draft;
        match edit {
            Edit::From(s) => {
                d.from = s;
                self.from_open = false;
            }
            Edit::ToggleFrom => self.from_open = !self.from_open,
            Edit::DismissFrom => self.from_open = false,
            Edit::To(s) => {
                d.to = s;
                self.active = Some(Field::To);
                self.edited = true;
            }
            Edit::Cc(s) => {
                d.cc = s;
                self.active = Some(Field::Cc);
                self.edited = true;
            }
            Edit::Subject(s) => {
                d.subject = s;
                self.active = None;
                self.edited = true;
            }
            Edit::Body(action) => {
                if action.is_edit() {
                    self.edited = true;
                    self.active = None;
                }
                d.body.perform(action);
            }
        }
    }

    pub fn field_text(&self, field: Field) -> &str {
        match field {
            Field::To => &self.draft.to,
            Field::Cc => &self.draft.cc,
            Field::Subject => &self.draft.subject,
            Field::Body => "",
        }
    }

    /// Put a suggestion in place of the address being typed.
    pub fn complete(&mut self, field: Field, chosen: &str) {
        let target = match field {
            Field::To => &mut self.draft.to,
            Field::Cc => &mut self.draft.cc,
            _ => return,
        };
        *target = recipients::complete(target, chosen);
        self.active = None;
        self.edited = true;
    }

    /// Why Send can't go yet (`None`: ready). Marks the fields.
    pub fn send_problem(&mut self) -> Option<String> {
        let d = &self.draft;
        let problem = if recipients::split(&d.to).is_empty() {
            Some("Add at least one recipient".to_string())
        } else {
            recipients::invalid(&d.to)
                .into_iter()
                .chain(recipients::invalid(&d.cc))
                .next()
                .map(|bad| format!("“{bad}” isn’t an email address"))
        };
        self.show_errors = problem.is_some();
        problem
    }

    pub fn outgoing(&self) -> Outgoing {
        let d = &self.draft;
        Outgoing {
            from: d.from.clone(),
            to: d.to.clone(),
            cc: d.cc.clone(),
            subject: d.subject.clone(),
            body: d.body.text(),
            in_reply_to: d.in_reply_to.clone(),
            references: d.references.clone(),
            attachments: d.attachments.clone(),
            replaces: d.replaces.clone(),
        }
    }

    pub fn view<'a>(&'a self, identities: &[String], contacts: &Contacts) -> Element<'a, Msg> {
        let d = &self.draft;
        let body = editor(&d.body)
            .id(Field::Body.id())
            .placeholder("Write a message…")
            .height(Length::Fill)
            .padding(12)
            .font(fonts::ui())
            .size(FIELD_SIZE)
            .style(style::compose_editor)
            .on_action(|a| Msg::Draft(Edit::Body(a)));

        let headers = column![
            from_row(&d.from, identities, self.from_open),
            h_hairline(),
            self.recipient_row("To", Field::To, contacts),
            h_hairline(),
            self.recipient_row("Cc", Field::Cc, contacts),
            h_hairline(),
            labeled(
                "Subject",
                field_input(Field::Subject, "Subject", &d.subject, |s| {
                    Msg::Draft(Edit::Subject(s))
                }),
            ),
        ];

        let mut form = column![headers].spacing(SPACE_SM);
        if !d.attachments.is_empty() {
            form = form
                .push(h_hairline())
                .push(attachment_list(&d.attachments));
        }
        let form = form
            .push(h_hairline())
            .push(
                container(body)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(style::compose_field),
            )
            .push(self.actions())
            .padding(Padding::from([20.0, 26.0]))
            .width(Length::Fill)
            .height(Length::Fill);

        let pane: Element<'a, Msg> = container(form)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::canvas)
            .into();
        if self.confirm_discard {
            stack![pane, self.discard_card()].into()
        } else {
            pane
        }
    }

    /// To / Cc: the field, a bad-address note after a failed Send, and
    /// suggestions while typing.
    fn recipient_row<'a>(
        &'a self,
        label: &'static str,
        field: Field,
        contacts: &Contacts,
    ) -> Element<'a, Msg> {
        let value = self.field_text(field);
        let on_input: fn(String) -> Msg = match field {
            Field::To => |s| Msg::Draft(Edit::To(s)),
            _ => |s| Msg::Draft(Edit::Cc(s)),
        };
        let placeholder = if field == Field::To {
            "Name or address — separate with commas"
        } else {
            "Optional"
        };
        let mut col = column![labeled(
            label,
            field_input(field, placeholder, value, on_input)
        )];

        if self.show_errors {
            let bad = recipients::invalid(value);
            let note = if field == Field::To && recipients::split(value).is_empty() {
                Some("Add at least one recipient".to_string())
            } else {
                bad.first().map(|b| match bad.len() {
                    1 => format!("“{b}” isn’t an email address"),
                    n => format!("“{b}” and {} more aren’t email addresses", n - 1),
                })
            };
            if let Some(note) = note {
                col = col.push(indented(
                    kit_text::caption(note).style(kit_text::danger).into(),
                ));
            }
        }

        if self.active == Some(field) {
            let hits = contacts.suggest(value, SUGGESTIONS);
            if !hits.is_empty() {
                let list = column(hits.into_iter().map(|hit| suggestion(field, hit))).spacing(0);
                col = col.push(indented(list.into()));
            }
        }
        col.spacing(SPACE_XS).into()
    }

    fn actions(&self) -> Element<'_, Msg> {
        let idle = self.busy.is_none();
        let label = match self.busy {
            Some(Busy::Sending) => "Sending…",
            Some(Busy::Saving) => "Saving…",
            None => "Send",
        };
        let send =
            kit_btn::labeled(label, kit_btn::primary).on_press_maybe(idle.then_some(Msg::Send));
        row![
            send,
            kit_btn::labeled_sm("Attach", kit_btn::ghost)
                .on_press_maybe(idle.then_some(Msg::Attach)),
            kit_btn::labeled_sm("Save Draft", kit_btn::ghost)
                .on_press_maybe(idle.then_some(Msg::SaveDraft)),
            kit_btn::labeled_sm("Cancel", kit_btn::ghost)
                .on_press_maybe(idle.then_some(Msg::CancelCompose)),
        ]
        .spacing(SPACE_SM)
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// Centred over the compose pane so it is seen whatever was clicked
    /// (a list row, Cancel, Esc). The scrim keeps editing on click.
    fn discard_card(&self) -> Element<'_, Msg> {
        let why = if self.draft.replaces.is_some() {
            "Your changes since you opened it will be lost."
        } else {
            "It isn’t saved anywhere yet."
        };
        let card = container(
            column![
                text("Discard this draft?")
                    .font(fonts::ui_medium())
                    .size(15),
                kit_text::caption(why).style(kit_text::muted),
                Space::new().height(SPACE_SM),
                row![
                    Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Keep Editing", kit_btn::ghost).on_press(Msg::KeepEditing),
                    kit_btn::labeled_sm("Save Draft", kit_btn::secondary).on_press(Msg::SaveDraft),
                    kit_btn::labeled_sm("Discard", kit_btn::danger).on_press(Msg::DiscardDraft),
                ]
                .spacing(SPACE_SM)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(SPACE_SM),
        )
        .padding(Padding::from([SPACE_LG, SPACE_LG]))
        .width(Length::Fixed(380.0))
        .style(style::card);
        let scrim = container(mouse_area(card).on_press(Msg::Noop))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
                ..container::Style::default()
            });
        mouse_area(scrim).on_press(Msg::KeepEditing).into()
    }
}

fn suggestion<'a>(field: Field, hit: String) -> Element<'a, Msg> {
    let (name, addr) = split_address(&hit);
    let mut line = row![text(name).font(fonts::ui()).size(FIELD_SIZE)]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center);
    if let Some(addr) = addr {
        line = line.push(kit_text::caption(addr).style(kit_text::muted));
    }
    button(line)
        .on_press(Msg::ComposeSuggest(field, hit))
        .padding(Padding::from([4.0, SPACE_SM]))
        .width(Length::Fill)
        .style(kit_btn::list_item(false))
        .into()
}

/// Line up with the field column (past the label).
fn indented(content: Element<'_, Msg>) -> Element<'_, Msg> {
    row![
        Space::new().width(Length::Fixed(LABEL_W + SPACE_MD)),
        content
    ]
    .width(Length::Fill)
    .into()
}

pub fn blank(from: String) -> Draft {
    Draft {
        from,
        to: String::new(),
        cc: String::new(),
        subject: String::new(),
        body: text_editor::Content::new(),
        in_reply_to: None,
        references: Vec::new(),
        attachments: Vec::new(),
        replaces: None,
    }
}

/// Reply (or reply-all) to `body`, quoting it.
pub fn reply(body: &MessageBody, from: String, all: bool, is_self: &dyn Fn(&str) -> bool) -> Draft {
    let (to, cc) = reply_recipients(body, all, is_self);
    let mut draft = blank(from);
    draft.to = to.join(", ");
    draft.cc = cc.join(", ");
    draft.subject = prefixed(&body.subject, "Re:", &["re:"]);
    draft.body = text_editor::Content::with_text(&quote(body));
    draft.in_reply_to = body.message_id.clone();
    draft.references = body.references.clone();
    draft
}

/// Forward `body` with its files.
pub fn forward(body: &MessageBody, from: String) -> Draft {
    let mut header = vec![
        "---------- Forwarded message ----------".to_string(),
        format!("From: {}", body.from),
    ];
    if body.date != 0 {
        header.push(format!(
            "Date: {} at {}",
            letter_date(body.date),
            letter_time(body.date)
        ));
    }
    header.push(format!("Subject: {}", body.subject));
    if !body.to.is_empty() {
        header.push(format!("To: {}", body.to.join(", ")));
    }
    if !body.cc.is_empty() {
        header.push(format!("Cc: {}", body.cc.join(", ")));
    }
    let mut draft = blank(from);
    draft.subject = prefixed(&body.subject, "Fwd:", &["fwd:", "fw:"]);
    draft.body = text_editor::Content::with_text(&format!(
        "\n\n{}\n\n{}",
        header.join("\n"),
        body.display_text()
    ));
    draft.attachments = body.attachments.clone();
    draft
}

/// Reopen a saved draft for editing; sending or re-saving replaces it.
pub fn reopen(body: &MessageBody, from: String) -> Draft {
    Draft {
        from,
        to: body.to.join(", "),
        cc: body.cc.join(", "),
        subject: body.subject.clone(),
        body: text_editor::Content::with_text(body.text.trim_end()),
        in_reply_to: body.in_reply_to.clone(),
        references: body.references.clone(),
        attachments: body.attachments.clone(),
        replaces: Some(body.id.clone()),
    }
}

/// Reply goes to Reply-To (else From). Reply-all adds the original To,
/// and keeps the original Cc as Cc — minus our own addresses and dupes.
pub fn reply_recipients(
    body: &MessageBody,
    all: bool,
    is_self: &dyn Fn(&str) -> bool,
) -> (Vec<String>, Vec<String>) {
    let primary: Vec<String> = if body.reply_to.is_empty() {
        vec![body.from.clone()]
    } else {
        body.reply_to.clone()
    };
    let mut seen: Vec<String> = Vec::new();
    let mut take = |list: &[String], out: &mut Vec<String>, skip_self: bool| {
        for addr in list {
            let key = mail_addr_key(addr);
            if key.is_empty() || seen.contains(&key) || (skip_self && is_self(addr)) {
                continue;
            }
            seen.push(key);
            out.push(addr.clone());
        }
    };
    let (mut to, mut cc) = (Vec::new(), Vec::new());
    // Replying to your own sent mail still goes to that address.
    take(&primary, &mut to, false);
    if all {
        take(&body.to, &mut to, true);
        take(&body.cc, &mut cc, true);
    }
    (to, cc)
}

/// `Re: subject` unless it already starts with one of `existing`.
fn prefixed(subject: &str, prefix: &str, existing: &[&str]) -> String {
    let lower = subject.trim_start().to_lowercase();
    if existing.iter().any(|p| lower.starts_with(p)) {
        subject.to_string()
    } else {
        format!("{prefix} {subject}")
    }
}

fn quote(body: &MessageBody) -> String {
    let who = short_name(&body.from);
    let intro = if body.date == 0 {
        format!("{who} wrote:")
    } else {
        format!("On {}, {who} wrote:", attribution(body.date))
    };
    let quoted = body
        .display_text()
        .lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\n{intro}\n{quoted}")
}

fn from_row<'a>(value: &str, identities: &[String], open: bool) -> Element<'a, Msg> {
    let control: Element<'a, Msg> = if identities.len() > 1 {
        let opts = identities.iter().map(|addr| {
            SelectOption::new(
                addr.clone(),
                mail_addr_key(addr) == mail_addr_key(value),
                Msg::Draft(Edit::From(addr.clone())),
            )
            .mark(addr.clone())
        });
        let label = if value.trim().is_empty() {
            identities[0].clone()
        } else {
            value.to_string()
        };
        select(
            label,
            opts,
            open,
            Msg::Draft(Edit::ToggleFrom),
            Msg::Draft(Edit::DismissFrom),
        )
    } else {
        text_input("you@example.com", value)
            .font(fonts::ui())
            .size(FIELD_SIZE)
            .padding(Padding::from([6.0, SPACE_SM]))
            .width(Length::Fill)
            .style(style::compose_header_input)
            .on_input(|s| Msg::Draft(Edit::From(s)))
            .into()
    };
    labeled("From", control)
}

fn field_input<'a>(
    field: Field,
    placeholder: &str,
    value: &str,
    on_input: fn(String) -> Msg,
) -> Element<'a, Msg> {
    text_input(placeholder, value)
        .id(field.id())
        .font(fonts::ui())
        .size(FIELD_SIZE)
        .padding(Padding::from([6.0, SPACE_SM]))
        .width(Length::Fill)
        .style(style::compose_header_input)
        .on_input(on_input)
        .on_submit(Msg::ComposeSubmit(field))
        .into()
}

fn labeled<'a>(label: &'static str, control: Element<'a, Msg>) -> Element<'a, Msg> {
    row![
        kit_text::caption(label)
            .style(kit_text::muted)
            .width(Length::Fixed(LABEL_W)),
        control,
    ]
    .spacing(SPACE_MD)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn attachment_list(atts: &[MailAttachment]) -> Element<'static, Msg> {
    let rows = atts.iter().enumerate().map(|(i, att)| {
        row![
            sola_kit::components::icon::icon_svg(icon("lucide/file"), 14),
            kit_text::body(att.filename.clone()).width(Length::Fill),
            kit_text::caption(attachments::human_size(att.size)).style(kit_text::muted),
            kit_btn::labeled_sm("Remove", kit_btn::ghost).on_press(Msg::RemoveAttach(i)),
        ]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
    });
    super::reader::attachment_well(column(rows).spacing(SPACE_SM).width(Length::Fill))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter() -> MessageBody {
        MessageBody {
            id: MailId::new("me@x.com", "INBOX", 1),
            from: "Ana <ana@a.com>".into(),
            reply_to: Vec::new(),
            to: vec!["Me <me@x.com>".into(), "Bo <bo@b.com>".into()],
            cc: vec!["cy@c.com".into(), "ana@a.com".into()],
            subject: "Plans".into(),
            date: 0,
            text: String::new(),
            message_id: Some("m1@a.com".into()),
            in_reply_to: None,
            references: vec!["<root@a.com>".into()],
            attachments: Vec::new(),
            blocks: Vec::new(),
            images: Vec::new(),
            remote_images: Vec::new(),
        }
    }

    fn me(addr: &str) -> bool {
        mail_addr_key(addr) == "me@x.com"
    }

    #[test]
    fn reply_goes_to_sender_only() {
        let (to, cc) = reply_recipients(&letter(), false, &me);
        assert_eq!(to, vec!["Ana <ana@a.com>"]);
        assert!(cc.is_empty());
    }

    #[test]
    fn reply_all_keeps_to_and_cc_minus_self_and_dupes() {
        let (to, cc) = reply_recipients(&letter(), true, &me);
        assert_eq!(to, vec!["Ana <ana@a.com>", "Bo <bo@b.com>"]);
        assert_eq!(cc, vec!["cy@c.com"]);
    }

    #[test]
    fn reply_to_header_wins() {
        let mut body = letter();
        body.reply_to = vec!["list@l.org".into()];
        let (to, _) = reply_recipients(&body, false, &me);
        assert_eq!(to, vec!["list@l.org"]);
    }

    #[test]
    fn subject_prefixes_once() {
        assert_eq!(prefixed("Plans", "Re:", &["re:"]), "Re: Plans");
        assert_eq!(prefixed("RE: Plans", "Re:", &["re:"]), "RE: Plans");
        assert_eq!(prefixed("Fw: x", "Fwd:", &["fwd:", "fw:"]), "Fw: x");
    }

    #[test]
    fn reply_threads_on_message_id() {
        let d = reply(&letter(), "me@x.com".into(), false, &me);
        assert_eq!(d.in_reply_to.as_deref(), Some("m1@a.com"));
        assert_eq!(d.references, vec!["<root@a.com>"]);
        assert!(d.body.text().contains("Ana wrote:"));
    }
}
