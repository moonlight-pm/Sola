//! What the desk menu's columns hold: the app's rows, Edit, and the
//! vault's, by the click's context. Pure, so the composition is tested
//! without a window.

use std::collections::HashMap;

use sola_bus::topics::{DeskEdit, DeskMenuContext, DeskMenuItem, DeskMenuOpen, DeskPick};
use sola_core::KeyChord;
use sola_vault_proto::{DeliverField, ItemKind, ItemSummary};

use super::vault::{ItemFields, Search, VaultView};

/// One row of a column.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Item(Item),
    Sep,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// At the right in quiet ink: a key, or a login's username.
    pub hint: Option<String>,
    pub enabled: bool,
    pub does: Does,
}

/// What choosing a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Does {
    /// Goes straight back to the app.
    Pick(DeskPick),
    /// Opens the next column.
    Sub(Sub),
    /// A vault value for the app's field: the daemon delivers it to the
    /// app's process, and the app gets a ticket.
    Deliver { item: String, field: DeliverField },
    /// Sign in or unlock: Vault comes forward.
    OpenVault,
    /// Vault comes forward on this item (a re-prompt item: its values
    /// wait for the master password there).
    OpenItem(String),
    /// The Vault column turns into a search over every item.
    Search,
    /// A note.
    Nothing,
}

/// The column a row opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    /// The app's submenu: its index in the app's rows.
    App(usize),
    Vault,
    /// A login of the site: its index in the vault's matches.
    Login(usize),
    /// A search result (an item of any kind): its index in what the
    /// search shows.
    Found(usize),
}

impl Row {
    pub fn item(&self) -> Option<&Item> {
        match self {
            Row::Item(item) => Some(item),
            Row::Sep => None,
        }
    }

    /// Takes the keyboard and the pointer's highlight.
    pub fn is_live(&self) -> bool {
        self.item().is_some_and(|i| i.enabled)
    }

    pub fn sub(&self) -> Option<Sub> {
        match self.item()? {
            Item {
                does: Does::Sub(sub),
                enabled: true,
                ..
            } => Some(*sub),
            _ => None,
        }
    }
}

fn row(label: &str, hint: Option<String>, enabled: bool, does: Does) -> Row {
    Row::Item(Item {
        label: label.to_string(),
        hint,
        enabled,
        does,
    })
}

fn note(label: &str) -> Row {
    row(label, None, false, Does::Nothing)
}

/// An Edit row's label.
pub fn edit_label(edit: DeskEdit) -> &'static str {
    match edit {
        DeskEdit::Cut => "Cut",
        DeskEdit::Copy => "Copy",
        DeskEdit::Paste => "Paste",
        DeskEdit::SelectAll => "Select All",
    }
}

/// The kit's shared action an Edit row is (its key is that action's).
pub fn edit_action(edit: DeskEdit) -> &'static str {
    match edit {
        DeskEdit::Cut => "cut",
        DeskEdit::Copy => "copy",
        DeskEdit::Paste => "paste",
        DeskEdit::SelectAll => "select_all",
    }
}

/// The first column: the app's rows, Edit in the app's order, then
/// Vault ▸ in an editable place. `key` gives an Edit row's key now.
pub fn main_rows(open: &DeskMenuOpen, key: impl Fn(DeskEdit) -> Option<KeyChord>) -> Vec<Row> {
    let mut groups: Vec<Vec<Row>> = Vec::new();
    let app: Vec<Row> = open
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| app_row(item, Does::Sub(Sub::App(i))))
        .collect();
    groups.push(app);
    groups.push(
        open.edit
            .iter()
            .map(|e| {
                row(
                    edit_label(*e),
                    key(*e).map(|k| k.display()),
                    true,
                    Does::Pick(DeskPick::Edit(*e)),
                )
            })
            .collect(),
    );
    if open.context.editable {
        groups.push(vec![row("Vault", None, true, Does::Sub(Sub::Vault))]);
    }
    join(groups)
}

/// An app's row; a submenu opens `sub`.
fn app_row(item: &DeskMenuItem, sub: Does) -> Row {
    match item {
        DeskMenuItem::Action {
            id,
            label,
            shortcut,
            enabled,
        } => row(
            label,
            shortcut.map(|k| k.display()),
            *enabled,
            Does::Pick(DeskPick::Action(id.clone())),
        ),
        DeskMenuItem::Separator => Row::Sep,
        DeskMenuItem::Submenu { label, items } => row(label, None, !items.is_empty(), sub),
    }
}

/// Groups with a separator between the ones that have rows; the app's
/// own separators kept, but never at an end or doubled.
fn join(groups: Vec<Vec<Row>>) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for group in groups.into_iter().filter(|g| g.iter().any(|r| *r != Row::Sep)) {
        if !rows.is_empty() {
            rows.push(Row::Sep);
        }
        rows.extend(group);
    }
    let mut out: Vec<Row> = Vec::with_capacity(rows.len());
    for r in rows {
        if r == Row::Sep && matches!(out.last(), None | Some(Row::Sep)) {
            continue;
        }
        out.push(r);
    }
    while out.last() == Some(&Row::Sep) {
        out.pop();
    }
    out
}

/// The item a column of values is for: a login of the site, or a search
/// result.
pub fn item_of<'a>(sub: Sub, vault: &'a VaultView, search: Option<&'a Search>) -> Option<&'a ItemSummary> {
    match sub {
        Sub::Login(i) => match vault {
            VaultView::Unlocked { logins } => logins.get(i),
            _ => None,
        },
        Sub::Found(i) => search.and_then(|s| s.found().get(i).copied()),
        Sub::App(_) | Sub::Vault => None,
    }
}

/// The column `sub` opens. `fields` holds the items' values the menu has
/// asked for, by item id.
pub fn sub_rows(
    open: &DeskMenuOpen,
    sub: Sub,
    vault: &VaultView,
    search: Option<&Search>,
    fields: &HashMap<String, ItemFields>,
) -> Vec<Row> {
    match sub {
        Sub::App(i) => match open.items.get(i) {
            // One level: a submenu inside a submenu is not opened.
            Some(DeskMenuItem::Submenu { items, .. }) => join(vec![
                items
                    .iter()
                    .map(|item| app_row(item, Does::Nothing))
                    .map(|r| match r {
                        Row::Item(mut item) if item.does == Does::Nothing => {
                            item.enabled = false;
                            Row::Item(item)
                        }
                        r => r,
                    })
                    .collect(),
            ]),
            _ => Vec::new(),
        },
        Sub::Vault => match search.map(|s| (s, &s.items)) {
            Some((_, None)) => vec![note("Reading the vault…")],
            Some((_, Some(Err(message)))) => vec![note(message)],
            Some((search, Some(Ok(_)))) => {
                let found: Vec<Row> = search
                    .found()
                    .iter()
                    .enumerate()
                    .map(|(i, item)| login_row(item, Sub::Found(i)))
                    .collect();
                if found.is_empty() {
                    vec![note("Nothing found")]
                } else {
                    found
                }
            }
            None => vault_rows(vault, &open.context),
        },
        // A search finds any site's items: Fill is only for the page's.
        Sub::Login(_) | Sub::Found(_) => {
            let fill = if matches!(sub, Sub::Login(_)) {
                Fill::Offered
            } else {
                Fill::NotHere
            };
            item_of(sub, vault, search)
                .map(|item| value_rows(item, fields.get(&item.id), &open.context, fill))
                .unwrap_or_default()
        }
    }
}

/// Vault ▸ by the vault's state.
fn vault_rows(vault: &VaultView, context: &DeskMenuContext) -> Vec<Row> {
    match vault {
        VaultView::Asking => vec![note("Asking the vault…")],
        VaultView::Unavailable => vec![note("Vault isn't running")],
        VaultView::SignedOut => vec![row("Sign in to Vault…", None, true, Does::OpenVault)],
        VaultView::Locked => vec![row("Unlock Vault…", None, true, Does::OpenVault)],
        VaultView::Failed(message) => vec![note(message)],
        VaultView::Unlocked { logins } => {
            let mut rows: Vec<Row> = Vec::new();
            if context.site.is_some() {
                rows.extend(
                    logins
                        .iter()
                        .enumerate()
                        .map(|(i, item)| login_row(item, Sub::Login(i))),
                );
                if !rows.is_empty() {
                    rows.push(Row::Sep);
                }
            }
            rows.push(row("Search vault…", None, true, Does::Search));
            rows
        }
    }
}

/// An item that opens its values (a login of the page, or any item the
/// search found).
fn login_row(item: &ItemSummary, sub: Sub) -> Row {
    let hint = Some(item.subtitle.clone()).filter(|s| !s.is_empty());
    row(&item.name, hint, true, Does::Sub(sub))
}

/// Whether a login's values offer Fill (the whole login into the page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// One of the page's own logins (the service matched it to the page).
    Offered,
    /// A search result: it may be any site's, and the service fills a
    /// page only with its own logins.
    NotHere,
}

/// A re-prompt item's note, over the row that opens it in Vault.
pub const REPROMPT_NOTE: &str = "Asks for your master password";
pub const OPEN_IN_VAULT: &str = "Open in Vault…";
/// An item's column while its values are on their way.
pub const READING_ITEM: &str = "Reading the item…";
pub const NO_VALUES: &str = "No values";

/// What can be put into the field from `item`: every value it has, by
/// label, as the service listed them (`fields`, `None` before it is
/// asked); on a page (the browser) the page's own logins fill whole too.
/// Any field takes any value. A re-prompt item's rows are there but
/// dimmed: its values wait for the master password, in Vault.
pub fn value_rows(
    item: &ItemSummary,
    fields: Option<&ItemFields>,
    context: &DeskMenuContext,
    fill: Fill,
) -> Vec<Row> {
    let live = !item.reprompt;
    let mut rows = Vec::new();
    if fill == Fill::Offered && context.site.is_some() && item.kind == ItemKind::Login {
        rows.push(row(
            "Fill",
            None,
            live,
            Does::Pick(DeskPick::VaultFill {
                item: item.id.clone(),
            }),
        ));
        rows.push(Row::Sep);
    }
    match fields {
        None | Some(ItemFields::Asking) => rows.push(note(READING_ITEM)),
        Some(ItemFields::Failed(message)) => rows.push(note(message)),
        Some(ItemFields::Listed(list)) if list.is_empty() => rows.push(note(NO_VALUES)),
        Some(ItemFields::Listed(list)) => rows.extend(list.iter().map(|f| {
            let field = DeliverField::Field { key: f.key.clone() };
            row(&f.label, None, live, Does::Deliver {
                item: item.id.clone(),
                field,
            })
        })),
    }
    if item.reprompt {
        rows.push(Row::Sep);
        rows.push(note(REPROMPT_NOTE));
        rows.push(row(OPEN_IN_VAULT, None, true, Does::OpenItem(item.id.clone())));
    }
    rows
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sola_core::KeyCode;

    pub(crate) fn open(items: Vec<DeskMenuItem>, edit: Vec<DeskEdit>, context: DeskMenuContext) -> DeskMenuOpen {
        DeskMenuOpen {
            app_id: "sola-mail".into(),
            pid: 42,
            token: 1,
            x: 10,
            y: 20,
            context,
            items,
            edit,
        }
    }

    pub(crate) fn login(id: &str, totp: bool) -> ItemSummary {
        ItemSummary {
            id: id.into(),
            kind: ItemKind::Login,
            name: format!("{id} site"),
            subtitle: format!("{id}@example.com"),
            last_used: 0,
            revised: 0,
            has_totp: totp,
            has_passkey: false,
            uri_match: true,
            reprompt: false,
            favorite: false,
            folder_id: None,
            organization_id: None,
            collection_ids: Vec::new(),
            deleted: false,
            search: id.into(),
        }
    }

    /// An item's values as the service lists them: `(key, label)`.
    pub(crate) fn listed(fields: &[(&str, &str)]) -> ItemFields {
        ItemFields::Listed(
            fields
                .iter()
                .map(|(key, label)| sola_vault_proto::FieldInfo {
                    key: key.to_string(),
                    label: label.to_string(),
                    hidden: *key == "password",
                })
                .collect(),
        )
    }

    /// A login's: username, password, and a code when it has one.
    pub(crate) fn login_values(totp: bool) -> ItemFields {
        let mut fields = vec![("username", "Username"), ("password", "Password")];
        if totp {
            fields.push(("totp", "One-time code"));
        }
        listed(&fields)
    }

    fn none() -> HashMap<String, ItemFields> {
        HashMap::new()
    }

    fn labels(rows: &[Row]) -> Vec<&str> {
        rows.iter()
            .map(|r| match r {
                Row::Item(i) => i.label.as_str(),
                Row::Sep => "—",
            })
            .collect()
    }

    fn field(editable: bool, secret: bool, site: Option<&str>) -> DeskMenuContext {
        DeskMenuContext {
            editable,
            secret,
            selection: false,
            site: site.map(str::to_string),
        }
    }

    #[test]
    fn a_field_gets_edit_then_the_vault() {
        let rows = main_rows(
            &open(vec![], vec![DeskEdit::Paste, DeskEdit::SelectAll], field(true, false, None)),
            |_| None,
        );
        assert_eq!(labels(&rows), ["Paste", "Select All", "—", "Vault"]);
        assert_eq!(rows[3].sub(), Some(Sub::Vault));
    }

    #[test]
    fn app_rows_come_first_and_a_read_only_place_has_no_vault() {
        let items = vec![
            DeskMenuItem::Action {
                id: "back".into(),
                label: "Back".into(),
                shortcut: None,
                enabled: false,
            },
            DeskMenuItem::Separator,
            DeskMenuItem::Action {
                id: "inspect".into(),
                label: "Inspect Element".into(),
                shortcut: None,
                enabled: true,
            },
            DeskMenuItem::Separator,
        ];
        let rows = main_rows(
            &open(items, vec![DeskEdit::Copy], field(false, false, None)),
            |e| (e == DeskEdit::Copy).then(|| KeyCode::C.meta()),
        );
        assert_eq!(labels(&rows), ["Back", "—", "Inspect Element", "—", "Copy"]);
        assert!(!rows[0].is_live());
        let Row::Item(copy) = &rows[4] else { panic!() };
        assert_eq!(copy.hint.as_deref(), Some(KeyCode::C.meta().display().as_str()));
        assert_eq!(copy.does, Does::Pick(DeskPick::Edit(DeskEdit::Copy)));
    }

    #[test]
    fn only_app_rows_have_no_stray_separator() {
        let items = vec![DeskMenuItem::Action {
            id: "copy".into(),
            label: "Copy".into(),
            shortcut: None,
            enabled: true,
        }];
        let rows = main_rows(&open(items, vec![], field(false, false, None)), |_| None);
        assert_eq!(labels(&rows), ["Copy"]);
    }

    #[test]
    fn an_app_submenu_is_one_level() {
        let items = vec![DeskMenuItem::Submenu {
            label: "Share".into(),
            items: vec![
                DeskMenuItem::Action {
                    id: "mail".into(),
                    label: "Mail".into(),
                    shortcut: None,
                    enabled: true,
                },
                DeskMenuItem::Submenu {
                    label: "Deeper".into(),
                    items: vec![],
                },
            ],
        }];
        let o = open(items, vec![], field(false, false, None));
        let main = main_rows(&o, |_| None);
        assert_eq!(main[0].sub(), Some(Sub::App(0)));
        let sub = sub_rows(&o, Sub::App(0), &VaultView::Asking, None, &none());
        assert_eq!(labels(&sub), ["Mail", "Deeper"]);
        assert!(sub[0].is_live());
        assert!(!sub[1].is_live());
    }

    #[test]
    fn the_vault_column_follows_the_vault() {
        let o = open(vec![], vec![], field(true, false, None));
        let at = |v: &VaultView| labels(&sub_rows(&o, Sub::Vault, v, None, &none())).join("|");
        assert_eq!(at(&VaultView::Unavailable), "Vault isn't running");
        assert_eq!(at(&VaultView::SignedOut), "Sign in to Vault…");
        assert_eq!(at(&VaultView::Locked), "Unlock Vault…");
        let unlocked = VaultView::Unlocked {
            logins: vec![login("a", false)],
        };
        // No site: no logins to offer first.
        assert_eq!(at(&unlocked), "Search vault…");
        assert!(!sub_rows(&o, Sub::Vault, &VaultView::Unavailable, None, &none())[0].is_live());
    }

    #[test]
    fn a_pages_logins_come_first_with_fill() {
        let o = open(vec![], vec![], field(true, false, Some("https://a.example/login")));
        let vault = VaultView::Unlocked {
            logins: vec![login("a", true), login("b", false)],
        };
        let rows = sub_rows(&o, Sub::Vault, &vault, None, &none());
        assert_eq!(labels(&rows), ["a site", "b site", "—", "Search vault…"]);
        let Row::Item(a) = &rows[0] else { panic!() };
        assert_eq!(a.hint.as_deref(), Some("a@example.com"));
        let fields = HashMap::from([
            ("a".to_string(), login_values(true)),
            ("b".to_string(), login_values(false)),
        ]);
        let values = sub_rows(&o, Sub::Login(0), &vault, None, &fields);
        assert_eq!(labels(&values), ["Fill", "—", "Username", "Password", "One-time code"]);
        let Row::Item(fill) = &values[0] else { panic!() };
        assert_eq!(fill.does, Does::Pick(DeskPick::VaultFill { item: "a".into() }));
        let Row::Item(pass) = &values[3] else { panic!() };
        assert_eq!(
            pass.does,
            Does::Deliver {
                item: "a".into(),
                field: DeliverField::Field { key: "password".into() }
            }
        );
        assert_eq!(labels(&sub_rows(&o, Sub::Login(1), &vault, None, &fields)), [
            "Fill", "—", "Username", "Password"
        ]);
    }

    #[test]
    fn an_items_column_reads_until_its_values_come() {
        let item = login("a", false);
        let page = field(true, false, None);
        assert_eq!(labels(&value_rows(&item, None, &page, Fill::NotHere)), [READING_ITEM]);
        let asking = ItemFields::Asking;
        assert_eq!(labels(&value_rows(&item, Some(&asking), &page, Fill::NotHere)), [READING_ITEM]);
        let failed = ItemFields::Failed("The vault is locked.".into());
        let rows = value_rows(&item, Some(&failed), &page, Fill::NotHere);
        assert_eq!(labels(&rows), ["The vault is locked."]);
        assert!(!rows[0].is_live());
        let empty = listed(&[]);
        assert_eq!(labels(&value_rows(&item, Some(&empty), &page, Fill::NotHere)), [NO_VALUES]);
    }

    #[test]
    fn any_field_gets_every_value() {
        // A password field (secret) too: no special handling.
        let mut card = login("visa", false);
        card.kind = ItemKind::Card;
        let values = listed(&[
            ("cardholder", "Cardholder"),
            ("number", "Number"),
            ("exp", "Expiration"),
            ("code", "Security code"),
            ("custom0", "PIN"),
            ("notes", "Notes"),
        ]);
        let rows = value_rows(&card, Some(&values), &field(true, true, None), Fill::Offered);
        assert_eq!(labels(&rows), ["Cardholder", "Number", "Expiration", "Security code", "PIN", "Notes"]);
        let Row::Item(code) = &rows[3] else { panic!() };
        assert_eq!(
            code.does,
            Does::Deliver {
                item: "visa".into(),
                field: DeliverField::Field { key: "code".into() }
            }
        );
        // Fill is a login's, for the page's own.
        let page = field(true, false, Some("https://shop.example"));
        assert_eq!(value_rows(&card, Some(&values), &page, Fill::Offered).len(), 6);
    }

    #[test]
    fn a_search_result_never_offers_fill() {
        let o = open(vec![], vec![], field(true, false, Some("https://a.example/login")));
        let mut search = Search::new(8);
        search.query = "other".into();
        search.items = Some(Ok(vec![login("other", false)]));
        let vault = VaultView::Unlocked { logins: vec![] };
        let fields = HashMap::from([("other".to_string(), login_values(false))]);
        let values = sub_rows(&o, Sub::Found(0), &vault, Some(&search), &fields);
        assert_eq!(labels(&values), ["Username", "Password"]);
    }

    #[test]
    fn a_re_prompt_item_is_dimmed_and_opens_in_vault() {
        let mut item = login("a", true);
        item.reprompt = true;
        let values = login_values(true);
        let rows = value_rows(&item, Some(&values), &field(true, false, Some("https://a.example")), Fill::Offered);
        assert_eq!(labels(&rows), [
            "Fill",
            "—",
            "Username",
            "Password",
            "One-time code",
            "—",
            REPROMPT_NOTE,
            OPEN_IN_VAULT
        ]);
        let live: Vec<bool> = rows.iter().map(Row::is_live).collect();
        assert_eq!(live, [false, false, false, false, false, false, false, true]);
        let Row::Item(open) = &rows[7] else { panic!() };
        assert_eq!(open.does, Does::OpenItem("a".into()));
    }
}
