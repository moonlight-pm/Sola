//! Items as the socket shows them: a summary row (never a secret) and the
//! whole record.

use std::collections::HashMap;

use bitwarden_vault::{CipherRepromptType, CipherType, CipherView, FieldType, IdentityView};
use sola_vault_proto::{
    Attachment, HistoryEntry, ItemKind, ItemRecord, ItemSummary, PasskeyInfo, RecordField,
    TotpSpec, UriEntry,
};

use super::card;
use super::convert;
use super::match_uri::uri_matches;

/// A row for the list, or `None` for an archived item or one with no id.
/// Trashed items are kept, marked. With a `page_url`, a login that
/// matches it is marked too.
pub fn summary_from_view(
    view: &CipherView,
    page_url: &str,
    mru: &HashMap<String, i64>,
) -> Option<ItemSummary> {
    if view.archived_date.is_some() {
        return None;
    }
    let id = view.id?.to_string();
    let kind = convert::kind(view.r#type);
    let (subtitle, uri_match, has_totp, has_passkey) = list_meta(view, page_url);
    Some(ItemSummary {
        last_used: cipher_last_used(view, &id, mru),
        id,
        kind,
        name: display_name(view),
        subtitle,
        revised: view.revision_date.timestamp(),
        has_totp,
        has_passkey,
        uri_match,
        reprompt: view.reprompt == CipherRepromptType::Password,
        favorite: view.favorite,
        folder_id: view.folder_id.map(|f| f.to_string()),
        organization_id: view.organization_id.map(|o| o.to_string()),
        collection_ids: view.collection_ids.iter().map(|c| c.to_string()).collect(),
        deleted: view.deleted_date.is_some(),
        search: search_haystack(view),
    })
}

/// The whole item. `passkeys` are its passkeys' public facts (decrypted
/// by the caller, which holds the keys). A login whose password the
/// account may not see (`view_password` off) comes without it and its
/// history.
pub fn record_from_view(view: CipherView, passkeys: Vec<PasskeyInfo>) -> Option<ItemRecord> {
    let id = view.id?.to_string();
    let kind = convert::kind(view.r#type);
    let mut fields = Fields::default();
    let mut totp_secret = None;
    let mut totp_period = 30u32;
    let mut uris = Vec::new();
    let mut has_passkey = false;
    let date = |d: Option<chrono::NaiveDate>| d.map(|d| d.to_string());

    match kind {
        ItemKind::Login => {
            if let Some(login) = view.login.as_ref() {
                fields.push("username", "Username", &login.username, false, false);
                if view.view_password {
                    fields.push("password", "Password", &login.password, true, false);
                }
                for (i, u) in login.uris.iter().flatten().enumerate() {
                    let label = if i == 0 {
                        "Website".to_string()
                    } else {
                        format!("Website {}", i + 1)
                    };
                    fields.push(&format!("uri{i}"), &label, &u.uri, false, false);
                    uris.push(UriEntry {
                        uri: u.uri.clone().unwrap_or_default(),
                        match_type: u.r#match.map(convert::uri_match),
                    });
                }
                has_passkey = login.fido2_credentials.as_ref().is_some_and(|c| !c.is_empty());
                // Kept even when no code can be made from it, so the
                // record shows it and the edit form carries it.
                if let Some(raw) = login.totp.as_deref().filter(|s| !s.trim().is_empty()) {
                    if let Some(spec) = TotpSpec::parse(raw) {
                        totp_period = spec.period;
                    }
                    totp_secret = Some(raw.to_string());
                }
            }
        }
        ItemKind::Card => {
            if let Some(c) = view.card.as_ref() {
                fields.push("cardholder", "Cardholder", &c.cardholder_name, false, false);
                fields.push("brand", "Brand", &c.brand, false, false);
                fields.push("number", "Number", &c.number, true, true);
                let exp = card::exp_display(c.exp_month.as_deref(), c.exp_year.as_deref());
                fields.push("exp", "Expiration", &exp, false, false);
                fields.push("code", "Security code", &c.code, true, true);
            }
        }
        ItemKind::Identity => {
            if let Some(idv) = view.identity.as_ref() {
                push_identity(&mut fields, idv);
            }
        }
        ItemKind::SshKey => {
            if let Some(ssh) = view.ssh_key.as_ref() {
                fields.push("fingerprint", "Fingerprint", &Some(ssh.fingerprint.clone()), false, true);
                fields.push("public", "Public key", &Some(ssh.public_key.clone()), false, true);
                fields.push("private", "Private key", &Some(ssh.private_key.clone()), true, true);
            }
        }
        ItemKind::BankAccount => {
            if let Some(b) = view.bank_account.as_ref() {
                fields.push("bank", "Bank", &b.bank_name, false, false);
                fields.push("name_on", "Name on account", &b.name_on_account, false, false);
                fields.push("type", "Type", &b.account_type, false, false);
                fields.push("acct", "Account number", &b.account_number, true, true);
                fields.push("routing", "Routing number", &b.routing_number, true, true);
                fields.push("branch", "Branch", &b.branch_number, false, false);
                fields.push("pin", "PIN", &b.pin, true, true);
                fields.push("swift", "SWIFT", &b.swift_code, false, true);
                fields.push("iban", "IBAN", &b.iban, true, true);
                fields.push("phone", "Bank phone", &b.bank_contact_phone, false, false);
            }
        }
        ItemKind::DriversLicense => {
            if let Some(d) = view.drivers_license.as_ref() {
                fields.push("first", "First name", &d.first_name, false, false);
                fields.push("middle", "Middle name", &d.middle_name, false, false);
                fields.push("last", "Last name", &d.last_name, false, false);
                fields.push("dob", "Date of birth", &date(d.date_of_birth), false, false);
                fields.push("number", "License number", &d.license_number, true, false);
                fields.push("country", "Country", &d.issuing_country, false, false);
                fields.push("state", "State", &d.issuing_state, false, false);
                fields.push("issued", "Issued", &date(d.issue_date), false, false);
                fields.push("expires", "Expires", &date(d.expiration_date), false, false);
                fields.push("authority", "Authority", &d.issuing_authority, false, false);
                fields.push("class", "Class", &d.license_class, false, false);
            }
        }
        ItemKind::Passport => {
            if let Some(p) = view.passport.as_ref() {
                fields.push("given", "Given name", &p.given_name, false, false);
                fields.push("surname", "Surname", &p.surname, false, false);
                fields.push("dob", "Date of birth", &date(p.date_of_birth), false, false);
                fields.push("sex", "Sex", &p.sex, false, false);
                fields.push("birth", "Place of birth", &p.birth_place, false, false);
                fields.push("nat", "Nationality", &p.nationality, false, false);
                fields.push("country", "Issuing country", &p.issuing_country, false, false);
                fields.push("number", "Passport number", &p.passport_number, true, false);
                fields.push("type", "Type", &p.passport_type, false, false);
                fields.push("nid", "National ID", &p.national_identification_number, true, false);
                fields.push("authority", "Authority", &p.issuing_authority, false, false);
                fields.push("issued", "Issued", &date(p.issue_date), false, false);
                fields.push("expires", "Expires", &date(p.expiration_date), false, false);
            }
        }
        ItemKind::SecureNote => {}
    }

    for (i, f) in view.fields.iter().flatten().enumerate() {
        let label = f
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Field");
        // A linked field is a reference to another field, not a value.
        let value = if f.r#type == FieldType::Linked {
            None
        } else {
            f.value.clone()
        };
        fields.push(&format!("custom{i}"), label, &value, f.r#type == FieldType::Hidden, false);
    }

    let password_history = if view.view_password {
        view.password_history
            .iter()
            .flatten()
            .map(|h| HistoryEntry {
                password: h.password.clone(),
                last_used: h.last_used_date.to_rfc3339(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let attachments = view
        .attachments
        .iter()
        .flatten()
        .filter_map(|a| {
            Some(Attachment {
                id: a.id.clone()?,
                file_name: a.file_name.clone().unwrap_or_default(),
                size: a.size_name.clone().unwrap_or_default(),
            })
        })
        .collect();

    Some(ItemRecord {
        id,
        kind,
        name: display_name(&view),
        notes: view
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        fields: fields.0,
        totp_secret,
        totp_period,
        has_passkey,
        can_edit: view.edit,
        view_password: view.view_password,
        reprompt: view.reprompt == CipherRepromptType::Password,
        favorite: view.favorite,
        folder_id: view.folder_id.map(|f| f.to_string()),
        organization_id: view.organization_id.map(|o| o.to_string()),
        collection_ids: view.collection_ids.iter().map(|c| c.to_string()).collect(),
        uris,
        passkeys,
        password_history,
        attachments,
        created: Some(view.creation_date.to_rfc3339()),
        revised: Some(view.revision_date.to_rfc3339()),
        deleted: view.deleted_date.map(|d| d.to_rfc3339()),
    })
}

/// The item's name, or its kind when it has none.
fn display_name(view: &CipherView) -> String {
    if view.name.trim().is_empty() {
        convert::kind(view.r#type).label().to_string()
    } else {
        view.name.clone()
    }
}

/// A record's fields in order; empty values are left out.
#[derive(Default)]
struct Fields(Vec<RecordField>);

impl Fields {
    fn push(&mut self, key: &str, label: &str, value: &Option<String>, hidden: bool, mono: bool) {
        let Some(value) = nonempty(value.clone()) else {
            return;
        };
        self.0.push(RecordField {
            key: key.to_string(),
            label: label.to_string(),
            value,
            hidden,
            mono,
        });
    }
}

fn push_identity(fields: &mut Fields, id: &IdentityView) {
    fields.push("title", "Title", &id.title, false, false);
    fields.push("first", "First name", &id.first_name, false, false);
    fields.push("middle", "Middle name", &id.middle_name, false, false);
    fields.push("last", "Last name", &id.last_name, false, false);
    fields.push("username", "Username", &id.username, false, false);
    fields.push("company", "Company", &id.company, false, false);
    fields.push("ssn", "SSN", &id.ssn, true, false);
    fields.push("passport", "Passport number", &id.passport_number, true, false);
    fields.push("license", "License number", &id.license_number, true, false);
    fields.push("email", "Email", &id.email, false, false);
    fields.push("phone", "Phone", &id.phone, false, false);
    fields.push("addr1", "Address 1", &id.address1, false, false);
    fields.push("addr2", "Address 2", &id.address2, false, false);
    fields.push("addr3", "Address 3", &id.address3, false, false);
    fields.push("city", "City", &id.city, false, false);
    fields.push("state", "State", &id.state, false, false);
    fields.push("postal", "Postal code", &id.postal_code, false, false);
    fields.push("country", "Country", &id.country, false, false);
}

/// Subtitle, matches the page, has a readable code, has a passkey.
fn list_meta(view: &CipherView, page_url: &str) -> (String, bool, bool, bool) {
    match view.r#type {
        CipherType::Login => {
            let login = view.login.as_ref();
            let username = login
                .and_then(|l| l.username.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let mut uri_match = false;
            let mut shown_uri = None;
            for u in login.and_then(|l| l.uris.as_ref()).into_iter().flatten() {
                let Some(uri) = u.uri.as_deref() else { continue };
                if !page_url.is_empty() && uri_matches(page_url, uri, u.r#match) {
                    uri_match = true;
                    shown_uri = Some(uri.to_string());
                    break;
                }
                shown_uri.get_or_insert_with(|| uri.to_string());
            }
            let subtitle = username
                .map(str::to_string)
                .or_else(|| shown_uri.as_deref().and_then(host_of))
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
            let c = view.card.as_ref();
            let last4 = c.and_then(|c| card::last4(c.number.as_deref()));
            let mut sub = card::subtitle(c.and_then(|c| c.brand.as_deref()), last4.as_deref());
            if let Some(exp) = c.and_then(|c| card::exp_display(c.exp_month.as_deref(), c.exp_year.as_deref())) {
                if !sub.is_empty() {
                    sub.push_str(" · ");
                }
                sub.push_str(&exp);
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
            let fp: String = view
                .ssh_key
                .as_ref()
                .map(|s| s.fingerprint.as_str())
                .unwrap_or("")
                .chars()
                .collect();
            let sub = if fp.chars().count() > 28 {
                format!("{}…", fp.chars().take(28).collect::<String>())
            } else {
                fp
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

/// What search reads: name, notes, usernames, websites, card brand and
/// last digits, a person's names and address, text custom fields. Never a
/// password, card number, code key or hidden field.
fn search_haystack(view: &CipherView) -> String {
    let mut parts: Vec<String> = vec![view.name.clone()];
    parts.extend(view.notes.clone());
    match view.r#type {
        CipherType::Login => {
            if let Some(login) = view.login.as_ref() {
                parts.extend(login.username.clone());
                parts.extend(login.uris.iter().flatten().filter_map(|u| u.uri.clone()));
            }
        }
        CipherType::Card => {
            if let Some(c) = view.card.as_ref() {
                parts.extend(c.brand.clone());
                parts.extend(card::last4(c.number.as_deref()));
                parts.extend(c.cardholder_name.clone());
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
                    parts.extend(s.clone());
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
                parts.extend(b.bank_name.clone());
                parts.extend(b.name_on_account.clone());
            }
        }
        CipherType::DriversLicense | CipherType::Passport | CipherType::SecureNote => {}
    }
    for f in view.fields.iter().flatten() {
        parts.extend(f.name.clone());
        if f.r#type == FieldType::Text {
            parts.extend(f.value.clone());
        }
    }
    parts.join(" ").to_lowercase()
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn identity_full_name(id: Option<&IdentityView>) -> Option<String> {
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
    (!parts.is_empty()).then(|| parts.join(" "))
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
        .and_then(|u| u.host_str().map(str::to_string))
        .or_else(|| nonempty(Some(uri.to_string())))
}

/// Unix seconds this item was last used: our fill / passkey clock, then
/// Bitwarden's local last-used date, else its revision.
pub fn cipher_last_used(view: &CipherView, id: &str, mru: &HashMap<String, i64>) -> i64 {
    // `LocalDataView` keeps its fields private; it serializes the date as
    // RFC 3339.
    let bw_used = view.local_data.as_ref().and_then(|ld| {
        let v = serde_json::to_value(ld).ok()?;
        let at = v.get("lastUsedDate")?.as_str()?;
        Some(chrono::DateTime::parse_from_rfc3339(at).ok()?.timestamp())
    });
    mru.get(id)
        .copied()
        .into_iter()
        .chain(bw_used)
        .max()
        .unwrap_or_else(|| view.revision_date.timestamp())
}

#[cfg(test)]
mod tests {
    use bitwarden_vault::{FieldView, LoginUriView, UriMatchType};

    use super::*;
    use crate::vault::new_item::new_view;

    fn login() -> CipherView {
        let mut view = new_view(ItemKind::Login);
        view.id = Some("11111111-1111-1111-1111-111111111111".parse().unwrap());
        view.name = "GitHub".into();
        let l = view.login.as_mut().unwrap();
        l.username = Some("alice".into());
        l.password = Some("hunter2".into());
        l.uris = Some(vec![
            LoginUriView {
                uri: None,
                r#match: Some(UriMatchType::Exact),
                uri_checksum: None,
            },
            LoginUriView {
                uri: Some("https://github.com".into()),
                r#match: Some(UriMatchType::Host),
                uri_checksum: None,
            },
        ]);
        view.fields = Some(vec![FieldView {
            name: Some("Recovery".into()),
            value: Some("secret words".into()),
            r#type: FieldType::Hidden,
            linked_id: None,
        }]);
        view
    }

    #[test]
    fn a_summary_never_carries_a_secret() {
        let s = summary_from_view(&login(), "https://github.com/login", &HashMap::new()).unwrap();
        assert!(s.uri_match);
        assert_eq!(s.subtitle, "alice");
        assert!(!s.search.contains("hunter2"));
        assert!(!s.search.contains("secret words"));
        assert!(s.search.contains("recovery"));
        assert!(s.matches_query("git ALICE"));
    }

    #[test]
    fn trash_is_listed_and_marked_archive_is_not() {
        let mut view = login();
        view.deleted_date = Some(chrono::Utc::now());
        assert!(summary_from_view(&view, "", &HashMap::new()).unwrap().deleted);
        view.archived_date = Some(chrono::Utc::now());
        assert!(summary_from_view(&view, "", &HashMap::new()).is_none());
    }

    #[test]
    fn a_record_keeps_every_website_in_place() {
        let r = record_from_view(login(), Vec::new()).unwrap();
        assert_eq!(r.uris.len(), 2);
        assert_eq!(r.uris[1].match_type, Some(sola_vault_proto::UriMatch::Host));
        // The empty first website is not a field; the second keeps index 1.
        assert_eq!(r.field("uri1"), Some("https://github.com"));
        assert_eq!(r.field("password"), Some("hunter2"));
    }

    #[test]
    fn a_hidden_password_stays_hidden() {
        let mut view = login();
        view.view_password = false;
        let r = record_from_view(view, Vec::new()).unwrap();
        assert_eq!(r.field("password"), None);
        assert!(r.password_history.is_empty());
    }
}
