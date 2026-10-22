//! App config dir. One-shot migrate from the app's earlier names
//! (`workspaces`, before that `agent-terminal`).

use std::path::{Path, PathBuf};
use std::sync::Once;

const DIR: &str = "projects";
/// Newest first: the first that exists moves to [`DIR`].
const LEGACY: &[&str] = &["workspaces", "agent-terminal"];

static MIGRATE: Once = Once::new();

pub fn config_dir() -> PathBuf {
    let root = sola_core::config::sola_config_dir();
    let new = root.join(DIR);
    MIGRATE.call_once(|| {
        if let Some(old) = LEGACY.iter().map(|d| root.join(d)).find(|d| d.exists()) {
            migrate(&old, &new);
        }
    });
    new
}

fn migrate(old: &Path, new: &Path) {
    if new.exists() || !old.exists() {
        return;
    }
    if let Some(parent) = new.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(old, new) {
        Ok(()) => tracing::info!(
            from = %old.display(),
            to = %new.display(),
            "migrated the Projects config dir"
        ),
        Err(e) => tracing::warn!(
            from = %old.display(),
            to = %new.display(),
            "Projects config migrate failed: {e}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn migrate_renames_legacy_dir() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sola-ws-cfg-{n}"));
        let old = root.join(LEGACY[0]);
        let new = root.join(DIR);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("catalog.json"), "{}").unwrap();
        migrate(&old, &new);
        assert!(new.join("catalog.json").is_file());
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migrate_leaves_existing_new_dir() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sola-ws-cfg-keep-{n}"));
        let old = root.join(LEGACY[0]);
        let new = root.join(DIR);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("catalog.json"), "old").unwrap();
        std::fs::write(new.join("catalog.json"), "new").unwrap();
        migrate(&old, &new);
        assert_eq!(
            std::fs::read_to_string(new.join("catalog.json")).unwrap(),
            "new"
        );
        assert!(old.join("catalog.json").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
