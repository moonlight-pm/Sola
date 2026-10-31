//! The record, as the canvas draws it (`design/vault/item.html`): Settings'
//! grouped rows, label over value on the left and the row's tools on the
//! right — copy on every field, reveal on a secret, open on a website.
//! The one-time code is the largest thing on it, with its time left as a
//! ring. In the trash it says so and offers Restore and Delete forever.

use iced::widget::{Column, Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::context_menu::{self, MenuItem};
use sola_kit::components::popover::{Placement, popover_anchored};
use sola_kit::components::scroll;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_vault_proto::{ItemRecord, RecordField, UriMatch};

use super::copy::CopyWhat;
use super::fields::Field;
use super::signin::labeled_input;
use super::widgets::{self, PlateSize};
use super::{App, Msg, Pane, Popup, Reprompt, style};
use crate::model::code::{CodeNow, grouped};
use crate::model::plate::plate;
use crate::model::when;

/// Room for the record's groups.
const RECORD_MAX_W: f32 = 640.0;

impl App {
    pub(super) fn view_record(&self) -> Element<'_, Msg> {
        if let Some(form) = &self.edit {
            return self.view_edit(form);
        }
        if let Some(r) = &self.reprompt {
            return self.view_reprompt(r);
        }
        match &self.record {
            Pane::Open(r) => self.view_open(r),
            Pane::Loading => self.loading_head(),
            Pane::Failed(message) => quiet(format!("Couldn't open it: {message}")),
            Pane::Empty if self.loaded && self.shown_items().is_empty() => Space::new()
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            Pane::Empty => quiet("Choose an item".into()),
        }
    }

    /// While the record is on its way: its head from the list, nothing
    /// under it yet.
    fn loading_head(&self) -> Element<'_, Msg> {
        let Some(s) = self.selected_summary() else {
            return Space::new().into();
        };
        column![
            pane_head(
                Some(widgets::plate(&plate(s.kind, &s.name), PlateSize::Head)),
                title(&s.name),
                s.subtitle.clone(),
                Vec::new(),
            ),
            Space::new().height(Length::Fill),
        ]
        .into()
    }

    fn view_open<'a>(&'a self, r: &'a ItemRecord) -> Element<'a, Msg> {
        let sub = self
            .selected_summary()
            .map(|s| s.subtitle.clone())
            .unwrap_or_default();
        let mut tools: Vec<Element<'a, Msg>> = Vec::new();
        if r.deleted.is_none() {
            tools.push(widgets::tool_toggle(
                "lucide/star",
                if r.favorite { "Unfavorite" } else { "Favorite" },
                r.favorite,
                Msg::Favorite,
            ));
            if r.can_edit {
                tools.push(widgets::tool("lucide/pencil", "Edit", Some(Msg::Edit)));
            }
        }
        tools.push(self.more_menu(r));
        let head = pane_head(
            Some(widgets::plate(&plate(r.kind, &r.name), PlateSize::Head)),
            title(&r.name),
            sub,
            tools,
        );

        let mut body = Column::new().spacing(SPACE_LG).padding(Padding {
            top: SPACE_SM,
            right: 20.0,
            bottom: 24.0,
            left: 20.0,
        });
        if let Some(since) = &r.deleted {
            body = body.push(trash_banner(since));
        }
        let groups = [
            self.main_group(r),
            self.web_group(r),
            self.custom_group(r),
            self.notes_group(r),
            self.history_group(r),
            attachments_group(r),
        ];
        for g in groups.into_iter().flatten() {
            body = body.push(g);
        }
        let meta = when::edited_created(r.revised.as_deref(), r.created.as_deref(), when::today());
        if !meta.is_empty() {
            body = body.push(
                container(text(meta).font(fonts::ui()).size(11).style(tertiary))
                    .padding(Padding::ZERO.left(6)),
            );
        }
        column![
            head,
            scroll::scrollable(container(body.max_width(RECORD_MAX_W)).width(Length::Fill))
                .height(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }

    /// ···: Clone, Move to Folder, Trash — or Restore and Delete forever.
    fn more_menu<'a>(&'a self, r: &'a ItemRecord) -> Element<'a, Msg> {
        let open = self.popup == Some(Popup::RecordMore);
        let trigger = widgets::tool_toggle(
            "lucide/ellipsis",
            "More",
            open,
            Msg::Popup(if open { None } else { Some(Popup::RecordMore) }),
        );
        if !open {
            return trigger;
        }
        let items = if r.deleted.is_some() {
            vec![
                MenuItem::action("Restore", Msg::Restore),
                MenuItem::separator(),
                MenuItem::action("Delete Forever…", Msg::AskDeleteForever),
            ]
        } else {
            let mut items = vec![
                MenuItem::action("Clone", Msg::Clone),
                MenuItem::action("Move to Folder…", Msg::AskMove),
            ];
            if self.can_share() {
                items.push(MenuItem::action("Move to Organization…", Msg::AskShare));
            }
            items.push(MenuItem::separator());
            items.push(MenuItem::action("Move to Trash", Msg::Trash));
            items
        };
        popover_anchored(trigger, context_menu::menu(items), Msg::Popup(None))
            .placement(Placement::Below)
            .into()
    }

    /// The kind's own fields, then the code.
    fn main_group<'a>(&'a self, r: &'a ItemRecord) -> Option<Element<'a, Msg>> {
        let mut rows: Vec<Element<'a, Msg>> = r
            .fields
            .iter()
            .filter(|f| !is_uri(&f.key) && !is_custom(&f.key))
            .map(|f| self.field_row(f, &f.label))
            .collect();
        if let Some(code) = CodeNow::of(r, self.now) {
            rows.push(self.code_row(&code));
        } else if let Some(raw) = r.unreadable_totp() {
            rows.push(self.unreadable_key_row(raw));
        }
        (!rows.is_empty()).then(|| group(rows))
    }

    fn web_group<'a>(&'a self, r: &'a ItemRecord) -> Option<Element<'a, Msg>> {
        let mut rows: Vec<Element<'a, Msg>> = Vec::new();
        for f in r.fields.iter().filter(|f| is_uri(&f.key)) {
            let index: Option<usize> = f.key.trim_start_matches("uri").parse().ok();
            let m = index.and_then(|i| r.uris.get(i)).and_then(|u| u.match_type);
            let label = match m {
                Some(m) if m != UriMatch::Domain => format!("{} · {}", f.label, m.label()),
                _ => f.label.clone(),
            };
            let copy = self.copy_tool(CopyWhat::Field(f.key.clone()));
            let open = widgets::tool(
                "lucide/external-link",
                "Open",
                Some(Msg::OpenUrl(f.value.clone())),
            );
            rows.push(self.row_with(&f.key, label, plain_value(&f.value), vec![open, copy]));
        }
        for p in &r.passkeys {
            let who = p
                .user_name
                .clone()
                .or_else(|| p.user_display_name.clone())
                .unwrap_or_else(|| p.rp_id.clone());
            let saved = p
                .created
                .as_deref()
                .and_then(when::parse)
                .map(|d| format!(" · saved {}", when::day(d.date(), when::today())))
                .unwrap_or_default();
            rows.push(row_layout(
                label_text("Passkey".into(), false),
                plain_value(&format!("{who}{saved}")),
                vec![container(widgets::faint("lucide/fingerprint-pattern", 16.0))
                    .padding(Padding::ZERO.right(8))
                    .into()],
            ));
        }
        (!rows.is_empty()).then(|| group(rows))
    }

    fn custom_group<'a>(&'a self, r: &'a ItemRecord) -> Option<Element<'a, Msg>> {
        let rows: Vec<Element<'a, Msg>> = r
            .fields
            .iter()
            .filter(|f| is_custom(&f.key))
            .map(|f| self.field_row(f, &f.label))
            .collect();
        (!rows.is_empty()).then(|| group(rows))
    }

    fn notes_group<'a>(&'a self, r: &'a ItemRecord) -> Option<Element<'a, Msg>> {
        let notes = r.notes.as_deref().filter(|n| !n.trim().is_empty())?;
        let value = text(notes)
            .font(fonts::ui())
            .size(13)
            .line_height(iced::widget::text::LineHeight::Absolute(18.0.into()));
        let copy = self.copy_tool(CopyWhat::Notes);
        Some(group(vec![self.row_with("notes", "Notes".into(), value.into(), vec![copy])]))
    }

    /// Password history, folded until asked for.
    fn history_group<'a>(&'a self, r: &'a ItemRecord) -> Option<Element<'a, Msg>> {
        if r.password_history.is_empty() {
            return None;
        }
        let n = r.password_history.len();
        let fold = button(
            row![
                kit_text::body(format!("Password history · {n}")).width(Length::Fill),
                widgets::muted(
                    if self.history_open { "lucide/chevron-down" } else { "lucide/chevron-right" },
                    14.0
                ),
            ]
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([12.0, 12.0]))
        .width(Length::Fill)
        .style(style::list_row(false))
        .on_press(Msg::History(!self.history_open));
        let mut rows: Vec<Element<'a, Msg>> = vec![fold.into()];
        if self.history_open {
            for (i, h) in r.password_history.iter().enumerate() {
                let key = format!("history:{i}");
                let shown = self.revealed.contains(&key);
                let label = match when::parse(&h.last_used) {
                    Some(d) => format!("Used until {}", when::day(d.date(), when::today())),
                    None => "Used before".into(),
                };
                let tools = vec![
                    self.reveal_tool(&key),
                    self.copy_tool(CopyWhat::History(i)),
                ];
                rows.push(self.row_with(&key, label, secret_value(&h.password, shown), tools));
            }
        }
        Some(group(rows))
    }

    /// One field: copy always, reveal on a secret.
    fn field_row<'a>(&'a self, f: &'a RecordField, label: &str) -> Element<'a, Msg> {
        let mut tools = Vec::new();
        let value = if f.hidden {
            let shown = self.revealed.contains(&f.key);
            tools.push(self.reveal_tool(&f.key));
            secret_value(&f.value, shown)
        } else if f.mono {
            mono_value(&f.value)
        } else {
            plain_value(&f.value)
        };
        tools.push(self.copy_tool(CopyWhat::Field(f.key.clone())));
        self.row_with(&f.key, label.to_string(), value, tools)
    }

    fn code_row<'a>(&'a self, code: &CodeNow) -> Element<'a, Msg> {
        let value = text(grouped(&code.code))
            .font(iced::Font {
                weight: iced::font::Weight::Medium,
                ..fonts::mono()
            })
            .size(20);
        let tools = vec![
            widgets::ring(code.left, code.period, code.warning()),
            self.copy_tool(CopyWhat::Code),
        ];
        self.row_with("code", "One-time code".into(), value.into(), tools)
    }

    /// A key Vault can't make codes from: kept, and said so.
    fn unreadable_key_row<'a>(&'a self, raw: &'a str) -> Element<'a, Msg> {
        let shown = self.revealed.contains("totp");
        let value = column![
            secret_value(raw, shown),
            text("Vault can't make codes from this key. It is kept as it is.")
                .font(fonts::ui())
                .size(11)
                .style(tertiary),
        ]
        .spacing(2);
        let tools = vec![
            self.reveal_tool("totp"),
            self.copy_tool(CopyWhat::TotpKey),
        ];
        self.row_with("totp", "Authenticator key".into(), value.into(), tools)
    }

    /// A row whose label (and tools) say "Copied" for a moment after a copy.
    fn row_with<'a>(
        &'a self,
        key: &str,
        label: String,
        value: Element<'a, Msg>,
        tools: Vec<Element<'a, Msg>>,
    ) -> Element<'a, Msg> {
        let copied = self.just_copied(key);
        let mut tools = tools;
        if copied {
            tools.insert(0, copied_mark());
        }
        row_layout(label_text(label, copied), value, tools)
    }

    fn copy_tool<'a>(&'a self, what: CopyWhat) -> Element<'a, Msg> {
        widgets::tool("lucide/copy", "Copy", Some(Msg::Copy(what)))
    }

    fn reveal_tool<'a>(&'a self, key: &str) -> Element<'a, Msg> {
        let shown = self.revealed.contains(key);
        widgets::tool_toggle(
            if shown { "lucide/eye-off" } else { "lucide/eye" },
            if shown { "Hide" } else { "Show" },
            shown,
            Msg::Reveal(key.to_string()),
        )
    }

    /// An item that asks for the master password before it opens.
    fn view_reprompt<'a>(&'a self, r: &'a Reprompt) -> Element<'a, Msg> {
        let (head_plate, name) = match self.summary(&r.id) {
            Some(s) => (
                Some(widgets::plate(&plate(s.kind, &s.name), PlateSize::Head)),
                s.name.clone(),
            ),
            None => (None, String::new()),
        };
        let head = pane_head(
            head_plate,
            title(&name),
            "Asks for your master password".into(),
            Vec::new(),
        );
        let form = column![
            row![
                widgets::muted("lucide/shield-check", 20.0),
                text("You set this item to ask for your master password before it opens.")
                    .font(fonts::ui())
                    .size(12.5)
                    .style(|t: &Theme| text::Style {
                        color: Some(style::ink2(t)),
                    }),
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center),
            labeled_input(
                super::signin::secret_input(
                    Field::Reprompt,
                    "Master password",
                    &r.password,
                    r.reveal,
                    r.error.is_some(),
                    Msg::RevealForm,
                ),
                r.error.as_deref(),
            ),
            row![
                kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::RepromptCancel),
                Space::new().width(Length::Fill),
                kit_btn::labeled(if r.busy { "Checking…" } else { "Continue" }, kit_btn::primary)
                    .on_press_maybe((!r.busy).then_some(Msg::Reprompt)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(SPACE_LG)
        .max_width(380);
        column![
            head,
            container(form).padding(Padding {
                top: SPACE_LG,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            }),
        ]
        .into()
    }
}

/// The pane's head: a plate, the title over a line, tools at the right.
pub fn pane_head<'a>(
    plate: Option<Element<'a, Msg>>,
    title: Element<'a, Msg>,
    sub: String,
    tools: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let mut r = row![].spacing(SPACE_MD).align_y(Alignment::Center);
    if let Some(p) = plate {
        r = r.push(p);
    }
    let mut who = column![title].spacing(2).width(Length::Fill);
    if !sub.is_empty() {
        who = who.push(
            kit_text::caption(sub)
                .style(kit_text::muted)
                .wrapping(iced::widget::text::Wrapping::None),
        );
    }
    r = r.push(who);
    r = r.push(row(tools).spacing(2).align_y(Alignment::Center));
    container(r)
        .height(Length::Fixed(60.0))
        .padding(Padding {
            top: 0.0,
            right: 14.0,
            bottom: 0.0,
            left: 20.0,
        })
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn title<'a>(name: &str) -> Element<'a, Msg> {
    text(name.to_string())
        .font(fonts::ui_medium())
        .size(15)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

/// Rows on one raised surface, a rule between each two.
pub fn group<'a>(rows: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let mut col = Column::new().width(Length::Fill);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            col = col.push(
                container(
                    container(Space::new().width(Length::Fill).height(1))
                        .width(Length::Fill)
                        .style(style::group_rule),
                )
                .padding(Padding::ZERO.left(12)),
            );
        }
        col = col.push(r);
    }
    container(col).width(Length::Fill).style(style::group).into()
}

/// Label over value on the left, tools on the right.
fn row_layout<'a>(
    label: Element<'a, Msg>,
    value: Element<'a, Msg>,
    tools: Vec<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    container(
        row![
            column![label, value].spacing(3).width(Length::Fill),
            row(tools).spacing(2).align_y(Alignment::Center),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 6.0,
        right: 6.0,
        bottom: 6.0,
        left: 12.0,
    })
    .height(Length::Shrink)
    .width(Length::Fill)
    .into()
}

fn label_text<'a>(label: String, copied: bool) -> Element<'a, Msg> {
    text(label)
        .font(fonts::ui())
        .size(11)
        .style(move |t: &Theme| text::Style {
            color: Some(if copied {
                t.extended_palette().success.base.color
            } else {
                style::ink2(t)
            }),
        })
        .into()
}

pub(super) fn copied_mark<'a>() -> Element<'a, Msg> {
    container(
        row![
            widgets::glyph("lucide/check", 12.0, |t: &Theme| t
                .extended_palette()
                .success
                .base
                .color),
            text("Copied").font(fonts::ui()).size(11).style(|t: &Theme| text::Style {
                color: Some(t.extended_palette().success.base.color),
            }),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding(Padding::ZERO.right(6))
    .into()
}

fn plain_value<'a>(v: &str) -> Element<'a, Msg> {
    text(v.to_string())
        .font(fonts::ui())
        .size(13)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

fn mono_value<'a>(v: &str) -> Element<'a, Msg> {
    text(v.to_string())
        .font(fonts::mono())
        .size(13)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

/// A secret: dots until shown. The dots never say its length past twelve.
fn secret_value<'a>(v: &str, shown: bool) -> Element<'a, Msg> {
    if shown {
        return mono_value(v);
    }
    let n = v.chars().count().clamp(6, 12);
    text("•".repeat(n))
        .font(fonts::ui())
        .size(14)
        .into()
}

fn trash_banner<'a>(since: &str) -> Element<'a, Msg> {
    let when = when::parse(since)
        .map(|d| format!("In the trash since {}.", when::day(d.date(), when::today())))
        .unwrap_or_else(|| "In the trash.".into());
    group(vec![
        container(
            row![
                widgets::muted("lucide/trash", 14.0),
                kit_text::body(when).width(Length::Fill),
                kit_btn::labeled_sm("Restore", kit_btn::secondary).on_press(Msg::Restore),
                kit_btn::labeled_sm("Delete Forever…", kit_btn::danger_outline)
                    .on_press(Msg::AskDeleteForever),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([8.0, 12.0]))
        .into(),
    ])
}

fn attachments_group<'a>(r: &'a ItemRecord) -> Option<Element<'a, Msg>> {
    if r.attachments.is_empty() {
        return None;
    }
    let rows = r
        .attachments
        .iter()
        .map(|a| {
            row_layout(
                label_text(a.size.clone(), false),
                row![widgets::muted("lucide/paperclip", 12.0), plain_value(&a.file_name)]
                    .spacing(SPACE_XS + 2.0)
                    .align_y(Alignment::Center)
                    .into(),
                Vec::new(),
            )
        })
        .collect();
    Some(group(rows))
}

fn quiet<'a>(message: String) -> Element<'a, Msg> {
    container(kit_text::caption(message).style(kit_text::muted))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn tertiary(t: &Theme) -> text::Style {
    text::Style {
        color: Some(style::ink3(t)),
    }
}

fn is_uri(key: &str) -> bool {
    key.strip_prefix("uri")
        .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
}

fn is_custom(key: &str) -> bool {
    key.starts_with("custom")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_custom_fields_by_key() {
        assert!(is_uri("uri0"));
        assert!(is_uri("uri12"));
        assert!(!is_uri("username"));
        assert!(is_custom("custom3"));
        assert!(!is_custom("code"));
    }
}
