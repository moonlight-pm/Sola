//! Display text for addresses and counts.

/// `(name, Some(addr))` from `Name <addr>`; a bare address is the name.
pub fn split_address(from: &str) -> (String, Option<String>) {
    let t = from.trim();
    if let Some(start) = t.find('<') {
        let name = t[..start].trim().trim_matches('"');
        let addr = t[start + 1..].trim().trim_end_matches('>').trim();
        if !name.is_empty() {
            return (name.to_string(), Some(addr.to_string()));
        }
        if !addr.is_empty() {
            return (addr.to_string(), None);
        }
    }
    (t.to_string(), None)
}

/// List / attribution name: the display name, else the address's local part.
pub fn short_name(from: &str) -> String {
    let t = from.trim();
    if let Some(start) = t.find('<') {
        let name = t[..start].trim().trim_matches('"');
        if !name.is_empty() {
            return name.to_string();
        }
        if let Some(end) = t.rfind('>') {
            return t[start + 1..end].trim().to_string();
        }
    }
    match t.split_once('@') {
        Some((local, _)) if !local.is_empty() => local.to_string(),
        _ => t.to_string(),
    }
}

/// `Ana`, or `Ana +2` for several.
pub fn recipients_short(to: &[String]) -> String {
    match to {
        [] => "No recipients".into(),
        [one] => short_name(one),
        [first, rest @ ..] => format!("{} +{}", short_name(first), rest.len()),
    }
}

/// `12,345`.
pub fn format_count(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Short tag for "which inbox got this": the address's domain label
/// (`gmail`), or its local part when two `peers` share that domain.
pub fn account_tag(addr: &str, peers: &[String]) -> String {
    let addr = sola_bus::topics::mail_addr_key(addr);
    let Some((local, domain)) = addr.split_once('@') else {
        return addr;
    };
    let label = domain.split('.').next().unwrap_or(domain);
    let shared = peers.iter().any(|p| {
        let p = sola_bus::topics::mail_addr_key(p);
        p != addr
            && p.split_once('@')
                .is_some_and(|(_, d)| d.split('.').next() == Some(label))
    });
    if shared {
        local.to_string()
    } else {
        label.to_string()
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(short_name("Ana Lopez <ana@x.com>"), "Ana Lopez");
        assert_eq!(short_name("\"Bob\" <bob@x.com>"), "Bob");
        assert_eq!(short_name("<c@x.com>"), "c@x.com");
        assert_eq!(short_name("dee@x.com"), "dee");
        assert_eq!(
            split_address("Ana <ana@x.com>"),
            ("Ana".into(), Some("ana@x.com".into()))
        );
        assert_eq!(split_address("ana@x.com"), ("ana@x.com".into(), None));
    }

    #[test]
    fn recipients() {
        assert_eq!(recipients_short(&[]), "No recipients");
        assert_eq!(
            recipients_short(&["Ana <a@x>".into(), "b@x".into(), "c@x".into()]),
            "Ana +2"
        );
    }

    #[test]
    fn counts_group_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(12345), "12,345");
        assert_eq!(format_count(1234567), "1,234,567");
    }

    #[test]
    fn tags_disambiguate_shared_domains() {
        let peers = vec!["me@gmail.com".to_string(), "work@moonlight.pm".to_string()];
        assert_eq!(account_tag("me@gmail.com", &peers), "gmail");
        assert_eq!(account_tag("work@moonlight.pm", &peers), "moonlight");
        let twins = vec!["a@gmail.com".to_string(), "b@gmail.com".to_string()];
        assert_eq!(account_tag("b@gmail.com", &twins), "b");
    }
}
