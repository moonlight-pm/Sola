//! Load From identities from a Wicket-style `GET /api/auth/me`.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sola_bus::topics::{is_catchall_addr, mail_addr_key};

/// Addresses the operator can check, minus catch-alls and the account
/// email (that one is the Email field). Sorted A–Z.
pub fn prepare_from_list(emails: &[String], account_email: &str) -> Vec<String> {
    let self_key = mail_addr_key(account_email);
    let mut out: Vec<String> = Vec::new();
    for raw in emails {
        let t = raw.trim();
        if t.is_empty() || is_catchall_addr(t) {
            continue;
        }
        let key = mail_addr_key(t);
        if key.is_empty() || (!self_key.is_empty() && key == self_key) {
            continue;
        }
        if out.iter().any(|e| mail_addr_key(e) == key) {
            continue;
        }
        out.push(t.to_string());
    }
    out.sort_by(|a, b| mail_addr_key(a).cmp(&mail_addr_key(b)));
    out
}

/// `GET https://{host}/api/auth/me` Basic auth → `emails`.
pub fn fetch(host: &str, username: &str, password: &str) -> Result<Vec<String>, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("IMAP host is empty".into());
    }
    let url = format!("https://{host}/api/auth/me");
    let credentials = STANDARD.encode(format!("{username}:{password}"));
    let mut response = ureq::get(&url)
        .header("Authorization", &format!("Basic {credentials}"))
        .call()
        .map_err(|e| format!("Couldn't load addresses from {host}: {e}"))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Couldn't read addresses from {host}: {e}"))?;
    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Address list from {host} wasn't JSON: {e}"))?;
    let Some(arr) = body["emails"].as_array() else {
        return Err(format!("Address list from {host} had no emails"));
    };
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_catchall_self_and_sorts() {
        let got = prepare_from_list(
            &[
                "Ops@niarada.co".into(),
                "*@moonlight.pm".into(),
                "josh@niarada.co".into(),
                "hello@niarada.co".into(),
                "ops@niarada.co".into(),
            ],
            "Josh@niarada.co",
        );
        assert_eq!(got, vec!["hello@niarada.co", "Ops@niarada.co"]);
    }
}
