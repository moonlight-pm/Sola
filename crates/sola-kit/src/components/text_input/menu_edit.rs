//! The Edit menu on the focused text input.
//!
//! The shell grabs an app menu's Super chords at the compositor (Super+V,
//! Super+C, …), so the input never sees the key; the app gets a
//! `MenuAction` instead. [`edit_focused`] applies that action where the
//! key would have: at the focused input's caret and selection, or for
//! Undo and Redo, through the input's own history (`history.rs`). The app
//! stores the new value as it would an `on_input` value.

use std::any::Any;

use iced::Task;
use iced_core::Rectangle;
use iced_core::widget::Id;
use iced_core::widget::operation::{Operation, Outcome};

use super::history::{History, Step};
use super::{Cursor, Value};

/// An Edit menu command.
#[derive(Debug, Clone)]
pub enum MenuEdit {
    /// The clipboard's text.
    Paste(String),
    Copy,
    Cut,
    SelectAll,
    /// Back one step in the input's history.
    Undo,
    /// Forward again after an Undo.
    Redo,
}

/// What [`edit_focused`] did to the focused input.
#[derive(Debug, Clone, Default)]
pub struct MenuEdited {
    pub id: Option<Id>,
    /// The input's new value (paste, cut, undo, redo).
    pub value: Option<String>,
    /// Text for the clipboard (copy, cut).
    pub copied: Option<String>,
}

/// Apply `edit` to the focused kit text input. `None`: no text input has
/// focus, so the edit is the app's own (a text editor, a selection).
pub fn edit_focused(edit: MenuEdit) -> Task<Option<MenuEdited>> {
    iced::advanced::widget::operate(EditFocused { edit, done: None })
}

/// The focused input as `operate` hands it to [`EditFocused`]; the
/// widget keeps the cursor afterwards.
pub(super) struct Focused {
    pub value: Value,
    pub cursor: Cursor,
    /// Has `on_input` (a read-only input only copies and selects).
    pub editable: bool,
    pub secure: bool,
    /// The input's history, lent for the operation.
    pub history: History,
}

impl Focused {
    fn apply(&mut self, edit: &MenuEdit, id: Option<&Id>) -> MenuEdited {
        let mut done = MenuEdited {
            id: id.cloned(),
            ..MenuEdited::default()
        };
        let selected = self
            .cursor
            .selection(&self.value)
            .map(|(start, end)| self.value.select(start, end).to_string());
        match edit {
            MenuEdit::Paste(text) if self.editable => {
                // As the key path: a field is one line.
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                done.value = Some(self.history.edit(
                    Step::Own,
                    &mut self.value,
                    &mut self.cursor,
                    |editor| editor.paste(Value::new(&text)),
                ));
            }
            MenuEdit::Copy if !self.secure => done.copied = selected,
            MenuEdit::Cut if self.editable && !self.secure && selected.is_some() => {
                done.copied = selected;
                done.value = Some(self.history.edit(
                    Step::Own,
                    &mut self.value,
                    &mut self.cursor,
                    |editor| editor.delete(),
                ));
            }
            MenuEdit::SelectAll => self.cursor.select_all(&self.value),
            MenuEdit::Undo if self.editable => {
                done.value = self.history.undo(&mut self.value, &mut self.cursor);
            }
            MenuEdit::Redo if self.editable => {
                done.value = self.history.redo(&mut self.value, &mut self.cursor);
            }
            _ => {}
        }
        done
    }
}

struct EditFocused {
    edit: MenuEdit,
    done: Option<MenuEdited>,
}

impl Operation<Option<MenuEdited>> for EditFocused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<MenuEdited>>)) {
        operate(self);
    }

    fn custom(&mut self, id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if self.done.is_none()
            && let Some(focused) = state.downcast_mut::<Focused>()
        {
            self.done = Some(focused.apply(&self.edit, id));
        }
    }

    fn finish(&self) -> Outcome<Option<MenuEdited>> {
        Outcome::Some(self.done.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focused(value: &str, start: usize, end: usize) -> Focused {
        let value = Value::new(value);
        let mut cursor = Cursor::default();
        cursor.select_range(start, end);
        Focused {
            value,
            cursor,
            editable: true,
            secure: false,
            history: History::default(),
        }
    }

    #[test]
    fn paste_lands_at_the_caret() {
        let mut f = focused("ann@a.com, ", 11, 11);
        let done = f.apply(&MenuEdit::Paste("bob@b.com\n".into()), None);
        assert_eq!(done.value.as_deref(), Some("ann@a.com, bob@b.com"));
    }

    #[test]
    fn paste_replaces_the_selection() {
        let mut f = focused("ann@a.com", 0, 3);
        let done = f.apply(&MenuEdit::Paste("joe".into()), None);
        assert_eq!(done.value.as_deref(), Some("joe@a.com"));
    }

    #[test]
    fn cut_takes_the_selection() {
        let mut f = focused("ann@a.com", 3, 9);
        let done = f.apply(&MenuEdit::Cut, None);
        assert_eq!(done.copied.as_deref(), Some("@a.com"));
        assert_eq!(done.value.as_deref(), Some("ann"));
    }

    #[test]
    fn a_secure_input_copies_nothing() {
        let mut f = focused("hunter2", 0, 7);
        f.secure = true;
        assert!(f.apply(&MenuEdit::Copy, None).copied.is_none());
    }

    #[test]
    fn undo_takes_back_a_paste_and_redo_puts_it_again() {
        let mut f = focused("ann@a.com", 9, 9);
        f.apply(&MenuEdit::Paste(", bob@b.com".into()), None);
        let done = f.apply(&MenuEdit::Undo, None);
        assert_eq!(done.value.as_deref(), Some("ann@a.com"));
        assert_eq!(f.apply(&MenuEdit::Undo, None).value, None);
        let done = f.apply(&MenuEdit::Redo, None);
        assert_eq!(done.value.as_deref(), Some("ann@a.com, bob@b.com"));
    }

    #[test]
    fn undo_takes_back_a_cut() {
        let mut f = focused("ann@a.com", 3, 9);
        f.apply(&MenuEdit::Cut, None);
        let done = f.apply(&MenuEdit::Undo, None);
        assert_eq!(done.value.as_deref(), Some("ann@a.com"));
        assert!(done.copied.is_none());
    }

    #[test]
    fn the_operation_undoes_with_the_history_the_input_lent() {
        let mut f = focused("ann", 3, 3);
        f.apply(&MenuEdit::Paste("@a.com".into()), None);
        let id = Id::new("to");
        let mut op = EditFocused {
            edit: MenuEdit::Undo,
            done: None,
        };
        op.custom(Some(&id), Rectangle::default(), &mut f);
        let Outcome::Some(Some(done)) = op.finish() else {
            panic!("the focused input answers");
        };
        assert_eq!(done.id, Some(id));
        assert_eq!(done.value.as_deref(), Some("ann"));
        // The input keeps the history back, Redo waiting in it.
        assert_eq!(f.apply(&MenuEdit::Redo, None).value.as_deref(), Some("ann@a.com"));
    }

    #[test]
    fn a_read_only_input_does_not_undo() {
        let mut f = focused("ann", 3, 3);
        f.apply(&MenuEdit::Paste("@a.com".into()), None);
        f.editable = false;
        assert_eq!(f.apply(&MenuEdit::Undo, None).value, None);
    }
}
