//! Vault list + item-record types (no secrets on the summary).
//!
//! Chrome browses [`ItemSummary`] rows, then loads one [`ItemRecord`] to
//! show the whole cipher — notes, identity, card, custom fields, TOTP.

use std::collections::HashMap;

use bitwarden_vault::{CipherType, CipherView, FieldType, IdentityView};
use zeroize::Zeroize;

use super::client::{card_exp_display, card_last4, card_subtitle};
use super::match_uri::uri_matches;
use super::totp::TotpSpec;

/// Cipher family shown in the unified vault panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Login,
    Card,
    Identity,
    SecureNote,
    SshKey,
    BankAccount,
    DriversLicense,
    Passport,
}

impl ItemKind {
    pub fn from_cipher_type(t: CipherType) -> Self {
        match t {
            CipherType::Login => Self::Login,
            CipherType::Card => Self::Card,
            CipherType::Identity => Self::Identity,
            CipherType::SecureNote => Self::SecureNote,
            CipherType::SshKey => Self::SshKey,
            CipherType::BankAccount => Self::BankAccount,
            CipherType::DriversLicense => Self::DriversLicense,
            CipherType::Passport => Self::Passport,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Card => "Card",
            Self::Identity => "Identity",
            Self::SecureNote => "Note",
            Self::SshKey => "SSH key",
            Self::BankAccount => "Bank account",
            Self::DriversLicense => "License",
            Self::Passport => "Passport",
        }
    }

    pub fn can_fill(self) -> bool {
        matches!(self, Self::Login | Self::Card | Self::Identity)
    }
}

/// Type chip on the vault browse surface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemFilter {
    #[default]
    All,
    Login,
    Card,
    Identity,
    Note,
}

impl ItemFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Login => "Login",
            Self::Card => "Card",
            Self::Identity => "Identity",
            Self::Note => "Note",
        }
    }

    pub fn matches(self, kind: ItemKind) -> bool {
        match self {
            Self::All => true,
            Self::Login => kind == ItemKind::Login,
            Self::Card => kind == ItemKind::Card,
            Self::Identity => kind == ItemKind::Identity,
            Self::Note => kind == ItemKind::SecureNote,
        }
    }

    pub fn all() -> [ItemFilter; 5] {
        [
            Self::All,
            Self::Login,
            Self::Card,
            Self::Identity,
            Self::Note,
        ]
    }
}

/// One vault row — never carries passwords, PAN, CVC, TOTP secrets, or notes body.
#[derive(Debug, Clone)]
pub struct ItemSummary {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub subtitle: String,
    pub last_used: i64,
    pub has_totp: bool,
    pub has_passkey: bool,
    /// Login URI matches the active page (autofill suggestions).
    pub uri_match: bool,
    /// Lowercased haystack for in-chrome search (notes + usernames + URIs + …).
    search: String,
}

impl ItemSummary {
    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim();
        if q.is_empty() {
            return true;
        }
        q.split_whitespace()
            .filter(|w| !w.is_empty())
            .all(|w| self.search.contains(&w.to_lowercase()))
    }
}

/// One labelled value on the item record.
#[derive(Debug, Clone)]
pub struct RecordField {
    pub key: String,
    pub label: String,
    pub value: String,
    pub hidden: bool,
    pub mono: bool,
}

/// Full decrypted record for the item view. Drop zeroizes secrets.
#[derive(Debug, Clone)]
pub struct ItemRecord {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub notes: Option<String>,
    pub fields: Vec<RecordField>,
    /// Raw Bitwarden TOTP string (`otpauth://` or secret) when present.
    pub totp_secret: Option<String>,
    pub totp_period: u32,
    pub has_passkey: bool,
}

impl ItemRecord {
    pub fn can_fill(&self) -> bool {
        self.kind.can_fill()
    }

    pub fn totp_code_at(&self, unix_secs: u64) -> Option<(String, u32)> {
        let raw = self.totp_secret.as_deref()?;
        let spec = TotpSpec::parse(raw)?;
        Some((spec.code_at(unix_secs), spec.period))
    }
}

impl Drop for ItemRecord {
    fn drop(&mut self) {
        if let Some(ref mut s) = self.totp_secret {
            s.zeroize();
        }
        if let Some(ref mut n) = self.notes {
            n.zeroize();
        }
        for f in &mut self.fields {
            if f.hidden {
                f.value.zeroize();
            }
        }
    }
}

/// Identity fields for page fill — zeroize sensitive members on drop.
#[derive(Debug, Clone, Default)]
pub struct IdentityFillMaterial {
    pub title: Option<String>,
    pub first_name: Option<String>,
    pub middle_name: Option<String>,
    pub last_name: Option<String>,
    pub address1: Option<String>,
    pub address2: Option<String>,
    pub address3: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub company: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub ssn: Option<String>,
    pub username: Option<String>,
    pub passport_number: Option<String>,
    pub license_number: Option<String>,
}

impl Drop for IdentityFillMaterial {
    fn drop(&mut self) {
        if let Some(ref mut s) = self.ssn {
            s.zeroize();
        }
        if let Some(ref mut s) = self.passport_number {
            s.zeroize();
        }
        if let Some(ref mut s) = self.license_number {
            s.zeroize();
        }
    }
}

impl From<&IdentityView> for IdentityFillMaterial {
    fn from(id: &IdentityView) -> Self {
        Self {
            title: nonempty(id.title.clone()),
            first_name: nonempty(id.first_name.clone()),
            middle_name: nonempty(id.middle_name.clone()),
            last_name: nonempty(id.last_name.clone()),
            address1: nonempty(id.address1.clone()),
            address2: nonempty(id.address2.clone()),
            address3: nonempty(id.address3.clone()),
            city: nonempty(id.city.clone()),
            state: nonempty(id.state.clone()),
            postal_code: nonempty(id.postal_code.clone()),
            country: nonempty(id.country.clone()),
            company: nonempty(id.company.clone()),
            email: nonempty(id.email.clone()),
            phone: nonempty(id.phone.clone()),
            ssn: nonempty(id.ssn.clone()),
            username: nonempty(id.username.clone()),
            passport_number: nonempty(id.passport_number.clone()),
            license_number: nonempty(id.license_number.clone()),
        }
    }
}

/// Filter + search. Preserves caller sort.
pub fn filter_items<'a>(
    items: &'a [ItemSummary],
    query: &str,
    filter: ItemFilter,
) -> Vec<&'a ItemSummary> {
    items
        .iter()
        .filter(|i| filter.matches(i.kind) && i.matches_query(query))
        .collect()
}

pub fn summary_from_view(
    view: &CipherView,
    page_url: &str,
    mru: &HashMap<String, i64>,
) -> Option<ItemSummary> {
    if view.deleted_date.is_some() || view.archived_date.is_some() {
        return None;
    }
    let id = view.id.map(|id| id.to_string()).unwrap_or_default();
    if id.is_empty() {
        return None;
    }
    let kind = ItemKind::from_cipher_type(view.r#type);
    let name = if view.name.trim().is_empty() {
        kind.label().to_string()
    } else {
        view.name.clone()
    };
    let (subtitle, uri_match, has_totp, has_passkey) = list_meta(view, page_url);
    let search = search_haystack(view);
    Some(ItemSummary {
        id: id.clone(),
        kind,
        name,
        subtitle,
        last_used: cipher_last_used(view, &id, mru),
        has_totp,
        has_passkey,
        uri_match,
        search,
    })
}

pub fn record_from_view(view: CipherView) -> Option<ItemRecord> {
    if view.deleted_date.is_some() {
        return None;
    }
    let id = view.id.map(|id| id.to_string()).unwrap_or_default();
    if id.is_empty() {
        return None;
    }
    let kind = ItemKind::from_cipher_type(view.r#type);
    let name = if view.name.trim().is_empty() {
        kind.label().to_string()
    } else {
        view.name.clone()
    };
    let mut fields = Vec::new();
    let mut totp_secret = None;
    let mut totp_period = 30u32;
    let mut has_passkey = false;

    match kind {
        ItemKind::Login => {
            if let Some(login) = view.login.as_ref() {
                push_field(
                    &mut fields,
                    "username",
                    "Username",
                    login.username.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "password",
                    "Password",
                    login.password.clone(),
                    true,
                    false,
                );
                if let Some(uris) = login.uris.as_ref() {
                    for (i, u) in uris.iter().enumerate() {
                        let label = if i == 0 {
                            "Website".to_string()
                        } else {
                            format!("Website {}", i + 1)
                        };
                        push_field(
                            &mut fields,
                            &format!("uri{i}"),
                            &label,
                            u.uri.clone(),
                            false,
                            false,
                        );
                    }
                }
                has_passkey = login
                    .fido2_credentials
                    .as_ref()
                    .is_some_and(|c| !c.is_empty());
                if let Some(raw) = login.totp.as_deref().filter(|s| !s.is_empty()) {
                    if let Some(spec) = TotpSpec::parse(raw) {
                        totp_period = spec.period;
                        totp_secret = Some(raw.to_string());
                    }
                }
            }
        }
        ItemKind::Card => {
            if let Some(card) = view.card.as_ref() {
                push_field(
                    &mut fields,
                    "cardholder",
                    "Cardholder",
                    card.cardholder_name.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "brand",
                    "Brand",
                    card.brand.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "number",
                    "Number",
                    card.number.clone(),
                    true,
                    true,
                );
                let exp = card_exp_display(card.exp_month.as_deref(), card.exp_year.as_deref());
                push_field(&mut fields, "exp", "Expiration", exp, false, false);
                push_field(
                    &mut fields,
                    "code",
                    "Security code",
                    card.code.clone(),
                    true,
                    true,
                );
            }
        }
        ItemKind::Identity => {
            if let Some(idv) = view.identity.as_ref() {
                push_identity_fields(&mut fields, idv);
            }
        }
        ItemKind::SshKey => {
            if let Some(ssh) = view.ssh_key.as_ref() {
                push_field(
                    &mut fields,
                    "fingerprint",
                    "Fingerprint",
                    Some(ssh.fingerprint.clone()).filter(|s| !s.is_empty()),
                    false,
                    true,
                );
                push_field(
                    &mut fields,
                    "public",
                    "Public key",
                    Some(ssh.public_key.clone()).filter(|s| !s.is_empty()),
                    false,
                    true,
                );
                push_field(
                    &mut fields,
                    "private",
                    "Private key",
                    Some(ssh.private_key.clone()).filter(|s| !s.is_empty()),
                    true,
                    true,
                );
            }
        }
        ItemKind::BankAccount => {
            if let Some(b) = view.bank_account.as_ref() {
                push_field(
                    &mut fields,
                    "bank",
                    "Bank",
                    b.bank_name.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "name_on",
                    "Name on account",
                    b.name_on_account.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "type",
                    "Type",
                    b.account_type.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "acct",
                    "Account number",
                    b.account_number.clone(),
                    true,
                    true,
                );
                push_field(
                    &mut fields,
                    "routing",
                    "Routing number",
                    b.routing_number.clone(),
                    true,
                    true,
                );
                push_field(
                    &mut fields,
                    "branch",
                    "Branch",
                    b.branch_number.clone(),
                    false,
                    false,
                );
                push_field(&mut fields, "pin", "PIN", b.pin.clone(), true, true);
                push_field(
                    &mut fields,
                    "swift",
                    "SWIFT",
                    b.swift_code.clone(),
                    false,
                    true,
                );
                push_field(&mut fields, "iban", "IBAN", b.iban.clone(), true, true);
                push_field(
                    &mut fields,
                    "phone",
                    "Bank phone",
                    b.bank_contact_phone.clone(),
                    false,
                    false,
                );
            }
        }
        ItemKind::DriversLicense => {
            if let Some(d) = view.drivers_license.as_ref() {
                push_field(
                    &mut fields,
                    "first",
                    "First name",
                    d.first_name.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "middle",
                    "Middle name",
                    d.middle_name.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "last",
                    "Last name",
                    d.last_name.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "dob",
                    "Date of birth",
                    d.date_of_birth.map(|d| d.to_string()),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "number",
                    "License number",
                    d.license_number.clone(),
                    true,
                    false,
                );
                push_field(
                    &mut fields,
                    "country",
                    "Country",
                    d.issuing_country.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "state",
                    "State",
                    d.issuing_state.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "issued",
                    "Issued",
                    d.issue_date.map(|d| d.to_string()),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "expires",
                    "Expires",
                    d.expiration_date.map(|d| d.to_string()),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "authority",
                    "Authority",
                    d.issuing_authority.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "class",
                    "Class",
                    d.license_class.clone(),
                    false,
                    false,
                );
            }
        }
        ItemKind::Passport => {
            if let Some(p) = view.passport.as_ref() {
                push_field(
                    &mut fields,
                    "given",
                    "Given name",
                    p.given_name.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "surname",
                    "Surname",
                    p.surname.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "dob",
                    "Date of birth",
                    p.date_of_birth.map(|d| d.to_string()),
                    false,
                    false,
                );
                push_field(&mut fields, "sex", "Sex", p.sex.clone(), false, false);
                push_field(
                    &mut fields,
                    "birth",
                    "Place of birth",
                    p.birth_place.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "nat",
                    "Nationality",
                    p.nationality.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "country",
                    "Issuing country",
                    p.issuing_country.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "number",
                    "Passport number",
                    p.passport_number.clone(),
                    true,
                    false,
                );
                push_field(
                    &mut fields,
                    "type",
                    "Type",
                    p.passport_type.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "nid",
                    "National ID",
                    p.national_identification_number.clone(),
                    true,
                    false,
                );
                push_field(
                    &mut fields,
                    "authority",
                    "Authority",
                    p.issuing_authority.clone(),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "issued",
                    "Issued",
                    p.issue_date.map(|d| d.to_string()),
                    false,
                    false,
                );
                push_field(
                    &mut fields,
                    "expires",
                    "Expires",
                    p.expiration_date.map(|d| d.to_string()),
                    false,
                    false,
                );
            }
        }
        ItemKind::SecureNote => {}
    }

    if let Some(custom) = view.fields.as_ref() {
        for (i, f) in custom.iter().enumerate() {
            let label = f
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Field");
            let hidden = f.r#type == FieldType::Hidden;
            let value = match f.r#type {
                FieldType::Boolean => f.value.clone(),
                FieldType::Linked => None, // linked fields are references, not copy values
                _ => f.value.clone(),
            };
            push_field(
                &mut fields,
                &format!("custom{i}"),
                label,
                value,
                hidden,
                false,
            );
        }
    }

    let notes = view
        .notes
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some(ItemRecord {
        id,
        kind,
        name,
        notes,
        fields,
        totp_secret,
        totp_period,
        has_passkey,
    })
}

fn list_meta(view: &CipherView, page_url: &str) -> (String, bool, bool, bool) {
    match view.r#type {
        CipherType::Login => {
            let login = view.login.as_ref();
            let username = login
                .and_then(|l| l.username.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let mut uri_match = false;
            let mut matched_uri = None;
            if let Some(uris) = login.and_then(|l| l.uris.as_ref()) {
                for u in uris {
                    let Some(ref uri) = u.uri else { continue };
                    if !page_url.is_empty() && uri_matches(page_url, uri, u.r#match) {
                        uri_match = true;
                        matched_uri = Some(uri.clone());
                        break;
                    }
                    if matched_uri.is_none() {
                        matched_uri = Some(uri.clone());
                    }
                }
            }
            let subtitle = username
                .map(|s| s.to_string())
                .or_else(|| matched_uri.as_deref().and_then(host_of))
                .unwrap_or_default();
            let has_totp = login
                .and_then(|l| l.totp.as_deref())
                .is_some_and(|s| !s.is_empty() && TotpSpec::parse(s).is_some());
            let has_passkey = login
                .and_then(|l| l.fido2_credentials.as_ref())
                .is_some_and(|c| !c.is_empty());
            (subtitle, uri_match, has_totp, has_passkey)
        }
        CipherType::Card => {
            let card = view.card.as_ref();
            let last4 = card.and_then(|c| card_last4(c.number.as_deref()));
            let brand = card.and_then(|c| c.brand.clone());
            let mut sub = card_subtitle(brand.as_deref(), last4.as_deref());
            if let Some(exp) =
                card.and_then(|c| card_exp_display(c.exp_month.as_deref(), c.exp_year.as_deref()))
            {
                if sub.is_empty() {
                    sub = exp;
                } else {
                    sub.push_str(" · ");
                    sub.push_str(&exp);
                }
            }
            (sub, false, false, false)
        }
        CipherType::Identity => {
            let id = view.identity.as_ref();
            let sub = id
                .and_then(|i| nonempty(i.email.clone()))
                .or_else(|| id.and_then(identity_place))
                .or_else(|| id.and_then(|i| nonempty(i.company.clone())))
                .unwrap_or_else(|| identity_full_name(id).unwrap_or_default());
            (sub, false, false, false)
        }
        CipherType::SecureNote => {
            let sub = view
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let line = s.lines().next().unwrap_or(s);
                    if line.chars().count() > 48 {
                        let t: String = line.chars().take(47).collect();
                        format!("{t}…")
                    } else {
                        line.to_string()
                    }
                })
                .unwrap_or_default();
            (sub, false, false, false)
        }
        CipherType::SshKey => {
            let fp = view
                .ssh_key
                .as_ref()
                .map(|s| s.fingerprint.as_str())
                .unwrap_or("");
            let sub = if fp.len() > 28 {
                format!("{}…", &fp[..28])
            } else {
                fp.to_string()
            };
            (sub, false, false, false)
        }
        CipherType::BankAccount => {
            let b = view.bank_account.as_ref();
            let bank = b.and_then(|x| nonempty(x.bank_name.clone()));
            let last = b
                .and_then(|x| x.account_number.as_deref())
                .map(|n| n.chars().filter(|c| c.is_ascii_digit()).collect::<String>())
                .filter(|d| d.len() >= 4)
                .map(|d| format!("•••• {}", &d[d.len() - 4..]));
            let sub = match (bank, last) {
                (Some(b), Some(n)) => format!("{b} · {n}"),
                (Some(b), None) => b,
                (None, Some(n)) => n,
                (None, None) => String::new(),
            };
            (sub, false, false, false)
        }
        CipherType::DriversLicense => {
            let d = view.drivers_license.as_ref();
            let sub = d
                .and_then(|x| nonempty(x.issuing_state.clone()))
                .or_else(|| d.and_then(|x| nonempty(x.issuing_country.clone())))
                .unwrap_or_default();
            (sub, false, false, false)
        }
        CipherType::Passport => {
            let p = view.passport.as_ref();
            let sub = p
                .and_then(|x| nonempty(x.nationality.clone()))
                .or_else(|| p.and_then(|x| nonempty(x.issuing_country.clone())))
                .unwrap_or_default();
            (sub, false, false, false)
        }
    }
}

/// Search corpus: name, notes, username, URIs, card brand/last4, identity
/// names/email/address, text custom fields. Never passwords, PAN, CVC, TOTP.
fn search_haystack(view: &CipherView) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(view.name.clone());
    if let Some(n) = view.notes.as_deref() {
        parts.push(n.to_string());
    }
    match view.r#type {
        CipherType::Login => {
            if let Some(login) = view.login.as_ref() {
                if let Some(u) = &login.username {
                    parts.push(u.clone());
                }
                if let Some(uris) = &login.uris {
                    for uri in uris {
                        if let Some(u) = &uri.uri {
                            parts.push(u.clone());
                        }
                    }
                }
            }
        }
        CipherType::Card => {
            if let Some(card) = view.card.as_ref() {
                if let Some(b) = &card.brand {
                    parts.push(b.clone());
                }
                if let Some(n) = card_last4(card.number.as_deref()) {
                    parts.push(n);
                }
                if let Some(h) = &card.cardholder_name {
                    parts.push(h.clone());
                }
            }
        }
        CipherType::Identity => {
            if let Some(id) = view.identity.as_ref() {
                for s in [
                    &id.title,
                    &id.first_name,
                    &id.middle_name,
                    &id.last_name,
                    &id.email,
                    &id.username,
                    &id.company,
                    &id.phone,
                    &id.address1,
                    &id.city,
                    &id.state,
                    &id.postal_code,
                    &id.country,
                ] {
                    if let Some(v) = s {
                        parts.push(v.clone());
                    }
                }
            }
        }
        CipherType::SshKey => {
            if let Some(ssh) = view.ssh_key.as_ref() {
                parts.push(ssh.fingerprint.clone());
            }
        }
        CipherType::BankAccount => {
            if let Some(b) = view.bank_account.as_ref() {
                if let Some(n) = &b.bank_name {
                    parts.push(n.clone());
                }
                if let Some(n) = &b.name_on_account {
                    parts.push(n.clone());
                }
            }
        }
        CipherType::DriversLicense | CipherType::Passport | CipherType::SecureNote => {}
    }
    if let Some(fields) = view.fields.as_ref() {
        for f in fields {
            if let Some(n) = &f.name {
                parts.push(n.clone());
            }
            if f.r#type == FieldType::Text {
                if let Some(v) = &f.value {
                    parts.push(v.clone());
                }
            }
        }
    }
    parts.join(" ").to_lowercase()
}

fn push_identity_fields(fields: &mut Vec<RecordField>, id: &IdentityView) {
    push_field(fields, "title", "Title", id.title.clone(), false, false);
    push_field(
        fields,
        "first",
        "First name",
        id.first_name.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "middle",
        "Middle name",
        id.middle_name.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "last",
        "Last name",
        id.last_name.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "username",
        "Username",
        id.username.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "company",
        "Company",
        id.company.clone(),
        false,
        false,
    );
    push_field(fields, "ssn", "SSN", id.ssn.clone(), true, false);
    push_field(
        fields,
        "passport",
        "Passport number",
        id.passport_number.clone(),
        true,
        false,
    );
    push_field(
        fields,
        "license",
        "License number",
        id.license_number.clone(),
        true,
        false,
    );
    push_field(fields, "email", "Email", id.email.clone(), false, false);
    push_field(fields, "phone", "Phone", id.phone.clone(), false, false);
    push_field(
        fields,
        "addr1",
        "Address 1",
        id.address1.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "addr2",
        "Address 2",
        id.address2.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "addr3",
        "Address 3",
        id.address3.clone(),
        false,
        false,
    );
    push_field(fields, "city", "City", id.city.clone(), false, false);
    push_field(fields, "state", "State", id.state.clone(), false, false);
    push_field(
        fields,
        "postal",
        "Postal code",
        id.postal_code.clone(),
        false,
        false,
    );
    push_field(
        fields,
        "country",
        "Country",
        id.country.clone(),
        false,
        false,
    );
}

fn push_field(
    fields: &mut Vec<RecordField>,
    key: &str,
    label: &str,
    value: Option<String>,
    hidden: bool,
    mono: bool,
) {
    let Some(value) = nonempty(value) else {
        return;
    };
    fields.push(RecordField {
        key: key.to_string(),
        label: label.to_string(),
        value,
        hidden,
        mono,
    });
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn identity_full_name(id: Option<&IdentityView>) -> Option<String> {
    let id = id?;
    let parts: Vec<&str> = [
        id.title.as_deref(),
        id.first_name.as_deref(),
        id.middle_name.as_deref(),
        id.last_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn identity_place(id: &IdentityView) -> Option<String> {
    match (nonempty(id.city.clone()), nonempty(id.state.clone())) {
        (Some(c), Some(s)) => Some(format!("{c}, {s}")),
        (Some(c), None) => Some(c),
        (None, Some(s)) => Some(s),
        (None, None) => nonempty(id.country.clone()),
    }
}

fn host_of(uri: &str) -> Option<String> {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .or_else(|| {
            let t = uri.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
}

fn cipher_last_used(view: &CipherView, id: &str, mru: &HashMap<String, i64>) -> i64 {
    let bw_used = view.local_data.as_ref().and_then(|ld| {
        let v = serde_json::to_value(ld).ok()?;
        v.get("lastUsedDate")?.as_i64()
    });
    let ours = mru.get(id).copied();
    ours.into_iter()
        .chain(bw_used)
        .max()
        .unwrap_or_else(|| view.revision_date.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        kind: ItemKind,
        name: &str,
        subtitle: &str,
        search: &str,
        uri_match: bool,
    ) -> ItemSummary {
        ItemSummary {
            id: name.to_string(),
            kind,
            name: name.into(),
            subtitle: subtitle.into(),
            last_used: 0,
            has_totp: false,
            has_passkey: false,
            uri_match,
            search: search.to_lowercase(),
        }
    }

    #[test]
    fn query_matches_all_words() {
        let s = summary(
            ItemKind::Login,
            "GitHub",
            "alice",
            "github alice https://github.com backup codes",
            true,
        );
        assert!(s.matches_query(""));
        assert!(s.matches_query("git"));
        assert!(s.matches_query("GITHUB alice"));
        assert!(!s.matches_query("github missing"));
        assert!(s.matches_query("backup"));
    }

    #[test]
    fn filter_by_kind_and_query() {
        let items = vec![
            summary(ItemKind::Login, "GitHub", "a", "github a", true),
            summary(ItemKind::Card, "Visa", "1111", "visa 1111", false),
            summary(
                ItemKind::Identity,
                "Home",
                "jane",
                "home jane portland",
                false,
            ),
            summary(ItemKind::SecureNote, "Wifi", "psk", "wifi psk note", false),
        ];
        assert_eq!(filter_items(&items, "", ItemFilter::All).len(), 4);
        assert_eq!(filter_items(&items, "", ItemFilter::Card).len(), 1);
        assert_eq!(filter_items(&items, "jane", ItemFilter::All).len(), 1);
        assert!(filter_items(&items, "visa", ItemFilter::Login).is_empty());
        assert_eq!(
            filter_items(&items, "wifi", ItemFilter::Note)[0].name,
            "Wifi"
        );
    }

    #[test]
    fn filter_note_excludes_ssh() {
        let items = vec![summary(
            ItemKind::SshKey,
            "deploy",
            "SHA256",
            "deploy sha256",
            false,
        )];
        assert_eq!(filter_items(&items, "", ItemFilter::All).len(), 1);
        assert!(filter_items(&items, "", ItemFilter::Note).is_empty());
    }

    #[test]
    fn kind_fill_flags() {
        assert!(ItemKind::Login.can_fill());
        assert!(ItemKind::Card.can_fill());
        assert!(ItemKind::Identity.can_fill());
        assert!(!ItemKind::SecureNote.can_fill());
        assert!(!ItemKind::SshKey.can_fill());
    }
}
