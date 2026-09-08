//! JSON persistence for settings + the event store.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{Settings, Store};
use crate::paths::AppDirs;

pub fn load_store(dirs: &AppDirs) -> Store {
    let mut store: Store = read_json(dirs.store_file()).unwrap_or_default();
    store.ensure_local();
    store
}

pub fn save_store(dirs: &AppDirs, store: &Store) -> Result<()> {
    dirs.ensure().context("calendar dirs")?;
    write_json(&dirs.store_file(), store)
}

pub fn load_settings(dirs: &AppDirs) -> Settings {
    read_json(dirs.settings_file()).unwrap_or_default()
}

pub fn save_settings(dirs: &AppDirs, settings: &Settings) -> Result<()> {
    dirs.ensure().context("calendar dirs")?;
    write_json(&dirs.settings_file(), settings)
}

fn read_json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Option<T> {
    let bytes = fs::read(path.as_ref()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}
