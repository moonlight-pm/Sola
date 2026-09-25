//! `~/Bots/<slug>/` and `~/.config/sola/bots/`.

use std::path::PathBuf;

pub fn bots_root() -> PathBuf {
    dirs_home().join("Bots")
}

pub fn bot_home(slug: &str) -> PathBuf {
    bots_root().join(slug)
}

pub fn config_dir() -> PathBuf {
    sola_core::config::sola_config_dir().join("bots")
}

pub fn catalog_path() -> PathBuf {
    config_dir().join("catalog.json")
}

pub fn dialog_path(slug: &str) -> PathBuf {
    bot_home(slug).join("dialog.json")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

pub fn grok_bin() -> PathBuf {
    if let Ok(p) = std::env::var("SOLA_GROK") {
        return PathBuf::from(p);
    }
    let home = dirs_home();
    let local = home.join(".local/bin/grok");
    if local.is_file() {
        return local;
    }
    PathBuf::from("grok")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_under_bots() {
        let p = bot_home("suno");
        assert!(p.ends_with("Bots/suno"));
    }
}
