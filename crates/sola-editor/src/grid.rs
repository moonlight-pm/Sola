//! Bytes to screen columns: tabs go to the next stop, wide characters take
//! two cells, and a grapheme is never split.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn is_simple(line: &str) -> bool {
    line.is_ascii() && !line.contains('\t')
}

fn cells(grapheme: &str, at: usize, tab: usize) -> usize {
    if grapheme == "\t" {
        tab - at % tab
    } else {
        grapheme.width()
    }
}

/// The screen column of byte `col`.
pub fn display_col(line: &str, col: usize, tab: usize) -> usize {
    if is_simple(line) {
        return col;
    }
    let mut at = 0;
    for (i, g) in line.grapheme_indices(true) {
        if i >= col {
            break;
        }
        at += cells(g, at, tab);
    }
    at
}

/// The byte whose cell holds screen column `target`, or the line's end.
pub fn col_at_display(line: &str, target: usize, tab: usize) -> usize {
    if is_simple(line) {
        return target.min(line.len());
    }
    let mut at = 0;
    for (i, g) in line.grapheme_indices(true) {
        let width = cells(g, at, tab);
        if target < at + width {
            return i;
        }
        at += width;
    }
    line.len()
}

/// One piece of a line to draw: its text, its screen column, and the byte
/// it starts at (which is what the highlighter's spans are in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<'a> {
    pub text: &'a str,
    pub column: usize,
    pub byte: usize,
    pub ascii: bool,
}

/// The pieces of `line` showing in columns `left..left + width`. ASCII comes
/// as long runs; anything else one grapheme at a time, each on its own cell,
/// so the font's fallback can't push what follows off the grid.
pub fn visible_runs<'a>(line: &'a str, left: usize, width: usize, tab: usize) -> Vec<Run<'a>> {
    let right = left + width;
    if is_simple(line) {
        if left >= line.len() {
            return Vec::new();
        }
        let end = right.min(line.len());
        return vec![Run { text: &line[left..end], column: left, byte: left, ascii: true }];
    }
    let mut runs: Vec<Run<'a>> = Vec::new();
    let mut at = 0;
    let mut open: Option<(usize, usize)> = None; // (byte, column) of an ASCII run
    let flush = |open: &mut Option<(usize, usize)>, end: usize, runs: &mut Vec<Run<'a>>| {
        if let Some((byte, column)) = open.take() {
            runs.push(Run { text: &line[byte..end], column, byte, ascii: true });
        }
    };
    for (i, g) in line.grapheme_indices(true) {
        if at >= right {
            flush(&mut open, i, &mut runs);
            return runs;
        }
        let w = cells(g, at, tab);
        let shown = at + w > left && at >= left;
        if g == "\t" || !shown {
            flush(&mut open, i, &mut runs);
        } else if g.is_ascii() {
            open.get_or_insert((i, at));
        } else {
            flush(&mut open, i, &mut runs);
            runs.push(Run { text: g, column: at, byte: i, ascii: false });
        }
        at += w;
    }
    flush(&mut open, line.len(), &mut runs);
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_is_its_own_column() {
        assert_eq!(display_col("hello", 3, 4), 3);
        assert_eq!(col_at_display("hello", 3, 4), 3);
        assert_eq!(col_at_display("hello", 99, 4), 5);
    }

    #[test]
    fn tabs_go_to_the_next_stop() {
        assert_eq!(display_col("\tx", 1, 4), 4);
        assert_eq!(display_col("ab\tx", 3, 4), 4);
        assert_eq!(col_at_display("\tx", 2, 4), 0);
        assert_eq!(col_at_display("\tx", 4, 4), 1);
    }

    #[test]
    fn wide_characters_take_two_cells() {
        assert_eq!(display_col("日本x", 6, 4), 4);
        assert_eq!(col_at_display("日本x", 1, 4), 0);
        assert_eq!(col_at_display("日本x", 2, 4), 3);
    }

    #[test]
    fn a_visible_run_is_clipped_to_the_window() {
        let runs = visible_runs("hello world", 6, 3, 4);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "wor");
        assert_eq!(runs[0].column, 6);
        assert!(visible_runs("hi", 5, 3, 4).is_empty());
    }

    #[test]
    fn tabs_split_runs_and_keep_columns() {
        let runs = visible_runs("ab\tcd", 0, 20, 4);
        assert_eq!(runs.iter().map(|r| (r.text, r.column)).collect::<Vec<_>>(), [
            ("ab", 0),
            ("cd", 4),
        ]);
    }

    #[test]
    fn other_text_comes_a_grapheme_at_a_time() {
        let runs = visible_runs("a日b", 0, 20, 4);
        assert_eq!(runs.iter().map(|r| (r.text, r.column, r.byte)).collect::<Vec<_>>(), [
            ("a", 0, 0),
            ("日", 1, 1),
            ("b", 3, 4),
        ]);
    }
}
