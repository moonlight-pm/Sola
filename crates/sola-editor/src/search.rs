//! Finding text. Lower case finds either case; any capital makes it exact.

use std::ops::Range;

use crate::buffer::{Buffer, Pos};

fn folds(query: &str) -> bool {
    !query.chars().any(char::is_uppercase)
}

/// Every match of `query` in `line`, as byte ranges.
pub fn in_line(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    // ASCII folding keeps byte offsets where they are.
    if folds(query) {
        let folded = line.to_ascii_lowercase();
        folded.match_indices(query).map(|(i, m)| i..i + m.len()).collect()
    } else {
        line.match_indices(query).map(|(i, m)| i..i + m.len()).collect()
    }
}

/// The first match at or after `from`, wrapping past the end.
pub fn next(buf: &Buffer, from: Pos, query: &str) -> Option<(Pos, Pos)> {
    let count = buf.line_count();
    for step in 0..=count {
        let line = (from.line + step) % count;
        let first = step == 0;
        for m in in_line(buf.line(line), query) {
            if !first || m.start >= from.col {
                return Some((Pos::new(line, m.start), Pos::new(line, m.end)));
            }
        }
    }
    None
}

/// The last match ending at or before `from`, wrapping past the start.
pub fn prev(buf: &Buffer, from: Pos, query: &str) -> Option<(Pos, Pos)> {
    let count = buf.line_count();
    for step in 0..=count {
        let line = (from.line + count - step % count) % count;
        let first = step == 0;
        for m in in_line(buf.line(line), query).into_iter().rev() {
            if !first || m.end <= from.col {
                return Some((Pos::new(line, m.start), Pos::new(line, m.end)));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: usize, col: usize) -> Pos {
        Pos::new(line, col)
    }

    #[test]
    fn lower_case_folds_and_a_capital_is_exact() {
        assert_eq!(in_line("Foo foo", "foo"), [0..3, 4..7]);
        assert_eq!(in_line("Foo foo", "Foo"), [0..3]);
    }

    #[test]
    fn next_moves_on_and_wraps() {
        let b = Buffer::from_text("foo\nbar\nfoo");
        assert_eq!(next(&b, p(0, 0), "foo"), Some((p(0, 0), p(0, 3))));
        assert_eq!(next(&b, p(0, 1), "foo"), Some((p(2, 0), p(2, 3))));
        assert_eq!(next(&b, p(2, 1), "foo"), Some((p(0, 0), p(0, 3))));
        assert_eq!(next(&b, p(0, 0), "nope"), None);
    }

    #[test]
    fn prev_moves_back_and_wraps() {
        let b = Buffer::from_text("foo\nbar\nfoo");
        assert_eq!(prev(&b, p(2, 3), "foo"), Some((p(2, 0), p(2, 3))));
        assert_eq!(prev(&b, p(2, 0), "foo"), Some((p(0, 0), p(0, 3))));
        assert_eq!(prev(&b, p(0, 0), "foo"), Some((p(2, 0), p(2, 3))));
    }

    #[test]
    fn a_match_in_the_starting_line_is_found_after_wrapping_all_the_way() {
        let b = Buffer::from_text("foo\nbar");
        assert_eq!(next(&b, p(0, 1), "foo"), Some((p(0, 0), p(0, 3))));
    }
}
