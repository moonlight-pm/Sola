//! Fill IMAP/SMTP from the email domain. Known providers first
//! (Gmail, Outlook, Fastmail, …) so adding a send identity is
//! address + password, not a host form.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerHint {
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
}

const IMAP: u16 = 993;
const SMTP: u16 = 587;

/// Domain after `@`, lowercased, trimmed. `None` if the address has no
/// `@` or the domain is empty.
pub fn domain(email: &str) -> Option<String> {
    let t = email.trim();
    let at = t.rfind('@')?;
    let d = t[at + 1..].trim();
    if d.is_empty() || !d.contains('.') {
        return None;
    }
    Some(d.to_ascii_lowercase())
}

pub fn hint_for_email(email: &str) -> Option<ServerHint> {
    hint_for_domain(domain(email)?.as_str())
}

pub fn hint_for_domain(domain: &str) -> Option<ServerHint> {
    let d = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    for (domains, hint) in PROVIDERS {
        if domains.iter().any(|x| *x == d) {
            return Some(*hint);
        }
    }
    None
}

pub fn needs_app_password(email: &str) -> bool {
    needs_app_password_email(email)
}

pub fn needs_app_password_email(email: &str) -> bool {
    matches!(
        domain(email).as_deref(),
        Some(
            "gmail.com"
                | "googlemail.com"
                | "google.com"
                | "yahoo.com"
                | "ymail.com"
                | "icloud.com"
        )
    )
}

/// Workspace / Gmail Apps custom domains still use imap.gmail.com.
pub fn uses_gmail_servers(imap_host: &str) -> bool {
    imap_host.trim().eq_ignore_ascii_case("imap.gmail.com")
}

pub const GMAIL: ServerHint = ServerHint {
    imap_host: "imap.gmail.com",
    imap_port: IMAP,
    smtp_host: "smtp.gmail.com",
    smtp_port: SMTP,
};

/// MX target is Google (Workspace / Gmail Apps).
pub fn mx_exchange_is_google(ex: &str) -> bool {
    let h = ex.trim().trim_end_matches('.').to_ascii_lowercase();
    h == "google.com"
        || h == "googlemail.com"
        || h.ends_with(".google.com")
        || h.ends_with(".googlemail.com")
}

pub fn mx_rdata_is_google(rdata: &str) -> bool {
    let ex = rdata.split_whitespace().last().unwrap_or(rdata);
    mx_exchange_is_google(ex)
}

fn domain_dns_safe(d: &str) -> bool {
    let d = d.as_bytes();
    (1..253).contains(&d.len())
        && d.iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'.' || *b == b'-')
}

/// DNS-over-HTTPS MX lookup. `true` if any MX points at Google.
pub fn domain_mx_is_google(domain: &str) -> bool {
    let d = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if !domain_dns_safe(&d) || hint_for_domain(&d).is_some() {
        return false;
    }
    let url = format!("https://cloudflare-dns.com/dns-query?name={d}&type=MX");
    let mut response = match ureq::get(&url)
        .header("Accept", "application/dns-json")
        .call()
    {
        Ok(r) => r,
        Err(_) => return false,
    };
    let Ok(text) = response.body_mut().read_to_string() else {
        return false;
    };
    doh_mx_answers_google(&text)
}

fn doh_mx_answers_google(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    let Some(answers) = v.get("Answer").and_then(|a| a.as_array()) else {
        return false;
    };
    answers.iter().any(|a| {
        a.get("type").and_then(|t| t.as_u64()) == Some(15)
            && mx_rdata_is_google(a.get("data").and_then(|d| d.as_str()).unwrap_or(""))
    })
}

const PROVIDERS: &[(&[&str], ServerHint)] = &[
    (&["gmail.com", "googlemail.com", "google.com"], GMAIL),
    (
        &["outlook.com", "hotmail.com", "live.com", "msn.com"],
        ServerHint {
            imap_host: "outlook.office365.com",
            imap_port: IMAP,
            smtp_host: "smtp.office365.com",
            smtp_port: SMTP,
        },
    ),
    (
        &["fastmail.com", "fastmail.fm"],
        ServerHint {
            imap_host: "imap.fastmail.com",
            imap_port: IMAP,
            smtp_host: "smtp.fastmail.com",
            smtp_port: SMTP,
        },
    ),
    (
        &["yahoo.com", "ymail.com", "rocketmail.com"],
        ServerHint {
            imap_host: "imap.mail.yahoo.com",
            imap_port: IMAP,
            smtp_host: "smtp.mail.yahoo.com",
            smtp_port: SMTP,
        },
    ),
    (
        &["icloud.com", "me.com", "mac.com"],
        ServerHint {
            imap_host: "imap.mail.me.com",
            imap_port: IMAP,
            smtp_host: "smtp.mail.me.com",
            smtp_port: SMTP,
        },
    ),
    (
        &["proton.me", "protonmail.com"],
        ServerHint {
            imap_host: "imap.proton.me",
            imap_port: IMAP,
            smtp_host: "smtp.proton.me",
            smtp_port: SMTP,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_hint() {
        let h = hint_for_email("Me@Gmail.com ").expect("gmail");
        assert_eq!(h.smtp_host, "smtp.gmail.com");
        assert_eq!(h.smtp_port, 587);
        assert_eq!(h.imap_host, "imap.gmail.com");
        assert_eq!(h.imap_port, 993);
    }

    #[test]
    fn googlemail_is_gmail() {
        let h = hint_for_email("a@googlemail.com").expect("googlemail");
        assert_eq!(h.smtp_host, "smtp.gmail.com");
    }

    #[test]
    fn google_com_is_gmail() {
        let h = hint_for_email("a@google.com").expect("google.com");
        assert_eq!(h.imap_host, "imap.gmail.com");
    }

    #[test]
    fn incomplete_domain_is_none() {
        assert!(hint_for_email("me@gmail").is_none());
        assert!(hint_for_email("me@").is_none());
        assert!(hint_for_email("me").is_none());
    }

    #[test]
    fn unknown_domain_is_none() {
        assert!(hint_for_email("josh@wicket.example").is_none());
    }

    #[test]
    fn gmail_needs_app_password() {
        assert!(needs_app_password("a@gmail.com"));
        assert!(needs_app_password("a@google.com"));
        assert!(!needs_app_password("josh@wicket.example"));
        assert!(uses_gmail_servers("imap.gmail.com"));
    }

    #[test]
    fn google_workspace_mx() {
        assert!(mx_rdata_is_google("1 aspmx.l.google.com."));
        assert!(mx_rdata_is_google("15 smtp.google.com."));
        assert!(!mx_rdata_is_google("10 mx.wicket.example."));
        assert!(doh_mx_answers_google(
            r#"{"Answer":[{"type":15,"data":"1 aspmx.l.google.com."}]}"#
        ));
        assert!(!doh_mx_answers_google(
            r#"{"Answer":[{"type":15,"data":"10 mx.example.com."}]}"#
        ));
    }
}
