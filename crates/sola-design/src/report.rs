//! What `status` says about a canvas's annotations and what its pages
//! declared: so an agent sees a bad exit or an unmatched note `at`, which
//! are not drawn.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::annot::{Annotations, Place};
use crate::boards::Board;
use crate::marks;

/// `sections`, `links`, `notes`, `flows` of a canvas.
pub fn annotations(annot: &Annotations, boards: &[Board]) -> Value {
    let notes: Vec<Value> = annot
        .notes
        .iter()
        .map(|n| {
            let mut v = json!({"n": n.n, "text": n.text});
            match &n.place {
                Place::Point { x, y } => {
                    v["x"] = json!(x);
                    v["y"] = json!(y);
                }
                Place::Board { board, at } => {
                    v["board"] = json!(board);
                    if let Some(at) = at {
                        v["at"] = json!(at);
                        // Unknown until the board settles.
                        let anchor = boards
                            .iter()
                            .find(|b| &b.art.id == board)
                            .and_then(|b| b.marks.anchor(n.n));
                        v["placed"] = json!(anchor.is_some_and(|a| a.rect.is_some()));
                        if anchor.is_some_and(|a| a.rect.is_none()) {
                            v["error"] = json!(format!("nothing in {board} matches {at:?}"));
                        }
                    }
                }
            }
            v
        })
        .collect();
    json!({
        "sections": annot.sections.iter().map(|s| json!({
            "id": s.id, "title": s.title, "boards": s.boards, "note": s.note,
        })).collect::<Vec<_>>(),
        "links": annot.links.iter().map(|l| json!({
            "from": l.from, "to": l.to, "label": l.label,
        })).collect::<Vec<_>>(),
        "notes": notes,
        "flows": annot.flows.iter().map(|f| json!({
            "id": f.id, "title": f.title, "start": f.start, "note": f.note,
        })).collect::<Vec<_>>(),
    })
}

/// A board's `exits` (each with an `error` when it leads nowhere) and the
/// boards its plain links lead to (`board_links`).
pub fn exits(board: &Board, boards: &[Board]) -> (Value, Value) {
    let exists = |id: &str| boards.iter().any(|b| b.art.id == id);
    let exits: Vec<Value> = board
        .marks
        .exits
        .iter()
        .map(|e| {
            let error = if e.to == board.art.id {
                Some("leads to its own artboard".to_string())
            } else if !exists(&e.to) {
                Some(format!("no artboard {:?}", e.to))
            } else {
                None
            };
            json!({"to": e.to, "label": e.label, "text": e.text, "rect": e.rect, "error": error})
        })
        .collect();
    let urls: Vec<(&str, &str)> = boards.iter().map(|b| (b.art.id.as_str(), b.art.url.as_str())).collect();
    let linked: BTreeSet<&str> = board
        .marks
        .links
        .iter()
        .filter_map(|l| marks::board_for_url(&l.href, &urls, &board.art.id))
        .collect();
    (json!(exits), json!(linked))
}
