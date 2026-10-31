//! The desk menu (`docs/specs/2026-10-31-sola-vault-design.md`): the
//! right-click menu the shell draws for every kit app.
//!
//! An app sends `Topic::DeskMenuOpen` at a right-click; the shell shows
//! the app's rows, Edit, and Vault ▸ in an editable place, as a panel of
//! the menu overlay ([`crate::app::Panel::DeskMenu`]) over the window the
//! app is in, and answers with one `Topic::DeskMenuPick`. A vault value
//! never rides the bus: the daemon delivers it to the app's process and
//! the pick carries a ticket ([`vault`]).
//!
//! This module is the menu's state: its columns (the first, a submenu, an
//! item's values), the highlighted row in each, and what keys and
//! pointer do to them. The shell's side is `crate::app::desk`.

pub mod place;
pub mod rows;
pub mod vault;
pub mod view;

use std::collections::HashMap;
use std::time::Instant;

use sola_bus::topics::{DeskMenuOpen, DeskPick};
use sola_vault_proto::{DeliverField, ItemSummary};

pub use rows::{Does, Row, Sub};
pub use vault::{ItemFields, Search, Ticket, VaultView};

/// The search field's widget id.
pub const SEARCH_INPUT_ID: &str = "desk-menu-search";

/// Where a pick goes, kept past the menu (a delivery answers after it has
/// closed).
#[derive(Debug, Clone)]
pub struct Target {
    pub app_id: String,
    pub pid: u32,
    pub token: u64,
    /// The app's window, to give the keyboard back to.
    pub window: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum DeskMsg {
    Hover { level: usize, row: usize },
    Press { level: usize, row: usize },
    /// A press outside the sheets.
    Dismiss,
    SearchInput(String),
    /// The vault's state (and the site's logins) for menu `generation`.
    Asked { generation: u64, view: VaultView },
    /// Every item, for the search of menu `generation`.
    Listed {
        generation: u64,
        items: Result<Vec<ItemSummary>, String>,
    },
    /// An item's values (labels only), for menu `generation`.
    Fields {
        generation: u64,
        item: String,
        fields: ItemFields,
    },
    Delivered {
        to: Target,
        result: Result<Ticket, String>,
    },
}

/// The keys the menu takes (Escape is the shell's grabbed key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
}

/// What a key or a press comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Nothing,
    /// The highlight or the columns changed.
    Moved,
    /// Done: this goes back to the app.
    Pick(DeskPick),
    /// Done: a vault value for the app.
    Deliver { item: String, field: DeliverField },
    /// Done: Vault comes forward (sign in, unlock).
    OpenVault,
    /// Done: Vault comes forward on this item (a re-prompt item).
    OpenItem(String),
    /// The Vault column became the search: it needs the list and the
    /// keyboard.
    Search,
}

/// One open desk menu.
pub struct DeskMenu {
    /// The app's request.
    pub open: DeskMenuOpen,
    /// The app's window it hangs over.
    pub window: Option<u32>,
    /// The click, in output pixels.
    pub anchor: (f32, f32),
    pub opened_at: Instant,
    /// Tells this menu's vault answers from an earlier one's.
    pub generation: u64,
    pub vault: VaultView,
    pub search: Option<Search>,
    /// Search results shown at once (as many as fit the output).
    pub found_max: usize,
    /// The values of the items whose columns opened, by item id.
    pub fields: HashMap<String, ItemFields>,
    /// The first column (fixed for the menu's life).
    main: Vec<Row>,
    /// The row whose submenu is open, in each column from the first.
    open_rows: Vec<usize>,
    /// The highlighted row in each column shown.
    hot: Vec<Option<usize>>,
    /// The column the keys move in.
    level: usize,
    /// The search field has the keyboard (while the keys are in the
    /// results; in a result's values the arrows are the menu's).
    pub field_focused: bool,
}

impl DeskMenu {
    pub fn new(
        open: DeskMenuOpen,
        main: Vec<Row>,
        window: Option<u32>,
        anchor: (f32, f32),
        generation: u64,
    ) -> Self {
        Self {
            open,
            window,
            anchor,
            opened_at: Instant::now(),
            generation,
            vault: VaultView::Asking,
            search: None,
            found_max: vault::FOUND_MIN,
            fields: HashMap::new(),
            main,
            open_rows: Vec::new(),
            hot: vec![None],
            level: 0,
            field_focused: false,
        }
    }

    /// The column the keys move in.
    pub fn level(&self) -> usize {
        self.level
    }

    /// The search field should have the keyboard: the keys are in the
    /// results.
    pub fn wants_field(&self) -> bool {
        self.level == 1 && self.searching(1)
    }

    pub fn target(&self) -> Target {
        Target {
            app_id: self.open.app_id.clone(),
            pid: self.open.pid,
            token: self.open.token,
            window: self.window,
        }
    }

    /// The columns shown, from the first.
    pub fn columns(&self) -> Vec<Vec<Row>> {
        let mut cols = vec![self.main.clone()];
        for (level, &row) in self.open_rows.iter().enumerate() {
            let Some(sub) = cols[level].get(row).and_then(Row::sub) else {
                break;
            };
            cols.push(rows::sub_rows(
                &self.open,
                sub,
                &self.vault,
                self.search.as_ref(),
                &self.fields,
            ));
        }
        cols
    }

    /// Column `level` is the search (its field on top).
    pub fn searching(&self, level: usize) -> bool {
        level == 1
            && self.search.is_some()
            && self
                .open_rows
                .first()
                .and_then(|&row| self.main.get(row))
                .and_then(Row::sub)
                == Some(Sub::Vault)
    }

    pub fn hot(&self, level: usize) -> Option<usize> {
        self.hot.get(level).copied().flatten()
    }

    /// The row open in column `level`, if a submenu hangs from it.
    pub fn open_row(&self, level: usize) -> Option<usize> {
        self.open_rows.get(level).copied()
    }

    /// An item whose column is open and whose values the menu has not
    /// asked for: they are asked for now (marked [`ItemFields::Asking`]).
    pub fn wanted_fields(&mut self) -> Option<String> {
        let cols = self.columns();
        let id = self.open_rows.iter().enumerate().find_map(|(level, &row)| {
            let sub = cols.get(level)?.get(row)?.sub()?;
            rows::item_of(sub, &self.vault, self.search.as_ref()).map(|i| i.id.clone())
        })?;
        if self.fields.contains_key(&id) {
            return None;
        }
        self.fields.insert(id.clone(), ItemFields::Asking);
        Some(id)
    }

    /// An item's values came: when the keys are in its column with
    /// nothing highlighted (it said it was reading), the first is.
    pub fn fields_came(&mut self, item: String, fields: ItemFields) {
        self.fields.insert(item.clone(), fields);
        self.settle();
        let level = self.level;
        if level == 0 || self.hot(level).is_some() {
            return;
        }
        let cols = self.columns();
        let showing = self
            .open_row(level - 1)
            .and_then(|row| cols[level - 1].get(row)?.sub())
            .and_then(|sub| rows::item_of(sub, &self.vault, self.search.as_ref()))
            .is_some_and(|i| i.id == item);
        if showing {
            self.hot[level] = cols[level].iter().position(Row::is_live);
        }
    }

    /// Keep the highlight and the open rows inside what the columns hold
    /// now (the vault answered, the search narrowed).
    pub fn settle(&mut self) {
        let cols = self.columns();
        self.open_rows.truncate(cols.len() - 1);
        self.hot.resize(cols.len(), None);
        self.hot.truncate(cols.len());
        for (level, col) in cols.iter().enumerate() {
            if self.hot[level].is_some_and(|i| !col.get(i).is_some_and(Row::is_live)) {
                self.hot[level] = None;
            }
        }
        self.level = self.level.min(cols.len() - 1);
    }

    /// The pointer is over row `row` of column `level`: it is the
    /// highlight, and its submenu (if any) opens.
    pub fn hover(&mut self, level: usize, row: usize) -> Outcome {
        let cols = self.columns();
        let Some(r) = cols.get(level).and_then(|c| c.get(row)) else {
            return Outcome::Nothing;
        };
        if !r.is_live() || (self.hot(level) == Some(row) && self.level == level) {
            return Outcome::Nothing;
        }
        self.level = level;
        self.open_rows.truncate(level);
        self.hot.truncate(level + 1);
        self.hot[level] = Some(row);
        if r.sub().is_some() {
            self.open_rows.push(row);
            self.hot.push(None);
        }
        Outcome::Moved
    }

    /// A press on row `row` of column `level`.
    pub fn press(&mut self, level: usize, row: usize) -> Outcome {
        let cols = self.columns();
        let Some(Row::Item(item)) = cols.get(level).and_then(|c| c.get(row)) else {
            return Outcome::Nothing;
        };
        if !item.enabled {
            return Outcome::Nothing;
        }
        match &item.does {
            Does::Pick(pick) => Outcome::Pick(pick.clone()),
            Does::Sub(_) => {
                self.enter(level, row);
                Outcome::Moved
            }
            Does::Deliver { item, field } => Outcome::Deliver {
                item: item.clone(),
                field: field.clone(),
            },
            Does::OpenVault => Outcome::OpenVault,
            Does::OpenItem(id) => Outcome::OpenItem(id.clone()),
            Does::Search => {
                self.search = Some(Search::new(self.found_max));
                self.open_rows.truncate(level);
                self.hot.truncate(level + 1);
                self.hot[level] = None;
                self.level = level;
                Outcome::Search
            }
            Does::Nothing => Outcome::Nothing,
        }
    }

    /// Open row `row`'s submenu and move the keys into it.
    fn enter(&mut self, level: usize, row: usize) {
        self.open_rows.truncate(level);
        self.hot.truncate(level + 1);
        self.hot[level] = Some(row);
        self.open_rows.push(row);
        self.level = level + 1;
        let first = self
            .columns()
            .get(level + 1)
            .and_then(|col| col.iter().position(Row::is_live));
        self.hot.push(first);
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        let cols = self.columns();
        let level = self.level.min(cols.len() - 1);
        let col = &cols[level];
        let hot = self.hot(level);
        match key {
            Key::Up | Key::Down => {
                let next = step(col, hot, key == Key::Down);
                if next == hot {
                    return Outcome::Nothing;
                }
                self.open_rows.truncate(level);
                self.hot.truncate(level + 1);
                self.hot[level] = next;
                Outcome::Moved
            }
            Key::Right => match hot.filter(|i| col[*i].sub().is_some()) {
                Some(i) => {
                    self.enter(level, i);
                    Outcome::Moved
                }
                None => Outcome::Nothing,
            },
            Key::Left => {
                if self.searching(level) {
                    // The field's own caret, until it is empty: then back
                    // to the vault's rows.
                    if self.search.as_ref().is_some_and(|s| !s.query.is_empty()) {
                        return Outcome::Nothing;
                    }
                    self.search = None;
                    self.settle();
                    return Outcome::Moved;
                }
                if level == 0 {
                    return Outcome::Nothing;
                }
                self.open_rows.truncate(level - 1);
                self.hot.truncate(level);
                self.level = level - 1;
                Outcome::Moved
            }
            Key::Enter => match hot {
                Some(i) => self.press(level, i),
                None => Outcome::Nothing,
            },
        }
    }

    /// Where the sheets go on an output of `output` pixels.
    pub fn placed(&self, output: (f32, f32)) -> place::Placed {
        let cols = self.columns();
        let measure = |s: &str, size: f32| sola_kit::fonts::line_width(s, size, sola_kit::fonts::chrome());
        let sizes: Vec<(f32, f32)> = cols
            .iter()
            .enumerate()
            .map(|(level, col)| {
                let searching = self.searching(level);
                let h = if searching {
                    place::search_height(self.found_max)
                } else {
                    place::height(col, false)
                };
                (place::width(col, searching, measure), h)
            })
            .collect();
        let tops: Vec<f32> = (0..cols.len() - 1)
            .map(|level| {
                let row = self.open_row(level).unwrap_or(0);
                place::row_top(&cols[level], row, self.searching(level))
            })
            .collect();
        place::place(output, self.anchor, &sizes, &tops)
    }

    /// The search's query changed: the first result is the highlight.
    pub fn set_query(&mut self, query: String) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        search.query = query;
        self.open_rows.truncate(1);
        self.hot.truncate(2);
        self.level = 1;
        let first = self
            .columns()
            .get(1)
            .and_then(|col| col.iter().position(Row::is_live));
        self.hot.resize(2, None);
        self.hot[1] = first;
    }
}

/// The next live row from `from`, wrapping; `from` if there is none.
fn step(col: &[Row], from: Option<usize>, down: bool) -> Option<usize> {
    let n = col.len();
    if n == 0 {
        return from;
    }
    let mut i = from.unwrap_or(if down { n - 1 } else { 0 });
    for _ in 0..n {
        i = if down { (i + 1) % n } else { (i + n - 1) % n };
        if col[i].is_live() {
            return Some(i);
        }
    }
    from
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_menu::rows::tests::{login, login_values, open};
    use sola_bus::topics::{DeskEdit, DeskMenuContext, DeskMenuItem};

    fn menu(site: Option<&str>) -> DeskMenu {
        let o = open(
            vec![DeskMenuItem::Action {
                id: "reload".into(),
                label: "Reload".into(),
                shortcut: None,
                enabled: true,
            }],
            vec![DeskEdit::Paste],
            DeskMenuContext {
                editable: true,
                secret: false,
                selection: false,
                site: site.map(str::to_string),
            },
        );
        let main = rows::main_rows(&o, |_| None);
        DeskMenu::new(o, main, Some(7), (100.0, 100.0), 1)
    }

    // Reload — Paste — Vault
    const VAULT_ROW: usize = 4;

    #[test]
    fn keys_walk_the_live_rows() {
        let mut m = menu(None);
        assert_eq!(m.key(Key::Down), Outcome::Moved);
        assert_eq!(m.hot(0), Some(0));
        m.key(Key::Down);
        assert_eq!(m.hot(0), Some(2), "the separator is skipped");
        m.key(Key::Down);
        m.key(Key::Down);
        assert_eq!(m.hot(0), Some(0), "wraps");
        m.key(Key::Up);
        assert_eq!(m.hot(0), Some(VAULT_ROW));
        assert_eq!(m.key(Key::Enter), Outcome::Moved, "Vault opens");
        assert_eq!(m.columns().len(), 2);
        m.key(Key::Left);
        assert_eq!(m.columns().len(), 1);
        assert_eq!(m.hot(0), Some(VAULT_ROW));
    }

    #[test]
    fn enter_on_an_app_row_picks_it() {
        let mut m = menu(None);
        m.key(Key::Down);
        assert_eq!(m.key(Key::Enter), Outcome::Pick(DeskPick::Action("reload".into())));
        m.key(Key::Down);
        assert_eq!(m.key(Key::Enter), Outcome::Pick(DeskPick::Edit(DeskEdit::Paste)));
    }

    #[test]
    fn hovering_vault_opens_it_and_another_row_closes_it() {
        let mut m = menu(None);
        assert_eq!(m.hover(0, VAULT_ROW), Outcome::Moved);
        assert_eq!(m.columns().len(), 2);
        assert_eq!(m.open_row(0), Some(VAULT_ROW));
        m.hover(0, 0);
        assert_eq!(m.columns().len(), 1);
        // A separator takes no highlight.
        assert_eq!(m.hover(0, 1), Outcome::Nothing);
    }

    #[test]
    fn a_login_value_is_delivered() {
        let mut m = menu(Some("https://a.example"));
        m.vault = VaultView::Unlocked {
            logins: vec![login("a", true)],
        };
        m.hover(0, VAULT_ROW);
        m.hover(1, 0);
        assert_eq!(m.wanted_fields().as_deref(), Some("a"), "its values are asked for");
        assert_eq!(m.wanted_fields(), None, "once");
        m.fields_came("a".into(), login_values(true));
        let cols = m.columns();
        assert_eq!(cols.len(), 3);
        // Fill — Username Password Code
        assert_eq!(m.press(2, 3), Outcome::Deliver {
            item: "a".into(),
            field: DeliverField::Field { key: "password".into() }
        });
        assert_eq!(m.press(2, 0), Outcome::Pick(DeskPick::VaultFill { item: "a".into() }));
    }

    #[test]
    fn values_that_come_while_the_keys_wait_take_the_highlight() {
        let mut m = menu(None);
        m.vault = VaultView::Unlocked { logins: vec![] };
        m.hover(0, VAULT_ROW);
        m.press(1, 0);
        m.search.as_mut().unwrap().items = Some(Ok(vec![login("alpha", false)]));
        m.set_query(String::new());
        assert_eq!(m.key(Key::Right), Outcome::Moved);
        assert_eq!(m.level(), 2);
        assert_eq!(m.hot(2), None, "the column is reading");
        assert_eq!(m.wanted_fields().as_deref(), Some("alpha"));
        m.fields_came("other".into(), login_values(false));
        assert_eq!(m.hot(2), None, "another item's values");
        m.fields_came("alpha".into(), login_values(false));
        assert_eq!(m.hot(2), Some(0));
        assert_eq!(m.key(Key::Enter), Outcome::Deliver {
            item: "alpha".into(),
            field: DeliverField::Field { key: "username".into() }
        });
    }

    #[test]
    fn a_re_prompt_login_keys_to_open_in_vault() {
        let mut m = menu(Some("https://a.example"));
        let mut item = login("a", false);
        item.reprompt = true;
        m.vault = VaultView::Unlocked { logins: vec![item] };
        m.fields.insert("a".into(), login_values(false));
        m.hover(0, VAULT_ROW);
        m.hover(1, 0);
        m.key(Key::Right);
        // Fill — Username Password — the note, Open in Vault…
        assert_eq!(m.press(2, 0), Outcome::Nothing, "Fill is dimmed");
        assert_eq!(m.press(2, 3), Outcome::Nothing, "Password is dimmed");
        assert_eq!(m.hot(2), Some(6), "Open in Vault… is the first live row");
        assert_eq!(m.key(Key::Enter), Outcome::OpenItem("a".into()));
    }

    #[test]
    fn search_takes_over_the_vault_column() {
        let mut m = menu(None);
        m.vault = VaultView::Unlocked { logins: vec![] };
        m.hover(0, VAULT_ROW);
        assert_eq!(m.press(1, 0), Outcome::Search);
        assert!(m.searching(1));
        assert_eq!(m.search.as_ref().unwrap().max, m.found_max);
        m.search.as_mut().unwrap().items = Some(Ok(vec![login("alpha", false), login("beta", true)]));
        m.fields.insert("beta".into(), login_values(true));
        m.set_query("bet".into());
        assert_eq!(m.hot(1), Some(0));
        assert_eq!(m.key(Key::Enter), Outcome::Moved);
        assert_eq!(m.columns()[2].len(), 3, "Username, Password, One-time code");
        // Back out of the values, then out of an empty search.
        m.key(Key::Left);
        assert_eq!(m.key(Key::Left), Outcome::Nothing, "the query keeps ←");
        m.set_query(String::new());
        assert_eq!(m.key(Key::Left), Outcome::Moved);
        assert!(!m.searching(1));
    }

    #[test]
    fn a_vault_answer_that_drops_rows_keeps_the_menu_sound() {
        let mut m = menu(Some("https://a.example"));
        m.vault = VaultView::Unlocked {
            logins: vec![login("a", false), login("b", false)],
        };
        m.hover(0, VAULT_ROW);
        m.hover(1, 1);
        assert_eq!(m.columns().len(), 3);
        m.vault = VaultView::Locked;
        m.settle();
        assert_eq!(m.columns().len(), 2);
        assert_eq!(m.hot(1), None);
    }
}
