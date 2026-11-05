//! Sola's Google OAuth client, handed to the compiler for the programs that
//! carry it (`sola-google-client`). Not in git: it is in the environment or in
//! `~/.config/oath/sola-google-client.env`, beside `origin.env`.

use std::path::PathBuf;

const ID: &str = "SOLA_GOOGLE_CALENDAR_CLIENT_ID";
const SECRET: &str = "SOLA_GOOGLE_CALENDAR_CLIENT_SECRET";

pub type Vars = Vec<(&'static str, String)>;

/// Both variables, or none when this machine has no client.
pub fn vars() -> Result<Vars, String> {
    let path = file();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let env = |key| std::env::var(key).ok();
    resolve(env(ID), env(SECRET), &text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn vars_or_exit() -> Vars {
    vars().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    })
}

pub fn file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("oath/sola-google-client.env")
}

fn resolve(id: Option<String>, secret: Option<String>, text: &str) -> Result<Vars, String> {
    let pick = |from_env: Option<String>, key: &str| {
        from_env.filter(|v| !v.is_empty()).or_else(|| in_file(text, key))
    };
    match (pick(id, ID), pick(secret, SECRET)) {
        (None, None) => Ok(Vec::new()),
        (Some(id), Some(secret)) => {
            for (key, value) in [(ID, &id), (SECRET, &secret)] {
                // A plan exports these in a shell script. Names the value, never shows it.
                if value.is_empty() || !value.chars().all(safe) {
                    return Err(format!("{key} has a character it should not"));
                }
            }
            Ok(vec![(ID, id), (SECRET, secret)])
        }
        (Some(_), None) => Err(format!("{ID} without {SECRET}")),
        (None, Some(_)) => Err(format!("{SECRET} without {ID}")),
    }
}

fn in_file(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# the client\nSOLA_GOOGLE_CALENDAR_CLIENT_ID=abc.apps\nSOLA_GOOGLE_CALENDAR_CLIENT_SECRET=GOCSPX-x_y\n";

    #[test]
    fn no_file_and_no_environment_is_no_client() {
        assert!(resolve(None, None, "").unwrap().is_empty());
    }

    #[test]
    fn reads_the_file() {
        let vars = resolve(None, None, FILE).unwrap();
        assert_eq!(vars[0], (ID, "abc.apps".to_string()));
        assert_eq!(vars[1], (SECRET, "GOCSPX-x_y".to_string()));
    }

    #[test]
    fn the_environment_wins_and_empty_does_not_count() {
        let vars = resolve(Some("mine".into()), Some(String::new()), FILE).unwrap();
        assert_eq!(vars[0].1, "mine");
        assert_eq!(vars[1].1, "GOCSPX-x_y");
    }

    #[test]
    fn half_a_client_is_an_error() {
        assert!(resolve(Some("mine".into()), None, "").is_err());
        assert!(resolve(None, Some("mine".into()), "").is_err());
    }

    #[test]
    fn a_value_unsafe_in_a_script_is_refused_without_being_shown() {
        let e = resolve(Some("a b".into()), Some("$(rm)".into()), "").unwrap_err();
        assert!(!e.contains("a b") && !e.contains("$(rm)"), "{e}");
    }
}
