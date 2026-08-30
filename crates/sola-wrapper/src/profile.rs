//! Per-wrapper CEF dirs — never `browser_data_root()`.

use std::path::PathBuf;

use sola_browser::profiles::ActiveProfile;

/// Durable CEF user-data (`cookies` / localStorage) for this wrapper id.
pub fn data_dir(id: &str) -> PathBuf {
    sola_core::config::sola_config_dir()
        .join("wrapper")
        .join(id)
}

/// Discardable cache, namespaced by id under the Sola cache root.
pub fn cache_dir(id: &str) -> PathBuf {
    xdg_cache_home().join("sola").join("wrapper").join(id)
}

/// Bind this process so CefEngine helpers use wrapper paths, not browser profiles.
pub fn bind(id: &str) -> Result<ActiveProfile, String> {
    sola_browser::profiles::bind_external(id, data_dir(id), cache_dir(id))
}

fn xdg_cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_under_sola_config() {
        let p = data_dir("slack");
        let s = p.to_string_lossy();
        assert!(s.contains("sola/wrapper/slack"), "{s}");
        assert!(!s.contains("sola/browser"), "{s}");
    }

    #[test]
    fn cache_dir_is_namespaced() {
        let p = cache_dir("discord");
        let s = p.to_string_lossy();
        assert!(s.contains("sola/wrapper/discord"), "{s}");
        assert!(!s.contains("sola/browser"), "{s}");
    }

    #[test]
    fn notify_permissions_live_in_wrapper_data_dir() {
        let root = std::env::temp_dir().join(format!(
            "sola-wrapper-notify-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let data = root.join("data");
        let cache = root.join("cache");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        sola_browser::profiles::bind_external("slack-notify", data.clone(), cache).unwrap();
        let path = sola_browser::notify::permissions_path("slack-notify");
        assert_eq!(path, data.join("notifications.json"));
        let media = sola_browser::media::permissions_path("slack-notify");
        assert_eq!(media, data.join("media.json"));
        let _ = std::fs::remove_dir_all(root);
    }
}
