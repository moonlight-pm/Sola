//! Where sola-calendar keeps settings, the event store, and tokens.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Self {
        Self {
            config: sola_config_dir().join("calendar"),
            state: sola_state_dir().join("calendar"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn store_file(&self) -> PathBuf {
        self.state.join("store.json")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config)?;
        std::fs::create_dir_all(&self.state)?;
        Ok(())
    }
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .unwrap_or_else(|| PathBuf::from(fallback))
}

fn sola_config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("sola")
}

fn sola_state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("sola")
}
