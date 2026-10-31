//! `~/Bots/<slug>/` (a bot is its directory), `~/.config/sola/bots/`, and
//! the daemon's socket.

use std::path::PathBuf;

pub fn bots_root() -> PathBuf {
    dirs_home().join("Bots")
}

pub fn bot_home(slug: &str) -> PathBuf {
    bots_root().join(slug)
}

/// A bot's settings: its name, its AI, its session.
pub fn bot_config(slug: &str) -> PathBuf {
    bot_home(slug).join("bot.json")
}

pub fn config_dir() -> PathBuf {
    sola_core::config::sola_config_dir().join("bots")
}

/// The catalog before bots were their directories; read once to move it.
pub fn legacy_catalog_path() -> PathBuf {
    config_dir().join("catalog.json")
}

pub fn dialog_path(slug: &str) -> PathBuf {
    bot_home(slug).join("dialog.json")
}

/// The daemon's HTTP socket, beside `sola-call`'s.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("SOLA_BOTS_PATH") {
        return PathBuf::from(p);
    }
    sola_core::env::runtime_dir().join("sola-bots")
}

/// Gab AI's API key, when `GAB_API_KEY` is not set.
pub fn gab_key_path() -> PathBuf {
    config_dir().join("gab.key")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

pub fn grok_bin() -> PathBuf {
    cli_bin("SOLA_GROK", "grok")
}

pub fn claude_bin() -> PathBuf {
    cli_bin("SOLA_CLAUDE", "claude")
}

/// `$env` if set, else `~/.local/bin/<name>`, else `<name>` on `PATH`.
fn cli_bin(env: &str, name: &str) -> PathBuf {
    if let Ok(p) = std::env::var(env) {
        return PathBuf::from(p);
    }
    let local = dirs_home().join(".local/bin").join(name);
    if local.is_file() {
        return local;
    }
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_under_bots() {
        let p = bot_home("suno");
        assert!(p.ends_with("Bots/suno"));
        assert!(bot_config("suno").ends_with("Bots/suno/bot.json"));
    }
}
