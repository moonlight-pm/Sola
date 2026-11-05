//! Sola's Google Cloud Desktop OAuth client, for Calendar and Settings.
//!
//! A desktop program can't keep this secret (Google calls it a public client),
//! so it ships in the program. It is not in git: GitHub's push protection
//! refuses it and Google asks that it not be committed. `cargo make` hands it to
//! the compiler from `~/.config/oath/sola-google-client.env`. A build without
//! that file has no client, and Google sign-in says so.
//!
//! Its own crate so that only the two programs using it change with it.

const BUILT_ID: Option<&str> = option_env!("SOLA_GOOGLE_CALENDAR_CLIENT_ID");
const BUILT_SECRET: Option<&str> = option_env!("SOLA_GOOGLE_CALENDAR_CLIENT_SECRET");

/// `SOLA_GOOGLE_CALENDAR_CLIENT_ID` at run time (your own client), else this build's.
pub fn client_id() -> String {
    env_or("SOLA_GOOGLE_CALENDAR_CLIENT_ID", BUILT_ID)
}

/// `SOLA_GOOGLE_CALENDAR_CLIENT_SECRET` at run time (your own client), else this build's.
pub fn client_secret() -> String {
    env_or("SOLA_GOOGLE_CALENDAR_CLIENT_SECRET", BUILT_SECRET)
}

fn env_or(key: &str, built: Option<&str>) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => built.unwrap_or("").trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_without_a_client_has_none() {
        assert_eq!(env_or("SOLA_TEST_GOOGLE_CLIENT_UNSET", None), "");
    }

    #[test]
    fn the_build_supplies_it_when_the_environment_does_not() {
        assert_eq!(env_or("SOLA_TEST_GOOGLE_CLIENT_UNSET", Some("built")), "built");
    }

    #[test]
    fn the_environment_wins() {
        // PATH is set wherever tests run.
        assert_ne!(env_or("PATH", Some("built")), "built");
    }
}
