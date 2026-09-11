//! Edit an existing vault cipher in chrome, then persist.
//!
//! The draft is plain strings (same secrets the item record already
//! showed). Apply mutates a decrypted [`CipherView`] in place so
//! passkeys, attachments, collections, and linked fields stay put.

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

/// Custom field the user can rename, edit, or drop.
#[derive(Debug, Clone)]
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
    pub uris: Vec<String>,
    pub custom: Vec<CustomDraft>,
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
                rows.push(row("totp", "Authenticator key", true, false));
                if let Some(totp) = record.totp_secret.as_ref() {
                    if let Some(r) = rows.iter_mut().find(|r| r.key == "totp") {
                        r.value = totp.clone();
                    }
                }
                for f in &record.fields {
                    if f.key.starts_with("uri") {
                        uris.push(f.value.clone());
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

        Self {
            id: record.id.clone(),
            kind: record.kind,
            can_edit: record.can_edit,
            view_password: record.view_password,
            name: record.name.clone(),
            notes: record.notes.clone().unwrap_or_default(),
            rows,
            uris,
            custom,
        }
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

    let name = draft.name.trim();
    view.name = if name.is_empty() {
        kind.label().to_string()
    } else {
        name.to_string()
    };
    view.notes = opt(&draft.notes);

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
    apply_custom(view, draft);
    Ok(())
}

fn apply_login(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    if view.login.is_none() {
        view.login = Some(bitwarden_vault::LoginView {
            username: None,
            password: None,
            password_revision_date: None,
            uris: None,
            totp: None,
            autofill_on_page_load: None,
            fido2_credentials: None,
        });
    }
    if let Some(login) = view.login.as_mut() {
        login.username = opt(draft.row_value("username"));
        login.totp = opt(draft.row_value("totp"));
    }
    if draft.view_password {
        let new_password = opt(draft.row_value("password"));
        let old_password = view.login.as_ref().and_then(|l| l.password.clone());
        if old_password != new_password {
            if let Some(old) = old_password.filter(|s| !s.is_empty()) {
                push_history(view, old);
            }
            if let Some(login) = view.login.as_mut() {
                login.password = new_password;
                login.password_revision_date = Some(Utc::now());
            }
        }
    }

    let orig = view
        .login
        .as_ref()
        .and_then(|l| l.uris.clone())
        .unwrap_or_default();
    let mut uris = Vec::new();
    for (i, raw) in draft.uris.iter().enumerate() {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        let mut uri = orig.get(i).cloned().unwrap_or(LoginUriView {
            uri: None,
            r#match: Some(UriMatchType::Domain),
            uri_checksum: None,
        });
        uri.uri = Some(t.to_string());
        uris.push(uri);
    }
    if let Some(login) = view.login.as_mut() {
        login.uris = if uris.is_empty() { None } else { Some(uris) };
        login.generate_checksums();
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
    card.cardholder_name = opt(draft.row_value("cardholder"));
    card.brand = opt(draft.row_value("brand"));
    card.number = opt(draft.row_value("number"));
    card.exp_month = opt(draft.row_value("exp_month")).map(normalize_month);
    card.exp_year = opt(draft.row_value("exp_year"));
    card.code = opt(draft.row_value("code"));
}

fn apply_identity(view: &mut CipherView, draft: &ItemDraft) {
    let id = view.identity.get_or_insert_with(empty_identity);
    id.title = opt(draft.row_value("title"));
    id.first_name = opt(draft.row_value("first"));
    id.middle_name = opt(draft.row_value("middle"));
    id.last_name = opt(draft.row_value("last"));
    id.username = opt(draft.row_value("username"));
    id.company = opt(draft.row_value("company"));
    id.ssn = opt(draft.row_value("ssn"));
    id.passport_number = opt(draft.row_value("passport"));
    id.license_number = opt(draft.row_value("license"));
    id.email = opt(draft.row_value("email"));
    id.phone = opt(draft.row_value("phone"));
    id.address1 = opt(draft.row_value("addr1"));
    id.address2 = opt(draft.row_value("addr2"));
    id.address3 = opt(draft.row_value("addr3"));
    id.city = opt(draft.row_value("city"));
    id.state = opt(draft.row_value("state"));
    id.postal_code = opt(draft.row_value("postal"));
    id.country = opt(draft.row_value("country"));
}

fn apply_ssh(view: &mut CipherView, draft: &ItemDraft) {
    let ssh = view.ssh_key.get_or_insert_with(|| SshKeyView {
        private_key: String::new(),
        public_key: String::new(),
        fingerprint: String::new(),
    });
    ssh.fingerprint = draft.row_value("fingerprint").to_string();
    ssh.public_key = draft.row_value("public").to_string();
    ssh.private_key = draft.row_value("private").to_string();
}

fn apply_bank(view: &mut CipherView, draft: &ItemDraft) {
    let b = view
        .bank_account
        .get_or_insert_with(BankAccountView::default);
    b.bank_name = opt(draft.row_value("bank"));
    b.name_on_account = opt(draft.row_value("name_on"));
    b.account_type = opt(draft.row_value("type"));
    b.account_number = opt(draft.row_value("acct"));
    b.routing_number = opt(draft.row_value("routing"));
    b.branch_number = opt(draft.row_value("branch"));
    b.pin = opt(draft.row_value("pin"));
    b.swift_code = opt(draft.row_value("swift"));
    b.iban = opt(draft.row_value("iban"));
    b.bank_contact_phone = opt(draft.row_value("phone"));
}

fn apply_license(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    let d = view
        .drivers_license
        .get_or_insert_with(DriversLicenseView::default);
    d.first_name = opt(draft.row_value("first"));
    d.middle_name = opt(draft.row_value("middle"));
    d.last_name = opt(draft.row_value("last"));
    d.date_of_birth = parse_date(draft.row_value("dob"), "Date of birth")?;
    d.license_number = opt(draft.row_value("number"));
    d.issuing_country = opt(draft.row_value("country"));
    d.issuing_state = opt(draft.row_value("state"));
    d.issue_date = parse_date(draft.row_value("issued"), "Issued")?;
    d.expiration_date = parse_date(draft.row_value("expires"), "Expires")?;
    d.issuing_authority = opt(draft.row_value("authority"));
    d.license_class = opt(draft.row_value("class"));
    Ok(())
}

fn apply_passport(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    let p = view.passport.get_or_insert_with(PassportView::default);
    p.given_name = opt(draft.row_value("given"));
    p.surname = opt(draft.row_value("surname"));
    p.date_of_birth = parse_date(draft.row_value("dob"), "Date of birth")?;
    p.sex = opt(draft.row_value("sex"));
    p.birth_place = opt(draft.row_value("birth"));
    p.nationality = opt(draft.row_value("nat"));
    p.issuing_country = opt(draft.row_value("country"));
    p.passport_number = opt(draft.row_value("number"));
    p.passport_type = opt(draft.row_value("type"));
    p.national_identification_number = opt(draft.row_value("nid"));
    p.issuing_authority = opt(draft.row_value("authority"));
    p.issue_date = parse_date(draft.row_value("issued"), "Issued")?;
    p.expiration_date = parse_date(draft.row_value("expires"), "Expires")?;
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
            // Dropped from the form.
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
    use crate::vault::item::RecordField;
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
        draft.uris = vec!["https://github.com/login".into(), " ".into()];
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
    fn hidden_password_row_omitted_when_not_viewable() {
        let mut view = login_cipher();
        view.view_password = false;
        let draft = ItemDraft::from_record(&record_for(&view));
        assert!(!draft.rows.iter().any(|r| r.key == "password"));
    }
}
