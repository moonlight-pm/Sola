//! A field's undo history: what it held before each edit, so Undo and
//! Redo put it back — Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y in the field (and
//! Super+Z / Super+Shift+Z when they reach it), or the Edit menu's
//! [`MenuEdit::Undo`](super::MenuEdit::Undo) and `Redo` through
//! [`edit_focused`](super::edit_focused).
//!
//! Typing is grouped into runs, and so are one-character Backspaces and
//! Deletes: a run ends after a pause, where a new word starts, or when the
//! caret has moved. Every other edit (a paste, a cut, a deleted selection,
//! kill to end, a desk-menu pick) is a step of its own. When the app puts
//! a value in the field that the field did not publish, the history no
//! longer describes it and is dropped. Each snapshot is zeroized when it
//! is dropped, so a password field keeps a history too.

use std::collections::VecDeque;
use std::fmt;

use iced_core::text;
use iced_core::time::{Duration, Instant};
use iced_core::{Layout, Shell};
use zeroize::{ZeroizeOnDrop, Zeroizing};

use super::editor::Editor;
use super::{Catalog, Cursor, State, TextInput, Value, replace_paragraph};

/// Steps kept for Undo.
const CAP: usize = 100;
/// A pause this long in a run of typing starts a new step.
const PAUSE: Duration = Duration::from_millis(1000);

/// The kind of edit being recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// Typed text (a key, an input method's commit): `true` when it is
    /// whitespace.
    Type(bool),
    /// A one-character Backspace or Delete.
    Erase,
    /// A step of its own: a paste, a cut, kill to end, a desk-menu pick.
    Own,
}

/// The field's text and caret at one point in its history.
#[derive(Clone)]
struct Snapshot {
    text: Zeroizing<String>,
    cursor: Cursor,
}

/// Its text is a `Zeroizing`.
impl ZeroizeOnDrop for Snapshot {}

/// The run of typing (or erasing) the last edit belongs to.
#[derive(Debug, Clone, Copy)]
struct Run {
    step: Step,
    at: Instant,
    /// The caret after the last edit of the run.
    cursor: Cursor,
}

impl Run {
    /// Whether a `step` with the caret at `cursor` at `now` joins the run.
    fn joined_by(&self, step: Step, cursor: Cursor, now: Instant) -> bool {
        let same = match (self.step, step) {
            // A word typed after whitespace starts a new step.
            (Step::Type(space), Step::Type(next)) => !space || next,
            (Step::Erase, Step::Erase) => true,
            _ => false,
        };
        same && self.cursor == cursor && now.saturating_duration_since(self.at) < PAUSE
    }
}

/// A field's undo and redo stacks.
#[derive(Default, Clone)]
pub(super) struct History {
    undo: VecDeque<Snapshot>,
    redo: VecDeque<Snapshot>,
    /// What the field holds after its last recorded edit, undo or redo.
    known: Option<Zeroizing<String>>,
    run: Option<Run>,
}

/// Its text is all in snapshots and a `Zeroizing`.
impl ZeroizeOnDrop for History {}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the text: it may be a password.
        f.debug_struct("History")
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .finish_non_exhaustive()
    }
}

impl History {
    /// Make one edit on `value` at `cursor` with `edit` and record it;
    /// the field's new text, to publish.
    pub(super) fn edit(
        &mut self,
        step: Step,
        value: &mut Value,
        cursor: &mut Cursor,
        edit: impl FnOnce(&mut Editor<'_>),
    ) -> String {
        self.edit_at(Instant::now(), step, value, cursor, edit)
    }

    fn edit_at(
        &mut self,
        now: Instant,
        step: Step,
        value: &mut Value,
        cursor: &mut Cursor,
        edit: impl FnOnce(&mut Editor<'_>),
    ) -> String {
        let before = Snapshot {
            text: Zeroizing::new(value.to_string()),
            cursor: *cursor,
        };
        let selection = cursor.selection(value).is_some();
        let mut editor = Editor::new(value, cursor);
        edit(&mut editor);
        let text = editor.contents();
        if text == *before.text {
            // Nothing changed (Backspace at the start): no step, and
            // Redo still has what it had.
            return text;
        }
        // A deleted selection is a step of its own, not part of a run.
        let step = match step {
            Step::Erase if selection => Step::Own,
            step => step,
        };
        let joins = !selection
            && self
                .run
                .is_some_and(|run| run.joined_by(step, before.cursor, now));
        if !joins {
            push(&mut self.undo, before);
        }
        self.redo.clear();
        self.run = (step != Step::Own).then_some(Run {
            step,
            at: now,
            cursor: *cursor,
        });
        self.known = Some(Zeroizing::new(text.clone()));
        text
    }

    /// Undo on `value` and `cursor` in place: the text to publish, or
    /// `None` with nothing to undo.
    pub(super) fn undo(&mut self, value: &mut Value, cursor: &mut Cursor) -> Option<String> {
        self.travel(false, value, cursor)
    }

    /// Redo on `value` and `cursor` in place: the text to publish, or
    /// `None` with nothing undone.
    pub(super) fn redo(&mut self, value: &mut Value, cursor: &mut Cursor) -> Option<String> {
        self.travel(true, value, cursor)
    }

    fn travel(&mut self, redo: bool, value: &mut Value, cursor: &mut Cursor) -> Option<String> {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let shown = from.pop_back()?;
        push(
            to,
            Snapshot {
                text: Zeroizing::new(value.to_string()),
                cursor: *cursor,
            },
        );
        *value = Value::new(&shown.text);
        *cursor = shown.cursor;
        self.run = None;
        self.known = Some(shown.text.clone());
        Some(shown.text.as_str().to_owned())
    }

    /// The app's value for the field, as its view gives it. One the field
    /// did not publish means the app replaced it (or this is another
    /// field in the same place): the history is of something else.
    pub(super) fn sync(&mut self, value: &Value) {
        if self.known.as_ref().is_some_and(|known| !value.eq_str(known)) {
            *self = Self::default();
        }
    }
}

/// Push `snapshot`, dropping the oldest past [`CAP`].
fn push(stack: &mut VecDeque<Snapshot>, snapshot: Snapshot) {
    if stack.len() == CAP {
        stack.pop_front();
    }
    stack.push_back(snapshot);
}

impl<Message, Theme, Renderer> TextInput<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    /// Ctrl+Z and the like in the field: undo (`redo`: redo) and publish
    /// what the field holds now, as typing would.
    pub(super) fn history_key(
        &mut self,
        state: &mut State<Renderer::Paragraph>,
        redo: bool,
        layout: Layout<'_>,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(on_input) = &self.on_input else {
            return;
        };
        let text = if redo {
            state.history.redo(&mut self.value, &mut state.cursor)
        } else {
            state.history.undo(&mut self.value, &mut state.cursor)
        };
        let Some(text) = text else {
            return;
        };
        shell.publish(on_input(text));
        replace_paragraph(
            renderer,
            state,
            layout,
            &self.value,
            self.font,
            self.size,
            self.line_height,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::text_input::cursor::State::Index;

    /// A field driven through its history, with a clock of its own.
    struct Field {
        value: Value,
        cursor: Cursor,
        history: History,
        now: Instant,
    }

    impl Field {
        fn new() -> Self {
            Self {
                value: Value::new(""),
                cursor: Cursor::default(),
                history: History::default(),
                now: Instant::now(),
            }
        }

        fn text(&self) -> String {
            self.value.to_string()
        }

        fn edit(&mut self, step: Step, edit: impl FnOnce(&mut Editor<'_>)) {
            self.now += Duration::from_millis(50);
            self.history
                .edit_at(self.now, step, &mut self.value, &mut self.cursor, edit);
        }

        fn type_str(&mut self, s: &str) {
            for c in s.chars() {
                self.edit(Step::Type(c.is_whitespace()), |e| e.insert(c));
            }
        }

        fn paste(&mut self, s: &str) {
            self.edit(Step::Own, |e| e.paste(Value::new(s)));
        }

        fn backspace(&mut self) {
            self.edit(Step::Erase, |e| e.backspace());
        }

        fn undo(&mut self) -> Option<String> {
            self.history.undo(&mut self.value, &mut self.cursor)
        }

        fn redo(&mut self) -> Option<String> {
            self.history.redo(&mut self.value, &mut self.cursor)
        }
    }

    #[test]
    fn a_run_of_typing_is_one_step() {
        let mut f = Field::new();
        f.type_str("hello");
        assert_eq!(f.undo().as_deref(), Some(""));
        assert_eq!(f.undo(), None);
    }

    #[test]
    fn a_new_word_is_a_new_step() {
        let mut f = Field::new();
        f.type_str("hello world");
        assert_eq!(f.undo().as_deref(), Some("hello "));
        assert_eq!(f.undo().as_deref(), Some(""));
    }

    #[test]
    fn a_pause_is_a_new_step() {
        let mut f = Field::new();
        f.type_str("ab");
        f.now += PAUSE;
        f.type_str("cd");
        assert_eq!(f.undo().as_deref(), Some("ab"));
    }

    #[test]
    fn a_caret_move_is_a_new_step() {
        let mut f = Field::new();
        f.type_str("ab");
        f.cursor.move_to(0);
        f.type_str("x");
        assert_eq!(f.text(), "xab");
        assert_eq!(f.undo().as_deref(), Some("ab"));
        assert_eq!(f.cursor.state(&f.value), Index(0));
        assert_eq!(f.undo().as_deref(), Some(""));
    }

    #[test]
    fn a_paste_is_a_step_of_its_own() {
        let mut f = Field::new();
        f.type_str("ab");
        f.paste("cd");
        f.type_str("e");
        assert_eq!(f.undo().as_deref(), Some("abcd"));
        assert_eq!(f.undo().as_deref(), Some("ab"));
        assert_eq!(f.undo().as_deref(), Some(""));
    }

    #[test]
    fn backspaces_group_but_a_deleted_selection_does_not() {
        let mut f = Field::new();
        f.type_str("abc");
        f.now += PAUSE;
        f.backspace();
        f.backspace();
        assert_eq!(f.text(), "a");
        f.type_str("xyz");
        f.cursor.select_range(1, 4);
        f.backspace();
        f.backspace();
        assert_eq!(f.text(), "");
        // The lone Backspace after the deleted selection, the selection,
        // the typing, the two Backspaces, the first typing.
        assert_eq!(f.undo().as_deref(), Some("a"));
        assert_eq!(f.undo().as_deref(), Some("axyz"));
        assert_eq!(f.undo().as_deref(), Some("a"));
        assert_eq!(f.undo().as_deref(), Some("abc"));
        assert_eq!(f.undo().as_deref(), Some(""));
    }

    #[test]
    fn redo_puts_back_what_undo_took() {
        let mut f = Field::new();
        f.type_str("ab");
        f.paste("cd");
        f.undo();
        f.undo();
        assert_eq!(f.redo().as_deref(), Some("ab"));
        assert_eq!(f.redo().as_deref(), Some("abcd"));
        assert_eq!(f.redo(), None);
        assert_eq!(f.cursor.state(&f.value), Index(4));
    }

    #[test]
    fn an_edit_clears_redo() {
        let mut f = Field::new();
        f.type_str("ab");
        f.undo();
        f.type_str("x");
        assert_eq!(f.redo(), None);
        assert_eq!(f.undo().as_deref(), Some(""));
    }

    #[test]
    fn an_edit_that_changes_nothing_keeps_redo() {
        let mut f = Field::new();
        f.type_str("ab");
        f.undo();
        // Backspace at the start of an empty field.
        f.backspace();
        assert_eq!(f.redo().as_deref(), Some("ab"));
    }

    #[test]
    fn the_history_is_capped() {
        let mut f = Field::new();
        for _ in 0..CAP + 20 {
            f.paste("x");
        }
        let mut undos = 0;
        while f.undo().is_some() {
            undos += 1;
        }
        assert_eq!(undos, CAP);
        assert_eq!(f.text(), "x".repeat(20));
    }

    #[test]
    fn a_value_from_outside_drops_the_history() {
        let mut f = Field::new();
        f.type_str("ab");
        // The app stored what the field published.
        f.history.sync(&Value::new("ab"));
        assert_eq!(f.history.undo.len(), 1);
        // The app replaced it.
        f.value = Value::new("zz");
        f.history.sync(&f.value);
        assert_eq!(f.undo(), None);
        assert_eq!(f.redo(), None);
    }

    #[test]
    fn an_undone_value_stored_by_the_app_keeps_the_history() {
        let mut f = Field::new();
        f.type_str("ab");
        f.paste("cd");
        f.undo();
        f.history.sync(&Value::new("ab"));
        assert_eq!(f.redo().as_deref(), Some("abcd"));
    }

    #[test]
    fn a_secret_is_wiped_and_never_printed() {
        fn wiped_on_drop<T: ZeroizeOnDrop>() {}
        wiped_on_drop::<Snapshot>();
        wiped_on_drop::<History>();

        let mut f = Field::new();
        f.type_str("hunter2");
        f.paste("!");
        assert!(!format!("{:?}", f.history).contains("hunter2"));
        assert_eq!(f.undo().as_deref(), Some("hunter2"));
    }
}
