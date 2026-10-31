//! What chrome reads from a page's WebAuthn options before it asks the
//! vault. The ceremony itself (signing, saving) is the daemon's.

/// The account a `create()` is for, from its `publicKey.user`: the
/// display name, else the user name (for the save sheet's head).
pub fn create_account_hint(public_key_json: &str) -> Option<String> {
    let pk: serde_json::Value = serde_json::from_str(public_key_json).ok()?;
    let user = pk.get("user")?;
    let field = |k: &str| {
        user.get(k)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    field("displayName").or(field("name")).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_account_hint_prefers_display_name() {
        let pk =
            r#"{"user":{"id":"YWJj","name":"ada@ex.com","displayName":"Ada"},"challenge":"YQ"}"#;
        assert_eq!(create_account_hint(pk).as_deref(), Some("Ada"));
        let pk = r#"{"user":{"id":"YWJj","name":"ada@ex.com","displayName":" "}}"#;
        assert_eq!(create_account_hint(pk).as_deref(), Some("ada@ex.com"));
        assert_eq!(create_account_hint("{}"), None);
    }
}
