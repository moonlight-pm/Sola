//! Drawing a navigator: the step's heading, the field, then its rows —
//! each folder (one open here says so) with its sessions under it, New,
//! and On another computer…; or the recent hosts and the typed one.

use std::collections::HashSet;
use std::path::PathBuf;

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding, window};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon;
use sola_kit::components::scroll;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use super::hosts::HostRow;
use super::{Navigator, Row};
use crate::{Msg, NavMsg};

/// The field's id (focused when a navigator opens or changes step).
pub const FIELD_ID: &str = "projects-navigator-field";
/// The list's width at most: a list to read, not a page.
const MAX_W: f32 = 560.0;
/// How far a session's row sits in from its folder's.
const SESSION_INDENT: f32 = 22.0;
const ROW_ICON: u16 = 16;

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
            rows.push(pick_row(window, i, i == nav.selected, 0.0, icon_name, label, note));
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
                    pick_row(window, i, selected, 0.0, icon_name, folder.name.clone(), note)
                }
                Row::Session(_, s) => {
                    let session = &f.sessions[s];
                    let key = crate::catalog::key_for(&session.id, f.host.as_deref());
                    let note = open.sessions.contains(&key).then(|| "Open".to_string());
                    pick_row(window, i, selected, SESSION_INDENT, "lucide/terminal", session.label(), note)
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
                    pick_row(window, i, selected, 0.0, "lucide/folder-plus", label, None)
                }
                Row::Remote => pick_row(
                    window,
                    i,
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

    let hint = text(match (&nav.hosts, nav.folders.naming, &nav.folders.host) {
        (None, false, None) => "Esc closes this window",
        _ => "Esc goes back",
    })
    .font(fonts::chrome())
    .size(12)
    .style(kit_text::muted);

    let list = scroll::scrollable(column(rows).spacing(2)).height(Length::Fill);
    let body = column![
        row![
            text(nav.heading()).font(fonts::display()).size(20),
            Space::new().width(Length::Fill),
            hint
        ]
        .align_y(Alignment::Center),
        field,
        list,
    ]
    .spacing(SPACE_MD + SPACE_SM)
    .max_width(MAX_W);

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::new(28.0).bottom(12.0))
        .align_x(Alignment::Center)
        .into()
}

/// A pickable row: an icon, a label, and a note at the right.
fn pick_row<'a>(
    window: window::Id,
    i: usize,
    selected: bool,
    indent: f32,
    icon_name: &str,
    label: String,
    note: Option<String>,
) -> Element<'a, Msg> {
    let mut content = row![icon(icon_name, ROW_ICON), text(label).font(fonts::ui()).size(14)]
        .spacing(10)
        .align_y(Alignment::Center);
    if let Some(note) = note {
        content = content
            .push(Space::new().width(Length::Fill))
            .push(text(note).font(fonts::chrome()).size(12).style(kit_text::muted));
    }
    button(content)
        .on_press(Msg::Nav(window, NavMsg::Pick(i)))
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 10.0 + indent,
            right: 12.0,
        })
        .width(Length::Fill)
        .style(kit_btn::list_item(selected))
        .into()
}

/// A row that says something and cannot be picked.
fn note_row<'a>(icon_name: &str, note: String) -> Element<'a, Msg> {
    container(
        row![
            icon(icon_name, ROW_ICON),
            text(note).font(fonts::chrome()).size(13).style(kit_text::muted),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::new(10.0))
    .width(Length::Fill)
    .into()
}
