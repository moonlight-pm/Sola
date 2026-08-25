//! URL detection and open-in-browser for plain left-click.
//!
//! Scans the terminal grid for plain-text `http(s)://` / `www.` links (and
//! honours OSC 8 hyperlinks when a cell carries one). Visible links are always
//! underlined; a plain left-click (press + release with no drag) opens the
//! link in **sola-browser**. Dragging still starts a text
//! selection, including when the press begins on a URL.
//!
//! Super/⌘ is not used here (plain left-click). Opening uses
//! [`sola_core::open_url`] (sola-browser) — same path as mail, solactl,
//! and shell's `Topic::OpenUrl` handler.
//!
//! The scanner is deliberately simple — no regex crate — and matches the
//! conventions terminals like alacritty use: scheme prefix, then non-space
//! runes, with trailing punctuation stripped.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as GridPoint};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

/// A URL span on the visible viewport, ready for hit-testing and drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlSpan {
    /// Absolute URI to open (`https://…`, including a prepended scheme when
    /// the match started with `www.`).
    pub uri: String,
    /// Inclusive start cell in buffer coordinates.
    pub start: GridPoint,
    /// Inclusive end cell in buffer coordinates.
    pub end: GridPoint,
}

/// Open `uri` in sola-browser (shared sola-wide helper).
pub fn open_url(uri: &str) {
    sola_core::open_url_logged(uri);
}

/// Find a URL under `point`, if any.
///
/// Prefers an OSC 8 hyperlink attached to the cell; otherwise scans **only
/// the logical line containing `point`** (wrap-joined) for a plain-text match.
///
/// # Performance
///
/// This is called from `mouse_interaction` on essentially every pointer sample
/// while the cursor is over the terminal. It must stay O(line), never O(grid).
/// The original implementation called [`visible_urls`] (full viewport scan)
/// here and tanked scroll performance as soon as clickable links landed.
pub fn url_at_point<T>(term: &Term<T>, point: GridPoint) -> Option<String> {
    // OSC 8 wins when present — the app authored an explicit link.
    if let Some(link) = term.grid()[point].hyperlink() {
        let uri = link.uri().to_string();
        if !uri.is_empty() {
            return Some(uri);
        }
    }

    urls_on_line_at(term, point)
        .into_iter()
        .find(|span| point_in_span(point, span))
        .map(|span| span.uri)
}

/// Plain-text + OSC 8 URLs on the single logical line that contains `point`.
///
/// Walks wrapline flags so a URL split across rows still matches, but never
/// touches other lines of the viewport.
fn urls_on_line_at<T>(term: &Term<T>, point: GridPoint) -> Vec<UrlSpan> {
    let cols = term.columns();
    let (text, col_map) = collect_logical_line_at(term, point.line.0, cols);
    let mut out = Vec::new();
    collect_osc8_spans(term, point.line.0, &col_map, &mut out);
    for m in find_urls_in_text(&text) {
        if let Some(span) = match_to_span(&m, &col_map, point.line.0) {
            if !out
                .iter()
                .any(|s| s.start == span.start && s.end == span.end)
            {
                out.push(span);
            }
        }
    }
    out
}

/// All plain-text + OSC 8 URLs currently visible in the viewport.
///
/// **Not for the mouse hot path.** Use [`url_at_point`] for hit-testing.
#[allow(dead_code)] // kept for a future rate-limited underline pass
pub fn visible_urls<T>(term: &Term<T>) -> Vec<UrlSpan> {
    let display_offset = term.grid().display_offset();
    let cols = term.columns();
    let rows = term.screen_lines();
    let mut out = Vec::new();

    // Walk visible rows, joining wrapline runs into logical lines so a URL
    // that wraps across the viewport still matches as one span.
    let mut row = 0usize;
    while row < rows {
        let start_buf_line = row as i32 - display_offset as i32;
        let (text, col_map, end_row) = collect_logical_line(term, start_buf_line, row, cols, rows);
        // OSC 8 hyperlinks on any cell of this logical run.
        collect_osc8_spans(term, start_buf_line, &col_map, &mut out);
        // Plain-text matches.
        for m in find_urls_in_text(&text) {
            if let Some(span) = match_to_span(&m, &col_map, start_buf_line) {
                // Skip if an OSC 8 span already covers the same cells with
                // the same URI (avoid double-drawing underlines).
                if !out
                    .iter()
                    .any(|s| s.start == span.start && s.end == span.end)
                {
                    out.push(span);
                }
            }
        }
        row = end_row + 1;
    }

    out
}

/// Collect the wrap-joined logical line that contains buffer line `buf_line`.
///
/// Walks **up** via `WRAPLINE` on the previous row's last cell to find the
/// start, then **down** collecting characters — same semantics as
/// [`collect_logical_line`] but keyed by buffer line (for hit-testing) rather
/// than visible row.
fn collect_logical_line_at<T>(term: &Term<T>, buf_line: i32, cols: usize) -> (String, Vec<ColMap>) {
    let grid = term.grid();
    let last_col = cols.saturating_sub(1);

    // Walk back to the first row of this logical line.
    let mut start = buf_line;
    loop {
        let prev = start - 1;
        // Stop if previous line is outside the grid's absolute range.
        let top = grid.topmost_line().0;
        if prev < top {
            break;
        }
        let prev_last = GridPoint::new(Line(prev), Column(last_col));
        if !grid[prev_last].flags.contains(Flags::WRAPLINE) {
            break;
        }
        start = prev;
    }

    let mut text = String::new();
    let mut map = Vec::new();
    let mut line = start;
    let bottom = grid.bottommost_line().0;
    loop {
        for col in 0..cols {
            let point = GridPoint::new(Line(line), Column(col));
            let cell = &grid[point];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let c = cell.c;
            text.push(if c == '\0' { ' ' } else { c });
            map.push(ColMap {
                buf_line: line,
                col,
            });
        }
        let last = GridPoint::new(Line(line), Column(last_col));
        if !grid[last].flags.contains(Flags::WRAPLINE) || line >= bottom {
            break;
        }
        line += 1;
    }

    while text.ends_with(' ') {
        text.pop();
        map.pop();
    }
    (text, map)
}

/// Whether `point` falls inside `span` (same line range, inclusive columns).
fn point_in_span(point: GridPoint, span: &UrlSpan) -> bool {
    if point.line < span.start.line || point.line > span.end.line {
        return false;
    }
    if span.start.line == span.end.line {
        return point.column >= span.start.column && point.column <= span.end.column;
    }
    // Multi-line (wrapped) span.
    if point.line == span.start.line {
        return point.column >= span.start.column;
    }
    if point.line == span.end.line {
        return point.column <= span.end.column;
    }
    true
}

/// One character of a logical line, with the buffer column it came from.
#[derive(Debug, Clone, Copy)]
struct ColMap {
    buf_line: i32,
    col: usize,
}

/// Collect a wrap-joined logical line starting at visible `row`.
///
/// Returns `(text, per-char buffer coords, last visible row consumed)`.
fn collect_logical_line<T>(
    term: &Term<T>,
    start_buf_line: i32,
    start_row: usize,
    cols: usize,
    rows: usize,
) -> (String, Vec<ColMap>, usize) {
    let mut text = String::new();
    let mut map = Vec::new();
    let mut buf_line = start_buf_line;
    let mut row = start_row;

    loop {
        let grid = term.grid();
        for col in 0..cols {
            let point = GridPoint::new(Line(buf_line), Column(col));
            let cell = &grid[point];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let c = cell.c;
            // Skip NUL / trailing padding spaces only at the end later — keep
            // interior spaces so URL boundaries stay accurate.
            text.push(if c == '\0' { ' ' } else { c });
            map.push(ColMap { buf_line, col });
        }

        let last = GridPoint::new(Line(buf_line), Column(cols.saturating_sub(1)));
        let wraps = term.grid()[last].flags.contains(Flags::WRAPLINE);
        if !wraps || row + 1 >= rows {
            break;
        }
        row += 1;
        buf_line = row as i32 - term.grid().display_offset() as i32;
    }

    // Trim trailing spaces from the logical line (and matching map entries)
    // so URL end detection doesn't include blank cell padding.
    while text.ends_with(' ') {
        text.pop();
        map.pop();
    }

    (text, map, row)
}

/// Emit one span per contiguous OSC 8 hyperlink run on the logical line.
fn collect_osc8_spans<T>(
    term: &Term<T>,
    _start_buf_line: i32,
    col_map: &[ColMap],
    out: &mut Vec<UrlSpan>,
) {
    let mut i = 0;
    while i < col_map.len() {
        let point = GridPoint::new(Line(col_map[i].buf_line), Column(col_map[i].col));
        let Some(link) = term.grid()[point].hyperlink() else {
            i += 1;
            continue;
        };
        let uri = link.uri().to_string();
        if uri.is_empty() {
            i += 1;
            continue;
        }
        let start_i = i;
        i += 1;
        while i < col_map.len() {
            let p = GridPoint::new(Line(col_map[i].buf_line), Column(col_map[i].col));
            match term.grid()[p].hyperlink() {
                Some(h) if h.uri() == uri => i += 1,
                _ => break,
            }
        }
        let s = &col_map[start_i];
        let e = &col_map[i - 1];
        out.push(UrlSpan {
            uri,
            start: GridPoint::new(Line(s.buf_line), Column(s.col)),
            end: GridPoint::new(Line(e.buf_line), Column(e.col)),
        });
    }
}

/// A match inside a plain-text line: byte/char indices into the string and
/// the absolute URI to open.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TextMatch {
    /// Inclusive start char index.
    start: usize,
    /// Exclusive end char index.
    end: usize,
    uri: String,
}

fn match_to_span(m: &TextMatch, col_map: &[ColMap], _start_buf_line: i32) -> Option<UrlSpan> {
    if m.start >= col_map.len() || m.end == 0 || m.end > col_map.len() {
        return None;
    }
    let s = &col_map[m.start];
    let e = &col_map[m.end - 1];
    Some(UrlSpan {
        uri: m.uri.clone(),
        start: GridPoint::new(Line(s.buf_line), Column(s.col)),
        end: GridPoint::new(Line(e.buf_line), Column(e.col)),
    })
}

/// Scan `text` for URL-like runs. Pure; unit-tested.
fn find_urls_in_text(text: &str) -> Vec<TextMatch> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some((consumed, uri_prefix)) = scheme_at(&chars, i) {
            let start = i;
            i += consumed;
            // Consume the rest of the URL body.
            while i < chars.len() && is_url_body(chars[i]) {
                i += 1;
            }
            // Trim trailing punctuation that is rarely part of the URI.
            let mut end = i;
            while end > start && is_trailing_punct(chars[end - 1]) {
                end -= 1;
            }
            if end > start + consumed.saturating_sub(1) {
                let raw: String = chars[start..end].iter().collect();
                let uri = if uri_prefix.is_empty() {
                    // `www.` match — assume https.
                    format!("https://{raw}")
                } else {
                    raw
                };
                out.push(TextMatch { start, end, uri });
            }
            continue;
        }
        i += 1;
    }
    out
}

/// If `chars[i..]` starts with a recognised scheme, return
/// `(chars consumed by the scheme prefix including `://` or through `www.`,
///  scheme-uri-prefix)` where the prefix is empty for bare `www.`.
fn scheme_at(chars: &[char], i: usize) -> Option<(usize, &'static str)> {
    // https://
    if starts_with_ci(chars, i, "https://") {
        return Some((8, "https://"));
    }
    // http://
    if starts_with_ci(chars, i, "http://") {
        return Some((7, "http://"));
    }
    // www. — only at a token boundary (start or after a non-body char).
    if starts_with_ci(chars, i, "www.") {
        let boundary = i == 0 || !is_url_body(chars[i - 1]);
        if boundary {
            return Some((4, ""));
        }
    }
    None
}

fn starts_with_ci(chars: &[char], i: usize, prefix: &str) -> bool {
    let pref: Vec<char> = prefix.chars().collect();
    if i + pref.len() > chars.len() {
        return false;
    }
    chars[i..i + pref.len()]
        .iter()
        .zip(pref.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Characters allowed inside a URL body after the scheme.
fn is_url_body(c: char) -> bool {
    // Reject whitespace and common delimiters terminals treat as separators.
    // Keep: alnum, and the usual URL punctuation.
    match c {
        ' ' | '\t' | '\n' | '\r' | '<' | '>' | '"' | '\'' | '`' | '{' | '}' | '|' | '\\' | '^'
        | '[' | ']' => false,
        _ => !c.is_control(),
    }
}

/// Trailing characters to strip from a match (sentence punctuation, brackets).
fn is_trailing_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_https_url() {
        let m = find_urls_in_text("see https://example.com/path for more");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].uri, "https://example.com/path");
        // "see " = 4 chars → start at 4.
        assert_eq!(m[0].start, 4);
    }

    #[test]
    fn finds_http_url() {
        let m = find_urls_in_text("http://localhost:8080/x");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].uri, "http://localhost:8080/x");
    }

    #[test]
    fn finds_www_and_prefixes_https() {
        let m = find_urls_in_text("go to www.example.com now");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].uri, "https://www.example.com");
    }

    #[test]
    fn strips_trailing_punctuation() {
        let m = find_urls_in_text("link: https://example.com/a.");
        assert_eq!(m[0].uri, "https://example.com/a");

        let m = find_urls_in_text("(see https://example.com/a)");
        assert_eq!(m[0].uri, "https://example.com/a");

        let m = find_urls_in_text("https://example.com/a,");
        assert_eq!(m[0].uri, "https://example.com/a");
    }

    #[test]
    fn multiple_urls_on_one_line() {
        let m = find_urls_in_text("a https://a.com b http://b.com/c");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].uri, "https://a.com");
        assert_eq!(m[1].uri, "http://b.com/c");
    }

    #[test]
    fn no_false_positive_on_plain_text() {
        assert!(find_urls_in_text("just some words and foo.bar").is_empty());
    }

    #[test]
    fn case_insensitive_scheme() {
        let m = find_urls_in_text("HTTPS://Example.COM");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].uri, "HTTPS://Example.COM");
    }

    #[test]
    fn keeps_query_and_fragment() {
        let m = find_urls_in_text("https://ex.com/x?a=1&b=2#frag");
        assert_eq!(m[0].uri, "https://ex.com/x?a=1&b=2#frag");
    }

    #[test]
    fn point_in_span_single_line() {
        let span = UrlSpan {
            uri: "https://x".into(),
            start: GridPoint::new(Line(0), Column(5)),
            end: GridPoint::new(Line(0), Column(14)),
        };
        assert!(point_in_span(GridPoint::new(Line(0), Column(5)), &span));
        assert!(point_in_span(GridPoint::new(Line(0), Column(10)), &span));
        assert!(point_in_span(GridPoint::new(Line(0), Column(14)), &span));
        assert!(!point_in_span(GridPoint::new(Line(0), Column(4)), &span));
        assert!(!point_in_span(GridPoint::new(Line(0), Column(15)), &span));
        assert!(!point_in_span(GridPoint::new(Line(1), Column(10)), &span));
    }

    /// End-to-end against a real `Term`: write a URL into the grid, then hit
    /// both the URL body and a neighbouring non-URL cell.
    #[test]
    fn url_at_point_on_live_term() {
        use crate::emulator::{Emulator, Listener};
        use std::sync::mpsc;

        let (ptx, _prx) = mpsc::channel::<(String, Vec<u8>)>();
        let (ntx, _nrx) = mpsc::channel::<String>();
        let (ttx, _trx) = mpsc::channel::<(String, String)>();
        let mut e = Emulator::new(80, 24, Listener::new("t".into(), ptx, ntx, ttx));
        e.advance(b"see https://example.com/path please");

        let term = e.term();
        let term = term.lock();

        // Column of the 'h' in https — "see " is 4 cells.
        let on_url = url_at_point(&term, GridPoint::new(Line(0), Column(4)));
        assert_eq!(on_url.as_deref(), Some("https://example.com/path"));

        // Column of the final 'h' in path.
        let on_end = url_at_point(
            &term,
            GridPoint::new(Line(0), Column(4 + "https://example.com/path".len() - 1)),
        );
        assert_eq!(on_end.as_deref(), Some("https://example.com/path"));

        // The space before the URL is not a link.
        assert_eq!(
            url_at_point(&term, GridPoint::new(Line(0), Column(3))),
            None
        );
        // "please" is not a link.
        assert_eq!(
            url_at_point(&term, GridPoint::new(Line(0), Column(30))),
            None
        );
    }

    /// Hit-testing must only look at the line under the pointer — a full
    /// viewport scan here is what made scroll unusable after clickable links.
    #[test]
    fn url_at_point_ignores_urls_on_other_lines() {
        use crate::emulator::{Emulator, Listener};
        use std::sync::mpsc;

        let (ptx, _prx) = mpsc::channel::<(String, Vec<u8>)>();
        let (ntx, _nrx) = mpsc::channel::<String>();
        let (ttx, _trx) = mpsc::channel::<(String, String)>();
        let mut e = Emulator::new(80, 24, Listener::new("t".into(), ptx, ntx, ttx));
        // Line 0 has a URL; line 1 is plain text under the pointer.
        e.advance(b"https://other.com/nowhere\r\nplain text here");

        let term = e.term();
        let term = term.lock();
        // Pointer on line 1, column 0 — must not pick up line 0's URL.
        assert_eq!(
            url_at_point(&term, GridPoint::new(Line(1), Column(0)),),
            None
        );
        // And line 0 still works.
        assert_eq!(
            url_at_point(&term, GridPoint::new(Line(0), Column(0))).as_deref(),
            Some("https://other.com/nowhere")
        );
    }
}
