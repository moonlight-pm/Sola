//! JSON config and state persistence for Sola components.
//!
//! Apps declare a config type, implement [`JsonConfig`] (flat) or
//! [`JsonConfigIn`] (nested under an app directory), and get `load`/`save`
//! for free. Files land under `$XDG_CONFIG_HOME/sola/` (or
//! `$HOME/.config/sola/` as fallback), with atomic writes via temp-file +
//! rename so a crash mid-write can't leave torn JSON.

pub mod mail;

use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use tracing::{info, warn};

/// Trait for app-owned JSON config/state files persisted under the Sola config directory.
///
/// Implement this on your config type, then use the provided default methods:
/// - `Self::load()`
/// - `self.save()`
///
/// # Example
///
/// ```ignore
/// #[derive(serde::Serialize, serde::Deserialize, Default)]
/// struct ShellConfig {
///     zones: std::collections::HashMap<String, sola_bus::topics::Zone>,
/// }
///
/// impl sola_core::config::JsonConfig for ShellConfig {
///     const FILE_NAME: &'static str = "shell.json";
/// }
/// ```
pub trait JsonConfig: Serialize + DeserializeOwned + Default {
    /// File name under `<user_config>/sola/`.
    ///
    /// Example: `"shell.json"`, `"terminal-state.json"`.
    const FILE_NAME: &'static str;

    /// Full path to this config file.
    fn path() -> PathBuf {
        sola_config_file(Self::FILE_NAME)
    }

    /// Load config from disk, logging success/failure and falling back to default.
    fn load() -> Self {
        match Self::try_load_or_default() {
            Ok(cfg) => {
                info!(file = Self::FILE_NAME, "restored config");
                cfg
            }
            Err(e) => {
                warn!(file = Self::FILE_NAME, "failed to load config: {e}");
                Self::default()
            }
        }
    }

    /// Load config from disk, returning an error if missing.
    fn try_load() -> Result<Self, ConfigError> {
        let path = Self::path();
        load_json(&path)
    }

    /// Load config from disk, returning `Ok(default)` when the file does not exist.
    fn try_load_or_default() -> Result<Self, ConfigError> {
        let path = Self::path();
        load_json_or_default(&path)
    }

    /// Save pretty-printed JSON to disk, logging any failure.
    ///
    /// Creates parent directories as needed.
    fn save(&self) {
        if let Err(e) = self.try_save_pretty() {
            warn!(file = Self::FILE_NAME, "failed to write config: {e}");
        }
    }

    /// Save pretty-printed JSON to disk.
    ///
    /// Creates parent directories as needed.
    fn try_save_pretty(&self) -> Result<(), ConfigError> {
        let path = Self::path();
        save_json_pretty(&path, self)
    }

    /// Save compact JSON to disk.
    ///
    /// Creates parent directories as needed.
    fn try_save(&self) -> Result<(), ConfigError> {
        let path = Self::path();
        save_json(&path, self)
    }
}

/// Like [`JsonConfig`] but places the file inside an app-owned sub-directory:
/// `<user_config>/sola/<APP_DIR>/<FILE_NAME>`.
///
/// Use this when an app owns multiple config files and wants to namespace them
/// under its own directory (e.g. `shell/applications.json`).
pub trait JsonConfigIn: Serialize + DeserializeOwned + Default {
    /// Sub-directory under `<user_config>/sola/`.
    const APP_DIR: &'static str;

    /// File name inside the sub-directory.
    const FILE_NAME: &'static str;

    fn path() -> PathBuf {
        sola_config_dir().join(Self::APP_DIR).join(Self::FILE_NAME)
    }

    fn load() -> Self {
        match Self::try_load_or_default() {
            Ok(cfg) => {
                info!(
                    dir = Self::APP_DIR,
                    file = Self::FILE_NAME,
                    "restored config"
                );
                cfg
            }
            Err(e) => {
                warn!(
                    dir = Self::APP_DIR,
                    file = Self::FILE_NAME,
                    "failed to load config: {e}"
                );
                Self::default()
            }
        }
    }

    fn try_load() -> Result<Self, ConfigError> {
        load_json(&Self::path())
    }

    fn try_load_or_default() -> Result<Self, ConfigError> {
        load_json_or_default(&Self::path())
    }

    fn save(&self) {
        if let Err(e) = self.try_save_pretty() {
            warn!(
                dir = Self::APP_DIR,
                file = Self::FILE_NAME,
                "failed to write config: {e}"
            );
        }
    }

    fn try_save_pretty(&self) -> Result<(), ConfigError> {
        save_json_pretty(&Self::path(), self)
    }

    fn try_save(&self) -> Result<(), ConfigError> {
        save_json(&Self::path(), self)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    JsonDeserialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    JsonSerialize(serde_json::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "I/O error at {}: {}", path.display(), source)
            }
            Self::JsonDeserialize { path, source } => {
                write!(f, "failed to parse JSON at {}: {}", path.display(), source)
            }
            Self::JsonSerialize(source) => {
                write!(f, "failed to serialize JSON: {}", source)
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::JsonDeserialize { source, .. } => Some(source),
            Self::JsonSerialize(source) => Some(source),
        }
    }
}

/// Load JSON from `path`.
pub fn load_json<T>(path: &Path) -> Result<T, ConfigError>
where
    T: DeserializeOwned,
{
    let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_str::<T>(&raw).map_err(|source| ConfigError::JsonDeserialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Load JSON from `path`, returning `T::default()` when file does not exist.
pub fn load_json_or_default<T>(path: &Path) -> Result<T, ConfigError>
where
    T: DeserializeOwned + Default,
{
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<T>(&raw).map_err(|source| ConfigError::JsonDeserialize {
            path: path.to_path_buf(),
            source,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Save value as pretty-printed JSON to `path`.
pub fn save_json_pretty<T>(path: &Path, value: &T) -> Result<(), ConfigError>
where
    T: Serialize,
{
    ensure_parent_dir(path)?;
    let content = serde_json::to_string_pretty(value).map_err(ConfigError::JsonSerialize)?;
    atomic_write(path, content.as_bytes())
}

/// Save value as compact JSON to `path`.
pub fn save_json<T>(path: &Path, value: &T) -> Result<(), ConfigError>
where
    T: Serialize,
{
    ensure_parent_dir(path)?;
    let content = serde_json::to_string(value).map_err(ConfigError::JsonSerialize)?;
    atomic_write(path, content.as_bytes())
}

/// Write `content` to `path` via a temp file + rename so a crash between
/// truncate and rewrite can't leave the destination torn. The temp file
/// lives alongside the target (same directory, same filesystem) so
/// `rename` is atomic.
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), ConfigError> {
    let tmp = path.with_extension({
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.is_empty() {
            "tmp".to_string()
        } else {
            format!("{ext}.tmp")
        }
    });
    std::fs::write(&tmp, content).map_err(|source| ConfigError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn ensure_parent_dir(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

/// XDG-style user config root: `$XDG_CONFIG_HOME` if it's an absolute path,
/// else `$HOME/.config`, else `.config` as a last-ditch relative path.
fn user_config_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return p;
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config");
    }
    PathBuf::from(".config")
}

/// Returns the Sola config directory, creating it if needed.
///
/// Resolves to `<user_config>/sola` where `<user_config>` is
/// `$XDG_CONFIG_HOME` (if absolute) or `$HOME/.config`.
pub fn sola_config_dir() -> PathBuf {
    let dir = user_config_dir().join("sola");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Returns the full path to a file under the Sola config directory.
///
/// Example:
/// - `sola_config_file("shell.json")` -> `<config>/sola/shell.json`
pub fn sola_config_file(file_name: &str) -> PathBuf {
    sola_config_dir().join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Serialize, serde::Deserialize)]
    struct FlatCfg;
    impl JsonConfig for FlatCfg {
        const FILE_NAME: &'static str = "flat.json";
    }

    #[derive(Default, Serialize, serde::Deserialize)]
    struct NestedCfg;
    impl JsonConfigIn for NestedCfg {
        const APP_DIR: &'static str = "shell";
        const FILE_NAME: &'static str = "applications.json";
    }

    #[test]
    fn flat_path_resolves_under_sola_dir() {
        let p = <FlatCfg as JsonConfig>::path();
        assert!(p.ends_with("sola/flat.json"), "got {}", p.display());
    }

    #[test]
    fn nested_path_resolves_under_app_dir() {
        let p = <NestedCfg as JsonConfigIn>::path();
        assert!(
            p.ends_with("sola/shell/applications.json"),
            "got {}",
            p.display()
        );
    }
}
