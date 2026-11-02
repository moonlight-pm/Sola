//! The editing itself: a buffer, the point, the mark, and what each
//! [`Command`] does to them. No window here, so all of it can be tested.
//!
//! Three ways to have a region, as in Zed: C-space sets the mark and moves
//! extend it until C-g; Shift extends it for as long as Shift is held; a
//! mouse drag makes one. A plain move drops the last two.

use std::path::Path;

use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::{Buffer, Group, Pos};
use crate::command::{Command, Motion};
use crate::grid;
use crate::lang;

/// What a command wants done outside the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    None,
    Clipboard(String),
    Note(String),
}

pub struct Editor {
    pub buf: Buffer,
    pub point: Pos,
    pub mark: Option<Pos>,
    /// C-space is down: moves extend the region until C-g.
    pub mark_mode: bool,
    pub read_only: bool,
    pub tab_size: usize,
    pub hard_tabs: bool,
    comment: Option<&'static str>,
    /// The screen column up and down keep to.
    goal: Option<usize>,
    /// What consecutive C-k's have killed so far.
    kill: Option<String>,
}

impl Editor {
    pub fn new(text: &str, path: Option<&Path>) -> Self {
        let buf = Buffer::from_text(text);
        let hard_tabs = buf.lines().iter().take(4096).any(|l| l.starts_with('\t'));
        Self {
            buf,
            point: Pos::default(),
            mark: None,
            mark_mode: false,
            read_only: false,
            tab_size: lang::tab_size(path),
            hard_tabs,
            comment: lang::comment(path),
            goal: None,
            kill: None,
        }
    }

    /// The file's name changed (Save As).
    pub fn set_path(&mut self, path: &Path) {
        self.tab_size = lang::tab_size(Some(path));
        self.comment = lang::comment(Some(path));
    }

    /// The region, in order, if it holds anything.
    pub fn region(&self) -> Option<(Pos, Pos)> {
        let mark = self.mark?;
        match mark.cmp(&self.point) {
            std::cmp::Ordering::Less => Some((mark, self.point)),
            std::cmp::Ordering::Greater => Some((self.point, mark)),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// The screen column of the point.
    pub fn point_column(&self) -> usize {
        grid::display_col(self.buf.line(self.point.line), self.point.col, self.tab_size)
    }

    pub fn apply(&mut self, command: Command, page: usize) -> Out {
        if self.read_only && command.edits() {
            return Out::Note("This file is read-only".into());
        }
        if command != Command::KillLine {
            self.kill = None;
        }
        let vertical = matches!(
            command,
            Command::Move(Motion::Down | Motion::Up | Motion::PageDown | Motion::PageUp)
                | Command::Select(Motion::Down | Motion::Up | Motion::PageDown | Motion::PageUp)
        );
        if !vertical {
            self.goal = None;
        }
        match command {
            Command::Move(motion) => {
                let to = self.target(motion, page);
                self.go(to, false);
            }
            Command::Select(motion) => {
                let to = self.target(motion, page);
                self.go(to, true);
            }
            Command::SetMark => {
                self.mark = Some(self.point);
                self.mark_mode = true;
                return Out::Note("Mark set".into());
            }
            Command::Cancel => self.drop_mark(),
            Command::SwapPointAndMark => match self.mark {
                Some(mark) => {
                    self.mark = Some(self.point);
                    self.point = mark;
                }
                None => return Out::Note("No mark set".into()),
            },
            Command::SelectAll => {
                self.mark = Some(Pos::default());
                self.point = self.buf.end();
                self.mark_mode = false;
            }
            Command::Newline => self.newline(),
            Command::Tab => self.tab(),
            Command::Dedent => self.dedent(),
            Command::DeleteBackward => self.delete_backward(),
            Command::DeleteForward => self.delete_forward(),
            Command::KillWord => {
                let to = self.word_forward(self.point);
                self.delete_region_or(self.point, to);
            }
            Command::BackwardKillWord => {
                let from = self.word_backward(self.point);
                self.delete_region_or(from, self.point);
            }
            Command::KillLine => return self.kill_line(),
            Command::Cut => {
                let (a, b) = self.region_or_line();
                let text = self.buf.slice(a, b);
                self.point = self.buf.replace(a, b, "", self.point, Group::None);
                self.drop_mark();
                return Out::Clipboard(text);
            }
            Command::Copy => {
                let (a, b) = self.region_or_line();
                let text = self.buf.slice(a, b);
                self.drop_mark();
                return Out::Clipboard(text);
            }
            Command::Transpose => self.transpose(),
            Command::ToggleComment => return self.toggle_comment(),
            Command::Undo => match self.buf.undo() {
                Some(at) => self.settle_at(at),
                None => return Out::Note("Nothing to undo".into()),
            },
            Command::Redo => match self.buf.redo() {
                Some(at) => self.settle_at(at),
                None => return Out::Note("Nothing to redo".into()),
            },
            // The app's to do.
            Command::Center
            | Command::Yank
            | Command::Find
            | Command::GotoLine
            | Command::Palette
            | Command::OpenFile
            | Command::Save
            | Command::SaveAs
            | Command::CloseFile
            | Command::Quit => {}
        }
        Out::None
    }

    /// Typed or pasted text, over the region if there is one.
    pub fn insert(&mut self, text: &str, group: Group) -> Out {
        if self.read_only {
            return Out::Note("This file is read-only".into());
        }
        self.goal = None;
        self.kill = None;
        let (a, b) = self.region().unwrap_or((self.point, self.point));
        // Over a region it is its own step of history.
        let group = if a == b { group } else { Group::None };
        self.point = self.buf.replace(a, b, text, self.point, group);
        self.drop_mark();
        Out::None
    }

    /// A click: the point goes there, the region goes away.
    pub fn click(&mut self, at: Pos) {
        self.goal = None;
        self.kill = None;
        self.point = self.buf.clamp(at);
        self.drop_mark();
    }

    /// A Shift-click: the region runs from here to there.
    pub fn extend_to(&mut self, at: Pos) {
        self.goal = None;
        self.kill = None;
        if self.mark.is_none() {
            self.mark = Some(self.point);
        }
        self.point = self.buf.clamp(at);
    }

    /// A double click: the word under `at`.
    pub fn select_word(&mut self, at: Pos) {
        let at = self.buf.clamp(at);
        let line = self.buf.line(at.line);
        let mut start = at.col;
        let mut end = at.col;
        for (i, word) in line.split_word_bound_indices() {
            if (i..i + word.len()).contains(&at.col) {
                start = i;
                end = i + word.len();
                break;
            }
        }
        self.mark = Some(Pos::new(at.line, start));
        self.point = Pos::new(at.line, end);
        self.mark_mode = false;
        self.goal = None;
    }

    /// A triple click: the line under `at`.
    pub fn select_line(&mut self, at: Pos) {
        let line = at.line.min(self.buf.line_count() - 1);
        self.mark = Some(Pos::new(line, 0));
        self.point = if line + 1 < self.buf.line_count() {
            Pos::new(line + 1, 0)
        } else {
            self.buf.end()
        };
        self.mark_mode = false;
        self.goal = None;
    }

    /// Select `a..b`: a match, while finding.
    pub fn select(&mut self, a: Pos, b: Pos) {
        self.mark = Some(a);
        self.point = b;
        self.mark_mode = false;
        self.goal = None;
    }

    /// The text changed under us (a reload): keep the place as best we can.
    pub fn replace_all(&mut self, text: &str) {
        let point = self.point;
        let (start, end) = (Pos::default(), self.buf.end());
        self.buf.replace(start, end, text, point, Group::None);
        self.point = self.buf.clamp(point);
        self.drop_mark();
    }

    /// The point, as after [`Self::click`], without the bookkeeping of a move.
    pub fn set_point(&mut self, at: Pos) {
        self.point = self.buf.clamp(at);
        self.drop_mark();
        self.goal = None;
    }

    fn settle_at(&mut self, at: Pos) {
        self.point = self.buf.clamp(at);
        self.drop_mark();
    }

    fn drop_mark(&mut self) {
        self.mark = None;
        self.mark_mode = false;
    }

    fn go(&mut self, to: Pos, shift: bool) {
        if self.mark_mode {
            // The mark stays where C-space put it.
        } else if shift {
            if self.mark.is_none() {
                self.mark = Some(self.point);
            }
        } else {
            self.mark = None;
        }
        self.point = to;
    }

    fn target(&mut self, motion: Motion, page: usize) -> Pos {
        let p = self.point;
        match motion {
            Motion::Forward => self.char_forward(p),
            Motion::Backward => self.char_backward(p),
            Motion::Down => self.vertical(1),
            Motion::Up => self.vertical(-1),
            Motion::PageDown => self.vertical(page.max(1) as isize),
            Motion::PageUp => self.vertical(-(page.max(1) as isize)),
            Motion::LineStart => Pos::new(p.line, 0),
            Motion::LineEnd => Pos::new(p.line, self.buf.line(p.line).len()),
            Motion::IndentStart => {
                let line = self.buf.line(p.line);
                Pos::new(p.line, line.len() - line.trim_start().len())
            }
            Motion::WordForward => self.word_forward(p),
            Motion::WordBackward => self.word_backward(p),
            Motion::ParagraphForward => self.paragraph_forward(p),
            Motion::ParagraphBackward => self.paragraph_backward(p),
            Motion::BufferStart => Pos::default(),
            Motion::BufferEnd => self.buf.end(),
        }
    }

    fn vertical(&mut self, delta: isize) -> Pos {
        let goal = match self.goal {
            Some(goal) => goal,
            None => self.point_column(),
        };
        self.goal = Some(goal);
        let last = self.buf.line_count() - 1;
        let wanted = self.point.line as isize + delta;
        // Off either end the point goes to that end of the line.
        if wanted < 0 {
            return Pos::default();
        }
        if wanted as usize > last {
            return self.buf.end();
        }
        let line = wanted as usize;
        Pos::new(line, grid::col_at_display(self.buf.line(line), goal, self.tab_size))
    }

    fn char_forward(&self, p: Pos) -> Pos {
        let line = self.buf.line(p.line);
        match line[p.col..].graphemes(true).next() {
            Some(g) => Pos::new(p.line, p.col + g.len()),
            None if p.line + 1 < self.buf.line_count() => Pos::new(p.line + 1, 0),
            None => p,
        }
    }

    fn char_backward(&self, p: Pos) -> Pos {
        let line = self.buf.line(p.line);
        match line[..p.col].graphemes(true).next_back() {
            Some(g) => Pos::new(p.line, p.col - g.len()),
            None if p.line > 0 => Pos::new(p.line - 1, self.buf.line(p.line - 1).len()),
            None => p,
        }
    }

    /// The character after `p` (`\n` at a line's end), and the place past it.
    fn next_char(&self, p: Pos) -> Option<(char, Pos)> {
        let line = self.buf.line(p.line);
        match line[p.col..].chars().next() {
            Some(c) => Some((c, Pos::new(p.line, p.col + c.len_utf8()))),
            None if p.line + 1 < self.buf.line_count() => Some(('\n', Pos::new(p.line + 1, 0))),
            None => None,
        }
    }

    /// The character before `p`, and the place before it.
    fn prev_char(&self, p: Pos) -> Option<(char, Pos)> {
        let line = self.buf.line(p.line);
        match line[..p.col].chars().next_back() {
            Some(c) => Some((c, Pos::new(p.line, p.col - c.len_utf8()))),
            None if p.line > 0 => {
                Some(('\n', Pos::new(p.line - 1, self.buf.line(p.line - 1).len())))
            }
            None => None,
        }
    }

    fn word_forward(&self, mut p: Pos) -> Pos {
        while let Some((c, next)) = self.next_char(p) {
            if is_word(c) {
                break;
            }
            p = next;
        }
        while let Some((c, next)) = self.next_char(p) {
            if !is_word(c) {
                break;
            }
            p = next;
        }
        p
    }

    fn word_backward(&self, mut p: Pos) -> Pos {
        while let Some((c, prev)) = self.prev_char(p) {
            if is_word(c) {
                break;
            }
            p = prev;
        }
        while let Some((c, prev)) = self.prev_char(p) {
            if !is_word(c) {
                break;
            }
            p = prev;
        }
        p
    }

    fn is_blank(&self, line: usize) -> bool {
        self.buf.line(line).trim().is_empty()
    }

    fn paragraph_forward(&self, p: Pos) -> Pos {
        let count = self.buf.line_count();
        let mut i = p.line + 1;
        while i < count && self.is_blank(i) {
            i += 1;
        }
        while i < count && !self.is_blank(i) {
            i += 1;
        }
        if i < count { Pos::new(i, 0) } else { self.buf.end() }
    }

    fn paragraph_backward(&self, p: Pos) -> Pos {
        let mut i = p.line;
        while i > 0 && self.is_blank(i - 1) {
            i -= 1;
        }
        while i > 0 && !self.is_blank(i - 1) {
            i -= 1;
        }
        if i > 0 { Pos::new(i - 1, 0) } else { Pos::default() }
    }

    /// The region, or the whole line with its newline: C-w and M-w with no
    /// region take the line, as Zed's do.
    fn region_or_line(&self) -> (Pos, Pos) {
        self.region().unwrap_or_else(|| {
            let line = self.point.line;
            if line + 1 < self.buf.line_count() {
                (Pos::new(line, 0), Pos::new(line + 1, 0))
            } else {
                (Pos::new(line, 0), self.buf.end())
            }
        })
    }

    fn delete_region_or(&mut self, a: Pos, b: Pos) {
        let (a, b) = self.region().unwrap_or((a, b));
        self.point = self.buf.replace(a, b, "", self.point, Group::None);
        self.drop_mark();
    }

    fn newline(&mut self) {
        let line = self.buf.line(self.point.line);
        let upto = &line[..self.point.col];
        let indent = &upto[..upto.len() - upto.trim_start().len()];
        let text = format!("\n{indent}");
        self.insert(&text, Group::None);
    }

    fn tab(&mut self) {
        if self.region().is_some() {
            let unit = if self.hard_tabs { "\t".to_string() } else { " ".repeat(self.tab_size) };
            let lines = self.selected_lines();
            self.rewrite_lines(lines, |line| {
                if line.is_empty() { Change::Keep } else { Change::Insert(0, unit.clone()) }
            });
            return;
        }
        let text = if self.hard_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.tab_size - self.point_column() % self.tab_size)
        };
        self.insert(&text, Group::None);
    }

    fn dedent(&mut self) {
        let tab = self.tab_size;
        let lines = self.selected_lines();
        self.rewrite_lines(lines, |line| {
            if line.starts_with('\t') {
                return Change::Remove(0, 1);
            }
            let spaces = line.len() - line.trim_start_matches(' ').len();
            if spaces == 0 { Change::Keep } else { Change::Remove(0, spaces.min(tab)) }
        });
    }

    fn delete_backward(&mut self) {
        if let Some((a, b)) = self.region() {
            self.point = self.buf.replace(a, b, "", self.point, Group::None);
            self.drop_mark();
            return;
        }
        let p = self.point;
        let line = self.buf.line(p.line);
        // In the indent, back to the last tab stop.
        let from = if p.col > 0 && !self.hard_tabs && line[..p.col].bytes().all(|b| b == b' ') {
            Pos::new(p.line, (p.col - 1) / self.tab_size * self.tab_size)
        } else {
            self.char_backward(p)
        };
        if from != p {
            self.point = from;
            self.buf.replace(from, p, "", p, Group::DeleteBack);
        }
    }

    fn delete_forward(&mut self) {
        if let Some((a, b)) = self.region() {
            self.point = self.buf.replace(a, b, "", self.point, Group::None);
            self.drop_mark();
            return;
        }
        let p = self.point;
        let to = self.char_forward(p);
        if to != p {
            self.buf.replace(p, to, "", p, Group::DeleteForward);
        }
    }

    /// C-k: to the line's end, or the newline when there's nothing before
    /// it. Runs of C-k add up on the clipboard, so C-k C-k C-k C-y moves
    /// three lines.
    fn kill_line(&mut self) -> Out {
        let p = self.point;
        let (a, b) = match self.region() {
            Some(region) => region,
            None => {
                let end = self.buf.line(p.line).len();
                if p.col < end {
                    (p, Pos::new(p.line, end))
                } else if p.line + 1 < self.buf.line_count() {
                    (p, Pos::new(p.line + 1, 0))
                } else {
                    return Out::Note("End of file".into());
                }
            }
        };
        let killed = self.buf.slice(a, b);
        self.point = self.buf.replace(a, b, "", p, Group::None);
        self.drop_mark();
        let mut all = self.kill.take().unwrap_or_default();
        all.push_str(&killed);
        self.kill = Some(all.clone());
        Out::Clipboard(all)
    }

    /// C-t: swap the characters around the point and move on; at a line's
    /// end, the two before it.
    fn transpose(&mut self) {
        let p = self.point;
        let line = self.buf.line(p.line);
        let at_end = p.col == line.len();
        let middle = if at_end { self.char_backward(p) } else { p };
        if middle.line != p.line || middle.col == 0 {
            return;
        }
        let start = Pos::new(p.line, middle.col - line[..middle.col].graphemes(true).next_back().map_or(0, str::len));
        let end = Pos::new(p.line, middle.col + line[middle.col..].graphemes(true).next().map_or(0, str::len));
        let swapped = format!("{}{}", &line[middle.col..end.col], &line[start.col..middle.col]);
        self.point = self.buf.replace(start, end, &swapped, p, Group::None);
        self.drop_mark();
    }

    fn toggle_comment(&mut self) -> Out {
        let Some(token) = self.comment else {
            return Out::Note("No comment syntax for this file".into());
        };
        let (first, last) = self.selected_lines();
        let lines: Vec<&str> = (first..=last).map(|i| self.buf.line(i)).collect();
        let filled: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        if filled.is_empty() {
            return Out::None;
        }
        let commented = filled.iter().all(|l| l.trim_start().starts_with(token));
        let indent = filled.iter().map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
        self.rewrite_lines((first, last), |line| {
            if line.trim().is_empty() {
                return Change::Keep;
            }
            if commented {
                let at = line.len() - line.trim_start().len();
                let after = &line[at + token.len()..];
                let extra = usize::from(after.starts_with(' '));
                Change::Remove(at, token.len() + extra)
            } else {
                Change::Insert(indent, format!("{token} "))
            }
        });
        Out::None
    }

    /// The lines the region touches, or the point's. A region ending at a
    /// line's start doesn't count that line.
    fn selected_lines(&self) -> (usize, usize) {
        match self.region() {
            Some((a, b)) if b.col == 0 && b.line > a.line => (a.line, b.line - 1),
            Some((a, b)) => (a.line, b.line),
            None => (self.point.line, self.point.line),
        }
    }

    /// Change `first..=last` line by line as one undo, moving the point and
    /// mark along with the text they sit on.
    fn rewrite_lines(&mut self, (first, last): (usize, usize), change: impl Fn(&str) -> Change) {
        let mut point = self.point;
        let mut mark = self.mark;
        let mut out: Vec<String> = Vec::with_capacity(last - first + 1);
        for i in first..=last {
            let line = self.buf.line(i);
            let c = change(line);
            let new = c.apply(line);
            if point.line == i {
                point.col = c.moved(point.col);
            }
            if let Some(m) = mark.as_mut().filter(|m| m.line == i) {
                m.col = c.moved(m.col);
            }
            out.push(new);
        }
        let a = Pos::new(first, 0);
        let b = Pos::new(last, self.buf.line(last).len());
        self.buf.replace(a, b, &out.join("\n"), self.point, Group::None);
        self.point = point;
        self.mark = mark;
    }
}

/// One line's change in [`Editor::rewrite_lines`].
enum Change {
    Keep,
    Insert(usize, String),
    Remove(usize, usize),
}

impl Change {
    fn apply(&self, line: &str) -> String {
        match self {
            Change::Keep => line.to_string(),
            Change::Insert(at, text) => format!("{}{}{}", &line[..*at], text, &line[*at..]),
            Change::Remove(at, n) => format!("{}{}", &line[..*at], &line[*at + *n..]),
        }
    }

    /// Where column `col` is after this change.
    fn moved(&self, col: usize) -> usize {
        match self {
            Change::Keep => col,
            // A place right where it goes in stays before it.
            Change::Insert(at, text) if col > *at => col + text.len(),
            Change::Insert(..) => col,
            Change::Remove(at, n) if col >= *at + *n => col - *n,
            Change::Remove(at, _) if col > *at => *at,
            Change::Remove(..) => col,
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command as C;
    use crate::command::Motion as M;

    fn at(text: &str, line: usize, col: usize) -> Editor {
        let mut ed = Editor::new(text, Some(Path::new("a.rs")));
        ed.point = Pos::new(line, col);
        ed
    }

    fn p(line: usize, col: usize) -> Pos {
        Pos::new(line, col)
    }

    #[test]
    fn char_moves_cross_lines_and_stop_at_the_ends() {
        let mut ed = at("ab\nc", 0, 2);
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.point, p(1, 0));
        ed.apply(C::Move(M::Backward), 10);
        assert_eq!(ed.point, p(0, 2));
        let mut ed = at("a", 0, 1);
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.point, p(0, 1));
    }

    #[test]
    fn up_and_down_keep_their_column_past_short_lines() {
        let mut ed = at("long line\nx\nlong line", 0, 6);
        ed.apply(C::Move(M::Down), 10);
        assert_eq!(ed.point, p(1, 1));
        ed.apply(C::Move(M::Down), 10);
        assert_eq!(ed.point, p(2, 6));
        // Any other move forgets the column.
        ed.apply(C::Move(M::LineStart), 10);
        ed.apply(C::Move(M::Up), 10);
        assert_eq!(ed.point, p(1, 0));
    }

    #[test]
    fn up_and_down_go_to_the_ends_of_the_first_and_last_lines() {
        let mut ed = at("ab\ncd", 0, 1);
        ed.apply(C::Move(M::Up), 10);
        assert_eq!(ed.point, p(0, 0));
        ed.point = p(1, 1);
        ed.apply(C::Move(M::Down), 10);
        assert_eq!(ed.point, p(1, 2));
    }

    #[test]
    fn line_motions() {
        let mut ed = at("    let x", 0, 6);
        ed.apply(C::Move(M::LineStart), 10);
        assert_eq!(ed.point, p(0, 0));
        ed.apply(C::Move(M::LineEnd), 10);
        assert_eq!(ed.point, p(0, 9));
        ed.apply(C::Move(M::IndentStart), 10);
        assert_eq!(ed.point, p(0, 4));
    }

    #[test]
    fn word_motions_skip_what_is_between_words() {
        let mut ed = at("foo, bar_baz\n  qux", 0, 0);
        ed.apply(C::Move(M::WordForward), 10);
        assert_eq!(ed.point, p(0, 3));
        ed.apply(C::Move(M::WordForward), 10);
        assert_eq!(ed.point, p(0, 12));
        ed.apply(C::Move(M::WordForward), 10);
        assert_eq!(ed.point, p(1, 5));
        ed.apply(C::Move(M::WordBackward), 10);
        assert_eq!(ed.point, p(1, 2));
        ed.apply(C::Move(M::WordBackward), 10);
        assert_eq!(ed.point, p(0, 5));
    }

    #[test]
    fn paragraphs_stop_at_blank_lines() {
        let mut ed = at("a\nb\n\n\nc\nd", 0, 0);
        ed.apply(C::Move(M::ParagraphForward), 10);
        assert_eq!(ed.point, p(2, 0));
        ed.apply(C::Move(M::ParagraphForward), 10);
        assert_eq!(ed.point, p(5, 1));
        ed.apply(C::Move(M::ParagraphBackward), 10);
        assert_eq!(ed.point, p(3, 0));
        ed.apply(C::Move(M::ParagraphBackward), 10);
        assert_eq!(ed.point, p(0, 0));
    }

    #[test]
    fn set_mark_makes_moves_extend_until_c_g() {
        let mut ed = at("hello world", 0, 0);
        ed.apply(C::SetMark, 10);
        ed.apply(C::Move(M::WordForward), 10);
        assert_eq!(ed.region(), Some((p(0, 0), p(0, 5))));
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.region(), Some((p(0, 0), p(0, 6))));
        ed.apply(C::Cancel, 10);
        assert_eq!(ed.region(), None);
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.region(), None);
    }

    #[test]
    fn shift_extends_and_a_plain_move_drops_it() {
        let mut ed = at("hello", 0, 1);
        ed.apply(C::Select(M::Forward), 10);
        ed.apply(C::Select(M::Forward), 10);
        assert_eq!(ed.region(), Some((p(0, 1), p(0, 3))));
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.region(), None);
    }

    #[test]
    fn swap_point_and_mark() {
        let mut ed = at("hello", 0, 0);
        assert_eq!(ed.apply(C::SwapPointAndMark, 10), Out::Note("No mark set".into()));
        ed.apply(C::SetMark, 10);
        ed.apply(C::Move(M::LineEnd), 10);
        ed.apply(C::SwapPointAndMark, 10);
        assert_eq!(ed.point, p(0, 0));
        assert_eq!(ed.mark, Some(p(0, 5)));
        assert_eq!(ed.region(), Some((p(0, 0), p(0, 5))));
    }

    #[test]
    fn select_all_then_a_move_drops_it() {
        let mut ed = at("a\nb", 0, 0);
        ed.apply(C::SelectAll, 10);
        assert_eq!(ed.region(), Some((p(0, 0), p(1, 1))));
        ed.apply(C::Move(M::Backward), 10);
        assert_eq!(ed.region(), None);
    }

    #[test]
    fn kill_line_takes_the_rest_then_the_newline_and_adds_up() {
        let mut ed = at("one\ntwo", 0, 1);
        assert_eq!(ed.apply(C::KillLine, 10), Out::Clipboard("ne".into()));
        assert_eq!(ed.buf.text(), "o\ntwo");
        assert_eq!(ed.apply(C::KillLine, 10), Out::Clipboard("ne\n".into()));
        assert_eq!(ed.buf.text(), "otwo");
        assert_eq!(ed.apply(C::KillLine, 10), Out::Clipboard("ne\ntwo".into()));
        assert_eq!(ed.buf.text(), "o");
        assert_eq!(ed.apply(C::KillLine, 10), Out::Note("End of file".into()));
    }

    #[test]
    fn another_command_starts_a_new_kill() {
        let mut ed = at("one\ntwo", 0, 0);
        ed.apply(C::KillLine, 10);
        ed.apply(C::Move(M::Down), 10);
        ed.apply(C::Move(M::LineStart), 10);
        assert_eq!(ed.apply(C::KillLine, 10), Out::Clipboard("two".into()));
    }

    #[test]
    fn cut_and_copy_take_the_region_or_the_whole_line() {
        let mut ed = at("one\ntwo", 0, 1);
        assert_eq!(ed.apply(C::Copy, 10), Out::Clipboard("one\n".into()));
        assert_eq!(ed.buf.text(), "one\ntwo");
        assert_eq!(ed.apply(C::Cut, 10), Out::Clipboard("one\n".into()));
        assert_eq!(ed.buf.text(), "two");
        assert_eq!(ed.point, p(0, 0));
        ed.apply(C::SetMark, 10);
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.apply(C::Cut, 10), Out::Clipboard("t".into()));
        assert_eq!(ed.buf.text(), "wo");
    }

    #[test]
    fn copy_drops_the_mark_as_s_w_did() {
        let mut ed = at("hello", 0, 0);
        ed.apply(C::SetMark, 10);
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.apply(C::Copy, 10), Out::Clipboard("h".into()));
        assert_eq!(ed.mark, None);
        assert!(!ed.mark_mode);
    }

    #[test]
    fn word_kills_leave_the_clipboard_alone() {
        let mut ed = at("foo bar", 0, 0);
        assert_eq!(ed.apply(C::KillWord, 10), Out::None);
        assert_eq!(ed.buf.text(), " bar");
        ed.point = p(0, 4);
        assert_eq!(ed.apply(C::BackwardKillWord, 10), Out::None);
        assert_eq!(ed.buf.text(), " ");
    }

    #[test]
    fn typing_replaces_the_region() {
        let mut ed = at("hello", 0, 0);
        ed.apply(C::SelectAll, 10);
        ed.insert("x", Group::Typing);
        assert_eq!(ed.buf.text(), "x");
        assert_eq!(ed.point, p(0, 1));
        assert_eq!(ed.region(), None);
    }

    #[test]
    fn newline_keeps_the_indent() {
        let mut ed = at("    foo", 0, 7);
        ed.apply(C::Newline, 10);
        assert_eq!(ed.buf.text(), "    foo\n    ");
        assert_eq!(ed.point, p(1, 4));
    }

    #[test]
    fn backspace_in_the_indent_goes_back_a_stop() {
        let mut ed = at("      x", 0, 6);
        ed.apply(C::DeleteBackward, 10);
        assert_eq!(ed.buf.text(), "    x");
        assert_eq!(ed.point, p(0, 4));
    }

    #[test]
    fn backspace_and_delete_join_lines() {
        let mut ed = at("ab\ncd", 1, 0);
        ed.apply(C::DeleteBackward, 10);
        assert_eq!(ed.buf.text(), "abcd");
        assert_eq!(ed.point, p(0, 2));
        let mut ed = at("ab\ncd", 0, 2);
        ed.apply(C::DeleteForward, 10);
        assert_eq!(ed.buf.text(), "abcd");
        assert_eq!(ed.point, p(0, 2));
    }

    #[test]
    fn a_grapheme_goes_whole() {
        let mut ed = at("e\u{301}x", 0, 3);
        ed.apply(C::DeleteBackward, 10);
        assert_eq!(ed.buf.text(), "x");
        let mut ed = at("e\u{301}x", 0, 0);
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.point, p(0, 3));
    }

    #[test]
    fn tab_goes_to_the_next_stop_and_indents_a_region() {
        let mut ed = at("ab", 0, 1);
        ed.apply(C::Tab, 10);
        assert_eq!(ed.buf.text(), "a   b");
        let mut ed = at("a\n\nb", 0, 0);
        ed.apply(C::SelectAll, 10);
        ed.apply(C::Tab, 10);
        assert_eq!(ed.buf.text(), "    a\n\n    b");
        assert_eq!(ed.region(), Some((p(0, 0), p(2, 5))));
        ed.apply(C::Undo, 10);
        assert_eq!(ed.buf.text(), "a\n\nb");
    }

    #[test]
    fn a_file_indented_with_tabs_keeps_them() {
        let mut ed = Editor::new("\tx", Some(Path::new("a.go")));
        ed.apply(C::Tab, 10);
        assert_eq!(ed.buf.text(), "\t\tx");
    }

    #[test]
    fn dedent_takes_one_indent_from_each_line() {
        let mut ed = at("      a\n\tb\nc", 0, 6);
        ed.apply(C::SelectAll, 10);
        ed.apply(C::Dedent, 10);
        assert_eq!(ed.buf.text(), "  a\nb\nc");
    }

    #[test]
    fn transpose() {
        let mut ed = at("abc", 0, 1);
        ed.apply(C::Transpose, 10);
        assert_eq!(ed.buf.text(), "bac");
        assert_eq!(ed.point, p(0, 2));
        let mut ed = at("abc", 0, 3);
        ed.apply(C::Transpose, 10);
        assert_eq!(ed.buf.text(), "acb");
        assert_eq!(ed.point, p(0, 3));
        let mut ed = at("abc", 0, 0);
        ed.apply(C::Transpose, 10);
        assert_eq!(ed.buf.text(), "abc");
    }

    #[test]
    fn toggle_comment_adds_at_the_common_indent_and_takes_it_back() {
        let mut ed = at("  a\n\n    b", 0, 2);
        ed.apply(C::SelectAll, 10);
        ed.apply(C::ToggleComment, 10);
        assert_eq!(ed.buf.text(), "  // a\n\n  //   b");
        ed.apply(C::ToggleComment, 10);
        assert_eq!(ed.buf.text(), "  a\n\n    b");
    }

    #[test]
    fn toggle_comment_on_the_point_line_moves_the_point_with_it() {
        let mut ed = at("foo", 0, 1);
        ed.apply(C::ToggleComment, 10);
        assert_eq!(ed.buf.text(), "// foo");
        assert_eq!(ed.point, p(0, 4));
        ed.apply(C::Undo, 10);
        assert_eq!(ed.buf.text(), "foo");
    }

    #[test]
    fn a_region_ending_at_a_line_start_leaves_that_line_out() {
        let mut ed = at("a\nb\nc", 0, 0);
        ed.mark = Some(p(0, 0));
        ed.point = p(1, 0);
        ed.apply(C::ToggleComment, 10);
        assert_eq!(ed.buf.text(), "// a\nb\nc");
    }

    #[test]
    fn no_comment_syntax_says_so() {
        let mut ed = Editor::new("x", Some(Path::new("notes.md")));
        assert!(matches!(ed.apply(C::ToggleComment, 10), Out::Note(_)));
    }

    #[test]
    fn undo_and_redo_put_the_point_back() {
        let mut ed = at("ab", 0, 1);
        ed.apply(C::KillLine, 10);
        assert_eq!(ed.buf.text(), "a");
        ed.point = p(0, 0);
        ed.apply(C::Undo, 10);
        assert_eq!(ed.buf.text(), "ab");
        assert_eq!(ed.point, p(0, 1));
        ed.apply(C::Redo, 10);
        assert_eq!(ed.buf.text(), "a");
        assert_eq!(ed.point, p(0, 1));
        ed.apply(C::Undo, 10);
        ed.apply(C::Undo, 10);
        assert_eq!(ed.apply(C::Undo, 10), Out::Note("Nothing to undo".into()));
    }

    #[test]
    fn read_only_refuses_edits_but_not_moves() {
        let mut ed = at("ab", 0, 0);
        ed.read_only = true;
        assert!(matches!(ed.apply(C::KillLine, 10), Out::Note(_)));
        assert!(matches!(ed.insert("x", Group::Typing), Out::Note(_)));
        assert_eq!(ed.buf.text(), "ab");
        ed.apply(C::Move(M::Forward), 10);
        assert_eq!(ed.point, p(0, 1));
        assert_eq!(ed.apply(C::Copy, 10), Out::Clipboard("ab".into()));
    }

    #[test]
    fn page_moves_by_the_page_keeping_the_column() {
        let text = "aaa\n".repeat(50);
        let mut ed = at(&text, 0, 2);
        ed.apply(C::Move(M::PageDown), 20);
        assert_eq!(ed.point, p(20, 2));
        ed.apply(C::Move(M::PageUp), 20);
        assert_eq!(ed.point, p(0, 2));
    }

    #[test]
    fn selecting_by_the_mouse() {
        let mut ed = at("foo bar\nbaz", 0, 0);
        ed.select_word(p(0, 5));
        assert_eq!(ed.region(), Some((p(0, 4), p(0, 7))));
        ed.select_line(p(0, 2));
        assert_eq!(ed.region(), Some((p(0, 0), p(1, 0))));
        ed.select_line(p(1, 0));
        assert_eq!(ed.region(), Some((p(1, 0), p(1, 3))));
        ed.click(p(0, 1));
        assert_eq!(ed.region(), None);
        ed.extend_to(p(0, 3));
        assert_eq!(ed.region(), Some((p(0, 1), p(0, 3))));
    }

    #[test]
    fn replace_all_keeps_the_place_and_can_be_undone() {
        let mut ed = at("one\ntwo\nthree", 2, 3);
        ed.replace_all("one\nTWO\nthree");
        assert_eq!(ed.point, p(2, 3));
        assert!(ed.buf.is_dirty());
        ed.apply(C::Undo, 10);
        assert_eq!(ed.buf.text(), "one\ntwo\nthree");
    }
}
