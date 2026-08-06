//! Unix username validation for the installer field.

/// Reserved names we never create as the primary seat user.
const RESERVED: &[&str] = &[
    "root", "daemon", "bin", "sys", "sync", "games", "man", "mail", "news",
    "uucp", "proxy", "www-data", "backup", "list", "irc", "gnats", "nobody",
    "systemd-network", "systemd-resolve", "messagebus", "sshd", "nixbld",
    "sola-install",
];

/// Validate a candidate username. Returns `None` if ok, else an error string.
pub fn validate(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Enter a username");
    }
    if name.len() > 32 {
        return Some("32 characters max");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Some("Enter a username");
    };
    if !(first.is_ascii_lowercase() || first == '_') {
        return Some("Start with a–z or _");
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            return Some("Use a–z, 0–9, _, - only");
        }
    }
    if RESERVED.contains(&name) {
        return Some("That name is reserved");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple() {
        assert!(validate("joshua").is_none());
        assert!(validate("sola").is_none()); // installer default prefill
        assert!(validate("a").is_none());
        assert!(validate("user_1").is_none());
    }

    #[test]
    fn rejects_bad() {
        assert!(validate("").is_some());
        assert!(validate("Root").is_some());
        assert!(validate("root").is_some());
        assert!(validate("has space").is_some());
        assert!(validate("1leading").is_some());
    }
}
