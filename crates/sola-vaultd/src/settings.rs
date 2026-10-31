//! The daemon's settings (`settings.json` in the state dir), edited in
//! Vault → Settings: when the vault locks itself, the clipboard time,
//! whether a fill copies the code.

use std::path::Path;

use sola_vault_proto::Settings;
use tracing::warn;

const FILE_NAME: &str = "settings.json";

pub fn load(dir: &Path) -> Settings {
    let path = dir.join(FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            warn!(path = %path.display(), error = %e, "vault: settings unreadable; using the defaults");
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

pub fn save(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let body = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    crate::paths::write_private(&dir.join(FILE_NAME), &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        let s = Settings {
            idle_lock_minutes: None,
            lock_on_screen_lock: false,
            clipboard_clear_secs: 0,
            copy_code_on_fill: false,
        };
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()), s);
    }
}
