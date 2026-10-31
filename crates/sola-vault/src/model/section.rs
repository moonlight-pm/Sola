//! The sidebar's places and which items each one lists.

use sola_vault_proto::{ItemKind, ItemSummary};

/// A place in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Section {
    All,
    Favorites,
    Kind(ItemKind),
    Folder(String),
    /// Items in no folder.
    NoFolder,
    Collection(String),
    Trash,
}

/// The kinds the sidebar always lists, in order.
pub const KINDS: [ItemKind; 5] = [
    ItemKind::Login,
    ItemKind::Card,
    ItemKind::Identity,
    ItemKind::SecureNote,
    ItemKind::SshKey,
];

/// The newer kinds, listed only when the vault has one.
pub const MORE_KINDS: [ItemKind; 3] = [
    ItemKind::BankAccount,
    ItemKind::DriversLicense,
    ItemKind::Passport,
];

impl Section {
    /// `item` is listed here.
    pub fn holds(&self, item: &ItemSummary) -> bool {
        let live = !item.deleted;
        match self {
            Section::Trash => item.deleted,
            Section::All => live,
            Section::Favorites => live && item.favorite,
            Section::Kind(k) => live && item.kind == *k,
            Section::Folder(id) => live && item.folder_id.as_deref() == Some(id.as_str()),
            Section::NoFolder => live && item.folder_id.is_none(),
            Section::Collection(id) => live && item.collection_ids.iter().any(|c| c == id),
        }
    }

    /// A kind's plural, for the sidebar and the list's head.
    pub fn kind_label(kind: ItemKind) -> &'static str {
        match kind {
            ItemKind::Login => "Logins",
            ItemKind::Card => "Cards",
            ItemKind::Identity => "Identities",
            ItemKind::SecureNote => "Notes",
            ItemKind::SshKey => "SSH keys",
            ItemKind::BankAccount => "Bank accounts",
            ItemKind::DriversLicense => "Licenses",
            ItemKind::Passport => "Passports",
        }
    }

    /// New items made here start of this kind (a login elsewhere).
    pub fn new_kind(&self) -> ItemKind {
        match self {
            Section::Kind(k) => *k,
            _ => ItemKind::Login,
        }
    }
}

/// How many items each place lists.
pub fn count(items: &[ItemSummary], section: &Section) -> usize {
    items.iter().filter(|i| section.holds(i)).count()
}

/// The items `section` lists that match `query`, in the order given.
pub fn visible<'a>(items: &'a [ItemSummary], section: &Section, query: &str) -> Vec<&'a ItemSummary> {
    items
        .iter()
        .filter(|i| section.holds(i) && i.matches_query(query))
        .collect()
}

/// The newer kinds the vault has any of (trash aside).
pub fn present_more_kinds(items: &[ItemSummary]) -> Vec<ItemKind> {
    MORE_KINDS
        .into_iter()
        .filter(|k| items.iter().any(|i| !i.deleted && i.kind == *k))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn summary(id: &str, name: &str, kind: ItemKind) -> ItemSummary {
        ItemSummary {
            id: id.into(),
            kind,
            name: name.into(),
            subtitle: String::new(),
            last_used: 0,
            revised: 0,
            has_totp: false,
            has_passkey: false,
            uri_match: false,
            reprompt: false,
            favorite: false,
            folder_id: None,
            organization_id: None,
            collection_ids: Vec::new(),
            deleted: false,
            search: String::new(),
        }
    }

    #[test]
    fn trash_holds_only_deleted_and_nothing_else_does() {
        let mut gone = summary("1", "Old", ItemKind::Login);
        gone.deleted = true;
        gone.favorite = true;
        let live = summary("2", "New", ItemKind::Login);
        assert!(Section::Trash.holds(&gone));
        assert!(!Section::Trash.holds(&live));
        assert!(!Section::All.holds(&gone));
        assert!(!Section::Favorites.holds(&gone));
        assert!(Section::All.holds(&live));
    }

    #[test]
    fn places_by_kind_folder_and_collection() {
        let mut card = summary("1", "Visa", ItemKind::Card);
        card.folder_id = Some("f".into());
        card.collection_ids = vec!["c1".into()];
        let login = summary("2", "GitHub", ItemKind::Login);
        assert!(Section::Kind(ItemKind::Card).holds(&card));
        assert!(!Section::Kind(ItemKind::Card).holds(&login));
        assert!(Section::Folder("f".into()).holds(&card));
        assert!(!Section::Folder("f".into()).holds(&login));
        assert!(Section::NoFolder.holds(&login));
        assert!(!Section::NoFolder.holds(&card));
        assert!(Section::Collection("c1".into()).holds(&card));
        assert!(!Section::Collection("c2".into()).holds(&card));
    }

    #[test]
    fn visible_filters_by_place_and_query() {
        let items = vec![
            summary("1", "Cloudflare", ItemKind::Login),
            summary("2", "Google Cloud", ItemKind::Login),
            summary("3", "Bank", ItemKind::Card),
        ];
        let ids: Vec<&str> = visible(&items, &Section::All, "cloud")
            .iter()
            .map(|i| i.id.as_str())
            .collect();
        assert_eq!(ids, ["1", "2"]);
        assert_eq!(visible(&items, &Section::Kind(ItemKind::Card), "cloud").len(), 0);
        assert_eq!(count(&items, &Section::Kind(ItemKind::Login)), 2);
    }

    #[test]
    fn newer_kinds_show_only_when_present() {
        let mut items = vec![summary("1", "A", ItemKind::Login)];
        assert!(present_more_kinds(&items).is_empty());
        items.push(summary("2", "Passport", ItemKind::Passport));
        assert_eq!(present_more_kinds(&items), [ItemKind::Passport]);
    }
}
