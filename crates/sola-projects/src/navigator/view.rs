//! Drawing a navigator: the step's heading, the field, then its rows —
//! each folder (one open here says so) with its sessions under it, New,
//! and On another computer…; or the recent hosts and the typed one.

use std::collections::HashSet;
use std::path::PathBuf;

use iced::{Element, window};
use sola_kit::components::navigator::{INDENT, note_row, page, pick_row};
use sola_kit::components::text_input::text_input;

use super::hosts::HostRow;
use super::{Navigator, Row};
use crate::{Msg, NavMsg};

/// The field's id (focused when a navigator opens or changes step).
pub const FIELD_ID: &str = "projects-navigator-field";

/// What is open here, to mark rows: projects by `(host, root)`, sessions
/// by key (`<id>`, `<id>@<host>`).
#[derive(Default)]
pub struct Open {
    pub roots: HashSet<(Option<String>, PathBuf)>,
    pub sessions: HashSet<String>,
}

pub fn view<'a>(nav: &'a Navigator, window: window::Id, open: &Open) -> Element<'a, Msg> {
    let field = text_input(nav.placeholder(), &nav.query)
        .id(iced::widget::Id::new(FIELD_ID))
        .on_input(move |q| Msg::Nav(window, NavMsg::Query(q)))
        .on_submit(Msg::Nav(window, NavMsg::Pick(nav.selected)))
        .size(15)
        .padding([9, 12]);

    let mut rows: Vec<Element<'a, Msg>> = Vec::new();
    if let Some(busy) = &nav.busy {
        rows.push(note_row("lucide/loader", busy.note()));
    } else if let Some(hosts) = &nav.hosts {
        if let Some(error) = &hosts.error {
            rows.push(note_row("lucide/unplug", error.clone()));
        } else if hosts.rows().is_empty() {
            rows.push(note_row("lucide/monitor", "Type a computer's name to open a project on it".into()));
        }
        for (i, r) in hosts.rows().into_iter().enumerate() {
            let (icon_name, label, note) = match r {
                HostRow::Recent(h) => ("lucide/monitor", hosts.recent[h].clone(), None),
                HostRow::Typed => (
                    "lucide/monitor-up",
                    hosts.typed().unwrap_or_default().to_string(),
                    Some("Connect".to_string()),
                ),
            };
            rows.push(pick_row(pick(window, i), i == nav.selected, 0.0, icon_name, label, note));
        }
    } else {
        if let Some(error) = &nav.error {
            rows.push(note_row("lucide/circle-alert", error.clone()));
        }
        let f = &nav.folders;
        for (i, r) in f.rows().into_iter().enumerate() {
            let selected = i == nav.selected;
            let row = match r {
                Row::Folder(at) => {
                    let folder = &f.folders[at];
                    let is_open = open.roots.contains(&(f.host.clone(), folder.path.clone()));
                    let icon_name = if is_open { "lucide/folder-open" } else { "lucide/folder" };
                    let note = is_open.then(|| "Open".to_string());
                    pick_row(pick(window, i), selected, 0.0, icon_name, folder.name.clone(), note)
                }
                Row::Session(_, s) => {
                    let session = &f.sessions[s];
                    let key = crate::catalog::key_for(&session.id, f.host.as_deref());
                    let note = open.sessions.contains(&key).then(|| "Open".to_string());
                    pick_row(pick(window, i), selected, INDENT, "lucide/terminal", session.label(), note)
                }
                Row::New => {
                    let on = f.host.as_deref().map(|h| format!(" on {h}")).unwrap_or_default();
                    let name = nav.query.trim();
                    let label = match (f.naming, f.new_name()) {
                        (true, _) if name.is_empty() => format!("A name for its folder in ~/Projects{on}"),
                        (true, _) => format!("Create ~/Projects/{name}{on}"),
                        (false, Some(n)) => format!("New project “{n}”{on}"),
                        (false, None) => format!("New project{on}…"),
                    };
                    pick_row(pick(window, i), selected, 0.0, "lucide/folder-plus", label, None)
                }
                Row::Remote => pick_row(
                    pick(window, i),
                    selected,
                    0.0,
                    "lucide/monitor",
                    "On another computer…".into(),
                    None,
                ),
            };
            rows.push(row);
        }
    }

    let hint = match (&nav.hosts, nav.folders.naming, &nav.folders.host) {
        (None, false, None) => "Esc closes this window",
        _ => "Esc goes back",
    };
    page(nav.heading(), hint, field, rows)
}

fn pick(window: window::Id, i: usize) -> Msg {
    Msg::Nav(window, NavMsg::Pick(i))
}
