//! Browser profiles (D8) — registry, paths, first-run wipe, switch/manage.
//!
//! Freeze: `docs/specs/2026-08-10-sola-browser-profiles-design.md`.
//! One active profile in the process; switching updates the registry and
//! in-memory active handle so chrome can replace tabs + CEF request context
//! without tearing down the window.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

const REGISTRY_VERSION: u32 = 1;
const DEFAULT_NAME: &str = "Primary";

static ACTIVE: OnceLock<RwLock<ActiveProfile>> = OnceLock::new();
static REGISTRY: OnceLock<RwLock<Option<ProfilesRegistry>>> = OnceLock::new();

fn registry_cache() -> &'static RwLock<Option<ProfilesRegistry>> {
    REGISTRY.get_or_init(|| RwLock::new(None))
}

/// Resolved active profile for this process.
#[derive(Debug, Clone)]
pub struct ActiveProfile {
    pub id: String,
    pub name: String,
    /// Per-profile durable web data (cookies/storage for the engine, `session.json`).
    pub data_dir: PathBuf,
    /// Per-profile discardable cache.
    pub cache_dir: PathBuf,
}

impl ActiveProfile {
    pub fn session_path(&self) -> PathBuf {
        self.data_dir.join("session.json")
    }

    /// CEF request-context cache path (cookies/storage) for this profile.
    /// Must stay under [`browser_data_root`] so it is a child of the process
    /// `root_cache_path`.
    pub fn cef_user_data_dir(&self) -> PathBuf {
        self.data_dir.join("cef")
    }
}

/// One row in the registry (public for menus / manage UI).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfilesRegistry {
    version: u32,
    active: String,
    profiles: Vec<ProfileEntry>,
}

/// Ensure registry + active profile dirs exist, wipe legacy paths once,
/// and install the process-wide active profile.
pub fn ensure_active() -> ActiveProfile {
    let lock = ACTIVE.get_or_init(|| {
        let profile = load_or_create_active();
        wipe_legacy_paths();
        tracing::info!(
            id = %profile.id,
            name = %profile.name,
            data = %profile.data_dir.display(),
            cache = %profile.cache_dir.display(),
            "browser profile active (D8)"
        );
        RwLock::new(profile)
    });
    lock.read().expect("profile lock").clone()
}

/// Write `profiles.json` active id without changing this process's CEF bind.
pub fn set_registry_active(id: &str) -> Result<(), String> {
    let mut reg = load_registry_or_empty();
    if !reg.profiles.iter().any(|p| p.id == id) {
        return Err("profile not found".into());
    }
    if reg.active != id {
        reg.active = id.to_string();
        write_registry(&registry_path(), &reg).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Registry's active profile id (may differ from this process's CEF bind).
pub fn registry_active_id() -> String {
    load_registry_or_empty().active
}

/// Bind this process to `id` without changing `profiles.json`.
/// Used by per-profile CEF engine helpers so they don't steal the registry active.
pub fn bind_process_only(id: &str) -> Result<ActiveProfile, String> {
    let reg = load_registry_or_empty();
    let entry = reg
        .profiles
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| "profile not found".to_string())?;
    ensure_profile_dirs(&entry.id);
    set_process_active(resolve_entry(&entry))
}

/// Active profile (panics if [`ensure_active`] was never called).
pub fn active() -> ActiveProfile {
    ACTIVE
        .get()
        .expect("profiles::ensure_active() must run before profiles::active()")
        .read()
        .expect("profile lock")
        .clone()
}

/// All registered profiles in registry order (re-read from disk).
pub fn list() -> Vec<ProfileEntry> {
    read_registry(&registry_path())
        .map(|r| r.profiles)
        .unwrap_or_default()
}

/// Create a new profile with a friendly `name`, make it active in the
/// registry and in this process. Caller reloads session / CEF context.
pub fn create_and_activate(name: &str) -> Result<ActiveProfile, String> {
    let name = sanitize_name(name)?;
    let mut reg = load_registry_or_empty();
    let id = new_profile_id();
    let entry = ProfileEntry {
        id: id.clone(),
        name,
    };
    reg.profiles.push(entry.clone());
    reg.active = id;
    ensure_profile_dirs(&entry.id);
    write_registry(&registry_path(), &reg).map_err(|e| e.to_string())?;
    tracing::info!(id = %entry.id, name = %entry.name, "created and activated profile");
    set_process_active(resolve_entry(&entry))
}

/// Rename a profile by id.
pub fn rename(id: &str, name: &str) -> Result<(), String> {
    let name = sanitize_name(name)?;
    let mut reg = load_registry_or_empty();
    {
        let Some(entry) = reg.profiles.iter_mut().find(|p| p.id == id) else {
            return Err("profile not found".into());
        };
        entry.name = name.clone();
    }
    write_registry(&registry_path(), &reg).map_err(|e| e.to_string())?;
    // Keep in-memory name in sync if this is the active profile.
    if let Some(lock) = ACTIVE.get() {
        let mut g = lock.write().expect("profile lock");
        if g.id == id {
            g.name = name.clone();
        }
    }
    tracing::info!(id, name = %name, "renamed profile");
    Ok(())
}

/// Set the active profile in the registry and this process.
pub fn activate(id: &str) -> Result<ActiveProfile, String> {
    let mut reg = load_registry_or_empty();
    let entry = reg
        .profiles
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| "profile not found".to_string())?;
    if reg.active != id {
        reg.active = id.to_string();
        write_registry(&registry_path(), &reg).map_err(|e| e.to_string())?;
    }
    ensure_profile_dirs(&entry.id);
    let profile = resolve_entry(&entry);
    set_process_active(profile)
}

/// Delete a profile. Removes registry entry and data/cache dirs.
///
/// - Cannot delete the last remaining profile.
/// - If deleting the active profile, activates another and returns
///   `Some(new_active)` so the caller can reload the workspace.
/// - If deleting a non-active profile, returns `None`.
pub fn delete(id: &str) -> Result<Option<ActiveProfile>, String> {
    let mut reg = load_registry_or_empty();
    if reg.profiles.len() <= 1 {
        return Err("cannot delete the only profile".into());
    }
    if !reg.profiles.iter().any(|p| p.id == id) {
        return Err("profile not found".into());
    }

    let was_active = reg.active == id;
    reg.profiles.retain(|p| p.id != id);
    if was_active {
        reg.active = reg.profiles[0].id.clone();
    }
    write_registry(&registry_path(), &reg).map_err(|e| e.to_string())?;

    remove_path(&profile_data_dir(id));
    remove_path(&profile_cache_dir(id));
    tracing::info!(id, was_active, "deleted profile");

    if was_active {
        let entry = reg.profiles[0].clone();
        ensure_profile_dirs(&entry.id);
        Ok(Some(set_process_active(resolve_entry(&entry))?))
    } else {
        Ok(None)
    }
}

fn set_process_active(profile: ActiveProfile) -> Result<ActiveProfile, String> {
    let lock = ACTIVE.get_or_init(|| RwLock::new(profile.clone()));
    {
        let mut g = lock
            .write()
            .map_err(|_| "profile lock poisoned".to_string())?;
        *g = profile.clone();
    }
    tracing::info!(
        id = %profile.id,
        name = %profile.name,
        data = %profile.data_dir.display(),
        "browser profile activated"
    );
    Ok(profile)
}

fn resolve_entry(entry: &ProfileEntry) -> ActiveProfile {
    let data_dir = profile_data_dir(&entry.id);
    let cache_dir = profile_cache_dir(&entry.id);
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::create_dir_all(data_dir.join("cef"));
    ActiveProfile {
        id: entry.id.clone(),
        name: entry.name.clone(),
        data_dir,
        cache_dir,
    }
}

fn sanitize_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    if name.len() > 64 {
        return Err("name too long (max 64)".into());
    }
    // Avoid control chars / path tricks in the label only (id is UUID).
    if name
        .chars()
        .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err("name contains invalid characters".into());
    }
    Ok(name.to_string())
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
        ensure_profile_dirs(&p.id);
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

    resolve_entry(&entry)
}

fn load_registry_or_empty() -> ProfilesRegistry {
    read_registry(&registry_path()).unwrap_or_else(|| ProfilesRegistry {
        version: REGISTRY_VERSION,
        active: String::new(),
        profiles: Vec::new(),
    })
}

fn ensure_profile_dirs(id: &str) {
    let data = profile_data_dir(id);
    let cache = profile_cache_dir(id);
    let _ = std::fs::create_dir_all(&data);
    let _ = std::fs::create_dir_all(&cache);
    let _ = std::fs::create_dir_all(data.join("cef"));
}

fn new_profile_id() -> String {
    // UUID v4 without pulling uuid: 16 random bytes formatted.
    let mut b = [0u8; 16];
    fill_random(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
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
        *b = ((t >> (i * 8)) as u8)
            .wrapping_add(i as u8)
            .wrapping_mul(17);
    }
}

fn read_registry(path: &Path) -> Option<ProfilesRegistry> {
    if let Ok(g) = registry_cache().read() {
        if let Some(r) = g.as_ref() {
            return Some(r.clone());
        }
    }
    let raw = std::fs::read_to_string(path).ok()?;
    let r: ProfilesRegistry = serde_json::from_str(&raw).ok()?;
    if let Ok(mut g) = registry_cache().write() {
        *g = Some(r.clone());
    }
    Some(r)
}

fn write_registry(path: &Path, reg: &ProfilesRegistry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(reg).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    if let Ok(mut g) = registry_cache().write() {
        *g = Some(reg.clone());
    }
    Ok(())
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
    for rel in ["browser-session.json", "browser-vault.json", "browser.yaml"] {
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

/// Browser-wide durable data (downloads index, later history).
pub fn shared_dir() -> PathBuf {
    let dir = browser_data_root().join("shared");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn browser_cache_root() -> PathBuf {
    xdg_cache_home().join("sola/browser")
}

fn registry_path() -> PathBuf {
    browser_data_root().join("profiles.json")
}

pub fn profile_data_dir(id: &str) -> PathBuf {
    browser_data_root().join("profiles").join(id)
}

/// Per-profile `session.json` (does not require this process to be bound).
pub fn session_path_for(id: &str) -> PathBuf {
    profile_data_dir(id).join("session.json")
}

/// Unix socket the chrome process uses to talk to a headless CEF helper.
pub fn engine_sock_path(id: &str) -> PathBuf {
    profile_data_dir(id).join("engine.sock")
}

/// Dedicated pixel-frame socket (kept off the control channel).
pub fn engine_frame_sock_path(id: &str) -> PathBuf {
    profile_data_dir(id).join("engine.frame.sock")
}

/// Pid file for the headless CEF helper of this profile.
pub fn engine_pid_path(id: &str) -> PathBuf {
    profile_data_dir(id).join("engine.pid")
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

    #[test]
    fn sanitize_name_rejects_empty_and_path_chars() {
        assert!(sanitize_name("  ").is_err());
        assert!(sanitize_name("a/b").is_err());
        assert_eq!(sanitize_name("  Work  ").unwrap(), "Work");
    }
}
