//! Browser profiles (D8) — registry, paths, first-run wipe, and the
//! workspaces they belong to.
//!
//! Freeze: `docs/specs/2026-08-10-sola-browser-profiles-design.md`; since
//! 2026-10-26 a profile is a desktop workspace's: one each, paired by the
//! workspace's id (`docs/specs/2026-10-21-next-tab-bar-design.md`). A
//! profile is only its uuid — no name, so a workspace renames freely. There
//! is no picking, making or deleting one by hand.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use sola_bus::topics::Workspace;

const REGISTRY_VERSION: u32 = 1;

static ACTIVE: OnceLock<RwLock<ActiveProfile>> = OnceLock::new();
static REGISTRY: OnceLock<RwLock<Option<ProfilesRegistry>>> = OnceLock::new();
/// Set by [`bind_external`] (wrappers). Isolates `list()` / engine sockets
/// from the browser profile registry and `browser_data_root()`.
static EXTERNAL: AtomicBool = AtomicBool::new(false);

fn registry_cache() -> &'static RwLock<Option<ProfilesRegistry>> {
    REGISTRY.get_or_init(|| RwLock::new(None))
}

/// Resolved active profile for this process.
#[derive(Debug, Clone)]
pub struct ActiveProfile {
    pub id: String,
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
    /// Browser profiles keep this under [`browser_data_root`]; wrapper
    /// binds via [`bind_external`] use `~/.config/sola/wrapper/<id>/cef`.
    pub fn cef_user_data_dir(&self) -> PathBuf {
        self.data_dir.join("cef")
    }
}

/// Bind this process to an externally-owned CEF profile (sola-design).
///
/// Does **not** read or write `profiles.json` / [`browser_data_root`].
/// After this, [`list`] returns only `id`, and engine sockets live under
/// `data_dir` so Slack and the product browser cannot share a CEF lock.
pub fn bind_external(
    id: &str,
    data_dir: PathBuf,
    cache_dir: PathBuf,
) -> Result<ActiveProfile, String> {
    if id.is_empty() {
        return Err("empty profile id".into());
    }
    let cef = data_dir.join("cef");
    std::fs::create_dir_all(&cef).map_err(|e| format!("create {}: {e}", cef.display()))?;
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("create {}: {e}", cache_dir.display()))?;
    EXTERNAL.store(true, Ordering::SeqCst);
    let profile = ActiveProfile {
        id: id.to_string(),
        data_dir,
        cache_dir,
    };
    set_process_active(profile)
}

/// Active profile if this process has already bound one.
pub fn active_if_bound() -> Option<ActiveProfile> {
    Some(ACTIVE.get()?.read().ok()?.clone())
}

/// True after [`bind_external`].
pub fn is_external() -> bool {
    EXTERNAL.load(Ordering::Relaxed)
}

/// One row in the registry: a uuid. (Rows from before 2026-10-26 carry a
/// `name`; it is ignored, and dropped on the next write.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileEntry {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProfilesRegistry {
    version: u32,
    /// The showing workspace's profile: pages open under it before the
    /// shell has said which workspace shows.
    active: String,
    profiles: Vec<ProfileEntry>,
    /// Desktop workspace id → its profile, one each (Next slice 5).
    #[serde(default)]
    workspaces: std::collections::BTreeMap<String, String>,
}

/// Ensure registry + active profile dirs exist, wipe legacy paths once,
/// and install the process-wide active profile.
pub fn ensure_active() -> ActiveProfile {
    let lock = ACTIVE.get_or_init(|| {
        let profile = load_or_create_active();
        wipe_legacy_paths();
        tracing::info!(
            id = %profile.id,
            data = %profile.data_dir.display(),
            cache = %profile.cache_dir.display(),
            "browser profile active (D8)"
        );
        RwLock::new(profile)
    });
    lock.read().expect("profile lock").clone()
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

// ---------------------------------------------------------------------------
// Workspaces (Next slice 5): each desktop workspace has a profile of its
// own, and every page window on it runs that profile.
// ---------------------------------------------------------------------------

/// The desktop workspace showing, as the shell last said.
static SHOWING: RwLock<Option<String>> = RwLock::new(None);

/// `workspace`'s profile; the registry's active one for a workspace not
/// heard of yet.
pub fn profile_for_workspace(workspace: &str) -> String {
    let reg = load_registry_or_empty();
    reg.workspaces
        .get(workspace)
        .filter(|id| reg.profiles.iter().any(|p| &p.id == *id))
        .cloned()
        .unwrap_or(reg.active)
}

/// The profile new pages open under: the showing workspace's, or the
/// registry's active one before the shell has said which shows.
pub fn showing_profile() -> String {
    match SHOWING.read().ok().and_then(|s| s.clone()) {
        Some(ws) => profile_for_workspace(&ws),
        None => registry_active_id(),
    }
}

/// The workspace showing, if the shell has said.
pub fn showing_workspace() -> Option<String> {
    SHOWING.read().ok().and_then(|s| s.clone())
}

/// The shell's workspaces changed: pair each with a profile of its own
/// ([`pair`]). Returns true when the showing workspace changed.
pub fn adopt_workspaces(workspaces: &[Workspace], showing: &str) -> bool {
    let previous = showing_workspace();
    let mut reg = load_registry_or_empty();
    let before = reg.clone();
    let ids: Vec<&str> = workspaces.iter().map(|w| w.id.as_str()).collect();
    pair(&mut reg, &ids, showing, new_profile_id);
    if reg != before {
        for p in reg.profiles.iter().filter(|p| !before.profiles.contains(p)) {
            ensure_profile_dirs(&p.id);
            tracing::info!(id = %p.id, "new workspace profile");
        }
        if let Err(e) = write_registry(&registry_path(), &reg) {
            tracing::warn!(error = %e, "save workspace profiles failed");
        }
    }
    if let Ok(mut s) = SHOWING.write() {
        *s = Some(showing.to_string());
    }
    previous.as_deref() != Some(showing)
}

/// Give every workspace (by id) a profile of its own:
///
/// - a workspace the shell no longer lists (deleted) lets go of its
///   profile; the profile and its data stay, unused;
/// - a profile two workspaces share (the days a workspace picked one)
///   stays with the first; the others get new ones;
/// - a workspace without one gets a new one, except that when none has
///   one (the first time, or every one is new) the first takes the
///   registry's active profile, so the pages already open keep theirs;
/// - the showing workspace's becomes the registry's active profile.
fn pair(
    reg: &mut ProfilesRegistry,
    workspaces: &[&str],
    showing: &str,
    mut new_id: impl FnMut() -> String,
) {
    if workspaces.is_empty() {
        return;
    }
    let exists = |reg: &ProfilesRegistry, id: &str| reg.profiles.iter().any(|p| p.id == id);
    let mut paired = std::collections::BTreeMap::new();
    let mut taken = std::collections::HashSet::new();
    for &ws in workspaces {
        if let Some(id) = reg.workspaces.get(ws) {
            if exists(reg, id) && taken.insert(id.clone()) {
                paired.insert(ws.to_string(), id.clone());
            }
        }
    }
    let mut heir = Some(reg.active.clone())
        .filter(|a| paired.is_empty() && exists(reg, a) && !taken.contains(a));
    for &ws in workspaces {
        if paired.contains_key(ws) {
            continue;
        }
        let id = heir.take().unwrap_or_else(|| {
            let id = new_id();
            reg.profiles.push(ProfileEntry { id: id.clone() });
            id
        });
        paired.insert(ws.to_string(), id);
    }
    if let Some(id) = paired.get(showing) {
        reg.active = id.clone();
    }
    reg.workspaces = paired;
}

/// The profiles some workspace runs.
pub fn workspace_profiles() -> Vec<String> {
    let reg = load_registry_or_empty();
    let mut out: Vec<String> = Vec::new();
    for id in reg.workspaces.values() {
        if reg.profiles.iter().any(|p| &p.id == id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
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
///
/// After [`bind_external`], only the bound id — wrappers must not
/// prewarm browser profiles.
pub fn list() -> Vec<ProfileEntry> {
    if is_external() {
        if let Some(p) = active_if_bound() {
            return vec![ProfileEntry { id: p.id }];
        }
    }
    read_registry(&registry_path())
        .map(|r| r.profiles)
        .unwrap_or_default()
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
        data_dir,
        cache_dir,
    }
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
                profiles: vec![ProfileEntry { id: id.clone() }],
                workspaces: Default::default(),
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
            let e = ProfileEntry { id };
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
        workspaces: Default::default(),
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
    new_uuid()
}

/// A UUID v4 string (profiles, page ids).
pub(crate) fn new_uuid() -> String {
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

/// Browser-wide durable data (downloads index, visit history).
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

/// Durable data dir for `id`. After [`bind_external`], the bound wrapper
/// directory — not `browser_data_root()/profiles/<id>`.
pub fn data_dir_for(id: &str) -> PathBuf {
    if let Some(p) = active_if_bound() {
        if p.id == id {
            return p.data_dir;
        }
    }
    profile_data_dir(id)
}

/// Unix socket the chrome process uses to talk to a headless CEF helper.
pub fn engine_sock_path(id: &str) -> PathBuf {
    data_dir_for(id).join("engine.sock")
}

/// Dedicated pixel-frame socket (kept off the control channel).
pub fn engine_frame_sock_path(id: &str) -> PathBuf {
    data_dir_for(id).join("engine.frame.sock")
}

/// Pid file for the headless CEF helper of this profile.
pub fn engine_pid_path(id: &str) -> PathBuf {
    data_dir_for(id).join("engine.pid")
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

    fn registry(profiles: &[&str], workspaces: &[(&str, &str)]) -> ProfilesRegistry {
        ProfilesRegistry {
            version: REGISTRY_VERSION,
            active: profiles[0].into(),
            profiles: profiles.iter().map(|id| ProfileEntry { id: (*id).into() }).collect(),
            workspaces: workspaces
                .iter()
                .map(|(w, p)| ((*w).into(), (*p).into()))
                .collect(),
        }
    }

    /// Pair, minting `new-1`, `new-2`, … for new profiles.
    fn paired(reg: &mut ProfilesRegistry, list: &[&str], showing: &str) -> Vec<(String, String)> {
        let mut n = 0;
        pair(reg, list, showing, || {
            n += 1;
            format!("new-{n}")
        });
        reg.workspaces.clone().into_iter().collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect()
    }

    #[test]
    fn the_first_workspace_keeps_the_open_pages_profile_and_the_rest_get_new_ones() {
        let mut reg = registry(&["primary"], &[]);
        let got = paired(&mut reg, &["main", "ws-2"], "ws-2");
        assert_eq!(got, pairs(&[("main", "primary"), ("ws-2", "new-1")]));
        assert_eq!(reg.active, "new-1");
    }

    #[test]
    fn a_shared_profile_stays_with_the_first_workspace() {
        let mut reg = registry(&["p"], &[("main", "p"), ("ws-2", "p")]);
        let got = paired(&mut reg, &["main", "ws-2"], "main");
        assert_eq!(got, pairs(&[("main", "p"), ("ws-2", "new-1")]));
    }

    #[test]
    fn a_deleted_workspace_lets_go_and_a_new_one_starts_clean() {
        let mut reg = registry(&["p", "q"], &[("main", "p"), ("ws-2", "q")]);
        let got = paired(&mut reg, &["main"], "main");
        assert_eq!(got, pairs(&[("main", "p")]));
        // Its profile stays (data and all), but no workspace takes it.
        assert!(reg.profiles.iter().any(|p| p.id == "q"));
        let got = paired(&mut reg, &["main", "ws-3"], "ws-3");
        assert_eq!(got, pairs(&[("main", "p"), ("ws-3", "new-1")]));
    }

    #[test]
    fn when_every_workspace_is_new_the_first_takes_the_active_profile() {
        let mut reg = registry(&["p", "q"], &[("old", "q")]);
        reg.active = "q".into();
        let got = paired(&mut reg, &["ws-a", "ws-b"], "ws-a");
        assert_eq!(got, pairs(&[("ws-a", "q"), ("ws-b", "new-1")]));
    }

    #[test]
    fn pairing_twice_changes_nothing() {
        let mut reg = registry(&["p"], &[("main", "p")]);
        paired(&mut reg, &["main", "ws-2"], "main");
        let before = reg.clone();
        paired(&mut reg, &["main", "ws-2"], "main");
        assert_eq!(reg, before, "nothing changed, nothing to save");
    }

    #[test]
    fn a_registry_row_with_a_name_still_reads() {
        let raw = r#"{"version":1,"active":"p","profiles":[{"id":"p","name":"Primary"}],"workspaces":{"main":"p"}}"#;
        let reg: ProfilesRegistry = serde_json::from_str(raw).unwrap();
        assert_eq!(reg.profiles, vec![ProfileEntry { id: "p".into() }]);
    }
}
