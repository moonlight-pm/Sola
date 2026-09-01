//! Where sola-spotify keeps tokens, credentials, and caches.
//!
//! Config and credentials live under Sola's XDG dirs so a cache wipe never
//! signs the user out. Tokens are `0600`.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Self {
        let config = sola_config_dir().join("spotify");
        let state = sola_state_dir().join("spotify");
        let cache = sola_cache_dir().join("spotify");
        Self {
            config,
            state,
            cache,
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn shared_web_token_file(&self) -> PathBuf {
        self.state.join("shared_web_api_token.json")
    }

    pub fn credentials_dir(&self) -> PathBuf {
        self.state.join("credentials")
    }

    pub fn volume_dir(&self) -> PathBuf {
        self.state.join("volume")
    }

    pub fn audio_cache_dir(&self) -> PathBuf {
        self.cache.join("audio")
    }

    pub fn art_cache_dir(&self) -> PathBuf {
        self.cache.join("art")
    }

    pub fn page_cache_dir(&self) -> PathBuf {
        self.cache.join("pages")
    }

    pub fn skipped_file(&self) -> PathBuf {
        self.state.join("skipped.json")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            &self.config,
            &self.state,
            &self.cache,
            &self.credentials_dir(),
            &self.volume_dir(),
            &self.audio_cache_dir(),
            &self.art_cache_dir(),
            &self.page_cache_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
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

fn sola_cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join("sola")
}
