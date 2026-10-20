//! Compose pane: the draft, how replies / forwards are built, the form.

use iced::widget::text_editor;
use iced::widget::{column, container, row, text_editor as editor};
use iced::{Element, Length, Padding};
use sola_bus::topics::mail_addr_key;
use sola_kit::components::button as kit_btn;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::format::short_name;
use super::widgets::{h_hairline, icon};
use super::{Msg, style};
use crate::protocol::date::{attribution, letter_date, letter_time};
use crate::protocol::sender::Outgoing;
use crate::protocol::{MailAttachment, MailId, MessageBody, attachments};

/// Compose header label column — wide enough for “Subject” at caption size.
const LABEL_W: f32 = 56.0;
/// Header fields (From/To/Cc/Subject) — quieter than kit body.
const HEADER_SIZE: f32 = 11.0;
/// Compose body — kit body size, not iced’s 16px default.
const BODY_SIZE: f32 = 13.0;

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
    /// "Discard this draft?" is showing.
    pub confirm_discard: bool,
    /// Letter to open once the draft is discarded / saved.
    pub then_open: Option<MailId>,
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
            edited: false,
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
                self.edited = true;
            }
            Edit::Cc(s) => {
                d.cc = s;
                self.edited = true;
            }
            Edit::Subject(s) => {
                d.subject = s;
                self.edited = true;
            }
            Edit::Body(action) => {
                self.edited |= action.is_edit();
                d.body.perform(action);
            }
        }
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

    pub fn view<'a>(&'a self, identities: &[String]) -> Element<'a, Msg> {
        let d = &self.draft;
        let body = editor(&d.body)
            .placeholder("Write a message…")
            .height(Length::Fill)
            .padding(12)
            .font(fonts::ui())
            .size(BODY_SIZE)
            .style(style::compose_editor)
            .on_action(|a| Msg::Draft(Edit::Body(a)));

        let headers = column![
            from_row(&d.from, identities, self.from_open),
            h_hairline(),
            header_row("To", "to@", &d.to, |s| Msg::Draft(Edit::To(s))),
            h_hairline(),
            header_row("Cc", "cc@", &d.cc, |s| Msg::Draft(Edit::Cc(s))),
            h_hairline(),
            header_row("Subject", "Subject", &d.subject, |s| {
                Msg::Draft(Edit::Subject(s))
            }),
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
            .push(if self.confirm_discard {
                self.discard_bar()
            } else {
                self.actions()
            })
            .padding(Padding::from([SPACE_LG, SPACE_LG]))
            .width(Length::Fill)
            .height(Length::Fill);

        container(form)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::read_pane)
            .into()
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

    fn discard_bar(&self) -> Element<'_, Msg> {
        row![
            kit_text::body("Discard this draft?").width(Length::Fill),
            kit_btn::labeled_sm("Keep Editing", kit_btn::ghost).on_press(Msg::KeepEditing),
            kit_btn::labeled_sm("Save Draft", kit_btn::secondary).on_press(Msg::SaveDraft),
            kit_btn::labeled_sm("Discard", kit_btn::danger).on_press(Msg::DiscardDraft),
        ]
        .spacing(SPACE_SM)
        .align_y(iced::Alignment::Center)
        .into()
    }
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
        header_input("from@", value, |s| Msg::Draft(Edit::From(s)))
    };
    labeled("From", control)
}

fn header_row<'a>(
    label: &'static str,
    placeholder: &str,
    value: &str,
    on_input: fn(String) -> Msg,
) -> Element<'a, Msg> {
    labeled(label, header_input(placeholder, value, on_input))
}

fn header_input<'a>(
    placeholder: &str,
    value: &str,
    on_input: fn(String) -> Msg,
) -> Element<'a, Msg> {
    text_input(placeholder, value)
        .font(fonts::ui())
        .size(HEADER_SIZE)
        .padding(Padding {
            top: 4.0,
            right: SPACE_SM,
            bottom: 4.0,
            left: SPACE_SM,
        })
        .width(Length::Fill)
        .style(style::compose_header_input)
        .on_input(on_input)
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
