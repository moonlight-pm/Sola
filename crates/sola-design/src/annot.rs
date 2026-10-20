//! Annotations in `canvas.toml`: sections, notes, board links, and flows.
//!
//! Read with the layout and checked against its board ids: a name that is
//! not a board makes the file an error, like any other. Exits declared in
//! the pages (`data-design-to`) are read when a page settles (see
//! [`crate::marks`]).

use std::collections::HashSet;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionSpec {
    id: Option<String>,
    title: String,
    boards: Vec<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteSpec {
    text: String,
    board: Option<String>,
    at: Option<String>,
    x: Option<f32>,
    y: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSpec {
    from: String,
    to: String,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowSpec {
    id: Option<String>,
    title: String,
    start: String,
    note: Option<String>,
}

/// A titled region around some boards.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub boards: Vec<String>,
    pub note: Option<String>,
}

/// Where a note sits.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// On a board: its top-right corner, or that of the first element
    /// matching `at`.
    Board { board: String, at: Option<String> },
    /// A canvas point.
    Point { x: f32, y: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// 1-based, in file order (the marker's number).
    pub n: usize,
    pub text: String,
    pub place: Place,
}

/// An arrow between two whole boards.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

/// A tour: starts at `start`, follows exits and links.
#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    pub id: String,
    pub title: String,
    pub start: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotations {
    pub sections: Vec<Section>,
    pub notes: Vec<Note>,
    pub links: Vec<Link>,
    pub flows: Vec<Flow>,
}

impl Annotations {
    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }

    pub fn flow(&self, id: &str) -> Option<&Flow> {
        self.flows.iter().find(|f| f.id == id)
    }

    /// Notes pinned into board `id` at an element: `(n, selector)`.
    pub fn pins(&self, id: &str) -> Vec<(usize, String)> {
        self.notes
            .iter()
            .filter_map(|n| match &n.place {
                Place::Board { board, at: Some(at) } if board == id => Some((n.n, at.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Check the specs against the canvas's board `ids`.
pub fn check(
    sections: Vec<SectionSpec>,
    notes: Vec<NoteSpec>,
    links: Vec<LinkSpec>,
    flows: Vec<FlowSpec>,
    ids: &HashSet<String>,
) -> Result<Annotations, String> {
    let board = |what: &str, id: &str| -> Result<String, String> {
        let id = id.trim();
        if ids.contains(id) {
            Ok(id.to_string())
        } else {
            Err(format!("{what}: no artboard {id:?}"))
        }
    };
    let mut out = Annotations::default();

    let mut seen = HashSet::new();
    for (i, s) in sections.into_iter().enumerate() {
        let what = format!("section {}", i + 1);
        let title = required(&what, "title", &s.title)?;
        let id = named_id(&what, s.id.as_deref(), &title)?;
        if !seen.insert(id.clone()) {
            return Err(format!("{what}: duplicate id {id:?}"));
        }
        if s.boards.is_empty() {
            return Err(format!("{what}: boards is empty"));
        }
        let boards = s
            .boards
            .iter()
            .map(|b| board(&what, b))
            .collect::<Result<_, _>>()?;
        out.sections.push(Section {
            id,
            title,
            boards,
            note: optional(s.note),
        });
    }

    for (i, n) in notes.into_iter().enumerate() {
        let what = format!("note {}", i + 1);
        let text = required(&what, "text", &n.text)?;
        let place = match (n.board, n.x, n.y) {
            (Some(b), None, None) => Place::Board {
                board: board(&what, &b)?,
                at: optional(n.at),
            },
            (None, Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
                if n.at.is_some() {
                    return Err(format!("{what}: at needs board"));
                }
                Place::Point { x, y }
            }
            (None, Some(_), Some(_)) => return Err(format!("{what}: x / y must be numbers")),
            _ => return Err(format!("{what}: give board, or x and y")),
        };
        out.notes.push(Note { n: i + 1, text, place });
    }

    for (i, l) in links.into_iter().enumerate() {
        let what = format!("link {}", i + 1);
        let from = board(&what, &l.from)?;
        let to = board(&what, &l.to)?;
        if from == to {
            return Err(format!("{what}: from and to are the same artboard"));
        }
        out.links.push(Link {
            from,
            to,
            label: optional(l.label),
        });
    }

    let mut seen = HashSet::new();
    for (i, f) in flows.into_iter().enumerate() {
        let what = format!("flow {}", i + 1);
        let title = required(&what, "title", &f.title)?;
        let id = named_id(&what, f.id.as_deref(), &title)?;
        if !seen.insert(id.clone()) {
            return Err(format!("{what}: duplicate id {id:?}"));
        }
        out.flows.push(Flow {
            id,
            title,
            start: board(&what, &f.start)?,
            note: optional(f.note),
        });
    }
    Ok(out)
}

fn required(what: &str, field: &str, v: &str) -> Result<String, String> {
    let v = v.trim();
    if v.is_empty() {
        return Err(format!("{what}: {field} is empty"));
    }
    Ok(v.to_string())
}

fn optional(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// `id`, else the slug of `title`.
fn named_id(what: &str, id: Option<&str>, title: &str) -> Result<String, String> {
    let id = match id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => id.to_string(),
        None => crate::projects::slug(title),
    };
    if id.is_empty() {
        return Err(format!("{what}: give an id (the title has no letters or digits)"));
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use crate::canvas::parse;
    use std::path::Path;

    use super::*;

    const BOARDS: &str = "
        [[artboard]]
        src = \"a.html\"
        width = 100
        height = 100
        [[artboard]]
        src = \"b.html\"
        width = 100
        height = 100
    ";

    fn load(extra: &str) -> Result<Annotations, String> {
        parse(Path::new("/p/design/c"), &format!("{BOARDS}\n{extra}")).map(|c| c.annot)
    }

    #[test]
    fn reads_every_kind() {
        let a = load(
            r#"
            [[section]]
            title = "Sign in"
            boards = ["a", "b"]
            note = "  Magic link only.  "
            [[note]]
            board = "a"
            at = ".lineage"
            text = "Reads left to right."
            [[note]]
            x = -600
            y = 0
            text = "General."
            [[link]]
            from = "a"
            to = "b"
            label = ""
            [[flow]]
            title = "Sign in"
            start = "a"
            "#,
        )
        .unwrap();
        assert_eq!(a.sections[0].id, "sign-in");
        assert_eq!(a.sections[0].note.as_deref(), Some("Magic link only."));
        assert_eq!(a.notes[1].n, 2);
        assert_eq!(a.notes[1].place, Place::Point { x: -600.0, y: 0.0 });
        assert_eq!(a.pins("a"), vec![(1, ".lineage".to_string())]);
        assert!(a.pins("b").is_empty());
        assert_eq!(a.links[0].label, None, "an empty label is none");
        assert_eq!(a.flow("sign-in").unwrap().start, "a");
        assert_eq!(load("").unwrap(), Annotations::default());
    }

    #[test]
    fn names_must_be_boards() {
        let e = load("[[section]]\ntitle = \"S\"\nboards = [\"a\", \"zz\"]").unwrap_err();
        assert!(e.contains("section 1") && e.contains("\"zz\""), "{e}");
        let e = load("[[link]]\nfrom = \"a\"\nto = \"zz\"").unwrap_err();
        assert!(e.contains("link 1"), "{e}");
        let e = load("[[flow]]\ntitle = \"F\"\nstart = \"zz\"").unwrap_err();
        assert!(e.contains("flow 1"), "{e}");
        let e = load("[[note]]\nboard = \"zz\"\ntext = \"t\"").unwrap_err();
        assert!(e.contains("note 1"), "{e}");
    }

    #[test]
    fn notes_need_one_place_and_text() {
        assert!(load("[[note]]\ntext = \"t\"").unwrap_err().contains("give board"));
        assert!(load("[[note]]\nboard = \"a\"\nx = 1\ny = 2\ntext = \"t\"").is_err());
        assert!(load("[[note]]\nx = 1\ny = 2\nat = \".x\"\ntext = \"t\"").unwrap_err().contains("at needs board"));
        assert!(load("[[note]]\nboard = \"a\"\ntext = \"  \"").unwrap_err().contains("text is empty"));
    }

    #[test]
    fn ids_are_unique_and_links_go_somewhere() {
        let two = "[[section]]\ntitle = \"S\"\nboards = [\"a\"]\n[[section]]\ntitle = \"s\"\nboards = [\"b\"]";
        assert!(load(two).unwrap_err().contains("duplicate id \"s\""));
        assert!(load("[[link]]\nfrom = \"a\"\nto = \"a\"").unwrap_err().contains("same artboard"));
        assert!(load("[[flow]]\ntitle = \"—\"\nstart = \"a\"").unwrap_err().contains("give an id"));
        assert!(load("[[section]]\ntitle = \"S\"\nboards = []").unwrap_err().contains("empty"));
    }
}
