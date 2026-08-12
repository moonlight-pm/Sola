//! Browser profiles (D8) — registry, paths, first-run wipe.
//!
//! Freeze: `docs/specs/2026-08-10-sola-browser-profiles-design.md`.
//! One active profile at runtime; switcher later.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const REGISTRY_VERSION: u32 = 1;
const DEFAULT_NAME: &str = "Primary";

static ACTIVE: OnceLock<ActiveProfile> = OnceLock::new();

/// Resolved active profile for this process.
#[derive(Debug, Clone)]
pub struct ActiveProfile {
    pub id: String,
    pub name: String,
    /// WebKit data_dir — cookies, storage, SW, mediakeys, `session.json`.
    pub data_dir: PathBuf,
    /// WebKit cache_dir (discardable).
    pub cache_dir: PathBuf,
}

impl ActiveProfile {
    pub fn session_path(&self) -> PathBuf {
        self.data_dir.join("session.json")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileEntry {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfilesRegistry {
    version: u32,
    active: String,
    profiles: Vec<ProfileEntry>,
}

/// Ensure registry + active profile dirs exist, wipe legacy paths once,
/// and return the process-wide active profile.
pub fn ensure_active() -> &'static ActiveProfile {
    ACTIVE.get_or_init(|| {
        let profile = load_or_create_active();
        wipe_legacy_paths();
        tracing::info!(
            id = %profile.id,
            name = %profile.name,
            data = %profile.data_dir.display(),
            cache = %profile.cache_dir.display(),
            "browser profile active (D8)"
        );
        profile
    })
}

/// Active profile (panics if [`ensure_active`] was never called).
pub fn active() -> &'static ActiveProfile {
    ACTIVE
        .get()
        .expect("profiles::ensure_active() must run before profiles::active()")
}

fn load_or_create_active() -> ActiveProfile {
    let root = browser_data_root();
    let _ = std::fs::create_dir_all(root.join("profiles"));
    let _ = std::fs::create_dir_all(root.join("shared"));
    let reg_path = registry_path();

    let mut reg = match read_registry(&reg_path) {
        Some(r) if !r.profiles.is_empty() && r.profiles.iter().any(|p| p.id == r.active) => r,
        Some(mut r) if !r.profiles.is_empty() => {
            // Active id missing — fall back to first entry.
            r.active = r.profiles[0].id.clone();
            let _ = write_registry(&reg_path, &r);
            r
        }
        _ => {
            let id = new_profile_id();
            let reg = ProfilesRegistry {
                version: REGISTRY_VERSION,
                active: id.clone(),
                profiles: vec![ProfileEntry {
                    id: id.clone(),
                    name: DEFAULT_NAME.to_string(),
                }],
            };
            if let Err(e) = write_registry(&reg_path, &reg) {
                tracing::error!(error = %e, path = %reg_path.display(), "failed to write profiles.json");
            }
            reg
        }
    };

    // Ensure every registered profile has data + cache dirs (active at least).
    for p in &reg.profiles {
        let data = profile_data_dir(&p.id);
        let cache = profile_cache_dir(&p.id);
        let _ = std::fs::create_dir_all(&data);
        let _ = std::fs::create_dir_all(&cache);
    }

    let entry = reg
        .profiles
        .iter()
        .find(|p| p.id == reg.active)
        .cloned()
        .unwrap_or_else(|| {
            let id = new_profile_id();
            reg.active = id.clone();
            let e = ProfileEntry {
                id,
                name: DEFAULT_NAME.to_string(),
            };
            reg.profiles.push(e.clone());
            let _ = write_registry(&reg_path, &reg);
            e
        });

    let data_dir = profile_data_dir(&entry.id);
    let cache_dir = profile_cache_dir(&entry.id);
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = std::fs::create_dir_all(&cache_dir);

    ActiveProfile {
        id: entry.id,
        name: entry.name,
        data_dir,
        cache_dir,
    }
}

fn new_profile_id() -> String {
    // UUID v4 without pulling uuid: 16 random bytes formatted.
    let mut b = [0u8; 16];
    fill_random(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn fill_random(buf: &mut [u8]) {
    // Prefer getrandom via /dev/urandom — no extra dep.
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if f.read_exact(buf).is_ok() {
            return;
        }
    }
    // Last resort: time-based (not crypto; dogfood first-run only).
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((t >> (i * 8)) as u8).wrapping_add(i as u8).wrapping_mul(17);
    }
}

fn read_registry(path: &Path) -> Option<ProfilesRegistry> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_registry(path: &Path, reg: &ProfilesRegistry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(reg).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// Remove pre-D8 flat WebKit trees and dead config files (no migration).
fn wipe_legacy_paths() {
    let data_root = browser_data_root();
    // Flat WebKit leftovers at share root (not under profiles/).
    for name in ["cookies.db", "storage", "serviceworkers", "mediakeys"] {
        let p = data_root.join(name);
        remove_path(&p);
    }

    // Old cache layout: everything under cache root except `profiles/`.
    let cache_root = browser_cache_root();
    if cache_root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&cache_root) {
            for e in entries.flatten() {
                if e.file_name() == "profiles" {
                    continue;
                }
                remove_path(&e.path());
            }
        }
    }

    // Dead / relocated config.
    for rel in [
        "browser-session.json",
        "browser-vault.json",
        "browser.yaml",
    ] {
        remove_path(&sola_config_dir().join(rel));
    }
    // Legacy port-era browser/ subtree (tabs yaml, history yaml).
    // Keep `browser/` itself for vault.json; only wipe known dead children.
    let browser_cfg = sola_config_dir().join("browser");
    remove_path(&browser_cfg.join("history.yaml"));
    let tabs = browser_cfg.join("tabs");
    if tabs.is_dir() {
        remove_path(&tabs);
    }
}

fn remove_path(p: &Path) {
    if !p.exists() {
        return;
    }
    let res = if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    };
    match res {
        Ok(()) => tracing::info!(path = %p.display(), "removed legacy browser path (D8)"),
        Err(e) => tracing::warn!(path = %p.display(), error = %e, "failed to remove legacy path"),
    }
}

pub fn browser_data_root() -> PathBuf {
    xdg_data_home().join("sola/browser")
}

pub fn browser_cache_root() -> PathBuf {
    xdg_cache_home().join("sola/browser")
}

fn registry_path() -> PathBuf {
    browser_data_root().join("profiles.json")
}

fn profile_data_dir(id: &str) -> PathBuf {
    browser_data_root().join("profiles").join(id)
}

fn profile_cache_dir(id: &str) -> PathBuf {
    browser_cache_root().join("profiles").join(id)
}

fn xdg_data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"))
}

fn xdg_cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
}

fn sola_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("sola")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_profile_id_looks_like_uuid() {
        let id = new_profile_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }
}
