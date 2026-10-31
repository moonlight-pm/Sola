//! Reading the unlocked vault: the list, a page's logins, one item, its
//! code, what a page form gets, a single value for a delivery.

use std::collections::HashMap;

use bitwarden_vault::{CipherRepromptType, CipherType, CipherView};
use sola_vault_proto::{
    DeliverField, ErrorCode, FillMaterial, IdentityFill, ItemKind, ItemRecord, ItemSummary, TotpSpec,
    UriEntry,
};
use zeroize::Zeroizing;

use super::convert;
use super::errors::{Result, err, not_found};
use super::item::{record_from_view, summary_from_view};
use super::match_uri::{log_host, uri_matches};
use super::passkey;
use super::service::VaultService;

/// Every item (trash marked), the page's logins first when `url` is
/// given, then most recently used, then by name; and how many items
/// could not be decrypted.
pub async fn list(svc: &VaultService, url: &str, mru: &HashMap<String, i64>) -> Result<(Vec<ItemSummary>, u32)> {
    let listed = svc.all().await?;
    let unreadable = listed.failures.len() as u32;
    if unreadable > 0 {
        let orgs: Vec<String> = listed
            .failures
            .iter()
            .filter_map(|c| c.organization_id.map(|id| id.to_string()))
            .collect();
        tracing::warn!(n = unreadable, org_ids = ?orgs, "vault: some items failed to decrypt");
    }
    let mut out: Vec<ItemSummary> = listed
        .successes
        .iter()
        .filter_map(|view| summary_from_view(view, url, mru))
        .collect();
    out.sort_by(|a, b| {
        b.uri_match
            .cmp(&a.uri_match)
            .then(b.last_used.cmp(&a.last_used))
            .then(a.name.cmp(&b.name))
    });
    tracing::debug!(n = out.len(), host = %log_host(url), "vault: list");
    Ok((out, unreadable))
}

/// The logins for a page, most recently used first (none in the trash).
pub async fn matches(svc: &VaultService, url: &str, mru: &HashMap<String, i64>) -> Result<Vec<ItemSummary>> {
    if url.trim().is_empty() {
        return Ok(Vec::new());
    }
    let (items, _) = list(svc, url, mru).await?;
    let out: Vec<ItemSummary> = items
        .into_iter()
        .filter(|s| s.uri_match && s.kind == ItemKind::Login && !s.deleted)
        .collect();
    tracing::info!(matches = out.len(), host = %log_host(url), "vault: page matches");
    Ok(out)
}

/// The whole item.
pub async fn get(svc: &VaultService, id: &str) -> Result<ItemRecord> {
    let view = svc.cipher(id).await?;
    let passkeys = passkey::infos(svc.pm().0.internal.get_key_store(), &view);
    record_from_view(view, passkeys).ok_or_else(not_found)
}

/// The item's code now, its period and the seconds it has left.
pub async fn code(svc: &VaultService, id: &str) -> Result<(String, u32, u32)> {
    let view = svc.cipher(id).await?;
    let spec = totp_spec(&view).ok_or_else(|| err(ErrorCode::NotFound, "This item has no code."))?;
    let now = unix_now();
    Ok((
        spec.code_at(now),
        spec.period,
        sola_vault_proto::remaining_secs(spec.period, now),
    ))
}

/// What a page form gets. A login only for a page one of its websites
/// matches; its code when `with_code`.
pub async fn fill(svc: &VaultService, id: &str, url: &str, with_code: bool) -> Result<FillMaterial> {
    let view = svc.cipher(id).await?;
    if view.deleted_date.is_some() {
        return Err(err(ErrorCode::Invalid, "This item is in the trash."));
    }
    match view.r#type {
        CipherType::Login => {
            let login = view.login.as_ref().ok_or_else(not_found)?;
            let matched = login
                .uris
                .iter()
                .flatten()
                .any(|u| u.uri.as_deref().is_some_and(|uri| uri_matches(url, uri, u.r#match)));
            if !matched {
                tracing::warn!(%id, host = %log_host(url), "vault: fill refused, no website matches");
                return Err(err(ErrorCode::Denied, "This login is not for this page."));
            }
            let code = with_code
                .then(|| totp_spec(&view).map(|s| s.code_at(unix_now())))
                .flatten();
            Ok(FillMaterial::Login {
                username: login.username.clone(),
                password: login.password.clone(),
                code,
                uris: login
                    .uris
                    .iter()
                    .flatten()
                    .filter_map(|u| {
                        Some(UriEntry {
                            uri: u.uri.clone()?,
                            match_type: u.r#match.map(convert::uri_match),
                        })
                    })
                    .collect(),
            })
        }
        CipherType::Card => {
            let c = view.card.as_ref().ok_or_else(not_found)?;
            Ok(FillMaterial::Card {
                cardholder_name: c.cardholder_name.clone(),
                number: c.number.clone(),
                exp_month: c.exp_month.clone(),
                exp_year: c.exp_year.clone(),
                code: c.code.clone(),
                brand: c.brand.clone(),
            })
        }
        CipherType::Identity => {
            let i = view.identity.as_ref().ok_or_else(not_found)?;
            let ne = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
            Ok(FillMaterial::Identity(IdentityFill {
                title: ne(&i.title),
                first_name: ne(&i.first_name),
                middle_name: ne(&i.middle_name),
                last_name: ne(&i.last_name),
                address1: ne(&i.address1),
                address2: ne(&i.address2),
                address3: ne(&i.address3),
                city: ne(&i.city),
                state: ne(&i.state),
                postal_code: ne(&i.postal_code),
                country: ne(&i.country),
                company: ne(&i.company),
                email: ne(&i.email),
                phone: ne(&i.phone),
                ssn: ne(&i.ssn),
                username: ne(&i.username),
                passport_number: ne(&i.passport_number),
                license_number: ne(&i.license_number),
            }))
        }
        _ => Err(err(ErrorCode::Invalid, "This item can't fill a page.")),
    }
}

/// One value of an item, for a delivery into another app's field. A
/// password is delivered even when the account may not view it, as
/// Bitwarden fills one. An item that asks for the master password first
/// (re-prompt) is not delivered: the desk menu cannot ask.
pub async fn value(svc: &VaultService, id: &str, field: &DeliverField) -> Result<Zeroizing<String>> {
    let view = svc.cipher(id).await?;
    if view.reprompt == CipherRepromptType::Password {
        return Err(err(
            ErrorCode::Denied,
            "This item asks for your master password — open it in Vault.",
        ));
    }
    if view.deleted_date.is_some() {
        return Err(err(ErrorCode::Invalid, "This item is in the trash."));
    }
    let missing = || err(ErrorCode::NotFound, "This item has no such value.");
    let login = view.login.as_ref();
    let value = match field {
        DeliverField::Username => match view.r#type {
            CipherType::Login => login.and_then(|l| l.username.clone()),
            CipherType::Identity => view.identity.as_ref().and_then(|i| i.username.clone()),
            _ => None,
        },
        DeliverField::Password => login.and_then(|l| l.password.clone()),
        DeliverField::Code => totp_spec(&view).map(|s| s.code_at(unix_now())),
        DeliverField::Field { key } => {
            let password = login.and_then(|l| l.password.clone());
            let record = record_from_view(view, Vec::new()).ok_or_else(not_found)?;
            if key == "password" {
                password
            } else {
                record.field(key).map(str::to_string)
            }
        }
    };
    value
        .filter(|v| !v.is_empty())
        .map(Zeroizing::new)
        .ok_or_else(missing)
}

fn totp_spec(view: &CipherView) -> Option<TotpSpec> {
    TotpSpec::parse(view.login.as_ref()?.totp.as_deref()?)
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
