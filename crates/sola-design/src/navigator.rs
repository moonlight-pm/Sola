//! A Design window with no project yet: where a project is opened from, as
//! in Projects (Joshua, 2026-11-10: "When i open a new design window, i'm
//! then presented (like projects) the list of project/design i can open to
//! view").
//!
//! The launcher's Design row asks for one (`window.navigate`), as does
//! Project → Open Project…. It lists the folders of `~/Projects` with
//! canvases in their `design/`, each with its canvases under it. Picking a
//! project opens its window, or brings it up; a canvas opens it there. The
//! navigator then closes. Esc closes it. Design makes no project and no
//! canvas: agents make them, and they show up here.

use std::collections::HashMap;

use iced::{Element, window};
use sola_kit::components::navigator::{INDENT, note_row, page, pick_row};
use sola_kit::components::text_input::text_input;

use crate::app::{Msg, NavMsg};
use crate::canvas::Entry;

/// The field's id (focused when a navigator opens).
pub const FIELD_ID: &str = "design-navigator-field";

/// A project as a navigator lists it.
pub struct Listed<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub canvases: &'a [Entry],
}

/// A row, in the order shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// `listed[i]`.
    Project(usize),
    /// `listed[i].canvases[c]`.
    Canvas(usize, usize),
}

/// What a pick asks Design to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Nothing,
    /// Open the project's window (on `canvas`, else the one it shows).
    Open {
        project: String,
        canvas: Option<String>,
    },
}

/// One navigator window's state.
#[derive(Debug, Default)]
pub struct Navigator {
    /// What the field holds.
    pub query: String,
    pub selected: usize,
}

impl Navigator {
    /// The projects whose name or a canvas's holds the query (case
    /// aside), each followed by its canvases — all of them when the
    /// project's name matched, else the ones that did.
    pub fn rows(&self, listed: &[Listed]) -> Vec<Row> {
        let q = self.query.trim().to_lowercase();
        let has = |name: &str| name.to_lowercase().contains(&q);
        let mut rows = Vec::new();
        for (i, p) in listed.iter().enumerate() {
            let all = has(p.name);
            let canvases: Vec<usize> = (0..p.canvases.len())
                .filter(|&c| all || has(&p.canvases[c].name))
                .collect();
            if all || !canvases.is_empty() {
                rows.push(Row::Project(i));
                rows.extend(canvases.into_iter().map(|c| Row::Canvas(i, c)));
            }
        }
        rows
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.selected = 0;
    }

    /// Up (`-1`) or down (`1`), held to the `len` rows.
    pub fn step(&mut self, by: i32, len: usize) {
        if len == 0 {
            return;
        }
        let at = self.selected as i32 + by;
        self.selected = at.clamp(0, len as i32 - 1) as usize;
    }

    /// Row `i` picked (Enter, or a click).
    pub fn pick(&self, listed: &[Listed], i: usize) -> Pick {
        match self.rows(listed).get(i).copied() {
            Some(Row::Project(p)) => Pick::Open {
                project: listed[p].id.to_string(),
                canvas: None,
            },
            Some(Row::Canvas(p, c)) => Pick::Open {
                project: listed[p].id.to_string(),
                canvas: Some(listed[p].canvases[c].name.clone()),
            },
            None => Pick::Nothing,
        }
    }
}

/// A navigator: the heading, the field, the rows. `open` maps each project
/// with a window to the canvas it shows, to mark them.
pub fn view<'a>(
    nav: &'a Navigator,
    window: window::Id,
    listed: &[Listed],
    open: &HashMap<String, Option<String>>,
) -> Element<'a, Msg> {
    let field = text_input("Find a project or canvas", &nav.query)
        .id(iced::widget::Id::new(FIELD_ID))
        .on_input(move |q| Msg::Nav(window, NavMsg::Query(q)))
        .on_submit(Msg::Nav(window, NavMsg::Pick(nav.selected)))
        .size(15)
        .padding([9, 12]);
    let pick = |i: usize| Msg::Nav(window, NavMsg::Pick(i));

    let rows = nav.rows(listed);
    let mut items: Vec<Element<'a, Msg>> = Vec::new();
    if listed.is_empty() {
        items.push(note_row(
            "lucide/frame",
            "No designs yet. An agent makes them in ~/Projects/<project>/design/.".into(),
        ));
    } else if rows.is_empty() {
        items.push(note_row("lucide/search", "Nothing here matches".into()));
    }
    for (i, r) in rows.into_iter().enumerate() {
        let selected = i == nav.selected;
        items.push(match r {
            Row::Project(p) => {
                let is_open = open.contains_key(listed[p].id);
                let icon_name = if is_open { "lucide/folder-open" } else { "lucide/folder" };
                let note = is_open.then(|| "Open".to_string());
                pick_row(pick(i), selected, 0.0, icon_name, listed[p].name.to_string(), note)
            }
            Row::Canvas(p, c) => {
                let name = &listed[p].canvases[c].name;
                let shown = open
                    .get(listed[p].id)
                    .is_some_and(|s| s.as_deref() == Some(name.as_str()));
                let note = shown.then(|| "Open".to_string());
                pick_row(pick(i), selected, INDENT, "lucide/frame", name.clone(), note)
            }
        });
    }
    page("Designs".into(), "Esc closes this window", field, items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(names: &[&str]) -> Vec<Entry> {
        names.iter().map(|n| Entry { name: (*n).into() }).collect()
    }

    #[test]
    fn a_projects_canvases_follow_it_and_the_query_finds_either() {
        let (sola, wow) = (entries(&["foundation", "settings"]), entries(&["halo"]));
        let listed = [
            Listed { id: "proj-sola", name: "Sola", canvases: &sola },
            Listed { id: "proj-wow", name: "WoW", canvases: &wow },
        ];
        let mut n = Navigator::default();
        assert_eq!(
            n.rows(&listed),
            [Row::Project(0), Row::Canvas(0, 0), Row::Canvas(0, 1), Row::Project(1), Row::Canvas(1, 0)]
        );
        n.set_query("SETT".into());
        assert_eq!(n.rows(&listed), [Row::Project(0), Row::Canvas(0, 1)]);
        n.set_query("wow".into());
        assert_eq!(n.rows(&listed), [Row::Project(1), Row::Canvas(1, 0)]);
        n.set_query("zzz".into());
        assert!(n.rows(&listed).is_empty());
    }

    #[test]
    fn picking_opens_a_project_or_one_of_its_canvases() {
        let sola = entries(&["foundation"]);
        let listed = [Listed { id: "proj-sola", name: "Sola", canvases: &sola }];
        let n = Navigator::default();
        assert_eq!(
            n.pick(&listed, 0),
            Pick::Open { project: "proj-sola".into(), canvas: None }
        );
        assert_eq!(
            n.pick(&listed, 1),
            Pick::Open { project: "proj-sola".into(), canvas: Some("foundation".into()) }
        );
        assert_eq!(n.pick(&listed, 2), Pick::Nothing);
    }

    #[test]
    fn steps_stay_on_the_rows() {
        let mut n = Navigator::default();
        n.step(-1, 3);
        assert_eq!(n.selected, 0);
        n.step(10, 3);
        assert_eq!(n.selected, 2);
        n.step(1, 0);
        assert_eq!(n.selected, 2);
    }
}
