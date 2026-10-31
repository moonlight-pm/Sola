//! Editing a draft in place: websites and custom fields added, changed and
//! removed, and what changed since the form opened (the dot by a label —
//! Save writes only those).

use sola_vault_proto::{CustomDraft, ItemDraft, ItemKind, ItemRecord, UriDraft, UriMatch};

/// A new item of `kind`, in `folder`; a login starts with one website.
pub fn blank(kind: ItemKind, folder: Option<String>) -> ItemDraft {
    let mut d = ItemDraft::blank(kind);
    d.folder_id = folder;
    if kind == ItemKind::Login {
        add_uri(&mut d);
    }
    d
}

/// A copy of `record` to save as a new item: every field written, nothing
/// tied to the original's positions.
pub fn clone_of(record: &ItemRecord) -> ItemDraft {
    let mut d = ItemDraft::from_record(record);
    d.id = String::new();
    d.base = None;
    d.can_edit = true;
    d.name = format!("{} - Clone", record.name);
    for u in &mut d.uris {
        u.index = None;
    }
    for c in &mut d.custom {
        c.index = None;
    }
    d
}

pub fn add_uri(d: &mut ItemDraft) {
    d.uris.push(UriDraft {
        index: None,
        value: String::new(),
        match_type: None,
    });
}

pub fn remove_uri(d: &mut ItemDraft, i: usize) {
    if i < d.uris.len() {
        d.uris.remove(i);
    }
}

pub fn set_uri(d: &mut ItemDraft, i: usize, value: String) {
    if let Some(u) = d.uris.get_mut(i) {
        u.value = value;
    }
}

/// `None`: the account's default (Domain).
pub fn set_match(d: &mut ItemDraft, i: usize, m: Option<UriMatch>) {
    if let Some(u) = d.uris.get_mut(i) {
        u.match_type = m;
    }
}

pub fn add_custom(d: &mut ItemDraft, hidden: bool) {
    d.custom.push(CustomDraft {
        index: None,
        name: String::new(),
        value: String::new(),
        hidden,
    });
}

pub fn remove_custom(d: &mut ItemDraft, i: usize) {
    if i < d.custom.len() {
        d.custom.remove(i);
    }
}

pub fn set_custom_name(d: &mut ItemDraft, i: usize, name: String) {
    if let Some(c) = d.custom.get_mut(i) {
        c.name = name;
    }
}

pub fn set_custom_value(d: &mut ItemDraft, i: usize, value: String) {
    if let Some(c) = d.custom.get_mut(i) {
        c.value = value;
    }
}

/// The row changed since the form opened (always, for a new item with a
/// value).
pub fn row_changed(d: &ItemDraft, key: &str) -> bool {
    match &d.base {
        Some(_) => d.changed_row(key).is_some(),
        None => !d.row_value(key).is_empty(),
    }
}

/// The website at `i` is new or differs from the one it was.
pub fn uri_changed(d: &ItemDraft, i: usize) -> bool {
    let Some(u) = d.uris.get(i) else {
        return false;
    };
    let Some(base) = &d.base else {
        return !u.value.is_empty();
    };
    match u.index {
        None => true,
        Some(n) => base.uris.iter().find(|b| b.index == Some(n)) != Some(u),
    }
}

/// The custom field at `i` is new or differs from the one it was.
pub fn custom_changed(d: &ItemDraft, i: usize) -> bool {
    let Some(c) = d.custom.get(i) else {
        return false;
    };
    let Some(base) = &d.base else {
        return !c.name.is_empty() || !c.value.is_empty();
    };
    match c.index {
        None => true,
        Some(n) => base.custom.iter().find(|b| b.index == Some(n)) != Some(c),
    }
}

pub fn reprompt_changed(d: &ItemDraft) -> bool {
    d.base.as_ref().is_some_and(|b| b.reprompt != d.reprompt)
}

pub fn folder_changed(d: &ItemDraft) -> bool {
    d.base.as_ref().is_some_and(|b| b.folder_id != d.folder_id)
}

/// Anything to save: for a new item, anything filled in.
pub fn dirty(d: &ItemDraft) -> bool {
    match &d.base {
        Some(_) => {
            d.name_changed()
                || d.notes_changed()
                || d.uris_changed()
                || d.custom_changed()
                || reprompt_changed(d)
                || folder_changed(d)
                || d.rows.iter().any(|r| d.changed_row(&r.key).is_some())
        }
        None => {
            !d.name.is_empty()
                || !d.notes.is_empty()
                || d.rows.iter().any(|r| !r.value.is_empty())
                || d.uris.iter().any(|u| !u.value.is_empty())
                || !d.custom.is_empty()
        }
    }
}

/// Why it cannot be saved yet, said under the name.
pub fn save_problem(d: &ItemDraft) -> Option<&'static str> {
    d.name.trim().is_empty().then_some("Give it a name.")
}

/// Drop websites left empty before saving.
pub fn tidy(d: &mut ItemDraft) {
    d.uris.retain(|u| !u.value.trim().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> ItemDraft {
        let mut d = ItemDraft::blank(ItemKind::Login);
        d.id = "x".into();
        d.name = "GitHub".into();
        d.uris = vec![UriDraft {
            index: Some(0),
            value: "github.com".into(),
            match_type: None,
        }];
        d.custom = vec![CustomDraft {
            index: Some(0),
            name: "PIN".into(),
            value: "1234".into(),
            hidden: true,
        }];
        d.base = Some(Box::new(d.clone()));
        d
    }

    #[test]
    fn nothing_changed_nothing_to_save() {
        let d = opened();
        assert!(!dirty(&d));
        assert!(!row_changed(&d, "password"));
        assert!(!uri_changed(&d, 0));
        assert!(!custom_changed(&d, 0));
    }

    #[test]
    fn a_changed_row_is_marked() {
        let mut d = opened();
        d.set_row("password", "new".into());
        assert!(row_changed(&d, "password"));
        assert!(!row_changed(&d, "username"));
        assert!(dirty(&d));
    }

    #[test]
    fn websites_track_their_match() {
        let mut d = opened();
        set_match(&mut d, 0, Some(UriMatch::Host));
        assert!(uri_changed(&d, 0));
        set_match(&mut d, 0, None);
        assert!(!uri_changed(&d, 0));
        add_uri(&mut d);
        assert!(uri_changed(&d, 1));
        remove_uri(&mut d, 1);
        assert!(!dirty(&d));
        remove_uri(&mut d, 0);
        assert!(d.uris_changed());
    }

    #[test]
    fn custom_fields_add_and_remove() {
        let mut d = opened();
        set_custom_value(&mut d, 0, "9999".into());
        assert!(custom_changed(&d, 0));
        add_custom(&mut d, false);
        assert!(custom_changed(&d, 1));
        remove_custom(&mut d, 1);
        set_custom_value(&mut d, 0, "1234".into());
        assert!(!dirty(&d));
    }

    #[test]
    fn a_blank_login_has_a_website_and_needs_a_name() {
        let mut d = blank(ItemKind::Login, Some("f".into()));
        assert_eq!(d.uris.len(), 1);
        assert_eq!(d.folder_id.as_deref(), Some("f"));
        assert!(!dirty(&d));
        assert_eq!(save_problem(&d), Some("Give it a name."));
        d.name = "Site".into();
        assert!(dirty(&d));
        assert_eq!(save_problem(&d), None);
        tidy(&mut d);
        assert!(d.uris.is_empty());
    }

    #[test]
    fn a_clone_is_new_and_unpositioned() {
        let record = ItemRecord {
            id: "x".into(),
            kind: ItemKind::Login,
            name: "GitHub".into(),
            notes: None,
            fields: vec![sola_vault_proto::RecordField {
                key: "uri0".into(),
                label: "Website".into(),
                value: "github.com".into(),
                hidden: false,
                mono: false,
            }],
            totp_secret: None,
            totp_period: 30,
            has_passkey: false,
            can_edit: true,
            view_password: true,
            reprompt: false,
            favorite: false,
            folder_id: None,
            organization_id: None,
            collection_ids: Vec::new(),
            uris: Vec::new(),
            passkeys: Vec::new(),
            password_history: Vec::new(),
            attachments: Vec::new(),
            created: None,
            revised: None,
            deleted: None,
        };
        let d = clone_of(&record);
        assert!(d.is_new());
        assert!(d.base.is_none());
        assert_eq!(d.name, "GitHub - Clone");
        assert!(d.uris.iter().all(|u| u.index.is_none()));
    }

    #[test]
    fn reprompt_and_folder_count_as_changes() {
        let mut d = opened();
        d.reprompt = true;
        assert!(dirty(&d));
        d.reprompt = false;
        d.folder_id = Some("f".into());
        assert!(folder_changed(&d));
    }
}
