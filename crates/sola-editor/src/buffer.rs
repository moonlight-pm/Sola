//! The text: its lines, edits to them, and their history.
//!
//! Lines hold no terminator. A [`Pos`] column is a byte offset in its line,
//! always on a character boundary. Every change is one [`Edit`], so undo is
//! the same splice in reverse.

/// A place in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// How an edit may join the one before it, so one undo takes back a word
/// of typing rather than a letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    None,
    Typing,
    DeleteBack,
    DeleteForward,
}

#[derive(Debug, Clone)]
struct Edit {
    id: u64,
    at: Pos,
    removed: String,
    inserted: String,
    /// Where the point was before and after, for undo and redo to put back.
    before: Pos,
    after: Pos,
    group: Group,
}

pub struct Buffer {
    lines: Vec<String>,
    crlf: bool,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    next_id: u64,
    /// The id of the last edit when the text was last saved (0: none).
    saved: u64,
    revision: u64,
    /// The first line changed since [`Self::take_changed`].
    changed: Option<usize>,
}

impl Buffer {
    /// `text` as loaded. No history: this is the saved state.
    pub fn from_text(text: &str) -> Self {
        let crlf = text.contains("\r\n");
        let lines = if crlf {
            text.replace("\r\n", "\n").split('\n').map(String::from).collect()
        } else {
            text.split('\n').map(String::from).collect()
        };
        Self {
            lines,
            crlf,
            undo: Vec::new(),
            redo: Vec::new(),
            next_id: 1,
            saved: 0,
            revision: 0,
            changed: None,
        }
    }

    /// The whole text, with the line ending it was loaded with.
    pub fn text(&self) -> String {
        self.lines.join(if self.crlf { "\r\n" } else { "\n" })
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map_or("", String::as_str)
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn end(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos::new(line, self.lines[line].len())
    }

    /// Bumps on every change to the text: what a drawing is keyed on.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// `pos` moved onto the text (after a reload shortened it, say).
    pub fn clamp(&self, pos: Pos) -> Pos {
        let line = pos.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        let mut col = pos.col.min(text.len());
        while !text.is_char_boundary(col) {
            col -= 1;
        }
        Pos::new(line, col)
    }

    /// The text between `a` and `b` (`a <= b`).
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        if a.line == b.line {
            return self.lines[a.line][a.col..b.col].to_string();
        }
        let mut out = self.lines[a.line][a.col..].to_string();
        for line in &self.lines[a.line + 1..b.line] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(&self.lines[b.line][..b.col]);
        out
    }

    /// Replace `a..b` with `text`, as one step of history. `point` is where
    /// the point was. Returns the end of what went in.
    pub fn replace(&mut self, a: Pos, b: Pos, text: &str, point: Pos, group: Group) -> Pos {
        let normalized;
        let text = if text.contains('\r') {
            normalized = text.replace("\r\n", "\n");
            normalized.as_str()
        } else {
            text
        };
        let (removed, after) = self.splice(a, b, text);
        self.redo.clear();
        if !self.join_last(a, &removed, text, after, group) {
            let id = self.next_id;
            self.next_id += 1;
            self.undo.push(Edit {
                id,
                at: a,
                removed,
                inserted: text.to_string(),
                before: point,
                after,
                group,
            });
        }
        after
    }

    /// Fold this edit into the last one where it continues it. Never into
    /// the edit the saved state names, so going back there stays exact.
    fn join_last(&mut self, a: Pos, removed: &str, text: &str, after: Pos, group: Group) -> bool {
        let saved = self.saved;
        let Some(last) = self.undo.last_mut() else {
            return false;
        };
        if last.id == saved || last.group != group {
            return false;
        }
        match group {
            Group::None => return false,
            Group::Typing => {
                if !removed.is_empty() || text.contains('\n') || a != last.after {
                    return false;
                }
                // A word boundary starts a new step.
                if text.starts_with(char::is_whitespace)
                    && !last.inserted.ends_with(char::is_whitespace)
                {
                    return false;
                }
                last.inserted.push_str(text);
            }
            Group::DeleteBack => {
                if removed.contains('\n') || end_of(a, removed) != last.at {
                    return false;
                }
                last.removed.insert_str(0, removed);
                last.at = a;
            }
            Group::DeleteForward => {
                if removed.contains('\n') || a != last.at {
                    return false;
                }
                last.removed.push_str(removed);
            }
        }
        last.after = after;
        true
    }

    /// Take back the last edit. Returns where the point goes.
    pub fn undo(&mut self) -> Option<Pos> {
        let edit = self.undo.pop()?;
        let end = end_of(edit.at, &edit.inserted);
        self.splice(edit.at, end, &edit.removed);
        let before = edit.before;
        self.redo.push(edit);
        Some(before)
    }

    /// Put back the edit last taken back. Returns where the point goes.
    pub fn redo(&mut self) -> Option<Pos> {
        let edit = self.redo.pop()?;
        let end = end_of(edit.at, &edit.removed);
        self.splice(edit.at, end, &edit.inserted);
        let after = edit.after;
        self.undo.push(edit);
        Some(after)
    }

    /// The text differs from what was last saved.
    pub fn is_dirty(&self) -> bool {
        self.undo.last().map_or(0, |e| e.id) != self.saved
    }

    /// The text as it stands is what's on disk.
    pub fn mark_saved(&mut self) {
        self.saved = self.undo.last().map_or(0, |e| e.id);
    }

    /// The first line changed since the last call, for the highlighter.
    pub fn take_changed(&mut self) -> Option<usize> {
        self.changed.take()
    }

    fn splice(&mut self, a: Pos, b: Pos, text: &str) -> (String, Pos) {
        let removed = self.slice(a, b);
        let head = self.lines[a.line][..a.col].to_string();
        let tail = self.lines[b.line][b.col..].to_string();
        let mut parts: Vec<String> = text.split('\n').map(String::from).collect();
        let last = parts.len() - 1;
        let after = Pos::new(
            a.line + last,
            if last == 0 { a.col + parts[0].len() } else { parts[last].len() },
        );
        parts[0].insert_str(0, &head);
        parts[last].push_str(&tail);
        self.lines.splice(a.line..=b.line, parts);
        self.revision += 1;
        self.changed = Some(self.changed.map_or(a.line, |c| c.min(a.line)));
        (removed, after)
    }
}

/// Where `text` ends when it starts at `at`.
fn end_of(at: Pos, text: &str) -> Pos {
    match text.rfind('\n') {
        Some(nl) => Pos::new(at.line + text.matches('\n').count(), text.len() - nl - 1),
        None => Pos::new(at.line, at.col + text.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, col: usize) -> Pos {
        Pos::new(line, col)
    }

    #[test]
    fn text_round_trips_with_its_line_ending_and_final_newline() {
        for text in ["", "a", "a\n", "a\nb", "a\nb\n\n"] {
            assert_eq!(Buffer::from_text(text).text(), text);
        }
        assert_eq!(Buffer::from_text("a\r\nb\r\n").text(), "a\r\nb\r\n");
        assert_eq!(Buffer::from_text("a\r\nb").line(0), "a");
    }

    #[test]
    fn replace_spans_lines_and_undo_redo_restore_them() {
        let mut b = Buffer::from_text("one\ntwo\nthree");
        let end = b.replace(p(0, 1), p(2, 2), "X\nY", p(0, 1), Group::None);
        assert_eq!(b.text(), "oX\nYree");
        assert_eq!(end, p(1, 1));
        assert_eq!(b.undo(), Some(p(0, 1)));
        assert_eq!(b.text(), "one\ntwo\nthree");
        assert_eq!(b.redo(), Some(p(1, 1)));
        assert_eq!(b.text(), "oX\nYree");
    }

    #[test]
    fn typing_joins_into_one_undo_up_to_a_word_boundary() {
        let mut b = Buffer::from_text("");
        let mut at = p(0, 0);
        for ch in ["a", "b", " ", "c"] {
            at = b.replace(at, at, ch, at, Group::Typing);
        }
        assert_eq!(b.text(), "ab c");
        b.undo();
        assert_eq!(b.text(), "ab");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn backspaces_join_and_undo_as_one() {
        let mut b = Buffer::from_text("abc");
        b.replace(p(0, 2), p(0, 3), "", p(0, 3), Group::DeleteBack);
        b.replace(p(0, 1), p(0, 2), "", p(0, 2), Group::DeleteBack);
        assert_eq!(b.text(), "a");
        assert_eq!(b.undo(), Some(p(0, 3)));
        assert_eq!(b.text(), "abc");
    }

    #[test]
    fn dirty_follows_undo_back_to_the_saved_state() {
        let mut b = Buffer::from_text("a");
        assert!(!b.is_dirty());
        b.replace(p(0, 1), p(0, 1), "b", p(0, 1), Group::Typing);
        assert!(b.is_dirty());
        b.mark_saved();
        assert!(!b.is_dirty());
        // Typing on does not fold into the saved edit.
        b.replace(p(0, 2), p(0, 2), "c", p(0, 2), Group::Typing);
        assert!(b.is_dirty());
        b.undo();
        assert_eq!(b.text(), "ab");
        assert!(!b.is_dirty());
        b.undo();
        assert_eq!(b.text(), "a");
        assert!(b.is_dirty());
    }

    #[test]
    fn clamp_lands_on_the_text_and_a_character_boundary() {
        let b = Buffer::from_text("é\nx");
        assert_eq!(b.clamp(p(0, 1)), p(0, 0));
        assert_eq!(b.clamp(p(9, 9)), p(1, 1));
    }

    #[test]
    fn take_changed_names_the_lowest_line() {
        let mut b = Buffer::from_text("a\nb\nc");
        b.replace(p(2, 0), p(2, 0), "x", p(2, 0), Group::None);
        b.replace(p(1, 0), p(1, 0), "y", p(1, 0), Group::None);
        assert_eq!(b.take_changed(), Some(1));
        assert_eq!(b.take_changed(), None);
    }
}
