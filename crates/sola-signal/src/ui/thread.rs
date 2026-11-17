//! The open conversation: its messages, and the field to write the next.

use iced::widget::{Space, button, column, container, image, rich_text, row, span, text, text_editor};
use iced::{Alignment, ContentFit, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon_handle;
use sola_kit::components::scroll;
use sola_kit::components::style::{BAR_H, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, mix, on_color};
use sola_kit::components::text as kit_text;
use sola_kit::components::toolbar::toolbar_icon_tip;
use sola_kit::fonts;

use super::list::{avatar, h_hairline};
use super::{App, Msg, editor_id, style, thread_id, when};
use crate::engine::duration;
use crate::store::chats::{Attachment, Bubble, Fetch, Kind, Status};

/// A bubble is no wider than this.
const BUBBLE_W: f32 = 520.0;
/// A picture in a bubble fits in a square of this side.
const PICTURE: f32 = 320.0;
const QUICK: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

impl App {
    pub(super) fn view_thread(&self) -> Element<'_, Msg> {
        let Some(chat) = self.open_chat() else {
            return container(kit_text::body("Pick a conversation").style(kit_text::muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        };

        let mut heading = row![
            avatar(chat.id, &chat.title, 28.0),
            text(chat.title.as_str()).size(15).font(fonts::ui_medium()),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center);
        if chat.expire_timer > 0 {
            heading = heading.push(
                kit_text::caption(format!("Messages disappear after {}", duration(chat.expire_timer))).style(kit_text::muted),
            );
        }
        heading = heading.push(Space::new().width(Length::Fill));
        if let Some(note) = self.wire_note() {
            heading = heading.push(note);
        }
        let bar = container(heading).height(BAR_H).padding(Padding::from([0.0, SPACE_LG])).center_y(BAR_H);

        let mut messages = column![].spacing(SPACE_SM).padding(Padding::from([SPACE_MD, SPACE_LG]));
        if self.thread.len() as u32 >= self.limit {
            messages = messages.push(
                container(kit_btn::labeled_sm("Earlier Messages", kit_btn::ghost).on_press(Msg::Earlier))
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            );
        }
        let mut before: Option<&Bubble> = None;
        for bubble in &self.thread {
            if before.is_none_or(|before| !when::same_day(before.received_at, bubble.received_at)) {
                messages = messages.push(
                    container(kit_text::caption(when::day(bubble.received_at)).style(kit_text::muted))
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(Padding::from([SPACE_SM, 0.0])),
                );
            }
            // In a group, a name over the first of someone's run of messages.
            let named = chat.group
                && !bubble.outgoing
                && bubble.kind == Kind::Text
                && before.is_none_or(|before| before.sender != bubble.sender || before.kind != Kind::Text);
            messages = messages.push(self.view_bubble(bubble, named));
            before = Some(bubble);
        }
        if let Some((_, who, _)) = self.typing.as_ref().filter(|(conversation, ..)| *conversation == chat.id) {
            messages = messages.push(kit_text::caption(format!("{who} is typing…")).style(kit_text::muted));
        }

        column![
            bar,
            h_hairline(),
            scroll::scrollable(messages).id(thread_id()).anchor_bottom().height(Length::Fill).width(Length::Fill),
            h_hairline(),
            self.view_composer(),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_bubble<'a>(&'a self, bubble: &'a Bubble, named: bool) -> Element<'a, Msg> {
        if bubble.kind == Kind::Note {
            return container(kit_text::caption(bubble.body.as_str()).align_x(Alignment::Center))
                .width(Length::Fill)
                .center_x(Length::Fill)
                .padding(Padding::from([SPACE_XS, SPACE_LG]))
                .style(style::note)
                .into();
        }
        let out = bubble.outgoing;
        let picked = self.picked == Some(bubble.id);
        let mut inside = column![].spacing(SPACE_SM);
        if bubble.deleted {
            inside = inside.push(text("This message was deleted").size(14).style(faded(out)));
        } else {
            if let Some((author, quoted)) = &bubble.quote {
                let quoted = if quoted.is_empty() { "Attachment" } else { quoted.as_str() };
                inside = inside.push(
                    container(column![
                        text(author.as_str()).size(12).font(fonts::ui_medium()),
                        text(first_lines(quoted, 3)).size(13),
                    ])
                    .padding(Padding::from([SPACE_SM, SPACE_MD]))
                    .style(style::quote(out)),
                );
            }
            for attachment in &bubble.attachments {
                inside = inside.push(view_attachment(attachment, out));
            }
            if !bubble.body.is_empty() {
                inside = inside.push(body(&bubble.body, out));
            }
        }
        let mut meta = when::time(bubble.sent_at);
        if bubble.edited {
            meta.push_str(" · Edited");
        }
        if bubble.expires_in > 0 {
            meta.push_str(" · Disappears");
        }
        if out {
            meta.push_str(match bubble.status {
                Status::Sending => " · Sending…",
                Status::Sent => " · Sent",
                Status::Delivered => " · Delivered",
                Status::Read => " · Read",
                Status::Failed => " · Not sent",
            });
        }
        inside = inside.push(text(meta).size(11).style(faded(out)));

        let shell = button(container(inside).padding(Padding::from([SPACE_MD, SPACE_LG])).max_width(BUBBLE_W).style(style::bubble(out, picked)))
            .on_press(Msg::Pick(bubble.id))
            .padding(0)
            .style(style::bare);

        let mut stack = column![].spacing(SPACE_XS).align_x(if out { Alignment::End } else { Alignment::Start });
        if named {
            stack = stack.push(
                text(bubble.sender_name.as_str()).size(12).font(fonts::ui_medium()).color(style::name_color(bubble.sender)),
            );
        }
        stack = stack.push(shell);
        if !bubble.reactions.is_empty() {
            let mut reactions = row![].spacing(SPACE_XS);
            for (emoji, count, mine) in &bubble.reactions {
                let label = if *count > 1 { format!("{emoji} {count}") } else { emoji.clone() };
                reactions = reactions.push(
                    button(container(text(label).size(13)).padding(Padding::from([2.0, SPACE_MD])).style(style::reaction(*mine)))
                        .on_press(Msg::React(bubble.id, emoji.clone()))
                        .padding(0)
                        .style(style::bare),
                );
            }
            stack = stack.push(reactions);
        }
        if out && bubble.status == Status::Failed {
            stack = stack.push(kit_btn::labeled_sm("Try Again", kit_btn::secondary).on_press(Msg::Retry(bubble.id)));
        }
        if picked {
            stack = stack.push(self.view_actions(bubble));
        }
        let line = if out {
            row![Space::new().width(Length::Fill), stack]
        } else {
            row![stack, Space::new().width(Length::Fill)]
        };
        line.width(Length::Fill).into()
    }

    /// What can be done with the picked message.
    fn view_actions<'a>(&'a self, bubble: &'a Bubble) -> Element<'a, Msg> {
        let mut actions = row![].spacing(SPACE_XS).align_y(Alignment::Center);
        if !bubble.deleted {
            for emoji in QUICK {
                actions = actions.push(
                    button(text(emoji).size(16))
                        .on_press(Msg::React(bubble.id, emoji.to_owned()))
                        .padding(Padding::from([2.0, SPACE_SM]))
                        .style(kit_btn::ghost),
                );
            }
            actions = actions.push(kit_btn::labeled_sm("Reply", kit_btn::ghost).on_press(Msg::Reply(bubble.id)));
            if !bubble.body.is_empty() {
                actions = actions.push(kit_btn::labeled_sm("Copy", kit_btn::ghost).on_press(Msg::Copy(bubble.id)));
            }
            if bubble.outgoing && !bubble.body.is_empty() && bubble.attachments.is_empty() {
                actions = actions.push(kit_btn::labeled_sm("Edit", kit_btn::ghost).on_press(Msg::Edit(bubble.id)));
            }
            if bubble.outgoing {
                actions = actions.push(
                    kit_btn::labeled_sm("Delete for Everyone", kit_btn::ghost).on_press(Msg::Retract(bubble.id)),
                );
            }
        }
        actions = actions.push(kit_btn::labeled_sm("Delete Here", kit_btn::ghost).on_press(Msg::DeleteHere(bubble.id)));
        actions.wrap().into()
    }

    fn view_composer(&self) -> Element<'_, Msg> {
        let mut over = column![].spacing(SPACE_SM);
        let banner = |label: String| -> Element<'_, Msg> {
            container(
                row![
                    container(kit_text::caption(label).wrapping(text::Wrapping::None)).width(Length::Fill).clip(true),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::Cancel),
                ]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([SPACE_SM, SPACE_MD]))
            .style(style::card)
            .into()
        };
        if self.editing.is_some() {
            over = over.push(banner("Editing your message".into()));
        } else if let Some(quoted) = self.replying.and_then(|id| self.thread.iter().find(|bubble| bubble.id == id)) {
            let said = if quoted.body.is_empty() { "an attachment" } else { quoted.body.lines().next().unwrap_or_default() };
            let who = if quoted.outgoing { "yourself" } else { quoted.sender_name.as_str() };
            over = over.push(banner(format!("Replying to {who}: {said}")));
        }
        if !self.files.is_empty() {
            let mut files = row![].spacing(SPACE_SM);
            for (index, file) in self.files.iter().enumerate() {
                let name = file.file_name().and_then(|name| name.to_str()).unwrap_or("file");
                files = files.push(
                    container(
                        row![
                            kit_text::caption(name),
                            button(text("×").size(14)).on_press(Msg::Unattach(index)).padding(Padding::from([0.0, SPACE_SM])).style(kit_btn::ghost),
                        ]
                        .spacing(SPACE_XS)
                        .align_y(Alignment::Center),
                    )
                    .padding(Padding::from([SPACE_XS, SPACE_MD]))
                    .style(style::card),
                );
            }
            over = over.push(files.wrap());
        }

        let editor = text_editor(&self.draft)
            .id(editor_id())
            .placeholder("Message")
            .on_action(Msg::Draft)
            .key_binding(|press| {
                use iced::keyboard::{Key, key::Named};
                match press.key.as_ref() {
                    // Enter sends; with Shift it is a new line.
                    Key::Named(Named::Enter) if !press.modifiers.shift() => Some(text_editor::Binding::Custom(Msg::Send)),
                    _ => text_editor::Binding::from_key_press(press),
                }
            })
            .height(Length::Shrink)
            .max_height(180.0)
            .padding(Padding::from([SPACE_MD, SPACE_LG]))
            .font(fonts::ui())
            .size(14)
            .style(style::editor);
        let attach = toolbar_icon_tip(icon_handle("lucide/paperclip"), "Attach files", (self.editing.is_none()).then_some(Msg::Attach));
        let ready = !self.draft.text().trim().is_empty() || !self.files.is_empty();
        let send = toolbar_icon_tip(icon_handle("lucide/send"), "Send", ready.then_some(Msg::Send));
        over.push(
            row![attach, container(editor).width(Length::Fill).style(style::field), send]
                .spacing(SPACE_SM)
                .align_y(Alignment::End),
        )
        .padding(Padding::from([SPACE_MD, SPACE_LG]))
        .into()
    }
}

/// The lesser ink of a bubble: its time, a deleted message.
fn faded(outgoing: bool) -> impl Fn(&Theme) -> text::Style {
    move |theme| {
        let fill = style::bubble_color(theme, outgoing);
        text::Style { color: Some(mix(on_color(fill), fill, 0.6)) }
    }
}

fn first_lines(text: &str, lines: usize) -> String {
    let mut kept: Vec<&str> = text.lines().take(lines).collect();
    if text.lines().count() > lines {
        kept.push("…");
    }
    kept.join("\n")
}

/// A message's text, its web addresses made to open.
fn body(text: &str, outgoing: bool) -> Element<'_, Msg> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("https://").or_else(|| rest.find("http://")) {
        let end = rest[start..].find(char::is_whitespace).map_or(rest.len(), |end| start + end);
        // A full stop or a bracket after an address is the sentence's.
        let url = rest[start..end].trim_end_matches(['.', ',', ')', ']', '!', '?', ';', ':', '"', '\'']);
        let end = start + url.len();
        if start > 0 {
            spans.push(span(&rest[..start]));
        }
        spans.push(span(url).underline(true).link(url.to_owned()));
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        spans.push(span(rest));
    }
    rich_text(spans)
        .size(14)
        .on_link_click(Msg::OpenLink)
        .style(move |theme: &Theme| text::Style { color: Some(on_color(style::bubble_color(theme, outgoing))) })
        .into()
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn view_attachment(attachment: &Attachment, outgoing: bool) -> Element<'_, Msg> {
    let picture = attachment.content_type.starts_with("image/");
    if let (true, Fetch::Here, Some(path)) = (picture, attachment.state, &attachment.path) {
        // Fit the picture in a square, keeping its shape where we know it.
        let (w, h) = match (attachment.width as f32, attachment.height as f32) {
            (w, h) if w > 0.0 && h > 0.0 => {
                let scale = (PICTURE / w).min(PICTURE / h).min(1.0);
                (w * scale, h * scale)
            }
            _ => (PICTURE, PICTURE * 0.75),
        };
        return button(image(image::Handle::from_path(path)).width(w).height(h).content_fit(ContentFit::Contain))
            .on_press(Msg::OpenFile(path.clone()))
            .padding(0)
            .style(style::bare)
            .into();
    }
    let what = match attachment.content_type.split('/').next() {
        Some("image") => "Photo",
        Some("video") => "Video",
        Some("audio") => "Audio",
        _ => "File",
    };
    let name = attachment.file_name.as_deref().unwrap_or(what);
    let (state, press) = match (attachment.state, &attachment.path) {
        (Fetch::Here, Some(path)) => ("Open", Some(Msg::OpenFile(path.clone()))),
        (Fetch::Fetching, _) => ("Fetching…", None),
        (Fetch::Failed, _) => ("Not fetched. Try again", Some(Msg::Fetch(attachment.id))),
        _ => ("Download", Some(Msg::Fetch(attachment.id))),
    };
    let label = column![
        text(name).size(14).font(fonts::ui_medium()),
        text(format!("{what} · {} · {state}", size(attachment.size))).size(12),
    ]
    .spacing(SPACE_XS);
    let card = container(label).padding(Padding::from([SPACE_SM, SPACE_MD])).style(style::quote(outgoing));
    button(card).on_press_maybe(press).padding(0).style(style::bare).into()
}
