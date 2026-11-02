//! Syntax colors. Syntect finds the scopes; a theme built here turns them
//! into [`Role`]s, and the view colors a role from Sola's palette, so colors
//! follow Settings → Appearance with nothing to reparse.
//!
//! Only what has been on screen is parsed. A saved parser state every
//! [`MARK_EVERY`] lines means an edit reparses from just above it.

use std::path::Path;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use syntect::highlighting::{
    Color, HighlightIterator, HighlightState, Highlighter, ScopeSelectors, StyleModifier, Theme,
    ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// What a piece of text is, as far as its color goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
    Plain,
    Comment,
    String,
    Keyword,
    Constant,
    Type,
    Function,
    Invalid,
}

impl Role {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Role::Comment,
            2 => Role::String,
            3 => Role::Keyword,
            4 => Role::Constant,
            5 => Role::Type,
            6 => Role::Function,
            7 => Role::Invalid,
            _ => Role::Plain,
        }
    }
}

/// A piece of a line: it runs up to this byte, in this role.
pub type Span = (u32, Role);

const MARK_EVERY: usize = 128;
/// A line longer than this (minified, usually) stays plain: fancy-regex
/// takes seconds on them.
const LONGEST_LINE: usize = 8192;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// The theme's "color" is the role, in the red channel.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let role = |role: Role| Color { r: role as u8, g: 0, b: 0, a: 255 };
    let rows: &[(&str, Role)] = &[
        ("comment", Role::Comment),
        ("string, meta.string", Role::String),
        ("constant.character.escape", Role::Constant),
        ("keyword, storage, keyword.operator.word", Role::Keyword),
        ("keyword.operator", Role::Plain),
        ("constant, support.constant", Role::Constant),
        ("entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, support.type, support.class, storage.type.primitive", Role::Type),
        ("entity.name.function, support.function, variable.function, meta.function-call", Role::Function),
        ("entity.name.tag, markup.heading, entity.other.attribute-name", Role::Keyword),
        ("invalid", Role::Invalid),
    ];
    Theme {
        settings: ThemeSettings {
            foreground: Some(role(Role::Plain)),
            ..ThemeSettings::default()
        },
        scopes: rows
            .iter()
            .filter_map(|(scope, r)| {
                Some(ThemeItem {
                    scope: ScopeSelectors::from_str(scope).ok()?,
                    style: StyleModifier {
                        foreground: Some(role(*r)),
                        background: None,
                        font_style: None,
                    },
                })
            })
            .collect(),
        ..Theme::default()
    }
});

static HIGHLIGHTER: LazyLock<Highlighter<'static>> = LazyLock::new(|| Highlighter::new(&THEME));

/// Load the grammars off the UI thread, so the first file opens at once.
pub fn warm_up() {
    std::thread::spawn(|| {
        LazyLock::force(&SYNTAXES);
        LazyLock::force(&HIGHLIGHTER);
    });
}

#[derive(Clone)]
struct State {
    parse: ParseState,
    highlight: HighlightState,
}

pub struct Highlight {
    syntax: Option<&'static SyntaxReference>,
    /// Each line parsed so far, from the top.
    spans: Vec<Box<[Span]>>,
    /// The state after line `marks[n].0 - 1`, for every [`MARK_EVERY`] lines.
    marks: Vec<(usize, State)>,
    state: Option<State>,
    generation: u64,
}

impl Highlight {
    pub fn new(path: Option<&Path>, first_line: &str) -> Self {
        let mut this = Self {
            syntax: find_syntax(path, first_line),
            spans: Vec::new(),
            marks: Vec::new(),
            state: None,
            generation: 0,
        };
        this.state = this.syntax.map(start);
        this
    }

    /// Bumps whenever the colors of some line changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The file's type, for the status line.
    pub fn name(&self) -> Option<&'static str> {
        self.syntax.map(|s| s.name.as_str())
    }

    /// The spans of `line`, once parsed.
    pub fn spans(&self, line: usize) -> Option<&[Span]> {
        self.spans.get(line).map(AsRef::as_ref)
    }

    /// Whether `until` (exclusive) is parsed, given `count` lines.
    pub fn covers(&self, until: usize, count: usize) -> bool {
        self.syntax.is_none() || self.spans.len() >= until.min(count)
    }

    /// Line `from` changed: forget it and everything after.
    pub fn invalidate(&mut self, from: usize) {
        let Some(syntax) = self.syntax else {
            return;
        };
        while self.marks.last().is_some_and(|(line, _)| *line > from) {
            self.marks.pop();
        }
        let (line, state) = match self.marks.last() {
            Some((line, state)) => (*line, state.clone()),
            None => (0, start(syntax)),
        };
        self.spans.truncate(line);
        self.state = Some(state);
        self.generation += 1;
    }

    /// Parse on up to line `until` (exclusive), for at most `budget`.
    /// True when it got there.
    pub fn advance(&mut self, lines: &[String], until: usize, budget: Duration) -> bool {
        let until = until.min(lines.len());
        let Some(state) = self.state.as_mut() else {
            return true;
        };
        if self.spans.len() >= until {
            return true;
        }
        let started = Instant::now();
        let mut text = String::new();
        while self.spans.len() < until {
            let i = self.spans.len();
            let line = &lines[i];
            let spans = if line.len() > LONGEST_LINE {
                Box::default()
            } else {
                text.clear();
                text.push_str(line);
                text.push('\n');
                match parse_line(state, &text, line.len()) {
                    Some(spans) => spans,
                    // A grammar that fails is no colors, not no file.
                    None => {
                        tracing::warn!(line = i, "syntax parse failed; dropping colors");
                        self.syntax = None;
                        self.state = None;
                        self.spans.clear();
                        self.marks.clear();
                        self.generation += 1;
                        return true;
                    }
                }
            };
            self.spans.push(spans);
            let done = self.spans.len();
            if done % MARK_EVERY == 0 {
                self.marks.push((done, state.clone()));
            }
            if started.elapsed() >= budget {
                break;
            }
        }
        self.generation += 1;
        self.spans.len() >= until
    }
}

fn start(syntax: &'static SyntaxReference) -> State {
    State {
        parse: ParseState::new(syntax),
        highlight: HighlightState::new(&HIGHLIGHTER, ScopeStack::new()),
    }
}

fn parse_line(state: &mut State, text: &str, len: usize) -> Option<Box<[Span]>> {
    let ops = state.parse.parse_line(text, &SYNTAXES).ok()?;
    let mut spans: Vec<Span> = Vec::new();
    let mut end = 0usize;
    for (style, piece) in HighlightIterator::new(&mut state.highlight, &ops, text, &HIGHLIGHTER) {
        end = (end + piece.len()).min(len);
        let role = Role::from_code(style.foreground.r);
        match spans.last_mut() {
            Some((last, r)) if *r == role => *last = end as u32,
            _ => spans.push((end as u32, role)),
        }
    }
    Some(spans.into_boxed_slice())
}

fn find_syntax(path: Option<&Path>, first_line: &str) -> Option<&'static SyntaxReference> {
    let by_name = path.and_then(|p| {
        let ext = p.extension().map(|e| e.to_string_lossy().into_owned());
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
        ext.and_then(|e| SYNTAXES.find_syntax_by_extension(&e))
            .or_else(|| name.and_then(|n| SYNTAXES.find_syntax_by_extension(&n)))
    });
    by_name
        .or_else(|| SYNTAXES.find_syntax_by_first_line(first_line))
        .filter(|s| s.name != "Plain Text")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: Duration = Duration::from_secs(30);

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(String::from).collect()
    }

    fn roles(h: &Highlight, line: usize) -> Vec<Role> {
        h.spans(line).expect("parsed").iter().map(|s| s.1).collect()
    }

    #[test]
    fn rust_gets_its_roles() {
        let text = lines("// note\nfn main() { let s = \"hi\"; }");
        let mut h = Highlight::new(Some(Path::new("a.rs")), &text[0]);
        assert!(h.advance(&text, 2, BUDGET));
        assert_eq!(h.name(), Some("Rust"));
        assert_eq!(roles(&h, 0), [Role::Comment]);
        assert!(roles(&h, 1).contains(&Role::Keyword));
        assert!(roles(&h, 1).contains(&Role::String));
        assert!(roles(&h, 1).contains(&Role::Function));
    }

    #[test]
    fn toml_and_typescript_are_known() {
        for (name, want) in [("Cargo.toml", "TOML"), ("a.ts", "TypeScript"), ("a.nix", "Nix")] {
            assert_eq!(Highlight::new(Some(Path::new(name)), "").name(), Some(want), "{name}");
        }
    }

    #[test]
    fn a_first_line_can_name_the_language() {
        assert_eq!(
            Highlight::new(Some(Path::new("script")), "#!/bin/bash").name(),
            Some("Bourne Again Shell (bash)")
        );
    }

    #[test]
    fn plain_text_has_no_syntax_and_is_always_covered() {
        let mut h = Highlight::new(Some(Path::new("a.unknownext")), "hello");
        assert_eq!(h.name(), None);
        assert!(h.covers(100, 200));
        assert!(h.advance(&lines("hello"), 1, BUDGET));
        assert_eq!(h.spans(0), None);
    }

    #[test]
    fn spans_end_at_the_line_end_not_past_the_newline() {
        let text = lines("let x = 1;");
        let mut h = Highlight::new(Some(Path::new("a.rs")), &text[0]);
        h.advance(&text, 1, BUDGET);
        assert_eq!(h.spans(0).and_then(|s| s.last()).map(|s| s.0), Some(10));
    }

    #[test]
    fn it_stops_where_asked_and_picks_up_again() {
        let text = lines(&"x\n".repeat(10));
        let mut h = Highlight::new(Some(Path::new("a.rs")), "");
        assert!(h.advance(&text, 3, BUDGET));
        assert!(h.covers(3, text.len()));
        assert!(!h.covers(4, text.len()));
        assert_eq!(h.spans(3), None);
        assert!(h.advance(&text, 5, BUDGET));
        assert!(h.covers(5, text.len()));
    }

    #[test]
    fn an_edit_reparses_from_above_it_with_the_right_state() {
        // A comment opened on line 300 colors the lines after it.
        let mut text = lines(&"let a = 1;\n".repeat(400));
        let mut h = Highlight::new(Some(Path::new("a.rs")), "");
        assert!(h.advance(&text, 400, BUDGET));
        assert!(roles(&h, 310).contains(&Role::Keyword));
        text[300] = "/* open".into();
        h.invalidate(300);
        assert!(!h.covers(301, text.len()));
        assert!(h.advance(&text, 400, BUDGET));
        assert_eq!(roles(&h, 310), [Role::Comment]);
        // And above it is as it was.
        assert!(roles(&h, 299).contains(&Role::Keyword));
    }

    #[test]
    fn a_very_long_line_stays_plain() {
        let text = vec!["x".repeat(LONGEST_LINE + 1), "let y = 1;".to_string()];
        let mut h = Highlight::new(Some(Path::new("a.rs")), "");
        assert!(h.advance(&text, 2, BUDGET));
        assert_eq!(h.spans(0).map(<[Span]>::len), Some(0));
        assert!(roles(&h, 1).contains(&Role::Keyword));
    }
}
