//! The Connections window: the machines this one is paired to view, each
//! opening in its own window, and a field that finds one or names a new
//! one to pair (`sola-kvm pair <host> --view`, over ssh). Laid out as the
//! kit's navigator, as Projects and Design pick theirs.

use iced::Element;
use iced::keyboard::key::Named;
use sola_kit::components::navigator::{note_row, page, pick_row};
use sola_kit::components::text_input::text_input;

/// The field's id (focused when the window opens).
pub const FIELD_ID: &str = "remote-desktop-connections-field";

/// A row, in the order shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A paired machine.
    Machine(String),
    /// A name typed that is not paired yet.
    Pair(String),
}

#[derive(Debug, Clone)]
pub enum Msg {
    Query(String),
    Pick(usize),
    Key(Named),
}

/// The window's state.
#[derive(Debug, Default)]
pub struct Connections {
    /// What the field holds.
    pub query: String,
    pub selected: usize,
    /// The machine being paired, while `sola-kvm pair` runs.
    pub pairing: Option<String>,
    /// What the last pairing or forgetting said went wrong.
    pub error: Option<String>,
}

impl Connections {
    /// The paired machines whose name holds the query (case aside), then a
    /// row to pair the name typed when it is none of theirs.
    pub fn rows(&self, machines: &[String]) -> Vec<Row> {
        let typed = self.query.trim();
        let find = typed.to_lowercase();
        let mut rows: Vec<Row> = machines
            .iter()
            .filter(|m| m.to_lowercase().contains(&find))
            .map(|m| Row::Machine(m.clone()))
            .collect();
        if is_host_name(typed) && !machines.iter().any(|m| m.eq_ignore_ascii_case(typed)) {
            rows.push(Row::Pair(typed.to_string()));
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
        self.selected = (self.selected as i32 + by).clamp(0, len as i32 - 1) as usize;
    }

    /// The machine the selected row names, when it is a paired one.
    pub fn selected_machine(&self, machines: &[String]) -> Option<String> {
        match self.rows(machines).into_iter().nth(self.selected) {
            Some(Row::Machine(m)) => Some(m),
            _ => None,
        }
    }
}

/// What ssh would take as a host: no spaces, no path, not an option.
fn is_host_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && !name.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\')
}

/// The heading, the field, the rows. `open` names the machines with a
/// window up.
pub fn view<'a>(list: &'a Connections, machines: &[String], open: &[&str]) -> Element<'a, Msg> {
    let field = text_input("Find a machine, or name one to pair", &list.query)
        .id(iced::widget::Id::new(FIELD_ID))
        .on_input(Msg::Query)
        .on_submit(Msg::Pick(list.selected))
        .size(15)
        .padding([9, 12]);

    let rows = list.rows(machines);
    let mut items: Vec<Element<'a, Msg>> = Vec::new();
    if let Some(error) = &list.error {
        items.push(note_row("lucide/triangle-exclamation-point", error.clone()));
    }
    if let Some(host) = &list.pairing {
        items.push(note_row("lucide/loader", format!("Pairing with {host}…")));
    } else if machines.is_empty() && rows.is_empty() {
        items.push(note_row(
            "lucide/screen-share",
            "No machine is paired yet. Type one's name to pair it over ssh: Sola on Linux, or a Mac with the agent."
                .into(),
        ));
    } else if rows.is_empty() {
        items.push(note_row("lucide/search", "Nothing here matches".into()));
    }
    for (i, row) in rows.into_iter().enumerate() {
        let selected = i == list.selected;
        items.push(match row {
            Row::Machine(machine) => {
                let note = open.contains(&machine.as_str()).then(|| "Open".to_string());
                pick_row(Msg::Pick(i), selected, 0.0, "lucide/monitor", machine, note)
            }
            Row::Pair(host) => pick_row(
                Msg::Pick(i),
                selected,
                0.0,
                "lucide/plus",
                format!("Pair with {host}"),
                Some("over ssh".into()),
            ),
        });
    }
    page("Connections".into(), "Esc closes this window", field, items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machines() -> Vec<String> {
        vec!["ember".into(), "koeurax".into()]
    }

    #[test]
    fn the_field_finds_machines_and_offers_to_pair_a_new_name() {
        let mut list = Connections::default();
        assert_eq!(
            list.rows(&machines()),
            [Row::Machine("ember".into()), Row::Machine("koeurax".into())]
        );
        list.set_query("KOE".into());
        assert_eq!(
            list.rows(&machines()),
            [Row::Machine("koeurax".into()), Row::Pair("KOE".into())]
        );
        list.set_query("Koeurax".into());
        assert_eq!(list.rows(&machines()), [Row::Machine("koeurax".into())]);
        list.set_query("canto".into());
        assert_eq!(list.rows(&machines()), [Row::Pair("canto".into())]);
        list.set_query("not a host".into());
        assert!(list.rows(&machines()).is_empty());
    }

    #[test]
    fn the_selection_names_a_paired_machine_only() {
        let mut list = Connections::default();
        list.set_query("e".into());
        assert_eq!(list.selected_machine(&machines()), Some("ember".into()));
        list.step(1, 3);
        assert_eq!(list.selected_machine(&machines()), Some("koeurax".into()));
        list.step(1, 3);
        assert_eq!(list.selected_machine(&machines()), None, "the row to pair \"e\"");
        list.step(5, 3);
        assert_eq!(list.selected, 2);
    }
}
