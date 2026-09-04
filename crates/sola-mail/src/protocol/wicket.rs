//! Fetch alias from-addresses from the host's auth API (wicket).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use tracing::warn;

/// GET `https://{host}/api/auth/me` with Basic auth.
/// Returns emails array, or empty vec on any failure.
///
/// Compose From is settings-only now; this stays for a later picker of
/// discovered aliases.
#[allow(dead_code)]
pub fn fetch_from_addresses(host: &str, username: &str, password: &str) -> Vec<String> {
    let url = format!("https://{host}/api/auth/me");
    let credentials = STANDARD.encode(format!("{username}:{password}"));
    let mut response = match ureq::get(&url)
        .header("Authorization", &format!("Basic {credentials}"))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            warn!("From-address API request failed: {e}");
            return vec![];
        }
    };

    let text = match response.body_mut().read_to_string() {
        Ok(s) => s,
        Err(e) => {
            warn!("From-address API: failed to read response: {e}");
            return vec![];
        }
    };

    let body: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            warn!("From-address API: failed to parse JSON: {e}");
            return vec![];
        }
    };

    match body["emails"].as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => {
            warn!("From-address API: response missing 'emails' array");
            vec![]
        }
    }
}
