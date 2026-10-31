//! A record: Settings' grouped rows — label over value, its tools at the
//! right (copy on every field, reveal on secrets, open on a website), the
//! one-time code largest with its ring — and one Fill at the foot.

use iced::widget::{column, container, text};
use iced::{Element, Length, Padding};
use sola_kit::components::button as kit_button;
use sola_kit::components::scroll;
use sola_vault_proto::{ItemKind, ItemRecord, RecordField};

use super::super::rows;
use super::super::update::{VaultMsg, now_secs};
use super::parts::{
    self, copied_mark, field_row, group, icon, icon_button, ink, ink2, ink3, kbd, label, msg,
};
use super::{COPIED_FOR, Head, Sheet, actions, body};
use crate::app::{App, Msg};
use crate::engine::Engine;

const DOTS: &str = "••••••••••••";

impl<E: Engine> App<E> {
    pub(super) fn sheet_record(&self) -> Sheet<'_> {
        let v = &self.vault;
        let Some(item) = v.record.item.as_ref() else {
            let line = if v.record.loading {
                "Opening…"
            } else {
                "This item couldn't be opened."
            };
            let mut col = column![label(line, 12.0, ink2)].spacing(8);
            if let Some(e) = v.error.as_ref() {
                col = col.push(parts::error_line(e));
            }
            return Sheet {
                head: Head::new("Item").back(VaultMsg::Back),
                body: body(col, true),
                foot: None,
            };
        };
        let sub = item
            .uris
            .first()
            .map(|u| rows::website_host(&u.uri))
            .or_else(|| item.field("username").map(str::to_string))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| item.kind.label().to_string());
        let mut head = Head::new(item.name.clone())
            .back(VaultMsg::Back)
            .plate(parts::plate(&item.name, item.kind, 22.0))
            .sub(sub);
        if item.can_edit {
            head = head.button(icon_button(
                "pencil",
                15,
                (!v.busy).then_some(VaultMsg::Edit),
            ));
        }
        head = head.button(self.more_button(Some(item.id.clone())));

        let mut col = column![].spacing(8).width(Length::Fill);
        if let Some(e) = v.error.as_ref() {
            col = col.push(parts::error_line(e));
        }
        for g in self.record_groups(item) {
            col = col.push(g);
        }
        let used = v
            .list
            .items
            .iter()
            .find(|i| i.id == item.id)
            .and_then(|i| rows::used_label(i.last_used, now_secs()));
        let meta = [rows::edited_label(item.revised.as_deref()), used]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        if !meta.is_empty() {
            col = col.push(
                container(label(meta, 11.0, ink3)).padding(Padding {
                    top: 2.0,
                    left: 6.0,
                    ..Padding::ZERO
                }),
            );
        }
        let scrolled = container(scroll::scrollable(col).width(Length::Fill))
            .max_height(super::SHEET_MAX_H - 44.0 - 60.0);
        let foot = item.can_fill().then(|| {
            let word = match item.kind {
                ItemKind::Card => "Fill card",
                ItemKind::Identity => "Fill identity",
                _ => "Fill",
            };
            let fill = iced::widget::button(
                iced::widget::row![
                    text(word).size(13).font(sola_kit::fonts::ui_medium()),
                    kbd("↵")
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .padding(sola_kit::components::style::PAD_CONTROL)
            .width(Length::Fill)
            .style(kit_button::primary);
            let fill = if v.busy {
                fill
            } else {
                fill.on_press(msg(VaultMsg::Fill(item.id.clone())))
            };
            actions(vec![container(fill).center_x(Length::Fill).into()], false)
        });
        Sheet {
            head,
            body: body(scrolled, false),
            foot,
        }
    }

    /// The record's groups: the credentials, where it is used, the rest.
    fn record_groups<'a>(&'a self, item: &'a ItemRecord) -> Vec<Element<'a, Msg>> {
        let mut first = Vec::new();
        let mut sites = Vec::new();
        let mut rest = Vec::new();
        for f in &item.fields {
            if f.key == "totp" {
                continue;
            }
            if f.key.starts_with("uri") {
                sites.push(self.website_row(f));
            } else if f.key.starts_with("custom") {
                rest.push(self.value_row(f));
            } else {
                first.push(self.value_row(f));
            }
        }
        let now = now_secs();
        if let Some((code, period)) = item.totp_code_at(now) {
            first.push(self.code_row(code, period, now));
        } else if let Some(raw) = item.unreadable_totp() {
            first.push(self.unreadable_key_row(raw));
        }
        for pk in &item.passkeys {
            let who = pk
                .user_name
                .clone()
                .or_else(|| pk.user_display_name.clone())
                .unwrap_or_else(|| pk.rp_id.clone());
            let saved = pk
                .created
                .as_deref()
                .and_then(|c| rows::edited_label(Some(c)))
                .map(|e| e.replacen("Edited", "saved", 1));
            let line = match saved {
                Some(s) => format!("{who} · {s}"),
                None => who,
            };
            sites.push(field_row(
                "Passkey".into(),
                label(line, 13.0, ink).into(),
                vec![container(icon("fingerprint-pattern", 16, ink2))
                    .padding(Padding {
                        right: 6.0,
                        ..Padding::ZERO
                    })
                    .into()],
                false,
            ));
        }
        if let Some(notes) = item.notes.as_deref().filter(|n| !n.trim().is_empty()) {
            let copied = self.copied("notes");
            let tools = if copied {
                vec![copied_mark()]
            } else {
                vec![icon_button(
                    "copy",
                    14,
                    Some(VaultMsg::Copy {
                        key: "notes".into(),
                        value: notes.to_string(),
                    }),
                )]
            };
            rest.push(field_row(
                "Notes".into(),
                label(notes.to_string(), 13.0, ink)
                    .wrapping(text::Wrapping::Word)
                    .into(),
                tools,
                copied,
            ));
        }
        [first, sites, rest]
            .into_iter()
            .filter(|g| !g.is_empty())
            .map(group)
            .collect()
    }

    fn copied(&self, key: &str) -> bool {
        self.vault
            .record
            .copied
            .as_ref()
            .is_some_and(|(k, t)| k == key && t.elapsed() < COPIED_FOR)
    }

    fn value_row<'a>(&'a self, f: &'a RecordField) -> Element<'a, Msg> {
        let shown = !f.hidden || self.vault.record.revealed.contains(&f.key);
        let value: Element<'a, Msg> = if shown {
            let t = label(f.value.clone(), 13.0, ink).wrapping(text::Wrapping::None);
            if f.mono {
                t.font(sola_kit::fonts::mono()).into()
            } else {
                t.into()
            }
        } else {
            label(DOTS, 14.0, ink).into()
        };
        let copied = self.copied(&f.key);
        let mut tools = Vec::new();
        if f.hidden {
            let eye = if shown { "eye-off" } else { "eye" };
            tools.push(icon_button(eye, 14, Some(VaultMsg::RevealField(f.key.clone()))));
        }
        tools.push(if copied {
            copied_mark()
        } else {
            icon_button(
                "copy",
                14,
                Some(VaultMsg::Copy {
                    key: f.key.clone(),
                    value: f.value.clone(),
                }),
            )
        });
        field_row(f.label.clone(), value, tools, copied)
    }

    fn website_row<'a>(&'a self, f: &'a RecordField) -> Element<'a, Msg> {
        let open = rows::website_url(&f.value).map(VaultMsg::OpenWebsite);
        let shown = rows::website_host(&f.value);
        let value = label(shown, 13.0, ink).wrapping(text::Wrapping::None).into();
        let copied = self.copied(&f.key);
        let tools = vec![if copied {
            copied_mark()
        } else {
            icon_button(
                "copy",
                14,
                Some(VaultMsg::Copy {
                    key: f.key.clone(),
                    value: f.value.clone(),
                }),
            )
        }, icon_button("external-link", 14, open)];
        field_row(f.label.clone(), value, tools, copied)
    }

    fn code_row(&self, code: String, period: u32, now: u64) -> Element<'_, Msg> {
        let left = sola_vault_proto::remaining_secs(period, now) as u32;
        let copied = self.copied("code");
        let value = label(rows::grouped_code(&code), 20.0, ink)
            .font(sola_kit::fonts::mono())
            .into();
        let mut tools = Vec::new();
        if copied {
            tools.push(copied_mark());
        }
        tools.push(parts::ring(left, period));
        if !copied {
            tools.push(icon_button(
                "copy",
                14,
                Some(VaultMsg::Copy {
                    key: "code".into(),
                    value: code,
                }),
            ));
        }
        field_row("One-time code".into(), value, tools, copied)
    }

    fn unreadable_key_row<'a>(&'a self, raw: &'a str) -> Element<'a, Msg> {
        let shown = self.vault.record.revealed.contains("totp");
        let value = if shown {
            label(raw.to_string(), 13.0, ink)
                .font(sola_kit::fonts::mono())
                .wrapping(text::Wrapping::None)
        } else {
            label(DOTS, 14.0, ink)
        };
        let value = column![
            value,
            label("Sola can't make codes from this key. It is kept as it is.", 11.0, ink3)
                .wrapping(text::Wrapping::Word),
        ]
        .spacing(2)
        .into();
        let copied = self.copied("totp");
        let tools = vec![
            icon_button(
                if shown { "eye-off" } else { "eye" },
                14,
                Some(VaultMsg::RevealField("totp".into())),
            ),
            if copied {
                copied_mark()
            } else {
                icon_button(
                    "copy",
                    14,
                    Some(VaultMsg::Copy {
                        key: "totp".into(),
                        value: raw.to_string(),
                    }),
                )
            },
        ];
        field_row("Authenticator key".into(), value, tools, copied)
    }
}
