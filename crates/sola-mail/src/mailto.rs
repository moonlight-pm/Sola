//! `mailto:` links (RFC 6068): the addresses before `?`, then `to`, `cc`,
//! `bcc`, `subject` and `body` fields, percent-encoded. `+` is a plus, not a
//! space.

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Mailto {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

pub fn parse(url: &str) -> Option<Mailto> {
    let rest = url.trim();
    let (scheme, rest) = rest.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("mailto") {
        return None;
    }
    let (addrs, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut m = Mailto {
        to: addresses(addrs),
        ..Mailto::default()
    };
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k.to_ascii_lowercase().as_str() {
            "to" => m.to.extend(addresses(&v)),
            "cc" => m.cc.extend(addresses(&v)),
            "bcc" => m.bcc.extend(addresses(&v)),
            "subject" => m.subject = v,
            "body" => m.body = v.replace("\r\n", "\n"),
            _ => {}
        }
    }
    Some(m)
}

fn addresses(s: &str) -> Vec<String> {
    decode(s)
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(String::from)
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_fields() {
        let m = parse("mailto:a@b.co,c@d.co?cc=e@f.co&subject=Hi%20there&body=Line%201%0D%0ALine+2&bcc=g@h.co").unwrap();
        assert_eq!(m.to, ["a@b.co", "c@d.co"]);
        assert_eq!(m.cc, ["e@f.co"]);
        assert_eq!(m.bcc, ["g@h.co"]);
        assert_eq!(m.subject, "Hi there");
        assert_eq!(m.body, "Line 1\nLine+2");
    }

    #[test]
    fn only_mailto_and_bare_forms() {
        assert_eq!(parse("MAILTO:x%40y.co").unwrap().to, ["x@y.co"]);
        assert_eq!(parse("mailto:?to=z@y.co").unwrap().to, ["z@y.co"]);
        assert!(parse("https://x").is_none());
    }
}
