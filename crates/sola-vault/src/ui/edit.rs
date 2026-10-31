//! Edit in place: the record's groups become fields, a dot marks what
//! changed, and Save writes only that (the daemon applies the draft onto
//! the item as the server has it). A new item, a clone: the same form,
//! with everything written.

use std::collections::HashSet;

use iced::widget::{Space, button, column, container, row, text, text_editor, toggler};
use iced::{Alignment, Element, Length, Padding, Task, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::toggle_style;
use sola_kit::components::scroll;
use sola_kit::components::select::{SelectOption, select};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;
use sola_vault_proto::{DraftRow, ItemDraft, ItemKind, Request, UriMatch};
use zeroize::Zeroizing;

use super::ask::Ask;
use super::fields::Field;
use super::record::{group, pane_head};
use super::sheets::GenTarget;
use super::signin::field_style;
use super::widgets::{self, changed_dot};
use super::{App, Msg, Pane, Popup, style};
use crate::model::edit::{self as model, row_changed};

pub struct EditForm {
    pub draft: ItemDraft,
    pub notes: text_editor::Content,
    /// Secrets shown: row keys and `custom<i>`.
    pub reveal: HashSet<String>,
    pub busy: bool,
    pub error: Option<String>,
    pub name_error: Option<&'static str>,
    pub confirm_discard: bool,
}

#[derive(Debug, Clone)]
pub enum DraftMsg {
    AddUri,
    RemoveUri(usize),
    Match(usize, Option<UriMatch>),
    AddCustom(bool),
    RemoveCustom(usize),
    Reveal(String),
    Reprompt(bool),
    Folder(Option<String>),
    /// The generator, for this row.
    Generate(String),
}

impl EditForm {
    fn new(draft: ItemDraft) -> Self {
        let notes = text_editor::Content::with_text(&draft.notes);
        Self {
            draft,
            notes,
            reveal: HashSet::new(),
            busy: false,
            error: None,
            name_error: None,
            confirm_discard: false,
        }
    }

    pub fn title(&self) -> String {
        if self.draft.is_new() {
            format!("New {}", self.draft.kind.label().to_lowercase())
        } else {
            let name = self.draft.base.as_ref().map_or(&self.draft.name, |b| &b.name);
            format!("Edit {name}")
        }
    }

    /// The notes as the editor holds them now.
    fn notes_now(&self) -> String {
        trim_notes(&self.notes.text())
    }
}

/// The editor ends its text with a newline the note never had.
fn trim_notes(s: &str) -> String {
    s.trim_end_matches('\n').to_string()
}

/// Anything to lose if the form closes now.
pub fn form_dirty(form: &EditForm) -> bool {
    let notes_changed = match &form.draft.base {
        Some(b) => trim_notes(&b.notes) != form.notes_now(),
        None => !form.notes_now().is_empty(),
    };
    model::dirty(&form.draft) || notes_changed
}

impl App {
    pub(super) fn begin_edit(&mut self) -> Task<Msg> {
        self.popup = None;
        let Pane::Open(record) = &self.record else {
            return Task::none();
        };
        if record.deleted.is_some() {
            return Task::none();
        }
        if !record.can_edit {
            return self.show_toast("This item is read-only for you.");
        }
        let id = record.id.clone();
        self.ask(Ask::Draft(id.clone()), Request::Draft { id })
    }

    pub(super) fn on_draft_loaded(&mut self, id: String, draft: ItemDraft) -> Task<Msg> {
        if self.selected.as_deref() != Some(id.as_str()) {
            return Task::none();
        }
        self.start_form(draft)
    }

    pub(super) fn start_form(&mut self, draft: ItemDraft) -> Task<Msg> {
        self.popup = None;
        self.edit = Some(EditForm::new(draft));
        Field::Name.focus()
    }

    pub(super) fn new_item(&mut self, kind: ItemKind) -> Task<Msg> {
        self.popup = None;
        if let Some(form) = self.edit.as_mut()
            && form_dirty(form)
        {
            form.confirm_discard = true;
            return Task::none();
        }
        let folder = match &self.section {
            super::Section::Folder(id) => Some(id.clone()),
            _ => None,
        };
        self.start_form(model::blank(kind, folder))
    }

    pub(super) fn cancel_edit(&mut self) -> Task<Msg> {
        self.popup = None;
        if let Some(form) = self.edit.as_mut() {
            if form_dirty(form) && !form.confirm_discard {
                form.confirm_discard = true;
                return Task::none();
            }
            self.edit = None;
        }
        Task::none()
    }

    pub(super) fn save(&mut self) -> Task<Msg> {
        let Some(form) = self.edit.as_mut() else {
            return Task::none();
        };
        if form.busy {
            return Task::none();
        }
        form.draft.notes = form.notes_now();
        model::tidy(&mut form.draft);
        if let Some(problem) = model::save_problem(&form.draft) {
            form.name_error = Some(problem);
            return Field::Name.focus();
        }
        if !form.draft.is_new() && !form_dirty(form) {
            self.edit = None;
            return Task::none();
        }
        form.busy = true;
        form.error = None;
        form.confirm_discard = false;
        let draft = form.draft.clone();
        if draft.is_new() {
            self.ask(Ask::Create, Request::Create { draft })
        } else {
            self.ask(Ask::Update, Request::Update { draft })
        }
    }

    pub(super) fn on_saved(&mut self, id: String) -> Task<Msg> {
        let was_new = self.edit.as_ref().is_some_and(|f| f.draft.is_new());
        self.edit = None;
        // Its master password was just in your hands.
        self.verified.insert(id.clone());
        self.reprompt = None;
        self.selected = Some(id.clone());
        self.revealed.clear();
        self.record = Pane::Loading;
        let _ = self.show_toast(if was_new { "Saved to the vault" } else { "Saved" });
        Task::batch([
            self.ask(Ask::Get(id.clone()), Request::Get { id }),
            self.ask(Ask::List, Request::List { url: None }),
        ])
    }

    pub(super) fn on_draft(&mut self, m: DraftMsg) -> Task<Msg> {
        if let DraftMsg::Generate(key) = m {
            return self.open_generator(Some(GenTarget::Row(key)));
        }
        let Some(form) = self.edit.as_mut() else {
            return Task::none();
        };
        let d = &mut form.draft;
        match m {
            DraftMsg::AddUri => {
                model::add_uri(d);
                let i = d.uris.len() - 1;
                return Field::Uri(i).focus();
            }
            DraftMsg::RemoveUri(i) => model::remove_uri(d, i),
            DraftMsg::Match(i, m) => {
                model::set_match(d, i, m);
                self.popup = None;
            }
            DraftMsg::AddCustom(hidden) => {
                model::add_custom(d, hidden);
                let i = d.custom.len() - 1;
                return Field::CustomName(i).focus();
            }
            DraftMsg::RemoveCustom(i) => model::remove_custom(d, i),
            DraftMsg::Reveal(key) => {
                if !form.reveal.remove(&key) {
                    form.reveal.insert(key);
                }
            }
            DraftMsg::Reprompt(on) => d.reprompt = on,
            DraftMsg::Folder(f) => {
                d.folder_id = f;
                self.popup = None;
            }
            DraftMsg::Generate(_) => {}
        }
        Task::none()
    }

    /// The generator's "Use" lands here.
    pub(super) fn fill_generated(&mut self, target: GenTarget, value: String) {
        if let Some(form) = self.edit.as_mut() {
            match target {
                GenTarget::Row(key) => {
                    form.draft.set_row(&key, value);
                    form.reveal.insert(key);
                }
            }
        }
    }

    // ── View ─────────────────────────────────────────────────────────

    pub(super) fn view_edit<'a>(&'a self, form: &'a EditForm) -> Element<'a, Msg> {
        let d = &form.draft;
        let sub = if d.is_new() {
            "Fill in what you have; the name is enough to save."
        } else {
            "Saves only what you change"
        };
        let head = pane_head(
            None,
            text(form.title()).font(fonts::ui_medium()).size(14).into(),
            sub.to_string(),
            Vec::new(),
        );

        let mut body = column![].spacing(SPACE_LG).padding(Padding {
            top: SPACE_SM,
            right: 20.0,
            bottom: 20.0,
            left: 20.0,
        });
        if let Some(e) = &form.error {
            body = body.push(
                container(kit_text::body(e.as_str()))
                    .width(Length::Fill)
                    .padding(Padding::from([9.0, 10.0]))
                    .style(style::danger_box),
            );
        }

        let mut main: Vec<Element<'a, Msg>> = vec![labeled(
            "Name",
            d.name_changed() && d.base.is_some(),
            text_input("Name", &d.name)
                .id(Field::Name.id())
                .on_input(|v| Msg::Input(Field::Name, Zeroizing::new(v)))
                .on_submit(Msg::Save)
                .style(field_style(form.name_error.is_some()))
                .into(),
            form.name_error.map(|e| (e, true)),
        )];
        for r in &d.rows {
            main.push(self.edit_row(form, r));
        }
        body = body.push(group(main));

        if d.kind == ItemKind::Login {
            body = body.push(group(vec![self.edit_websites(form)]));
        }
        body = body.push(group(vec![self.edit_custom(form)]));

        let notes_changed = d
            .base
            .as_ref()
            .is_some_and(|b| trim_notes(&b.notes) != form.notes_now());
        body = body.push(group(vec![labeled(
            "Notes",
            notes_changed,
            text_editor(&form.notes)
                .on_action(Msg::Notes)
                .height(Length::Fixed(96.0))
                .padding(Padding::from([7.0, 10.0]))
                .font(fonts::ui())
                .size(13)
                .style(style::notes_editor)
                .into(),
            None,
        )]));
        body = body.push(group(vec![self.edit_options(form)]));

        let scroller = scroll::scrollable(container(body.max_width(640)).width(Length::Fill))
            .height(Length::Fill);

        column![head, scroller, widgets::h_hairline(), self.edit_foot(form)]
            .height(Length::Fill)
            .into()
    }

    fn edit_row<'a>(&'a self, form: &'a EditForm, r: &'a DraftRow) -> Element<'a, Msg> {
        let d = &form.draft;
        let key = r.key.clone();
        let shown = !r.hidden || form.reveal.contains(&r.key);
        let mono = r.hidden || matches!(r.key.as_str(), "totp" | "number" | "code");
        let mut input = text_input("", &r.value)
            .id(Field::Row(key.clone()).id())
            .secure(!shown)
            .on_input(move |v| Msg::Input(Field::Row(key.clone()), Zeroizing::new(v)))
            .on_submit(Msg::Save);
        if mono {
            input = input.font(fonts::mono());
        }
        let mut field = row![input.width(Length::Fill)]
            .spacing(SPACE_XS)
            .align_y(Alignment::Center);
        if r.regen {
            field = field.push(widgets::tool(
                "lucide/dices",
                "Generate",
                Some(Msg::Draft(DraftMsg::Generate(r.key.clone()))),
            ));
        }
        if r.hidden {
            field = field.push(widgets::tool_toggle(
                if shown { "lucide/eye-off" } else { "lucide/eye" },
                if shown { "Hide" } else { "Show" },
                shown,
                Msg::Draft(DraftMsg::Reveal(r.key.clone())),
            ));
        }
        let changed = row_changed(d, &r.key);
        let hint = (r.key == "password" && changed && !d.is_new())
            .then_some(("The old one goes to password history.", false));
        labeled(&r.label, changed && d.base.is_some(), field.into(), hint)
    }

    fn edit_websites<'a>(&'a self, form: &'a EditForm) -> Element<'a, Msg> {
        let d = &form.draft;
        let mut col = column![].spacing(SPACE_SM);
        for (i, u) in d.uris.iter().enumerate() {
            let open = self.popup == Some(Popup::Match(i));
            let options = std::iter::once(SelectOption::new(
                "Default",
                u.match_type.is_none(),
                Msg::Draft(DraftMsg::Match(i, None)),
            ))
            .chain(UriMatch::all().into_iter().map(|m| {
                SelectOption::new(m.label(), u.match_type == Some(m), Msg::Draft(DraftMsg::Match(i, Some(m))))
            }));
            let label = u.match_type.map_or("Default", UriMatch::label);
            let picker = select(
                label,
                options,
                open,
                Msg::Popup(if open { None } else { Some(Popup::Match(i)) }),
                Msg::Popup(None),
            );
            col = col.push(
                row![
                    mark(model::uri_changed(d, i) && d.base.is_some()),
                    text_input("example.com", &u.value)
                        .id(Field::Uri(i).id())
                        .on_input(move |v| Msg::Input(Field::Uri(i), Zeroizing::new(v)))
                        .on_submit(Msg::Save)
                        .width(Length::Fill),
                    container(picker).width(Length::Fixed(132.0)),
                    widgets::tool("lucide/x", "Remove", Some(Msg::Draft(DraftMsg::RemoveUri(i)))),
                ]
                .spacing(SPACE_XS + 2.0)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(add_button("Add website", Msg::Draft(DraftMsg::AddUri)));
        let changed = d.base.is_some() && d.uris_changed();
        labeled("Websites", changed, col.into(), None)
    }

    fn edit_custom<'a>(&'a self, form: &'a EditForm) -> Element<'a, Msg> {
        let d = &form.draft;
        let mut col = column![].spacing(SPACE_SM);
        for (i, c) in d.custom.iter().enumerate() {
            let key = format!("custom{i}");
            let shown = !c.hidden || form.reveal.contains(&key);
            let mut r = row![
                mark(model::custom_changed(d, i) && d.base.is_some()),
                text_input("Name", &c.name)
                    .id(Field::CustomName(i).id())
                    .on_input(move |v| Msg::Input(Field::CustomName(i), Zeroizing::new(v)))
                    .width(Length::FillPortion(2)),
                text_input("Value", &c.value)
                    .id(Field::CustomValue(i).id())
                    .secure(!shown)
                    .on_input(move |v| Msg::Input(Field::CustomValue(i), Zeroizing::new(v)))
                    .width(Length::FillPortion(3)),
            ]
            .spacing(SPACE_XS + 2.0)
            .align_y(Alignment::Center);
            if c.hidden {
                r = r.push(widgets::tool_toggle(
                    if shown { "lucide/eye-off" } else { "lucide/eye" },
                    if shown { "Hide" } else { "Show" },
                    shown,
                    Msg::Draft(DraftMsg::Reveal(key)),
                ));
            }
            r = r.push(widgets::tool(
                "lucide/x",
                "Remove",
                Some(Msg::Draft(DraftMsg::RemoveCustom(i))),
            ));
            col = col.push(r);
        }
        col = col.push(
            row![
                add_button("Add field", Msg::Draft(DraftMsg::AddCustom(false))),
                add_button("Add hidden field", Msg::Draft(DraftMsg::AddCustom(true))),
            ]
            .spacing(SPACE_SM),
        );
        let changed = d.base.is_some() && d.custom_changed();
        labeled("Fields", changed, col.into(), None)
    }

    fn edit_options<'a>(&'a self, form: &'a EditForm) -> Element<'a, Msg> {
        let d = &form.draft;
        let open = self.popup == Some(Popup::EditFolder);
        let current = d
            .folder_id
            .as_ref()
            .and_then(|id| self.folders.iter().find(|f| f.id == *id))
            .map_or("No folder", |f| f.name.as_str());
        let options = std::iter::once(SelectOption::new(
            "No folder",
            d.folder_id.is_none(),
            Msg::Draft(DraftMsg::Folder(None)),
        ))
        .chain(self.folders.iter().map(|f| {
            SelectOption::new(
                f.name.clone(),
                d.folder_id.as_deref() == Some(f.id.as_str()),
                Msg::Draft(DraftMsg::Folder(Some(f.id.clone()))),
            )
        }));
        let folder = select(
            current,
            options,
            open,
            Msg::Popup(if open { None } else { Some(Popup::EditFolder) }),
            Msg::Popup(None),
        );
        column![
            option_row(
                "Folder",
                model::folder_changed(d),
                container(folder).width(Length::Fixed(200.0)).into(),
            ),
            option_row(
                "Ask for the master password to open it",
                model::reprompt_changed(d),
                toggler(d.reprompt)
                    .on_toggle(|on| Msg::Draft(DraftMsg::Reprompt(on)))
                    .style(toggle_style)
                    .into(),
            ),
        ]
        .spacing(SPACE_XS)
        .padding(Padding::from([SPACE_XS, 0.0]))
        .into()
    }

    fn edit_foot<'a>(&'a self, form: &'a EditForm) -> Element<'a, Msg> {
        let r = if form.confirm_discard {
            row![
                kit_text::body("Discard your changes?").width(Length::Fill),
                kit_btn::labeled("Keep editing", kit_btn::ghost).on_press(Msg::KeepEditing),
                kit_btn::labeled("Discard", kit_btn::danger).on_press(Msg::Discard),
            ]
        } else {
            row![
                kit_btn::labeled("Cancel", kit_btn::ghost).on_press(Msg::CancelEdit),
                Space::new().width(Length::Fill),
                kit_btn::labeled(if form.busy { "Saving…" } else { "Save" }, kit_btn::primary)
                    .on_press_maybe((!form.busy).then_some(Msg::Save)),
            ]
        };
        container(r.spacing(SPACE_SM).align_y(Alignment::Center))
            .padding(Padding::from([10.0, 20.0]))
            .width(Length::Fill)
            .into()
    }
}

/// A field in place: its label (a dot when changed), the field, a line
/// under it (a hint, or an error when `.1`).
fn labeled<'a>(
    label: &str,
    changed: bool,
    field: Element<'a, Msg>,
    under: Option<(&'a str, bool)>,
) -> Element<'a, Msg> {
    let mut head = row![].spacing(6).align_y(Alignment::Center);
    if changed {
        head = head.push(changed_dot());
    }
    head = head.push(
        text(label.to_string())
            .font(fonts::ui())
            .size(11)
            .style(|t: &Theme| text::Style {
                color: Some(style::ink2(t)),
            }),
    );
    let mut col = column![head, field].spacing(5);
    if let Some((line, error)) = under {
        col = col.push(if error {
            kit_text::caption(line).style(kit_text::danger)
        } else {
            kit_text::caption(line).style(|t: &Theme| text::Style {
                color: Some(style::ink3(t)),
            })
        });
    }
    container(col)
        .padding(Padding::from([8.0, 10.0]))
        .width(Length::Fill)
        .into()
}

fn option_row<'a>(label: &str, changed: bool, control: Element<'a, Msg>) -> Element<'a, Msg> {
    let mut head = row![].spacing(6).align_y(Alignment::Center);
    if changed {
        head = head.push(changed_dot());
    }
    head = head.push(kit_text::body(label.to_string()));
    container(
        row![container(head).width(Length::Fill), control]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 10.0]))
    .width(Length::Fill)
    .into()
}

/// The changed dot before a website or a field, or the room for it.
fn mark<'a>(changed: bool) -> Element<'a, Msg> {
    if changed {
        changed_dot()
    } else {
        Space::new().width(5).into()
    }
}

fn add_button<'a>(label: &'a str, msg: Msg) -> Element<'a, Msg> {
    button(
        row![widgets::muted("lucide/plus", 12.0), text(label).font(fonts::ui()).size(12)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([6.0, 8.0]))
    .style(kit_btn::ghost)
    .on_press(msg)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editors_newline_is_not_a_change() {
        let mut d = ItemDraft::blank(ItemKind::SecureNote);
        d.id = "x".into();
        d.name = "Note".into();
        d.notes = "one\ntwo".into();
        d.base = Some(Box::new(d.clone()));
        let mut form = EditForm::new(d);
        assert!(!form_dirty(&form));
        form.notes.perform(text_editor::Action::Edit(text_editor::Edit::Insert('!')));
        assert!(form_dirty(&form));
    }
}
