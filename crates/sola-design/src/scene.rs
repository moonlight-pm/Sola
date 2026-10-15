//! A canvas's annotations as the plane draws them, in canvas units: built
//! from its boards (and what their pages declared, see [`crate::marks`])
//! and `canvas.toml` (see [`crate::annot`]). Rebuilt each view; cheap.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use iced::{Point, Rectangle, Size};

use crate::annot::{Annotations, Place};
use crate::boards::Board;
use crate::marks;
use crate::routes::{self, Wire};
use crate::select::Rect;

#[derive(Debug, Clone, PartialEq)]
pub struct SectionMark {
    pub boards: Vec<Rectangle>,
    pub title: String,
    pub note: Option<String>,
}

/// An arrow: from an exit element or a whole board, to a board.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowMark {
    pub from_board: String,
    /// Index of the source board in the boards.
    pub source: usize,
    pub from: Rectangle,
    /// Index of the target in the boards.
    pub target: usize,
    pub to: String,
    /// Drawn in a pill at the middle (`data-design-label`, a link's label).
    pub label: Option<String>,
    /// What a tour calls it: the label, else the exit element's text.
    pub name: Option<String>,
    /// The exit's clickable region (see [`Scene::hots`]); `None` for a
    /// `[[link]]`.
    pub hot: Option<usize>,
}

/// A clickable region that moves the camera to board `to`: an exit, or a
/// plain link whose URL is another board's page (no arrow).
#[derive(Debug, Clone, PartialEq)]
pub struct Hot {
    pub tab: u64,
    pub board: String,
    /// Board-local CSS px.
    pub local: Rectangle,
    pub to: String,
    pub exit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteMark {
    pub n: usize,
    pub at: Point,
    pub board: Option<String>,
    pub text: String,
}

/// A step a tour can take from a board.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub to: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub sections: Vec<SectionMark>,
    pub arrows: Vec<ArrowMark>,
    pub hots: Vec<Hot>,
    pub notes: Vec<NoteMark>,
    /// Each arrow laid out through the gutters (canvas units).
    pub wires: Vec<Wire>,
}

fn rect(r: Rect) -> Rectangle {
    Rectangle::new(Point::new(r.x, r.y), Size::new(r.width.max(0.0), r.height.max(0.0)))
}

impl Scene {
    pub fn build(boards: &[Board], annot: &Annotations) -> Self {
        let index: HashMap<&str, usize> = boards
            .iter()
            .enumerate()
            .map(|(i, b)| (b.art.id.as_str(), i))
            .collect();
        let urls: Vec<(&str, &str)> = boards.iter().map(|b| (b.art.id.as_str(), b.art.url.as_str())).collect();
        let pages = marks::Pages::new(&urls);
        let mut scene = Scene {
            sections: annot
                .sections
                .iter()
                .map(|s| SectionMark {
                    boards: s.boards.iter().filter_map(|id| Some(boards[*index.get(id.as_str())?].rect())).collect(),
                    title: s.title.clone(),
                    note: s.note.clone(),
                })
                .filter(|s| !s.boards.is_empty())
                .collect(),
            ..Self::default()
        };

        for (source, b) in boards.iter().enumerate() {
            let origin = b.rect().position();
            let to_canvas = |r: Rectangle| Rectangle::new(origin + iced::Vector::new(r.x, r.y), r.size());
            for exit in &b.marks.exits {
                let Some(&target) = index.get(exit.to.as_str()).filter(|_| exit.to != b.art.id) else {
                    continue;
                };
                let local = rect(exit.rect);
                scene.hots.push(Hot {
                    tab: b.tab.0,
                    board: b.art.id.clone(),
                    local,
                    to: exit.to.clone(),
                    exit: true,
                });
                scene.arrows.push(ArrowMark {
                    from_board: b.art.id.clone(),
                    source,
                    from: to_canvas(local),
                    target,
                    to: exit.to.clone(),
                    label: exit.label.clone().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()),
                    name: exit.name().map(str::to_string),
                    hot: Some(scene.hots.len() - 1),
                });
            }
            for link in &b.marks.links {
                if let Some(to) = pages.board_for(&link.href, &b.art.id) {
                    scene.hots.push(Hot {
                        tab: b.tab.0,
                        board: b.art.id.clone(),
                        local: rect(link.rect),
                        to: to.to_string(),
                        exit: false,
                    });
                }
            }
        }

        for l in &annot.links {
            let (Some(&from), Some(&target)) = (index.get(l.from.as_str()), index.get(l.to.as_str())) else {
                continue;
            };
            scene.arrows.push(ArrowMark {
                from_board: l.from.clone(),
                source: from,
                from: boards[from].rect(),
                target,
                to: l.to.clone(),
                label: l.label.clone(),
                name: l.label.clone(),
                hot: None,
            });
        }

        for note in &annot.notes {
            let (at, board) = match &note.place {
                Place::Point { x, y } => (Point::new(*x, *y), None),
                Place::Board { board, at } => {
                    let Some(b) = index.get(board.as_str()).map(|&i| &boards[i]) else {
                        continue;
                    };
                    let corner = match at {
                        None => Point::new(b.art.x + b.art.width as f32, b.art.y),
                        // Not drawn until the page says where (or when nothing matched).
                        Some(_) => match b.marks.anchor(note.n).and_then(|a| a.rect) {
                            Some(r) => Point::new(b.art.x + r.x + r.width, b.art.y + r.y),
                            None => continue,
                        },
                    };
                    (corner, Some(board.clone()))
                }
            };
            scene.notes.push(NoteMark {
                n: note.n,
                at,
                board,
                text: note.text.clone(),
            });
        }
        let rects: Vec<Rectangle> = boards.iter().map(Board::rect).collect();
        let arrows: Vec<routes::Arrow> = scene
            .arrows
            .iter()
            .map(|a| routes::Arrow {
                board: rects[a.source],
                from: a.from,
                to: rects[a.target],
                target: a.target,
            })
            .collect();
        scene.wires = lay_out_once(&rects, &arrows);
        scene
    }

    /// The clickable region under board-local point `(x, y)` of board
    /// `tab`: the smallest one holding it.
    pub fn hot_at(&self, tab: u64, x: f32, y: f32) -> Option<usize> {
        let p = Point::new(x, y);
        self.hots
            .iter()
            .enumerate()
            .filter(|(_, h)| h.tab == tab && h.local.contains(p))
            .min_by(|(_, a), (_, b)| {
                let area = |h: &Hot| h.local.width * h.local.height;
                area(a).total_cmp(&area(b))
            })
            .map(|(i, _)| i)
    }

    /// Where a tour can go from board `from`: its exits, then its links,
    /// one choice per target (the first named wins).
    pub fn choices(&self, from: &str) -> Vec<Choice> {
        let mut out: Vec<Choice> = Vec::new();
        for a in self.arrows.iter().filter(|a| a.from_board == from) {
            match out.iter_mut().find(|c| c.to == a.to) {
                Some(c) if c.name.is_none() => c.name = a.name.clone(),
                Some(_) => {}
                None => out.push(Choice {
                    to: a.to.clone(),
                    name: a.name.clone(),
                }),
            }
        }
        out
    }
}

thread_local! {
    /// The last layout and its wires: a scene is rebuilt every view, its
    /// wires only when boards or arrows move.
    static LAST: RefCell<Option<(u64, Vec<Wire>)>> = const { RefCell::new(None) };
}

fn lay_out_once(boards: &[Rectangle], arrows: &[routes::Arrow]) -> Vec<Wire> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let rect = |r: &Rectangle, h: &mut std::collections::hash_map::DefaultHasher| {
        [r.x, r.y, r.width, r.height].map(f32::to_bits).hash(h);
    };
    for b in boards {
        rect(b, &mut h);
    }
    for a in arrows {
        rect(&a.board, &mut h);
        rect(&a.from, &mut h);
        rect(&a.to, &mut h);
        a.target.hash(&mut h);
    }
    let key = h.finish();
    LAST.with(|last| {
        if let Some((k, wires)) = &*last.borrow() {
            if *k == key {
                return wires.clone();
            }
        }
        let wires = routes::lay_out(boards, arrows);
        *last.borrow_mut() = Some((key, wires.clone()));
        wires
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sola_browser::engine::TabId;

    use super::*;
    use crate::annot::{Link, Note, Section};
    use crate::canvas::Artboard;
    use crate::marks::Marks;

    fn board(id: &str, x: f32, marks: serde_json::Value) -> Board {
        Board {
            tab: TabId(x as u64),
            art: Artboard {
                id: id.into(),
                title: id.into(),
                url: format!("http://{id}--c.p.localhost:1/design/c/{id}.html"),
                x,
                y: 0.0,
                width: 100,
                height: 100,
                auto_height: false,
            },
            scale: 2.0,
            surface: (200, 200),
            grown: false,
            error: None,
            marks: Marks::from_settle(&marks),
        }
    }

    fn r(x: f32, y: f32, w: f32, h: f32) -> serde_json::Value {
        json!({"x": x, "y": y, "width": w, "height": h})
    }

    fn scene() -> Scene {
        let boards = vec![
            board("a", 0.0, json!({
                "exits": [
                    {"to": "b", "label": "send", "text": "Send link", "rect": r(10., 10., 40., 20.)},
                    {"to": "zz", "text": "Nowhere", "rect": r(0., 0., 5., 5.)},
                    {"to": "a", "text": "Self", "rect": r(0., 0., 5., 5.)},
                ],
                "links": [
                    {"href": "http://a--c.p.localhost:1/design/c/c.html", "rect": r(0., 50., 100., 50.)},
                    {"href": "http://a--c.p.localhost:1/design/c/c.html", "rect": r(0., 60., 20., 10.)},
                    {"href": "http://a--c.p.localhost:1/design/c/nope.html", "rect": r(0., 0., 9., 9.)},
                ],
                "anchors": [{"n": 2, "rect": r(20., 30., 10., 10.)}, {"n": 3, "rect": null}],
            })),
            board("b", 200.0, json!({})),
            board("c", 400.0, json!({})),
        ];
        let annot = Annotations {
            sections: vec![Section {
                id: "s".into(),
                title: "S".into(),
                boards: vec!["a".into(), "b".into()],
                note: None,
            }],
            notes: vec![
                Note { n: 1, text: "corner".into(), place: Place::Board { board: "b".into(), at: None } },
                Note { n: 2, text: "pinned".into(), place: Place::Board { board: "a".into(), at: Some(".x".into()) } },
                Note { n: 3, text: "missing".into(), place: Place::Board { board: "a".into(), at: Some(".y".into()) } },
                Note { n: 4, text: "free".into(), place: Place::Point { x: -50.0, y: 5.0 } },
            ],
            links: vec![
                Link { from: "a".into(), to: "c".into(), label: Some("later".into()) },
                Link { from: "a".into(), to: "b".into(), label: Some("also".into()) },
            ],
            flows: vec![],
        };
        Scene::build(&boards, &annot)
    }

    #[test]
    fn exits_become_arrows_and_hot_regions() {
        let s = scene();
        assert_eq!(s.sections[0].boards.len(), 2);
        let exit = &s.arrows[0];
        assert_eq!((exit.to.as_str(), exit.target), ("b", 1));
        assert_eq!(exit.from, Rectangle::new(Point::new(10., 10.), Size::new(40., 20.)));
        assert_eq!(exit.label.as_deref(), Some("send"));
        assert_eq!(s.arrows.len(), 3, "unknown and self exits are skipped; two [[link]]s");
        assert_eq!(s.wires.len(), 3);
        assert_eq!(s.wires[0].path[0], Point::new(100., 24.), "the exit's port on a's right side, off the corner");
        assert_eq!(s.arrows[1].from, Rectangle::new(Point::new(0., 0.), Size::new(100., 100.)));
        // Plain links to a board are hot, without arrows; the smallest wins.
        assert_eq!(s.hots.len(), 3);
        let hot = s.hot_at(0, 5.0, 65.0).unwrap();
        assert_eq!((s.hots[hot].to.as_str(), s.hots[hot].exit, s.hots[hot].local.width), ("c", false, 20.0));
        assert_eq!(s.hot_at(0, 20.0, 20.0), Some(0));
        assert_eq!(s.hot_at(200, 20.0, 20.0), None);
    }

    #[test]
    fn notes_sit_on_corners_elements_and_points() {
        let s = scene();
        let at: Vec<(usize, Point)> = s.notes.iter().map(|n| (n.n, n.at)).collect();
        assert_eq!(
            at,
            vec![(1, Point::new(300., 0.)), (2, Point::new(30., 30.)), (4, Point::new(-50., 5.))],
            "an unmatched `at` is not drawn"
        );
    }

    #[test]
    fn choices_are_one_per_target() {
        let c = scene().choices("a");
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].to.as_str(), c[0].name.as_deref()), ("b", Some("send")));
        assert_eq!((c[1].to.as_str(), c[1].name.as_deref()), ("c", Some("later")));
        assert!(scene().choices("b").is_empty());
    }
}
