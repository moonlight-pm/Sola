//! Apply an edit ([`ItemDraft`]) onto a decrypted item.
//!
//! The draft is plain strings. Apply changes the [`CipherView`] in place,
//! so passkeys, attachments, collections and linked fields stay put, and
//! writes only what the form changed since it opened (`base`): the item is
//! synced just before, so a field the form never showed (a code key we
//! cannot read, an empty custom field) or one changed elsewhere meanwhile
//! is left as the server has it.

use bitwarden_vault::{
    CipherRepromptType, CipherType, CipherView, FieldType, FieldView, LoginUriView,
    PasswordHistoryView,
};
use chrono::{NaiveDate, Utc};
use sola_vault_proto::ItemDraft;

use super::convert;
use super::new_item::{empty_card, empty_identity, empty_login};

const MAX_PASSWORD_HISTORY: usize = 5;

/// Fold the draft into `view`.
pub fn apply_draft(view: &mut CipherView, draft: &ItemDraft) -> Result<(), String> {
    if !view.edit {
        return Err("This item cannot be edited.".into());
    }
    let kind = convert::kind(view.r#type);
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
    if draft.base.as_ref().is_none_or(|b| b.reprompt != draft.reprompt) {
        view.reprompt = if draft.reprompt {
            CipherRepromptType::Password
        } else {
            CipherRepromptType::None
        };
    }
    if draft.base.as_ref().is_none_or(|b| b.folder_id != draft.folder_id) {
        view.folder_id = match draft.folder_id.as_deref().filter(|s| !s.is_empty()) {
            Some(id) => Some(id.parse().map_err(|_| "No such folder.".to_string())?),
            None => None,
        };
    }

    match view.r#type {
        CipherType::Login => apply_login(view, draft),
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
fn set_date(slot: &mut Option<NaiveDate>, draft: &ItemDraft, key: &str, label: &str) -> Result<(), String> {
    if let Some(v) = draft.changed_row(key) {
        *slot = parse_date(v, label)?;
    }
    Ok(())
}

fn apply_login(view: &mut CipherView, draft: &ItemDraft) {
    let login = view.login.get_or_insert_with(empty_login);
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
                    r#match: None,
                    uri_checksum: None,
                });
            uri.uri = Some(t.to_string());
            uri.r#match = d.match_type.map(convert::uri_match_type);
            uris.push(uri);
        }
        login.uris = (!uris.is_empty()).then_some(uris);
    }
    // Checksum-less URIs are dropped by readers once the item has its own
    // key, which the blob write gives it: always stamp them.
    login.generate_checksums();

    if let Some(old) = retired.filter(|s| !s.is_empty()) {
        push_history(view, old);
    }
}

fn apply_card(view: &mut CipherView, draft: &ItemDraft) {
    let card = view.card.get_or_insert_with(empty_card);
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
    let ssh = view.ssh_key.get_or_insert_with(|| bitwarden_vault::SshKeyView {
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
    let b = view.bank_account.get_or_insert_with(Default::default);
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
    let d = view.drivers_license.get_or_insert_with(Default::default);
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
    let p = view.passport.get_or_insert_with(Default::default);
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
            // Removed on the form: a hidden value goes to the history.
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
            nf.r#type = if d.hidden { FieldType::Hidden } else { FieldType::Text };
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
            r#type: if d.hidden { FieldType::Hidden } else { FieldType::Text },
            linked_id: None,
        });
    }
    view.fields = (!next.is_empty()).then_some(next);
}

fn push_history(view: &mut CipherView, password: String) {
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
    (!t.is_empty()).then(|| t.to_string())
}

fn normalize_month(s: String) -> String {
    match s.parse::<u32>() {
        Ok(n) if (1..=12).contains(&n) => format!("{n:02}"),
        _ => s,
    }
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

#[cfg(test)]
mod tests {
    use bitwarden_vault::UriMatchType;
    use sola_vault_proto::{CustomDraft, ItemKind, UriDraft, UriMatch};

    use super::*;
    use crate::vault::item::record_from_view;
    use crate::vault::new_item::{new_login_view, new_view};

    fn login_cipher() -> CipherView {
        let mut view = new_login_view(
            "GitHub".into(),
            Some("alice".into()),
            Some("oldpass".into()),
            Some("https://github.com".into()),
        );
        view.id = Some("11111111-1111-1111-1111-111111111111".parse().unwrap());
        view
    }

    fn draft_of(view: &CipherView) -> ItemDraft {
        ItemDraft::from_record(&record_from_view(view.clone(), Vec::new()).unwrap())
    }

    #[test]
    fn apply_updates_login_fields() {
        let mut view = login_cipher();
        let mut draft = draft_of(&view);
        draft.name = "GH".into();
        draft.notes = "work".into();
        draft.set_row("username", "bob".into());
        draft.set_row("password", "newpass".into());
        draft.set_row("totp", "JBSWY3DPEHPK3PXP".into());
        draft.uris[0].value = "https://github.com/login".into();
        draft.uris.push(UriDraft {
            index: None,
            value: " ".into(),
            match_type: None,
        });
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.name, "GH");
        assert_eq!(view.notes.as_deref(), Some("work"));
        let login = view.login.as_ref().unwrap();
        assert_eq!(login.username.as_deref(), Some("bob"));
        assert_eq!(login.password.as_deref(), Some("newpass"));
        assert_eq!(login.totp.as_deref(), Some("JBSWY3DPEHPK3PXP"));
        let uris = login.uris.as_ref().unwrap();
        assert_eq!(uris.len(), 1);
        assert_eq!(uris[0].uri.as_deref(), Some("https://github.com/login"));
        assert_eq!(view.password_history.as_ref().unwrap()[0].password, "oldpass");
    }

    #[test]
    fn unchanged_password_skips_history() {
        let mut view = login_cipher();
        let draft = draft_of(&view);
        apply_draft(&mut view, &draft).unwrap();
        assert!(view.password_history.unwrap_or_default().is_empty());
        assert_eq!(view.login.as_ref().unwrap().password.as_deref(), Some("oldpass"));
    }

    #[test]
    fn refuse_when_the_item_is_not_editable() {
        let mut view = login_cipher();
        view.edit = false;
        let draft = draft_of(&view);
        assert!(apply_draft(&mut view, &draft).unwrap_err().contains("cannot be edited"));
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
        let mut draft = draft_of(&view);
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
    fn apply_draft_keeps_the_passkey_slot() {
        let mut view = login_cipher();
        view.login.as_mut().unwrap().fido2_credentials = Some(Vec::new());
        let mut draft = draft_of(&view);
        draft.set_row("username", "bob".into());
        apply_draft(&mut view, &draft).unwrap();
        assert!(view.login.as_ref().unwrap().fido2_credentials.is_some());
        assert_eq!(view.login.as_ref().unwrap().username.as_deref(), Some("bob"));
    }

    #[test]
    fn a_date_must_be_iso() {
        let mut view = new_view(ItemKind::DriversLicense);
        view.id = Some("11111111-1111-1111-1111-111111111111".parse().unwrap());
        let mut draft = draft_of(&view);
        draft.set_row("dob", "03/14/1990".into());
        assert!(apply_draft(&mut view, &draft).unwrap_err().contains("YYYY-MM-DD"));
    }

    #[test]
    fn unreadable_totp_survives_an_unrelated_edit() {
        let mut view = login_cipher();
        // SHA-256: stored by another client, no code from us.
        let raw = "otpauth://totp/X?secret=JBSWY3DPEHPK3PXP&algorithm=SHA256";
        view.login.as_mut().unwrap().totp = Some(raw.into());
        let record = record_from_view(view.clone(), Vec::new()).unwrap();
        assert_eq!(record.unreadable_totp(), Some(raw));
        let mut draft = ItemDraft::from_record(&record);
        assert_eq!(draft.row_value("totp"), raw);
        draft.notes = "recovery: 1111-2222".into();
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.login.as_ref().unwrap().totp.as_deref(), Some(raw));
    }

    #[test]
    fn a_field_changed_elsewhere_survives_the_save() {
        let mut view = login_cipher();
        let mut draft = draft_of(&view);
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
        let mut draft = draft_of(&view);
        draft.set_row("totp", String::new());
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.login.as_ref().unwrap().totp, None);
    }

    #[test]
    fn an_empty_custom_field_survives_an_edit_to_another() {
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
        let mut draft = draft_of(&view);
        assert_eq!(draft.custom.len(), 1);
        draft.custom[0].value = "beta".into();
        apply_draft(&mut view, &draft).unwrap();
        let fields = view.fields.as_ref().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.as_deref(), Some("Security answer"));
        assert_eq!(fields[1].value.as_deref(), Some("beta"));
    }

    #[test]
    fn an_edited_website_keeps_its_own_match_type() {
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
        let mut draft = draft_of(&view);
        assert_eq!(draft.uris.len(), 1);
        draft.uris[0].value = "https://example.org".into();
        apply_draft(&mut view, &draft).unwrap();
        let uris = view.login.as_ref().unwrap().uris.as_ref().unwrap();
        assert_eq!(uris.len(), 1);
        assert_eq!(uris[0].uri.as_deref(), Some("https://example.org"));
        assert_eq!(uris[0].r#match, Some(UriMatchType::Host));
    }

    #[test]
    fn a_website_takes_the_match_type_the_form_chose() {
        let mut view = login_cipher();
        let mut draft = draft_of(&view);
        draft.uris[0].match_type = Some(UriMatch::StartsWith);
        apply_draft(&mut view, &draft).unwrap();
        let uris = view.login.as_ref().unwrap().uris.as_ref().unwrap();
        assert_eq!(uris[0].r#match, Some(UriMatchType::StartsWith));
    }

    #[test]
    fn reprompt_and_folder_follow_the_form() {
        let mut view = login_cipher();
        let mut draft = draft_of(&view);
        draft.reprompt = true;
        draft.folder_id = Some("22222222-2222-2222-2222-222222222222".into());
        apply_draft(&mut view, &draft).unwrap();
        assert_eq!(view.reprompt, CipherRepromptType::Password);
        assert_eq!(
            view.folder_id.map(|f| f.to_string()).as_deref(),
            Some("22222222-2222-2222-2222-222222222222")
        );
        let mut draft = draft_of(&view);
        draft.folder_id = Some("not a folder".into());
        assert!(apply_draft(&mut view, &draft).is_err());
    }

    #[test]
    fn a_hidden_password_row_is_left_out() {
        let mut view = login_cipher();
        view.view_password = false;
        let draft = draft_of(&view);
        assert!(!draft.rows.iter().any(|r| r.key == "password"));
    }

    #[test]
    fn a_blank_draft_writes_a_new_item() {
        let mut view = new_view(ItemKind::Card);
        let mut draft = ItemDraft::blank(ItemKind::Card);
        draft.name = "Visa".into();
        draft.set_row("number", "4111111111111111".into());
        draft.set_row("exp_month", "3".into());
        apply_draft(&mut view, &draft).unwrap();
        let card = view.card.as_ref().unwrap();
        assert_eq!(view.name, "Visa");
        assert_eq!(card.number.as_deref(), Some("4111111111111111"));
        assert_eq!(card.exp_month.as_deref(), Some("03"));
    }
}
