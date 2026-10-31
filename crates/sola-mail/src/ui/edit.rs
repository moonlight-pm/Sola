//! The Edit menu: Cut, Copy, Paste, Select All.
//!
//! A focused field (To, Cc, Subject, Search) takes them first, at its
//! caret (`text_input::edit_focused`: the shell grabs the Super chords,
//! so the field never sees the key). Otherwise they go to the draft's
//! body, or to the open letter.

use iced::Task;
use iced::widget::text_editor;
use sola_kit::components::text_input::{self, MenuEdit, MenuEdited};

use super::compose::{Edit, Field};
use super::{App, Msg, bar};

impl App {
    /// Run `edit` on the focused field; [`Msg::MenuEdited`] comes back.
    pub(super) fn menu_edit(&self, edit: MenuEdit) -> Task<Msg> {
        text_input::edit_focused(edit.clone())
            .map(move |done| Msg::MenuEdited(edit.clone(), done))
    }

    pub(super) fn edit_paste(&self) -> Task<Msg> {
        iced::clipboard::read().map(Msg::ClipboardPasted)
    }

    pub(super) fn on_menu_edited(&mut self, edit: MenuEdit, done: Option<MenuEdited>) -> Task<Msg> {
        let Some(done) = done else {
            // No field has focus: the body, or the open letter.
            return match edit {
                MenuEdit::Paste(t) => {
                    if let Some(c) = self.compose.as_mut() {
                        c.apply(Edit::Body(text_editor::Action::Edit(
                            text_editor::Edit::Paste(t.into()),
                        )));
                    }
                    Task::none()
                }
                MenuEdit::Copy => self.edit_copy(),
                MenuEdit::Cut => self.edit_cut(),
                MenuEdit::SelectAll => {
                    self.select_all();
                    Task::none()
                }
            };
        };
        let stored = match (done.id, done.value) {
            (Some(id), Some(value)) if id == bar::search_id() => {
                self.update(Msg::SearchChanged(value))
            }
            (Some(id), Some(value)) => match Field::from_id(&id) {
                Some(Field::To) => self.update(Msg::Draft(Edit::To(value))),
                Some(Field::Cc) => self.update(Msg::Draft(Edit::Cc(value))),
                Some(Field::Subject) => self.update(Msg::Draft(Edit::Subject(value))),
                Some(Field::Body) | None => Task::none(),
            },
            _ => Task::none(),
        };
        match done.copied {
            Some(t) if !t.is_empty() => Task::batch([stored, iced::clipboard::write(t)]),
            _ => stored,
        }
    }

    pub(super) fn copy_letter(&self) -> Task<Msg> {
        match self.reader.body().map(|b| b.display_text()) {
            Some(t) if !t.is_empty() => iced::clipboard::write(t),
            _ => Task::none(),
        }
    }

    fn edit_copy(&self) -> Task<Msg> {
        if let Some(c) = &self.compose {
            let t = c
                .draft
                .body
                .selection()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| c.draft.body.text());
            return if t.is_empty() {
                Task::none()
            } else {
                iced::clipboard::write(t)
            };
        }
        match self.reader.letter().and_then(|l| l.selection.clone()) {
            Some(sel) if !sel.is_empty() => iced::clipboard::write(sel),
            // No selection → copy the open letter (flatten includes URLs).
            _ => self.copy_letter(),
        }
    }

    fn edit_cut(&mut self) -> Task<Msg> {
        let Some(c) = self.compose.as_mut() else {
            return Task::none();
        };
        let Some(sel) = c.draft.body.selection().filter(|s| !s.is_empty()) else {
            return Task::none();
        };
        c.apply(Edit::Body(text_editor::Action::Edit(
            text_editor::Edit::Delete,
        )));
        iced::clipboard::write(sel)
    }

    fn select_all(&mut self) {
        if let Some(c) = self.compose.as_mut() {
            c.apply(Edit::Body(text_editor::Action::SelectAll));
        } else if let Some(l) = self.reader.letter_mut() {
            l.select_all += 1;
            let visible = sola_kit::components::prose::visible_text(&l.body.blocks);
            l.selection = (!visible.is_empty()).then_some(visible);
        }
    }
}
