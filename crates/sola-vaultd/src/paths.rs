//! Where the daemon keeps its files, and where the browser kept the
//! vault before it (read once, for the migration).

use std::path::PathBuf;

/// `$XDG_DATA_HOME/sola/vault` (`~/.local/share/sola/vault`): prefs,
/// settings and the locked session. Nothing in it is secret in plain
/// text.
pub fn state_dir() -> PathBuf {
    data_home().join("sola/vault")
}

/// The browser's age-encrypted session (it held the decrypted user key).
pub fn browser_session() -> PathBuf {
    data_home().join("sola/browser/vault-session.json")
}

/// The browser's vault prefs (`~/.config/sola/browser/vault.json`): its
/// device id, last-used clock, remembered email.
pub fn browser_prefs() -> PathBuf {
    sola_core::config::sola_config_dir().join("browser/vault.json")
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"))
}

/// Make `dir` (mode `0700`).
pub fn ensure_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Write `body` to `path` atomically, mode `0600`.
pub fn write_private(path: &std::path::Path, body: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(body)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}
