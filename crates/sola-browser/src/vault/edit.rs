//! Edit an existing vault cipher in chrome, then persist.
//!
//! The draft is plain strings (same secrets the item record already
//! showed). Apply mutates a decrypted [`CipherView`] in place so
//! passkeys, attachments, collections, and linked fields stay put.
//!
//! Apply writes only what the user changed since the form opened. The
//! cipher is synced just before the write, so a field the form never
//! showed (a TOTP we cannot read, an empty custom field) or one changed
//! elsewhere meanwhile is left as the server has it.

use bitwarden_vault::{
    BankAccountView, CardView, CipherType, CipherView, DriversLicenseView, FieldType, FieldView,
    IdentityView, LoginUriView, PassportView, PasswordHistoryView, SshKeyView, UriMatchType,
};
use chrono::{NaiveDate, Utc};
use zeroize::Zeroize;

use super::item::{ItemKind, ItemRecord};

const MAX_PASSWORD_HISTORY: usize = 5;

/// One labelled input on the edit form (type-specific, not custom).
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriDraft {
    pub index: Option<usize>,
    pub value: String,
}

/// Custom field the user can rename, edit, or drop.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Chrome-owned edit buffer. Worker applies it onto the live cipher.
#[derive(Debug, Clone)]
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
    /// The draft as the form opened. `None` writes every field.
    base: Option<Box<ItemDraft>>,
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
                        uris.push(UriDraft {
                            index: i.parse().ok(),
                            value: f.value.clone(),
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
            base: None,
        };
        draft.base = Some(Box::new(draft.clone()));
        draft
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
    fn changed_row(&self, key: &str) -> Option<&str> {
        let now = self.row_value(key);
        match &self.base {
            Some(base) if base.row_value(key) == now => None,
            _ => Some(now),
        }
    }

    fn name_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.name != self.name)
    }

    fn notes_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.notes != self.notes)
    }

    fn uris_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.uris != self.uris)
    }

    fn custom_changed(&self) -> bool {
        self.base.as_ref().is_none_or(|b| b.custom != self.custom)
    }

    /// The form showed the cipher's custom field `index` when it opened.
    fn custom_was_shown(&self, index: usize) -> bool {
        self.base
            .as_ref()
            .is_none_or(|b| b.custom.iter().any(|c| c.index == Some(index)))
    }
}

const IDENTITY_ROWS: &[(&str, &str, bool)] = &[
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

const BANK_ROWS: &[(&str, &str, bool)] = &[
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

const LICENSE_ROWS: &[(&str, &str, bool)] = &[
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

const PASSPORT_ROWS: &[(&str, &str, bool)] = &[
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

/// Fold chrome edits into `view`. Passkeys / attachments / collections stay.
pub fn apply_draft(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    if !view.edit {
        return Err("This item cannot be edited.".into());
    }
    let kind = ItemKind::from_cipher_type(view.r#type);
    if kind != draft.kind {
        return Err("Item type changed while editing.".into());
    }

    if draft.name_changed() {
        let name = draft.name.trim();
        view.name = if name.is_empty() {
            kind.label().to_string()
        } else {
            name.to_string()
        };
    }
    if draft.notes_changed() {
        view.notes = opt(&draft.notes);
    }

    match view.r#type {
        CipherType::Login => apply_login(view, draft)?,
        CipherType::Card => apply_card(view, draft),
        CipherType::Identity => apply_identity(view, draft),
        CipherType::SecureNote => {}
        CipherType::SshKey => apply_ssh(view, draft),
        CipherType::BankAccount => apply_bank(view, draft),
        CipherType::DriversLicense => apply_license(view, draft)?,
        CipherType::Passport => apply_passport(view, draft)?,
    }
    if draft.custom_changed() {
        apply_custom(view, draft);
    }
    Ok(())
}

/// Write `key` into `slot` when the user changed it.
fn set(slot: &mut Option<String>, draft: &ItemDraft, key: &str) {
    if let Some(v) = draft.changed_row(key) {
        *slot = opt(v);
    }
}

/// [`set`] for a `YYYY-MM-DD` row.
fn set_date(
    slot: &mut Option<NaiveDate>,
    draft: &ItemDraft,
    key: &str,
    label: &str,
) -> Result<(), String> {
    if let Some(v) = draft.changed_row(key) {
        *slot = parse_date(v, label)?;
    }
    Ok(())
}

fn apply_login(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    let login = view.login.get_or_insert_with(|| bitwarden_vault::LoginView {
        username: None,
        password: None,
        password_revision_date: None,
        uris: None,
        totp: None,
        autofill_on_page_load: None,
        fido2_credentials: None,
    });
    set(&mut login.username, draft, "username");
    set(&mut login.totp, draft, "totp");

    let mut retired = None;
    if let Some(new) = draft.changed_row("password").filter(|_| draft.view_password) {
        let new = opt(new);
        if login.password != new {
            retired = std::mem::replace(&mut login.password, new);
            login.password_revision_date = Some(Utc::now());
        }
    }

    if draft.uris_changed() {
        let orig = login.uris.take().unwrap_or_default();
        let mut uris = Vec::new();
        for d in &draft.uris {
            let t = d.value.trim();
            if t.is_empty() {
                continue;
            }
            let mut uri = d
                .index
                .and_then(|i| orig.get(i).cloned())
                .unwrap_or(LoginUriView {
                    uri: None,
                    r#match: Some(UriMatchType::Domain),
                    uri_checksum: None,
                });
            uri.uri = Some(t.to_string());
            uris.push(uri);
        }
        login.uris = (!uris.is_empty()).then_some(uris);
    }
    // Checksum-less URIs are dropped by readers once the cipher has its
    // own key, which the blob write gives it: always stamp them.
    login.generate_checksums();

    if let Some(old) = retired.filter(|s| !s.is_empty()) {
        push_history(view, old);
    }
    Ok(())
}

fn apply_card(view: &mut CipherView, draft: &ItemDraft) {
    let card = view.card.get_or_insert_with(|| CardView {
        cardholder_name: None,
        exp_month: None,
        exp_year: None,
        code: None,
        brand: None,
        number: None,
    });
    set(&mut card.cardholder_name, draft, "cardholder");
    set(&mut card.brand, draft, "brand");
    set(&mut card.number, draft, "number");
    if let Some(v) = draft.changed_row("exp_month") {
        card.exp_month = opt(v).map(normalize_month);
    }
    set(&mut card.exp_year, draft, "exp_year");
    set(&mut card.code, draft, "code");
}

fn apply_identity(view: &mut CipherView, draft: &ItemDraft) {
    let id = view.identity.get_or_insert_with(empty_identity);
    set(&mut id.title, draft, "title");
    set(&mut id.first_name, draft, "first");
    set(&mut id.middle_name, draft, "middle");
    set(&mut id.last_name, draft, "last");
    set(&mut id.username, draft, "username");
    set(&mut id.company, draft, "company");
    set(&mut id.ssn, draft, "ssn");
    set(&mut id.passport_number, draft, "passport");
    set(&mut id.license_number, draft, "license");
    set(&mut id.email, draft, "email");
    set(&mut id.phone, draft, "phone");
    set(&mut id.address1, draft, "addr1");
    set(&mut id.address2, draft, "addr2");
    set(&mut id.address3, draft, "addr3");
    set(&mut id.city, draft, "city");
    set(&mut id.state, draft, "state");
    set(&mut id.postal_code, draft, "postal");
    set(&mut id.country, draft, "country");
}

fn apply_ssh(view: &mut CipherView, draft: &ItemDraft) {
    let ssh = view.ssh_key.get_or_insert_with(|| SshKeyView {
        private_key: String::new(),
        public_key: String::new(),
        fingerprint: String::new(),
    });
    if let Some(v) = draft.changed_row("fingerprint") {
        ssh.fingerprint = v.to_string();
    }
    if let Some(v) = draft.changed_row("public") {
        ssh.public_key = v.to_string();
    }
    if let Some(v) = draft.changed_row("private") {
        ssh.private_key = v.to_string();
    }
}

fn apply_bank(view: &mut CipherView, draft: &ItemDraft) {
    let b = view
        .bank_account
        .get_or_insert_with(BankAccountView::default);
    set(&mut b.bank_name, draft, "bank");
    set(&mut b.name_on_account, draft, "name_on");
    set(&mut b.account_type, draft, "type");
    set(&mut b.account_number, draft, "acct");
    set(&mut b.routing_number, draft, "routing");
    set(&mut b.branch_number, draft, "branch");
    set(&mut b.pin, draft, "pin");
    set(&mut b.swift_code, draft, "swift");
    set(&mut b.iban, draft, "iban");
    set(&mut b.bank_contact_phone, draft, "phone");
}

fn apply_license(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    let d = view
        .drivers_license
        .get_or_insert_with(DriversLicenseView::default);
    set(&mut d.first_name, draft, "first");
    set(&mut d.middle_name, draft, "middle");
    set(&mut d.last_name, draft, "last");
    set_date(&mut d.date_of_birth, draft, "dob", "Date of birth")?;
    set(&mut d.license_number, draft, "number");
    set(&mut d.issuing_country, draft, "country");
    set(&mut d.issuing_state, draft, "state");
    set_date(&mut d.issue_date, draft, "issued", "Issued")?;
    set_date(&mut d.expiration_date, draft, "expires", "Expires")?;
    set(&mut d.issuing_authority, draft, "authority");
    set(&mut d.license_class, draft, "class");
    Ok(())
}

fn apply_passport(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    let p = view.passport.get_or_insert_with(PassportView::default);
    set(&mut p.given_name, draft, "given");
    set(&mut p.surname, draft, "surname");
    set_date(&mut p.date_of_birth, draft, "dob", "Date of birth")?;
    set(&mut p.sex, draft, "sex");
    set(&mut p.birth_place, draft, "birth");
    set(&mut p.nationality, draft, "nat");
    set(&mut p.issuing_country, draft, "country");
    set(&mut p.passport_number, draft, "number");
    set(&mut p.passport_type, draft, "type");
    set(&mut p.national_identification_number, draft, "nid");
    set(&mut p.issuing_authority, draft, "authority");
    set_date(&mut p.issue_date, draft, "issued", "Issued")?;
    set_date(&mut p.expiration_date, draft, "expires", "Expires")?;
    Ok(())
}

fn apply_custom(view: &mut CipherView, draft: &ItemDraft) {
    let orig = view.fields.take().unwrap_or_default();
    let mut next = Vec::new();
    for (i, f) in orig.iter().enumerate() {
        if f.r#type == FieldType::Linked {
            next.push(f.clone());
            continue;
        }
        let Some(d) = draft.custom.iter().find(|c| c.index == Some(i)) else {
            if !draft.custom_was_shown(i) {
                // The record never listed it (an empty value): keep it.
                next.push(f.clone());
                continue;
            }
            // Removed on the form.
            if f.r#type == FieldType::Hidden {
                if let Some(old) = f.value.as_deref().filter(|s| !s.is_empty()) {
                    let label = f.name.as_deref().unwrap_or("Field");
                    push_history(view, format!("{label}: {old}"));
                }
            }
            continue;
        };
        if d.name.trim().is_empty() && d.value.trim().is_empty() {
            continue;
        }
        if f.r#type == FieldType::Hidden {
            let old = f.value.clone().unwrap_or_default();
            if old != d.value && !old.is_empty() {
                let label = if d.name.trim().is_empty() {
                    f.name.as_deref().unwrap_or("Field")
                } else {
                    d.name.trim()
                };
                push_history(view, format!("{label}: {old}"));
            }
        }
        let mut nf = f.clone();
        nf.name = opt(&d.name);
        nf.value = opt(&d.value);
        if matches!(nf.r#type, FieldType::Text | FieldType::Hidden) {
            nf.r#type = if d.hidden {
                FieldType::Hidden
            } else {
                FieldType::Text
            };
        }
        next.push(nf);
    }
    for d in draft.custom.iter().filter(|c| c.index.is_none()) {
        if d.name.trim().is_empty() && d.value.trim().is_empty() {
            continue;
        }
        next.push(FieldView {
            name: opt(&d.name),
            value: opt(&d.value),
            r#type: if d.hidden {
                FieldType::Hidden
            } else {
                FieldType::Text
            },
            linked_id: None,
        });
    }
    view.fields = if next.is_empty() { None } else { Some(next) };
}

fn push_history(view: &mut CipherView, password: impl Into<String>) {
    let password = password.into();
    if password.is_empty() {
        return;
    }
    let mut hist = view.password_history.take().unwrap_or_default();
    hist.insert(
        0,
        PasswordHistoryView {
            password,
            last_used_date: Utc::now(),
        },
    );
    hist.truncate(MAX_PASSWORD_HISTORY);
    view.password_history = Some(hist);
}

fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn split_exp(exp: &str) -> (String, String) {
    let t = exp.trim();
    if t.is_empty() {
        return (String::new(), String::new());
    }
    if let Some((m, y)) = t.split_once('/') {
        return (m.trim().to_string(), y.trim().to_string());
    }
    (t.to_string(), String::new())
}

fn normalize_month(s: String) -> String {
    if let Ok(n) = s.parse::<u32>() {
        if (1..=12).contains(&n) {
            return format!("{n:02}");
        }
    }
    s
}

fn parse_date(s: &str, label: &str) -> Result<Option<NaiveDate>, String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| format!("{label} must be YYYY-MM-DD."))
}

fn empty_identity() -> IdentityView {
    IdentityView {
        title: None,
        first_name: None,
        middle_name: None,
        last_name: None,
        address1: None,
        address2: None,
        address3: None,
        city: None,
        state: None,
        postal_code: None,
        country: None,
        company: None,
        email: None,
        phone: None,
        ssn: None,
        username: None,
        passport_number: None,
        license_number: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::client::new_login_view;
    use crate::vault::item::{RecordField, record_from_view};
    use bitwarden_vault::CipherRepromptType;

    fn login_cipher() -> CipherView {
        let mut view = new_login_view(
            "GitHub".into(),
            Some("alice".into()),
            Some("oldpass".into()),
            Some("https://github.com".into()),
        );
        view.id = Some("11111111-1111-1111-1111-111111111111".parse().unwrap());
        view.edit = true;
        view.view_password = true;
        view
    }

    fn record_for(view: &CipherView) -> ItemRecord {
        ItemRecord {
            id: view.id.map(|id| id.to_string()).unwrap_or_default(),
            kind: ItemKind::from_cipher_type(view.r#type),
            name: view.name.clone(),
            notes: view.notes.clone(),
            fields: vec![
                RecordField {
                    key: "username".into(),
                    label: "Username".into(),
                    value: view
                        .login
                        .as_ref()
                        .and_then(|l| l.username.clone())
                        .unwrap_or_default(),
                    hidden: false,
                    mono: false,
                },
                RecordField {
                    key: "password".into(),
                    label: "Password".into(),
                    value: view
                        .login
                        .as_ref()
                        .and_then(|l| l.password.clone())
                        .unwrap_or_default(),
                    hidden: true,
                    mono: false,
                },
                RecordField {
                    key: "uri0".into(),
                    label: "Website".into(),
                    value: view
                        .login
                        .as_ref()
                        .and_then(|l| l.uris.as_ref())
                        .and_then(|u| u.first())
                        .and_then(|u| u.uri.clone())
                        .unwrap_or_default(),
                    hidden: false,
                    mono: false,
                },
            ],
            totp_secret: view.login.as_ref().and_then(|l| l.totp.clone()),
            totp_period: 30,
            has_passkey: false,
            can_edit: view.edit,
            view_password: view.view_password,
        }
    }

    #[test]
    fn apply_updates_login_fields() {
        let mut view = login_cipher();
        let mut draft = ItemDraft::from_record(&record_for(&view));
        draft.name = "GH".into();
        draft.notes = "work".into();
        draft.set_row("username", "bob".into());
        draft.set_row("password", "newpass".into());
        draft.set_row("totp", "JBSWY3DPEHPK3PXP".into());
        draft.uris[0].value = "https://github.com/login".into();
        draft.uris.push(UriDraft {
            index: None,
            value: " ".into(),
        });
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.name, "GH");
        assert_eq!(view.notes.as_deref(), Some("work"));
        let login = view.login.as_ref().unwrap();
        assert_eq!(login.username.as_deref(), Some("bob"));
        assert_eq!(login.password.as_deref(), Some("newpass"));
        assert_eq!(login.totp.as_deref(), Some("JBSWY3DPEHPK3PXP"));
        assert_eq!(
            login.uris.as_ref().unwrap()[0].uri.as_deref(),
            Some("https://github.com/login")
        );
        assert_eq!(login.uris.as_ref().unwrap().len(), 1);
        assert_eq!(
            view.password_history.as_ref().unwrap()[0].password,
            "oldpass"
        );
    }

    #[test]
    fn unchanged_password_skips_history() {
        let mut view = login_cipher();
        let draft = ItemDraft::from_record(&record_for(&view));
        apply_draft(&mut view, &draft).unwrap();
        assert!(view.password_history.unwrap_or_default().is_empty());
        assert_eq!(
            view.login.as_ref().unwrap().password.as_deref(),
            Some("oldpass")
        );
    }

    #[test]
    fn refuse_when_cipher_is_not_editable() {
        let mut view = login_cipher();
        view.edit = false;
        let draft = ItemDraft::from_record(&record_for(&view));
        let err = apply_draft(&mut view, &draft).unwrap_err();
        assert!(err.contains("cannot be edited"));
    }

    #[test]
    fn custom_fields_round_trip_and_add() {
        let mut view = login_cipher();
        view.fields = Some(vec![FieldView {
            name: Some("Backup".into()),
            value: Some("alpha".into()),
            r#type: FieldType::Text,
            linked_id: None,
        }]);
        let mut record = record_for(&view);
        record.fields.push(RecordField {
            key: "custom0".into(),
            label: "Backup".into(),
            value: "alpha".into(),
            hidden: false,
            mono: false,
        });
        let mut draft = ItemDraft::from_record(&record);
        draft.custom[0].value = "beta".into();
        draft.custom.push(CustomDraft {
            index: None,
            name: "Pin".into(),
            value: "1234".into(),
            hidden: true,
        });
        apply_draft(&mut view, &draft).unwrap();
        let fields = view.fields.as_ref().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].value.as_deref(), Some("beta"));
        assert_eq!(fields[1].name.as_deref(), Some("Pin"));
        assert_eq!(fields[1].r#type, FieldType::Hidden);
    }

    #[test]
    fn apply_draft_keeps_passkey_slot() {
        let mut view = login_cipher();
        view.login.as_mut().unwrap().fido2_credentials = Some(Vec::new());
        let mut draft = ItemDraft::from_record(&record_for(&view));
        draft.set_row("username", "bob".into());
        apply_draft(&mut view, &draft).unwrap();
        assert!(view
            .login
            .as_ref()
            .unwrap()
            .fido2_credentials
            .is_some());
        assert_eq!(
            view.login.as_ref().unwrap().username.as_deref(),
            Some("bob")
        );
    }

    #[test]
    fn date_must_be_iso() {
        let mut view = CipherView {
            id: Some("11111111-1111-1111-1111-111111111111".parse().unwrap()),
            organization_id: None,
            folder_id: None,
            collection_ids: Vec::new(),
            key: None,
            name: "License".into(),
            notes: None,
            r#type: CipherType::DriversLicense,
            login: None,
            identity: None,
            card: None,
            secure_note: None,
            ssh_key: None,
            bank_account: None,
            drivers_license: Some(DriversLicenseView::default()),
            passport: None,
            favorite: false,
            reprompt: CipherRepromptType::None,
            organization_use_totp: false,
            edit: true,
            permissions: None,
            view_password: true,
            local_data: None,
            attachments: None,
            attachment_decryption_failures: None,
            fields: None,
            password_history: None,
            creation_date: Utc::now(),
            deleted_date: None,
            revision_date: Utc::now(),
            archived_date: None,
        };
        let mut draft = ItemDraft::from_record(&ItemRecord {
            id: "11111111-1111-1111-1111-111111111111".into(),
            kind: ItemKind::DriversLicense,
            name: "License".into(),
            notes: None,
            fields: Vec::new(),
            totp_secret: None,
            totp_period: 30,
            has_passkey: false,
            can_edit: true,
            view_password: true,
        });
        draft.set_row("dob", "03/14/1990".into());
        let err = apply_draft(&mut view, &draft).unwrap_err();
        assert!(err.contains("YYYY-MM-DD"));
    }

    #[test]
    fn from_record_fills_identity_blanks() {
        let record = ItemRecord {
            id: "x".into(),
            kind: ItemKind::Identity,
            name: "Jane".into(),
            notes: None,
            fields: vec![RecordField {
                key: "first".into(),
                label: "First name".into(),
                value: "Jane".into(),
                hidden: false,
                mono: false,
            }],
            totp_secret: None,
            totp_period: 30,
            has_passkey: false,
            can_edit: true,
            view_password: true,
        };
        let draft = ItemDraft::from_record(&record);
        assert!(
            draft
                .rows
                .iter()
                .any(|r| r.key == "email" && r.value.is_empty())
        );
        assert_eq!(draft.row_value("first"), "Jane");
    }

    #[test]
    fn unreadable_totp_survives_an_unrelated_edit() {
        let mut view = login_cipher();
        // SHA-256: stored by another client, no code from us.
        let raw = "otpauth://totp/X?secret=JBSWY3DPEHPK3PXP&algorithm=SHA256";
        view.login.as_mut().unwrap().totp = Some(raw.into());
        let record = record_from_view(view.clone()).unwrap();
        assert_eq!(record.unreadable_totp(), Some(raw));
        let mut draft = ItemDraft::from_record(&record);
        assert_eq!(draft.row_value("totp"), raw);
        draft.notes = "recovery: 1111-2222".into();
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.login.as_ref().unwrap().totp.as_deref(), Some(raw));
        assert_eq!(view.notes.as_deref(), Some("recovery: 1111-2222"));
    }

    #[test]
    fn a_field_changed_elsewhere_survives_the_save() {
        let mut view = login_cipher();
        let mut draft = ItemDraft::from_record(&record_for(&view));
        // Saved from another window or device after the form opened.
        view.login.as_mut().unwrap().totp = Some("JBSWY3DPEHPK3PXP".into());
        view.name = "GitHub (work)".into();
        draft.notes = "codes".into();
        apply_draft(&mut view, &draft).unwrap();
        let login = view.login.as_ref().unwrap();
        assert_eq!(login.totp.as_deref(), Some("JBSWY3DPEHPK3PXP"));
        assert_eq!(view.name, "GitHub (work)");
        assert_eq!(login.password.as_deref(), Some("oldpass"));
        assert_eq!(view.notes.as_deref(), Some("codes"));
    }

    #[test]
    fn clearing_the_totp_on_the_form_removes_it() {
        let mut view = login_cipher();
        view.login.as_mut().unwrap().totp = Some("JBSWY3DPEHPK3PXP".into());
        let mut draft = ItemDraft::from_record(&record_from_view(view.clone()).unwrap());
        draft.set_row("totp", String::new());
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.login.as_ref().unwrap().totp, None);
    }

    #[test]
    fn empty_custom_field_survives_an_edit_to_another() {
        let mut view = login_cipher();
        view.fields = Some(vec![
            FieldView {
                name: Some("Security answer".into()),
                value: None,
                r#type: FieldType::Hidden,
                linked_id: None,
            },
            FieldView {
                name: Some("Backup".into()),
                value: Some("alpha".into()),
                r#type: FieldType::Text,
                linked_id: None,
            },
        ]);
        let mut draft = ItemDraft::from_record(&record_from_view(view.clone()).unwrap());
        assert_eq!(draft.custom.len(), 1);
        draft.custom[0].value = "beta".into();
        apply_draft(&mut view, &draft).unwrap();
        let fields = view.fields.as_ref().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.as_deref(), Some("Security answer"));
        assert_eq!(fields[1].value.as_deref(), Some("beta"));
    }

    #[test]
    fn edited_website_keeps_its_own_match_type() {
        let mut view = login_cipher();
        view.login.as_mut().unwrap().uris = Some(vec![
            LoginUriView {
                uri: None,
                r#match: Some(UriMatchType::Exact),
                uri_checksum: None,
            },
            LoginUriView {
                uri: Some("https://example.com".into()),
                r#match: Some(UriMatchType::Host),
                uri_checksum: None,
            },
        ]);
        let mut draft = ItemDraft::from_record(&record_from_view(view.clone()).unwrap());
        assert_eq!(draft.uris.len(), 1);
        draft.uris[0].value = "https://example.org".into();
        apply_draft(&mut view, &draft).unwrap();
        let uris = view.login.as_ref().unwrap().uris.as_ref().unwrap();
        assert_eq!(uris.len(), 1);
        assert_eq!(uris[0].uri.as_deref(), Some("https://example.org"));
        assert_eq!(uris[0].r#match, Some(UriMatchType::Host));
    }

    #[test]
    fn hidden_password_row_omitted_when_not_viewable() {
        let mut view = login_cipher();
        view.view_password = false;
        let draft = ItemDraft::from_record(&record_for(&view));
        assert!(!draft.rows.iter().any(|r| r.key == "password"));
    }
}
