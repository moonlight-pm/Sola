//! What an item can put into another app's field, for the desk menu:
//! every value it has, by label ([`infos`], never the value), and one of
//! them for a delivery ([`value`]). Both read one list, so every key the
//! menu shows can be delivered.
//!
//! The list is the record's fields in order — a login's username,
//! password and one-time code first — then the notes. A login's password
//! is there even when an organization hides it from view (Bitwarden fills
//! it). An SSH key's private key is not: it is lines of key material, no
//! field's text.

use bitwarden_vault::CipherView;
use sola_vault_proto::{FIELD_NOTES, FIELD_TOTP, FieldInfo, ItemKind, TotpSpec};
use zeroize::Zeroizing;

use super::item::record_from_view;
use super::read::unix_now;

/// Record keys left out of the list.
const LEFT_OUT: &[(ItemKind, &str)] = &[(ItemKind::SshKey, "private")];

struct Entry {
    info: FieldInfo,
    value: Zeroizing<String>,
}

impl Entry {
    fn new(key: &str, label: &str, hidden: bool, value: String) -> Self {
        Self {
            info: FieldInfo {
                key: key.to_string(),
                label: label.to_string(),
                hidden,
            },
            value: Zeroizing::new(value),
        }
    }
}

/// The item's values by label, in order.
pub fn infos(view: CipherView) -> Vec<FieldInfo> {
    entries(view).into_iter().map(|e| e.info).collect()
}

/// The value listed under `key`, if the item has it.
pub fn value(view: CipherView, key: &str) -> Option<Zeroizing<String>> {
    entries(view)
        .into_iter()
        .find(|e| e.info.key == key)
        .map(|e| e.value)
}

fn entries(view: CipherView) -> Vec<Entry> {
    let login = view.login.as_ref().filter(|_| view.r#type == bitwarden_vault::CipherType::Login);
    // As stored: a password keeps its spaces (the record trims what it shows).
    let password = login
        .and_then(|l| l.password.clone())
        .filter(|p| !p.is_empty())
        .map(Zeroizing::new);
    let code = login
        .and_then(|l| l.totp.as_deref())
        .and_then(TotpSpec::parse)
        .map(|spec| spec.code_at(unix_now()));
    let is_login = login.is_some();
    let Some(record) = record_from_view(view, Vec::new()) else {
        return Vec::new();
    };
    let kind = record.kind;
    let mut out = Vec::new();
    if is_login {
        if let Some(f) = record.fields.iter().find(|f| f.key == "username") {
            out.push(Entry::new(&f.key, &f.label, f.hidden, f.value.clone()));
        }
        if let Some(mut p) = password {
            out.push(Entry::new("password", "Password", true, std::mem::take(&mut *p)));
        }
        if let Some(c) = code {
            out.push(Entry::new(FIELD_TOTP, "One-time code", false, c));
        }
    }
    for f in &record.fields {
        let head = is_login && matches!(f.key.as_str(), "username" | "password");
        let left_out = LEFT_OUT.iter().any(|(k, key)| *k == kind && *key == f.key);
        if head || left_out {
            continue;
        }
        out.push(Entry::new(&f.key, &f.label, f.hidden, f.value.clone()));
    }
    if let Some(notes) = record.notes.clone() {
        out.push(Entry::new(FIELD_NOTES, "Notes", false, notes));
    }
    out
}

#[cfg(test)]
mod tests {
    use bitwarden_vault::{FieldType, FieldView, LoginUriView};

    use super::*;
    use crate::vault::new_item::new_view;

    /// A new item of `kind` with an id (a record needs one).
    fn item(kind: ItemKind) -> CipherView {
        let mut view = new_view(kind);
        view.id = Some("11111111-1111-1111-1111-111111111111".parse().unwrap());
        view
    }

    fn login() -> CipherView {
        let mut view = item(ItemKind::Login);
        view.notes = Some("recovery: in the safe".into());
        let l = view.login.as_mut().unwrap();
        l.username = Some("alice".into());
        l.password = Some(" hunter2 ".into());
        l.totp = Some("JBSWY3DPEHPK3PXP".into());
        l.uris = Some(vec![LoginUriView {
            uri: Some("https://github.com".into()),
            r#match: None,
            uri_checksum: None,
        }]);
        view.fields = Some(vec![FieldView {
            name: Some("PIN".into()),
            value: Some("1234".into()),
            r#type: FieldType::Hidden,
            linked_id: None,
        }]);
        view
    }

    fn keys(view: CipherView) -> Vec<String> {
        infos(view).into_iter().map(|i| i.key).collect()
    }

    #[test]
    fn a_login_lists_every_value_head_first() {
        let list = infos(login());
        let labels: Vec<&str> = list.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Username", "Password", "One-time code", "Website", "PIN", "Notes"]);
        let hidden: Vec<bool> = list.iter().map(|i| i.hidden).collect();
        assert_eq!(hidden, [false, true, false, false, true, false]);
    }

    #[test]
    fn every_listed_key_delivers() {
        for key in keys(login()) {
            assert!(value(login(), &key).is_some_and(|v| !v.is_empty()), "{key}");
        }
        assert_eq!(value(login(), "password").unwrap().as_str(), " hunter2 ");
        assert_eq!(value(login(), FIELD_TOTP).unwrap().len(), 6);
        assert_eq!(value(login(), FIELD_NOTES).unwrap().as_str(), "recovery: in the safe");
        assert!(value(login(), "nonsense").is_none());
    }

    #[test]
    fn a_hidden_password_is_still_listed() {
        let mut view = login();
        view.view_password = false;
        assert!(keys(view.clone()).contains(&"password".to_string()));
        assert_eq!(value(view, "password").unwrap().as_str(), " hunter2 ");
    }

    #[test]
    fn a_card_lists_its_fields() {
        let mut view = item(ItemKind::Card);
        let c = view.card.as_mut().unwrap();
        c.cardholder_name = Some("Ana Smith".into());
        c.number = Some("4111111111111111".into());
        c.exp_month = Some("3".into());
        c.exp_year = Some("2029".into());
        c.code = Some("123".into());
        assert_eq!(keys(view.clone()), ["cardholder", "number", "exp", "code"]);
        assert_eq!(value(view, "exp").unwrap().as_str(), "03/29");
    }

    #[test]
    fn an_ssh_key_leaves_its_private_key_out() {
        let mut view = item(ItemKind::SshKey);
        let k = view.ssh_key.as_mut().unwrap();
        k.private_key = "-----BEGIN OPENSSH PRIVATE KEY-----".into();
        k.public_key = "ssh-ed25519 AAAA".into();
        k.fingerprint = "SHA256:abc".into();
        assert_eq!(keys(view.clone()), ["fingerprint", "public"]);
        assert!(value(view, "private").is_none());
    }
}
