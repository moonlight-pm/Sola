//! Keep a committed grid selection glued to the *text*, not the viewport.
//!
//! Alacritty already rotates `term.selection` when the engine scrolls
//! (`scroll_up` / `scroll_down`). That covers a real newline at the bottom.
//! Workspaces PTYs sit behind tmux with `smcup@` (no outer alt-screen), so a
//! TUI that "scrolls" (Grok, less, …) typically CUP-rewrites the live grid
//! in place. Buffer line numbers stay put and the highlight would otherwise
//! sit still while the glyphs move underneath.
//!
//! On each follow we compare fingerprints of the live viewport to the ones
//! captured when the selection was committed. A majority shift of unique
//! rows rotates the selection the same way. If the selected string is
//! already intact (engine rotation did its job), we do nothing.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Mutex;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

/// Live-viewport snapshot used to detect an in-place TUI scroll.
#[derive(Clone, Debug, Default)]
pub struct Track {
    /// One hash per visible row at commit / last follow.
    fingerprints: Vec<u64>,
    /// `selection_to_string` when the drag finished.
    committed_text: Option<String>,
    committed_start: Option<Point>,
}

impl Track {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn track_mutex() -> Mutex<Track> {
    Mutex::new(Track::new())
}

/// Remember the current selection as the text we should keep following.
pub fn commit<T: EventListener>(term: &Term<T>, track: &mut Track) {
    track.committed_text = term.selection_to_string();
    track.committed_start = term
        .selection
        .as_ref()
        .and_then(|s| s.to_range(term))
        .map(|r| r.start);
    track.fingerprints = fingerprints(term);
}

/// Drop a committed selection (plain click, new press, clear).
pub fn uncommit(track: &mut Track) {
    track.committed_text = None;
    track.committed_start = None;
}

/// Re-anchor `term.selection` after the live grid was rewritten in place.
///
/// No-op while scrolled into history (`display_offset != 0`): the engine
/// already keeps that viewport stationary and the renderer maps buffer
/// points through the offset. No-op when the selected string is still
/// under the current range (ANSI scroll already rotated it).
pub fn follow<T: EventListener>(term: &mut Term<T>, track: &mut Track) {
    let Some(committed) = track.committed_text.clone() else {
        track.fingerprints = fingerprints(term);
        return;
    };
    if term.grid().display_offset() != 0 {
        track.fingerprints = fingerprints(term);
        return;
    }

    let current = term.selection_to_string();
    if term.selection.is_some() && current.as_deref() == Some(committed.as_str()) {
        track.fingerprints = fingerprints(term);
        return;
    }

    let now = fingerprints(term);
    let shifted = if !track.fingerprints.is_empty() {
        match best_shift(&track.fingerprints, &now) {
            Some(shift) if shift != 0 => try_rotate(term, shift, &committed),
            _ => false,
        }
    } else {
        false
    };

    if !shifted {
        let prefer = track.committed_start.unwrap_or_else(|| {
            term.selection
                .as_ref()
                .and_then(|s| s.to_range(term))
                .map(|r| r.start)
                .unwrap_or_default()
        });
        if let Some((start, end)) = find_needle(term, &committed, prefer) {
            let mut sel = Selection::new(SelectionType::Simple, start, Side::Left);
            sel.update(end, Side::Right);
            sel.include_all();
            term.selection = Some(sel);
            if term.selection_to_string().as_deref() != Some(committed.as_str()) {
                // Mapping was approximate (wide cells / wrap). Keep the
                // engine range rather than a lying string.
            }
        }
    }

    track.fingerprints = fingerprints(term);
    if term.selection_to_string().as_deref() == Some(committed.as_str()) {
        track.committed_start = term
            .selection
            .as_ref()
            .and_then(|s| s.to_range(term))
            .map(|r| r.start);
    }
}

fn fingerprints<T: EventListener>(term: &Term<T>) -> Vec<u64> {
    let offset = term.grid().display_offset();
    let rows = term.screen_lines();
    let cols = term.columns();
    let mut out = Vec::with_capacity(rows);
    for vis in 0..rows {
        let buf = Line(vis as i32 - offset as i32);
        out.push(row_hash(term, buf, cols));
    }
    out
}

fn row_hash<T: EventListener>(term: &Term<T>, line: Line, cols: usize) -> u64 {
    let row = &term.grid()[line];
    let mut chars = Vec::with_capacity(cols);
    let mut last_non_space = 0usize;
    for col in 0..cols {
        let cell = &row[Column(col)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        chars.push(cell.c);
        if cell.c != ' ' && cell.c != '\0' {
            last_non_space = chars.len();
        }
    }
    chars.truncate(last_non_space);
    let mut hasher = DefaultHasher::new();
    chars.hash(&mut hasher);
    hasher.finish()
}

fn empty_line_hash() -> u64 {
    let mut hasher = DefaultHasher::new();
    Vec::<char>::new().hash(&mut hasher);
    hasher.finish()
}

/// Viewport-row delta that maps `old[i]` onto `new[i + shift]`.
/// Negative = content moved up (typical TUI scroll).
fn best_shift(old: &[u64], new: &[u64]) -> Option<i32> {
    if old.len() != new.len() || old.is_empty() {
        return None;
    }
    let empty = empty_line_hash();
    let n = old.len() as i32;
    let mut best_s = 0i32;
    let mut best_matches = 0usize;
    for s in -(n - 1)..n {
        let mut matches = 0usize;
        for (i, old_h) in old.iter().enumerate() {
            let j = i as i32 + s;
            if j < 0 || j >= n {
                continue;
            }
            if *old_h != empty && *old_h == new[j as usize] {
                matches += 1;
            }
        }
        if matches > best_matches {
            best_matches = matches;
            best_s = s;
        }
    }
    let nonempty = old.iter().filter(|h| **h != empty).count();
    if nonempty == 0 {
        return None;
    }
    // Majority of unique rows, at least a few — a single matching status
    // line must not count as a scroll.
    if best_matches >= 3 && best_matches * 2 >= nonempty {
        Some(best_s)
    } else {
        None
    }
}

/// `shift` is the visual delta (negative = moved up). Alacritty's
/// `Selection::rotate` delta is the opposite sign of a visual move:
/// content up 1 → line numbers decrease → delta = +1.
fn try_rotate<T: EventListener>(term: &mut Term<T>, shift: i32, committed: &str) -> bool {
    let screen = term.screen_lines() as i32;
    let region: Range<Line> = Line(0)..Line(screen);
    let backup = term.selection.clone();
    let delta = -shift;
    term.selection = term
        .selection
        .take()
        .and_then(|s| s.rotate(term, &region, delta));
    if term.selection_to_string().as_deref() == Some(committed) {
        true
    } else {
        term.selection = backup;
        false
    }
}

fn find_needle<T: EventListener>(
    term: &Term<T>,
    needle: &str,
    prefer: Point,
) -> Option<(Point, Point)> {
    let needle = needle.trim_end_matches('\n');
    if needle.chars().count() < 3 {
        return None;
    }
    let lines: Vec<&str> = needle.split('\n').collect();
    if lines.iter().any(|l| l.is_empty()) {
        // Blank lines in the needle make row matching ambiguous.
        return None;
    }
    let offset = term.grid().display_offset();
    let rows = term.screen_lines();
    let first = lines[0];
    let mut best: Option<(Point, Point, i32)> = None;
    for vis in 0..rows.saturating_sub(lines.len() - 1) {
        let buf = Line(vis as i32 - offset as i32);
        let text = row_text(term, buf);
        let Some(col) = text.find(first) else {
            continue;
        };
        let mut ok = true;
        for (k, part) in lines.iter().enumerate().skip(1) {
            let next = Line(buf.0 + k as i32);
            if row_text(term, next).find(part) != Some(col) && row_text(term, next) != *part {
                // Subsequent lines of a wrapped / full-width selection
                // often start at column 0.
                if !row_text(term, next).starts_with(part)
                    && row_text(term, next).find(part).is_none()
                {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let start = Point::new(buf, Column(col));
        let last_line = Line(buf.0 + (lines.len() as i32 - 1));
        let last_text = row_text(term, last_line);
        let last_col = if lines.len() == 1 {
            col + first.chars().count().saturating_sub(1)
        } else {
            last_text.find(lines[lines.len() - 1]).unwrap_or(0)
                + lines[lines.len() - 1].chars().count().saturating_sub(1)
        };
        let last_col = last_col.min(term.columns().saturating_sub(1));
        let end = Point::new(last_line, Column(last_col));
        let dist = (start.line.0 - prefer.line.0).abs();
        match best {
            Some((_, _, d)) if dist >= d => {}
            _ => best = Some((start, end, dist)),
        }
    }
    best.map(|(s, e, _)| (s, e))
}

fn row_text<T: EventListener>(term: &Term<T>, line: Line) -> String {
    let cols = term.columns();
    let row = &term.grid()[line];
    let mut s = String::with_capacity(cols);
    for col in 0..cols {
        let cell = &row[Column(col)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        s.push(cell.c);
    }
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::{Emulator, Listener};
    use alacritty_terminal::index::Side;
    use alacritty_terminal::selection::{Selection, SelectionType};
    use std::sync::mpsc;

    fn emu(cols: u16, rows: u16) -> Emulator {
        let (ptx, _prx) = mpsc::channel();
        let (ntx, _nrx) = mpsc::channel();
        let (ttx, _trx) = mpsc::channel();
        Emulator::new(cols, rows, Listener::new("t".into(), ptx, ntx, ttx))
    }

    fn write_numbered(e: &mut Emulator, start: i32, rows: u16) {
        // Overwrite in place (CUP + text). No EL/`2J`: those clear
        // `term.selection` when they touch the selected line, which is
        // not the bug — the bug is glyphs moving under a surviving range.
        e.advance(b"\x1b[H");
        for i in 0..rows {
            let n = start + i as i32;
            let line = format!("LINE-{n:04}");
            e.advance(line.as_bytes());
            if i + 1 < rows {
                e.advance(b"\r\n");
            }
        }
    }

    fn select_row(e: &Emulator, row: i32, cols: usize) {
        let handle = e.term();
        let mut term = handle.lock();
        let mut sel = Selection::new(
            SelectionType::Simple,
            Point::new(Line(row), Column(0)),
            Side::Left,
        );
        sel.update(
            Point::new(Line(row), Column(cols.saturating_sub(1))),
            Side::Right,
        );
        sel.include_all();
        term.selection = Some(sel);
    }

    #[test]
    fn cup_redraw_shift_keeps_selected_text() {
        let mut e = emu(20, 12);
        write_numbered(&mut e, 0, 12);
        select_row(&e, 5, 9); // "LINE-0005"
        let mut track = Track::new();
        {
            let handle = e.term();
            let term = handle.lock();
            assert_eq!(term.selection_to_string().as_deref(), Some("LINE-0005"));
            commit(&term, &mut track);
        }
        // TUI scrolled up one row: LINE-0005 moves from row 5 → row 4.
        write_numbered(&mut e, 1, 12);
        {
            let handle = e.term();
            let mut term = handle.lock();
            // Without follow the highlight still covers row 5 = LINE-0006.
            assert_eq!(term.selection_to_string().as_deref(), Some("LINE-0006"));
            follow(&mut term, &mut track);
            assert_eq!(
                term.selection_to_string().as_deref(),
                Some("LINE-0005"),
                "selection must track the CUP-rewritten text"
            );
        }
    }

    #[test]
    fn el_redraw_restores_via_search() {
        let mut e = emu(20, 12);
        write_numbered(&mut e, 0, 12);
        select_row(&e, 5, 9);
        let mut track = Track::new();
        {
            let handle = e.term();
            let term = handle.lock();
            assert_eq!(term.selection_to_string().as_deref(), Some("LINE-0005"));
            commit(&term, &mut track);
        }
        // EL on the selected line drops alacritty's range; the text still
        // exists one row up after the rewrite.
        e.advance(b"\x1b[H");
        for i in 0..12u16 {
            let n = 1 + i as i32;
            e.advance(format!("LINE-{n:04}\x1b[K").as_bytes());
            if i + 1 < 12 {
                e.advance(b"\r\n");
            }
        }
        {
            let handle = e.term();
            let mut term = handle.lock();
            assert!(
                term.selection.is_none(),
                "EL of the selected line clears the engine range"
            );
            follow(&mut term, &mut track);
            assert_eq!(
                term.selection_to_string().as_deref(),
                Some("LINE-0005"),
                "follow should re-find the text after EL"
            );
        }
    }

    #[test]
    fn ansi_newline_scroll_does_not_double_rotate() {
        let mut e = emu(20, 8);
        // Fill the screen so a later newline actually scrolls.
        write_numbered(&mut e, 0, 8);
        select_row(&e, 3, 9); // LINE-0003
        let mut track = Track::new();
        {
            let handle = e.term();
            let term = handle.lock();
            assert_eq!(term.selection_to_string().as_deref(), Some("LINE-0003"));
            commit(&term, &mut track);
        }
        e.advance(b"\r\nLINE-0008");
        {
            let handle = e.term();
            let mut term = handle.lock();
            let before = term.selection_to_string();
            // Engine rotation should already have kept the text.
            assert_eq!(before.as_deref(), Some("LINE-0003"));
            follow(&mut term, &mut track);
            assert_eq!(
                term.selection_to_string().as_deref(),
                Some("LINE-0003"),
                "follow must not rotate an already-correct ANSI scroll"
            );
        }
    }

    #[test]
    fn history_viewport_is_left_alone() {
        use alacritty_terminal::grid::Scroll;
        let mut e = emu(20, 8);
        write_numbered(&mut e, 0, 8);
        // Push the first rows into history.
        for n in 8..16 {
            e.advance(format!("\r\nLINE-{n:04}").as_bytes());
        }
        select_row(&e, 0, 9);
        let mut track = Track::new();
        let original;
        {
            let handle = e.term();
            let mut term = handle.lock();
            original = term.selection_to_string();
            assert!(original.is_some());
            commit(&term, &mut track);
            term.scroll_display(Scroll::Delta(4));
            assert!(term.grid().display_offset() > 0);
            follow(&mut term, &mut track);
            assert_eq!(
                term.selection_to_string(),
                original,
                "scrolled-back viewport must not re-anchor"
            );
        }
    }

    #[test]
    fn best_shift_detects_upward_move() {
        let old: Vec<u64> = (0..8).map(|i| 1000 + i).collect();
        // Content moved up one row: old[1] is now at new[0].
        let mut new: Vec<u64> = old[1..].to_vec();
        new.push(2000);
        assert_eq!(best_shift(&old, &new), Some(-1));
    }

    #[test]
    fn best_shift_ignores_empty_rows() {
        let empty = empty_line_hash();
        let old = vec![empty, empty, 1, 2, 3, 4, empty, empty];
        let new = vec![empty, empty, 1, 2, 3, 4, empty, empty];
        assert_eq!(best_shift(&old, &new), Some(0));
    }
}
