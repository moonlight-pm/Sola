//! An edit of a vault item, as plain strings: the form fills it, the
//! daemon applies it. It keeps the item as the form opened (`base`), so
//! the daemon writes only what changed since — a field the form never
//! showed, or one changed elsewhere meanwhile, is left as the server has it.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::types::{ItemKind, ItemRecord, UriMatch};

/// One labelled input on the edit form (type-specific, not custom).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftRow {
    pub key: String,
    pub label: String,
    pub value: String,
    pub hidden: bool,
    /// Show a Regenerate control (login password).
    pub regen: bool,
}

impl Drop for DraftRow {
    fn drop(&mut self) {
        if self.hidden {
            self.value.zeroize();
        }
    }
}

/// A website on a login. `index` is its place in the cipher's URI list
/// (`None` = newly added), so its match type stays with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UriDraft {
    pub index: Option<usize>,
    pub value: String,
    /// `None`: the account's default (Domain).
    #[serde(default)]
    pub match_type: Option<UriMatch>,
}

/// Custom field the user can rename, edit, or drop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomDraft {
    /// Index in the original `CipherView.fields` list. `None` = newly added.
    pub index: Option<usize>,
    pub name: String,
    pub value: String,
    pub hidden: bool,
}

impl Drop for CustomDraft {
    fn drop(&mut self) {
        if self.hidden {
            self.value.zeroize();
        }
    }
}

/// The form's edit buffer; the daemon applies it onto the live cipher.
/// An empty `id` is a new item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDraft {
    pub id: String,
    pub kind: ItemKind,
    pub can_edit: bool,
    pub view_password: bool,
    pub name: String,
    pub notes: String,
    pub rows: Vec<DraftRow>,
    pub uris: Vec<UriDraft>,
    pub custom: Vec<CustomDraft>,
    #[serde(default)]
    pub reprompt: bool,
    #[serde(default)]
    pub folder_id: Option<String>,
    /// The draft as the form opened. `None` writes every field.
    #[serde(default)]
    pub base: Option<Box<ItemDraft>>,
}

impl Drop for ItemDraft {
    fn drop(&mut self) {
        self.notes.zeroize();
        for row in &mut self.rows {
            if row.hidden {
                row.value.zeroize();
            }
        }
        for c in &mut self.custom {
            if c.hidden {
                c.value.zeroize();
            }
        }
    }
}

impl ItemDraft {
    pub fn from_record(record: &ItemRecord) -> Self {
        let field = |key: &str| {
            record
                .fields
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.value.clone())
                .unwrap_or_default()
        };
        let row = |key: &str, label: &str, hidden: bool, regen: bool| DraftRow {
            key: key.to_string(),
            label: label.to_string(),
            value: field(key),
            hidden,
            regen,
        };

        let mut rows = Vec::new();
        let mut uris = Vec::new();
        match record.kind {
            ItemKind::Login => {
                rows.push(row("username", "Username", false, false));
                if record.view_password {
                    rows.push(row("password", "Password", true, true));
                }
                let mut totp = row("totp", "Authenticator key", true, false);
                totp.value = record.totp_secret.clone().unwrap_or_default();
                rows.push(totp);
                for f in &record.fields {
                    if let Some(i) = f.key.strip_prefix("uri") {
                        let index: Option<usize> = i.parse().ok();
                        uris.push(UriDraft {
                            index,
                            value: f.value.clone(),
                            match_type: index
                                .and_then(|n| record.uris.get(n))
                                .and_then(|u| u.match_type),
                        });
                    }
                }
            }
            ItemKind::Card => {
                rows.push(row("cardholder", "Cardholder", false, false));
                rows.push(row("brand", "Brand", false, false));
                rows.push(row("number", "Number", true, false));
                let exp = field("exp");
                let (month, year) = split_exp(&exp);
                rows.push(DraftRow {
                    key: "exp_month".into(),
                    label: "Exp. month".into(),
                    value: month,
                    hidden: false,
                    regen: false,
                });
                rows.push(DraftRow {
                    key: "exp_year".into(),
                    label: "Exp. year".into(),
                    value: year,
                    hidden: false,
                    regen: false,
                });
                rows.push(row("code", "Security code", true, false));
            }
            ItemKind::Identity => {
                for &(key, label, hidden) in IDENTITY_ROWS {
                    rows.push(row(key, label, hidden, false));
                }
            }
            ItemKind::SecureNote => {}
            ItemKind::SshKey => {
                rows.push(row("fingerprint", "Fingerprint", false, false));
                rows.push(row("public", "Public key", false, false));
                rows.push(row("private", "Private key", true, false));
            }
            ItemKind::BankAccount => {
                for &(key, label, hidden) in BANK_ROWS {
                    rows.push(row(key, label, hidden, false));
                }
            }
            ItemKind::DriversLicense => {
                for &(key, label, hidden) in LICENSE_ROWS {
                    rows.push(row(key, label, hidden, false));
                }
            }
            ItemKind::Passport => {
                for &(key, label, hidden) in PASSPORT_ROWS {
                    rows.push(row(key, label, hidden, false));
                }
            }
        }

        let custom = record
            .fields
            .iter()
            .filter(|f| f.key.starts_with("custom"))
            .map(|f| CustomDraft {
                index: f.key.trim_start_matches("custom").parse().ok(),
                name: f.label.clone(),
                value: f.value.clone(),
                hidden: f.hidden,
            })
            .collect();

        let mut draft = Self {
            id: record.id.clone(),
            kind: record.kind,
            can_edit: record.can_edit,
            view_password: record.view_password,
            name: record.name.clone(),
            notes: record.notes.clone().unwrap_or_default(),
            rows,
            uris,
            custom,
            reprompt: record.reprompt,
            folder_id: record.folder_id.clone(),
            base: None,
        };
        draft.base = Some(Box::new(draft.clone()));
        draft
    }

    /// A new item of `kind`: every row empty, no base (all of it is
    /// written).
    pub fn blank(kind: ItemKind) -> Self {
        let record = ItemRecord {
            id: String::new(),
            kind,
            name: String::new(),
            notes: None,
            fields: Vec::new(),
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
        let mut draft = Self::from_record(&record);
        draft.base = None;
        draft
    }

    /// A new item (no id yet).
    pub fn is_new(&self) -> bool {
        self.id.is_empty()
    }

    pub fn set_row(&mut self, key: &str, value: String) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.key == key) {
            row.value = value;
        }
    }

    pub fn row_value(&self, key: &str) -> &str {
        self.rows
            .iter()
            .find(|r| r.key == key)
            .map(|r| r.value.as_str())
            .unwrap_or("")
    }

    /// The row's value when the user changed it since the form opened.
    pub fn changed_row(&self, key: &str) -> Option<&str> {
        let now = self.row_value(key);
        match &self.base {
            Some(base) if base.row_value(key) == now => None,
            _ => Some(now),
        }
    }

    pub fn name_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.name != self.name)
    }

    pub fn notes_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.notes != self.notes)
    }

    pub fn uris_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.uris != self.uris)
    }

    pub fn custom_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.custom != self.custom)
    }

    /// The form showed the cipher's custom field `index` when it opened.
    pub fn custom_was_shown(&self, index: usize) -> bool {
        self.base
            .as_ref()
            .is_none_or(|b| b.custom.iter().any(|c| c.index == Some(index)))
    }
}

pub const IDENTITY_ROWS: &[(&str, &str, bool)] = &[
    ("title", "Title", false),
    ("first", "First name", false),
    ("middle", "Middle name", false),
    ("last", "Last name", false),
    ("username", "Username", false),
    ("company", "Company", false),
    ("ssn", "SSN", true),
    ("passport", "Passport number", true),
    ("license", "License number", true),
    ("email", "Email", false),
    ("phone", "Phone", false),
    ("addr1", "Address 1", false),
    ("addr2", "Address 2", false),
    ("addr3", "Address 3", false),
    ("city", "City", false),
    ("state", "State", false),
    ("postal", "Postal code", false),
    ("country", "Country", false),
];

pub const BANK_ROWS: &[(&str, &str, bool)] = &[
    ("bank", "Bank", false),
    ("name_on", "Name on account", false),
    ("type", "Type", false),
    ("acct", "Account number", true),
    ("routing", "Routing number", true),
    ("branch", "Branch", false),
    ("pin", "PIN", true),
    ("swift", "SWIFT", false),
    ("iban", "IBAN", true),
    ("phone", "Bank phone", false),
];

pub const LICENSE_ROWS: &[(&str, &str, bool)] = &[
    ("first", "First name", false),
    ("middle", "Middle name", false),
    ("last", "Last name", false),
    ("dob", "Date of birth", false),
    ("number", "License number", true),
    ("country", "Country", false),
    ("state", "State", false),
    ("issued", "Issued", false),
    ("expires", "Expires", false),
    ("authority", "Authority", false),
    ("class", "Class", false),
];

pub const PASSPORT_ROWS: &[(&str, &str, bool)] = &[
    ("given", "Given name", false),
    ("surname", "Surname", false),
    ("dob", "Date of birth", false),
    ("sex", "Sex", false),
    ("birth", "Place of birth", false),
    ("nat", "Nationality", false),
    ("country", "Issuing country", false),
    ("number", "Passport number", true),
    ("type", "Type", false),
    ("nid", "National ID", true),
    ("authority", "Authority", false),
    ("issued", "Issued", false),
    ("expires", "Expires", false),
];


/// `"08/29"` → (`"08"`, `"29"`).
pub fn split_exp(exp: &str) -> (String, String) {
    let t = exp.trim();
    if t.is_empty() {
        return (String::new(), String::new());
    }
    if let Some((m, y)) = t.split_once('/') {
        return (m.trim().to_string(), y.trim().to_string());
    }
    (t.to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_round_trips_with_its_base() {
        let mut d = ItemDraft::blank(ItemKind::Login);
        d.base = Some(Box::new(d.clone()));
        d.set_row("username", "ana".into());
        let wire = serde_json::to_string(&d).unwrap();
        let back: ItemDraft = serde_json::from_str(&wire).unwrap();
        assert_eq!(back.changed_row("username"), Some("ana"));
        assert_eq!(back.changed_row("password"), None);
        assert!(back.is_new());
    }
}
