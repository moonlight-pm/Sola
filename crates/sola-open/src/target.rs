//! What is being opened: a link (by its scheme) or a file (by its type,
//! the extension in lower case). `file://` is a file.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A link: its scheme in lower case, and the link as given.
    Link { scheme: String, url: String },
    /// A file: its absolute path, and its type (`None` with no extension).
    File { path: PathBuf, kind: Option<String> },
}

impl Target {
    /// Read `raw` as a link or a file; a relative path is taken from `cwd`.
    pub fn parse(raw: &str, cwd: &Path) -> Option<Target> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some(rest) = raw.strip_prefix("file://") {
            return Some(Self::file(PathBuf::from(percent_decode(rest)), cwd));
        }
        match scheme_of(raw) {
            Some(scheme) => Some(Target::Link {
                scheme,
                url: raw.to_string(),
            }),
            None => Some(Self::file(PathBuf::from(raw), cwd)),
        }
    }

    fn file(path: PathBuf, cwd: &Path) -> Target {
        let path = if path.is_absolute() { path } else { cwd.join(path) };
        let kind = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        Target::File { path, kind }
    }

    /// The scheme or type a handler is chosen by.
    pub fn kind(&self) -> Option<Kind> {
        match self {
            Target::Link { scheme, .. } => Some(Kind::Scheme(scheme.clone())),
            Target::File { kind, .. } => kind.clone().map(Kind::Type),
        }
    }

    /// The target as handed to the handler: a link as given, a file's path.
    pub fn as_arg(&self) -> String {
        match self {
            Target::Link { url, .. } => url.clone(),
            Target::File { path, .. } => path.display().to_string(),
        }
    }
}

/// What a handler is for: a link scheme or a file type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Kind {
    Scheme(String),
    Type(String),
}

impl Kind {
    /// The key in `defaults.json`: `mailto:` for a scheme, `.png` for a type.
    pub fn key(&self) -> String {
        match self {
            Kind::Scheme(s) => format!("{s}:"),
            Kind::Type(t) => format!(".{t}"),
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Scheme(s) => write!(f, "{s}: links"),
            Kind::Type(t) => write!(f, ".{t} files"),
        }
    }
}

/// `mailto:a@b` → `mailto`. A scheme is a letter then letters, digits,
/// `+`, `-` or `.`, before the first `:`; two or more characters, so a
/// Windows drive letter (`C:\`) is not one.
fn scheme_of(raw: &str) -> Option<String> {
    let (head, _) = raw.split_once(':')?;
    let mut chars = head.chars();
    let first = chars.next()?;
    let valid = first.is_ascii_alphabetic()
        && head.len() >= 2
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| head.to_ascii_lowercase())
}

/// `%20` and the like in a `file://` path.
fn percent_decode(s: &str) -> String {
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

    fn parse(s: &str) -> Target {
        Target::parse(s, Path::new("/home/me")).unwrap()
    }

    #[test]
    fn a_link_is_known_by_its_scheme() {
        assert_eq!(
            parse("mailto:a@b.co?subject=hi").kind(),
            Some(Kind::Scheme("mailto".into()))
        );
        assert_eq!(parse("HTTPS://example.com").kind(), Some(Kind::Scheme("https".into())));
        assert_eq!(
            parse("claude-cli://open?q=1").kind(),
            Some(Kind::Scheme("claude-cli".into()))
        );
    }

    #[test]
    fn a_file_is_known_by_its_extension() {
        assert_eq!(
            parse("shot.PNG"),
            Target::File {
                path: "/home/me/shot.PNG".into(),
                kind: Some("png".into())
            }
        );
        assert_eq!(
            parse("file:///tmp/My%20Invite.ics").as_arg(),
            "/tmp/My Invite.ics"
        );
        assert_eq!(parse("/etc/hosts").kind(), None);
        assert_eq!(parse("C:\\x.txt").kind(), Some(Kind::Type("txt".into())));
    }

    #[test]
    fn keys_read_as_written() {
        assert_eq!(Kind::Scheme("mailto".into()).key(), "mailto:");
        assert_eq!(Kind::Type("png".into()).key(), ".png");
    }
}
