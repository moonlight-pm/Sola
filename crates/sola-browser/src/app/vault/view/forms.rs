//! Editing a record in place, and a new login. The groups stay where
//! the record had them; values become fields. A changed field has a dot
//! by its label, since Save writes only those.

use iced::widget::{Space, button, column, container, row, text_editor};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_button;
use sola_kit::components::scroll;
use sola_kit::components::select::{SelectOption, select_sized};
use sola_kit::components::text_input::text_input;
use sola_vault_proto::{DraftRow, ItemKind};

use super::super::update::{MATCHES, VaultMsg, match_label};
use super::parts::{self, group, icon, icon_button, ink, ink2, ink3, label, msg};
use super::{Head, Sheet, actions, body, id};
use crate::app::{App, Msg};
use crate::engine::Engine;

/// A website's match select, beside its field.
const MATCH_W: f32 = 104.0;

impl<E: Engine> App<E> {
    pub(super) fn sheet_edit(&self) -> Sheet<'_> {
        let v = &self.vault;
        let Some(d) = v.draft.as_ref() else {
            return Sheet {
                head: Head::new("Edit").back(VaultMsg::CancelEdit),
                body: body(label("This item couldn't be opened to edit.", 12.0, ink2), true),
                foot: None,
            };
        };
        let busy = v.busy;
        let mut col = column![].spacing(8).width(Length::Fill);
        if let Some(e) = v.error.as_ref() {
            col = col.push(parts::error_line(e));
        }

        let mut first = vec![edit_field(
            "Name",
            d.name_changed() && d.base.is_some(),
            text_input("Name", &d.name)
                .id(id("edit-name"))
                .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::EditName(s))))
                .on_submit(msg(VaultMsg::Save)),
            Vec::new(),
            None,
        )];
        for r in &d.rows {
            first.push(self.draft_row(r, d.changed_row(&r.key).is_some() && d.base.is_some(), d.kind));
        }
        col = col.push(group(first));

        if d.kind == ItemKind::Login {
            let mut sites = column![label("Websites", 11.0, ink2)].spacing(6);
            for (i, u) in d.uris.iter().enumerate() {
                let input = text_input("example.com", &u.value)
                    .id(id(&format!("uri:{i}")))
                    .size(13)
                    .style(sola_kit::components::text_input::style)
                    .width(Length::Fill)
                    .on_input_maybe(
                        (!busy).then_some(move |value| msg(VaultMsg::EditUri { index: i, value })),
                    )
                    .on_submit(msg(VaultMsg::Save));
                let options = MATCHES.iter().map(|m| {
                    SelectOption::new(
                        match_label(*m),
                        *m == u.match_type,
                        msg(VaultMsg::UriMatch {
                            index: i,
                            value: *m,
                        }),
                    )
                });
                let open = v.uri_menu == Some(i);
                let select = container(select_sized(
                    match_label(u.match_type),
                    options,
                    open,
                    msg(VaultMsg::UriMenu(if open { None } else { Some(i) })),
                    msg(VaultMsg::UriMenu(None)),
                    160.0,
                ))
                .width(Length::Fixed(MATCH_W));
                sites = sites.push(
                    row![
                        input,
                        select,
                        icon_button("x", 13, (!busy).then_some(VaultMsg::RemoveUri(i)))
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                );
            }
            sites = sites.push(add_link("Add website", (!busy).then_some(VaultMsg::AddUri)));
            col = col.push(group(vec![padded(sites.into())]));
        }

        let mut custom = Vec::new();
        for (i, c) in d.custom.iter().enumerate() {
            let name = text_input("Field name", &c.name)
                .id(id(&format!("custom-name:{i}")))
                .size(12)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill)
                .on_input_maybe(
                    (!busy).then_some(move |value| msg(VaultMsg::CustomName { index: i, value })),
                );
            let value = text_input("Value", &c.value)
                .id(id(&format!("custom:{i}")))
                .size(13)
                .secure(c.hidden)
                .style(sola_kit::components::text_input::style)
                .width(Length::Fill)
                .on_input_maybe(
                    (!busy).then_some(move |value| msg(VaultMsg::CustomValue { index: i, value })),
                );
            custom.push(padded(
                column![
                    row![
                        name,
                        icon_button("x", 13, (!busy).then_some(VaultMsg::RemoveCustom(i)))
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                    value
                ]
                .spacing(5)
                .into(),
            ));
        }
        custom.push(padded(add_link(
            "Add field",
            (!busy).then_some(VaultMsg::AddCustom),
        )));
        col = col.push(group(custom));

        let mut notes = text_editor(&v.notes)
            .id(id("notes"))
            .placeholder("Notes")
            .size(13)
            .min_height(64.0)
            .padding(sola_kit::components::text_input::DEFAULT_PADDING)
            .style(notes_style);
        if !busy {
            notes = notes.on_action(|a| msg(VaultMsg::Notes(a)));
        }
        col = col.push(group(vec![padded(
            column![
                changed_label("Notes", d.notes_changed() && d.base.is_some()),
                notes
            ]
            .spacing(5)
            .into(),
        )]));

        let scrolled = container(scroll::scrollable(col).width(Length::Fill))
            .max_height(super::SHEET_MAX_H - 44.0 - 60.0);
        let mut save = kit_button::labeled(if busy { "Saving…" } else { "Save" }, kit_button::primary);
        if !busy {
            save = save.on_press(msg(VaultMsg::Save));
        }
        let cancel = kit_button::labeled("Cancel", kit_button::ghost).on_press(msg(VaultMsg::CancelEdit));
        Sheet {
            head: Head::new(format!("Edit {}", d.name)).sub("Saves only what you change"),
            body: body(scrolled, false),
            foot: Some(actions(vec![cancel.into(), save.into()], true)),
        }
    }

    fn draft_row<'a>(&'a self, r: &'a DraftRow, changed: bool, kind: ItemKind) -> Element<'a, Msg> {
        let v = &self.vault;
        let busy = v.busy;
        let reveal_key = format!("edit:{}", r.key);
        let shown = !r.hidden || v.record.revealed.contains(&reveal_key);
        let key = r.key.clone();
        let input = text_input(r.label.as_str(), &r.value)
            .id(id(&reveal_key))
            .secure(!shown)
            .on_input_maybe((!busy).then_some(move |value| {
                msg(VaultMsg::EditRow {
                    key: key.clone(),
                    value,
                })
            }))
            .on_submit(msg(VaultMsg::Save));
        let mut tools = Vec::new();
        if r.regen {
            tools.push(icon_button("dices", 14, (!busy).then_some(VaultMsg::Regenerate)));
        }
        if r.hidden {
            tools.push(icon_button(
                if shown { "eye-off" } else { "eye" },
                14,
                Some(VaultMsg::RevealField(reveal_key.clone())),
            ));
        }
        let had_one = self
            .vault
            .draft
            .as_ref()
            .and_then(|d| d.base.as_ref())
            .is_some_and(|b| !b.row_value("password").is_empty());
        let hint = match r.key.as_str() {
            "password" if kind == ItemKind::Login && changed && had_one => {
                Some("The old one goes to password history.")
            }
            "dob" | "issued" | "expires" => Some("YYYY-MM-DD"),
            "exp_month" => Some("MM"),
            "exp_year" => Some("YYYY or YY"),
            "totp" => Some("otpauth:// or the secret"),
            _ => None,
        };
        edit_field(&r.label, changed, input, tools, hint)
    }

    pub(super) fn sheet_create(&self) -> Sheet<'_> {
        let v = &self.vault;
        let busy = v.busy;
        let Some(form) = v.create.as_ref() else {
            return self.sheet_browse();
        };
        let page = super::super::page_host(&self.vault_page_url());
        let mut head = Head::new("New login").back(VaultMsg::Back);
        if let Some(h) = &page {
            head = head.sub(format!("for {h}"));
        }
        let shown = v.record.revealed.contains("new-password");
        let username = text_input("Username", &form.username)
            .id(id("new-username"))
            .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::NewUsername(s))))
            .on_submit(msg(VaultMsg::NewSave));
        let password = text_input("Password", &form.password)
            .id(id("new-password"))
            .secure(!shown)
            .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::NewPassword(s))))
            .on_submit(msg(VaultMsg::NewSave));
        let website = text_input("example.com", &form.website)
            .id(id("new-website"))
            .on_input_maybe((!busy).then_some(|s| msg(VaultMsg::NewWebsite(s))))
            .on_submit(msg(VaultMsg::NewSave));
        let mut col = column![].spacing(8).width(Length::Fill);
        if let Some(e) = v.error.as_ref() {
            col = col.push(parts::error_line(e));
        }
        col = col.push(group(vec![
            edit_field("Username", false, username, Vec::new(), None),
            edit_field(
                "Password",
                false,
                password,
                vec![
                    icon_button("dices", 14, (!busy).then_some(VaultMsg::NewRegenerate)),
                    icon_button(
                        if shown { "eye-off" } else { "eye" },
                        14,
                        Some(VaultMsg::RevealField("new-password".into())),
                    ),
                ],
                form.generated
                    .then_some("Generated: 16 characters, letters, digits and symbols."),
            ),
            edit_field("Website", false, website, Vec::new(), None),
        ]));
        let word = if busy {
            "Saving…"
        } else if page.is_some() {
            "Save and fill"
        } else {
            "Save"
        };
        let mut save = kit_button::labeled(word, kit_button::primary);
        if !busy {
            save = save.on_press(msg(VaultMsg::NewSave));
        }
        let cancel = kit_button::labeled("Cancel", kit_button::ghost).on_press(msg(VaultMsg::Back));
        Sheet {
            head,
            body: body(col, false),
            foot: Some(actions(vec![cancel.into(), save.into()], true)),
        }
    }

    pub(super) fn sheet_created(&self) -> Sheet<'_> {
        let close = kit_button::labeled("Close", kit_button::secondary).on_press(msg(VaultMsg::Close));
        Sheet {
            head: Head::new("Saved to the vault").plate(parts::kind_plate("check", 22.0)),
            body: body(
                parts::empty(
                    "key-round",
                    "The login is saved",
                    "This page had no username or password field to fill.",
                    Some(close.into()),
                ),
                false,
            ),
            foot: None,
        }
    }
}

/// An edit row: its label (a dot when changed), the field, its tools,
/// a hint under it.
fn edit_field<'a>(
    caption: &str,
    changed: bool,
    input: sola_kit::components::text_input::TextInput<'a, Msg>,
    tools: Vec<Element<'a, Msg>>,
    hint: Option<&'a str>,
) -> Element<'a, Msg> {
    let input = input
        .size(13)
        .style(sola_kit::components::text_input::style)
        .width(Length::Fill);
    let mut field = row![input].spacing(2).align_y(Alignment::Center);
    for t in tools {
        field = field.push(t);
    }
    let mut col = column![changed_label(caption, changed), field].spacing(5);
    if let Some(h) = hint {
        col = col.push(label(h, 11.0, ink3).wrapping(iced::widget::text::Wrapping::Word));
    }
    padded(col.into())
}

fn changed_label<'a>(caption: &str, changed: bool) -> Element<'a, Msg> {
    let mut r = row![].spacing(6).align_y(Alignment::Center);
    if changed {
        r = r.push(
            container(Space::new().width(5.0).height(5.0)).style(|t: &Theme| container::Style {
                background: Some(Background::Color(ink2(t))),
                border: Border {
                    radius: 3.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
        );
    }
    r.push(label(caption.to_string(), 11.0, ink2)).into()
}

fn padded(e: Element<'_, Msg>) -> Element<'_, Msg> {
    container(e)
        .padding(Padding::from([8, 10]))
        .width(Length::Fill)
        .into()
}

fn add_link<'a>(word: &'a str, on: Option<VaultMsg>) -> Element<'a, Msg> {
    let b = button(
        row![icon("plus", 12, ink2), label(word, 12.0, ink2)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([5, 4]))
    .style(|t: &Theme, status| button::Style {
        background: matches!(status, button::Status::Hovered)
            .then(|| Background::Color(sola_kit::components::style::mix_white(parts::face(t), 0.05))),
        text_color: ink(t),
        border: Border {
            radius: sola_kit::components::style::RADIUS_SM.into(),
            ..Border::default()
        },
        ..button::Style::default()
    });
    match on {
        Some(m) => b.on_press(msg(m)).into(),
        None => b.into(),
    }
}

/// The kit field's look on the notes editor.
fn notes_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    use sola_kit::components::text_input::Status as Field;
    let field = sola_kit::components::text_input::style(
        theme,
        match status {
            text_editor::Status::Active => Field::Active,
            text_editor::Status::Hovered => Field::Hovered,
            text_editor::Status::Focused { is_hovered } => Field::Focused { is_hovered },
            text_editor::Status::Disabled => Field::Disabled,
        },
    );
    text_editor::Style {
        background: field.background,
        border: field.border,
        placeholder: field.placeholder,
        value: field.value,
        selection: field.selection,
    }
}
