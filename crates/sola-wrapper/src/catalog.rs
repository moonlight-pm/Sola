//! Wrapper identity from the Applications catalog (bus persistence file).

use std::path::Path;

use sola_bus::topics::{Application, Topic};
use sola_core::applications::is_wrapper_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    NotFound(String),
    NotWrapper { id: String },
    MissingUrl(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(
                f,
                "no application '{id}' in Settings → Applications\n\
                 add a Web wrapper with that app_id"
            ),
            Self::NotWrapper { id } => write!(
                f,
                "'{id}' is not a web wrapper — set Web wrapper in Settings → Applications"
            ),
            Self::MissingUrl(id) => write!(f, "wrapper '{id}' has no http(s) URL"),
        }
    }
}

impl std::error::Error for LookupError {}

/// Look up `id` in the bus host's `state.yaml` (durable Application stickies).
pub fn lookup(id: &str) -> Result<Application, LookupError> {
    lookup_in(&sola_bus::state::state_path(), id)
}

pub fn lookup_in(state_path: &Path, id: &str) -> Result<Application, LookupError> {
    let mut found = None;
    for msg in sola_bus::state::load(state_path) {
        if let Some(Topic::Application(app)) = Topic::parse(&msg) {
            if app.app_id == id {
                found = Some(app);
            }
        }
    }
    match found {
        None => Err(LookupError::NotFound(id.to_string())),
        Some(app) if !app.is_wrapper() => Err(LookupError::NotWrapper { id: id.to_string() }),
        Some(app) => {
            let ok = app
                .url
                .as_deref()
                .is_some_and(is_wrapper_url);
            if ok {
                Ok(app)
            } else {
                Err(LookupError::MissingUrl(id.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_state(body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sola-wrapper-catalog-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.yaml");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn finds_wrapper() {
        let path = write_state(
            r#"Application:
- app_id: slack
  label: Slack
  command: /opt/sola/bin/sola-wrapper slack
  icon: simpleicons/slack
  kind: wrapper
  url: https://app.slack.com
"#,
        );
        let app = lookup_in(&path, "slack").unwrap();
        assert_eq!(app.label, "Slack");
        assert_eq!(app.url.as_deref(), Some("https://app.slack.com"));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(path.parent().unwrap());
    }

    #[test]
    fn old_command_entry_is_not_a_wrapper() {
        let path = write_state(
            r#"Application:
- app_id: firefox
  label: Firefox
  command: firefox
  icon: simpleicons/firefox
"#,
        );
        match lookup_in(&path, "firefox") {
            Err(LookupError::NotWrapper { id }) => assert_eq!(id, "firefox"),
            other => panic!("{other:?}"),
        }
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(path.parent().unwrap());
    }

    #[test]
    fn missing_id() {
        let path = write_state("Application: []\n");
        match lookup_in(&path, "slack") {
            Err(LookupError::NotFound(id)) => assert_eq!(id, "slack"),
            other => panic!("{other:?}"),
        }
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(path.parent().unwrap());
    }
}
