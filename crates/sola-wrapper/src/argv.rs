//! `sola-wrapper <id>` chrome; `sola-wrapper --engine --profile=<id>` helper.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Args {
    Chrome { id: String },
    Engine { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse argv **after** `argv[0]`. Unknown flags fail (no `--url` in v1).
pub fn parse<I, S>(args: I) -> Result<Args, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut engine = false;
    let mut profile: Option<String> = None;
    let mut id: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(raw) = iter.next() {
        let a = raw.as_ref();
        if a == "--engine" {
            engine = true;
            continue;
        }
        if let Some(p) = a.strip_prefix("--profile=") {
            profile = Some(p.to_string());
            continue;
        }
        if a == "--profile" {
            let Some(p) = iter.next() else {
                return Err(ParseError("--profile requires an id".into()));
            };
            profile = Some(p.as_ref().to_string());
            continue;
        }
        if a.starts_with('-') {
            return Err(ParseError(format!(
                "unknown flag {a} (v1 identity is `sola-wrapper <id>`)"
            )));
        }
        if id.is_some() {
            return Err(ParseError("expected a single id".into()));
        }
        id = Some(a.to_string());
    }
    if engine {
        let id = profile.or(id).ok_or_else(|| {
            ParseError("sola-wrapper --engine requires --profile=<id>".into())
        })?;
        validate_id(&id)?;
        return Ok(Args::Engine { id });
    }
    let id = id.ok_or_else(|| ParseError("usage: sola-wrapper <id>".into()))?;
    validate_id(&id)?;
    Ok(Args::Chrome { id })
}

pub fn validate_id(id: &str) -> Result<(), ParseError> {
    if id.is_empty() {
        return Err(ParseError("empty id".into()));
    }
    if id.contains('/') || id.contains('\\') || id.contains('\0') || id.contains("..") {
        return Err(ParseError(
            "id must not contain path separators or '..'".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_id() {
        match parse(["slack"]).unwrap() {
            Args::Chrome { id } => assert_eq!(id, "slack"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn engine_profile_eq() {
        match parse(["--engine", "--profile=slack"]).unwrap() {
            Args::Engine { id } => assert_eq!(id, "slack"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn engine_profile_space() {
        match parse(["--engine", "--profile", "discord"]).unwrap() {
            Args::Engine { id } => assert_eq!(id, "discord"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_url_flag() {
        let err = parse(["--url", "https://app.slack.com"]).unwrap_err();
        assert!(err.0.contains("unknown flag"), "{err}");
    }

    #[test]
    fn rejects_path_id() {
        assert!(parse(["../evil"]).is_err());
        assert!(parse(["foo/bar"]).is_err());
    }

    #[test]
    fn usage_without_id() {
        let err = parse(Vec::<&str>::new()).unwrap_err();
        assert!(err.0.contains("usage"), "{err}");
    }

    #[test]
    fn extra_positional_rejected() {
        assert!(parse(["slack", "extra"]).is_err());
    }
}
