//! Steam library discovery from on-disk VDF / ACF files.
//!
//! No Steam API; no Settings catalog. Reads `libraryfolders.vdf` and each
//! library's `appmanifest_*.acf`. Filters tools/runtimes so the arcade list
//! is playable titles only.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// One installed Steam title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    pub install_dir: Option<String>,
    pub library_path: PathBuf,
    /// Resolved at scan time (avoids per-frame FS walks in the list view).
    pub banner: Option<PathBuf>,
}

impl SteamGame {
    /// Wide banner art for full-width list rows (faded background).
    pub fn banner_path(&self) -> Option<PathBuf> {
        self.banner.clone().or_else(|| banner_art_path(self.app_id))
    }
}

/// Resolve Steam **banner** art for `app_id` (wide hero / header).
///
/// Prefers `library_hero.jpg` (1920×620), then nested hero/header, then
/// legacy `header.jpg` (460×215). Portrait capsules are intentionally
/// skipped so list rows aren't portrait-cropped.
pub fn banner_art_path(app_id: u32) -> Option<PathBuf> {
    let id = app_id.to_string();
    for root in steam_client_roots() {
        let base = root.join("appcache/librarycache").join(&id);
        if !base.is_dir() {
            continue;
        }
        for name in ["library_hero.jpg", "header.jpg"] {
            let p = base.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
        if let Ok(entries) = fs::read_dir(&base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                for name in ["library_hero.jpg", "library_header.jpg", "header.jpg"] {
                    let p = path.join(name);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

/// Back-compat alias used by older tests / call sites.
pub fn cover_art_path(app_id: u32) -> Option<PathBuf> {
    banner_art_path(app_id)
}

/// Steam *client* install roots (where `appcache/` lives), not extra libraries.
fn steam_client_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    let consider = |p: PathBuf, out: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>| {
        if p.is_dir() && seen.insert(p.clone()) {
            out.push(p);
        }
    };
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        consider(home.join(".local/share/Steam"), &mut roots, &mut seen);
        consider(home.join(".steam/steam"), &mut roots, &mut seen);
        consider(home.join(".steam/root"), &mut roots, &mut seen);
        consider(
            home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
            &mut roots,
            &mut seen,
        );
    }
    if let Ok(dir) = std::env::var("STEAM_DIR") {
        consider(PathBuf::from(dir), &mut roots, &mut seen);
    }
    if let Ok(dir) = std::env::var("STEAM_ROOT") {
        consider(PathBuf::from(dir), &mut roots, &mut seen);
    }
    roots
}

/// Scan default Steam roots (+ `libraryfolders.vdf` library paths).
pub fn scan_installed_games() -> Vec<SteamGame> {
    let mut games = Vec::new();
    let mut seen = BTreeSet::new();
    for lib in steam_library_roots() {
        for g in scan_library(&lib) {
            if seen.insert(g.app_id) {
                games.push(g);
            }
        }
    }
    games.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    games
}

/// Candidate Steam install roots (client home), then library folders.
fn steam_library_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();

    let consider = |p: PathBuf, out: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>| {
        if p.is_dir() && seen.insert(p.clone()) {
            out.push(p);
        }
    };

    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        consider(home.join(".local/share/Steam"), &mut roots, &mut seen);
        consider(home.join(".steam/steam"), &mut roots, &mut seen);
        consider(home.join(".steam/root"), &mut roots, &mut seen);
        consider(
            home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
            &mut roots,
            &mut seen,
        );
    }
    if let Ok(dir) = std::env::var("STEAM_DIR") {
        consider(PathBuf::from(dir), &mut roots, &mut seen);
    }
    if let Ok(dir) = std::env::var("STEAM_ROOT") {
        consider(PathBuf::from(dir), &mut roots, &mut seen);
    }

    // Expand libraryfolders.vdf from each client root discovered so far.
    let client_roots = roots.clone();
    for root in client_roots {
        for lib in parse_library_folders(&root.join("steamapps/libraryfolders.vdf")) {
            consider(lib, &mut roots, &mut seen);
        }
        for lib in parse_library_folders(&root.join("libraryfolders.vdf")) {
            consider(lib, &mut roots, &mut seen);
        }
    }

    roots
}

fn scan_library(library_root: &Path) -> Vec<SteamGame> {
    let steamapps = library_root.join("steamapps");
    let dir = if steamapps.is_dir() {
        steamapps
    } else if library_root.join("appmanifest_400.acf").exists()
        || library_root
            .read_dir()
            .ok()
            .into_iter()
            .flatten()
            .any(|e| {
                e.ok()
                    .map(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("appmanifest_")
                    })
                    .unwrap_or(false)
            })
    {
        library_root.to_path_buf()
    } else {
        return Vec::new();
    };

    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
            continue;
        }
        if let Some(game) = parse_appmanifest(&path, library_root) {
            if is_playable(&game) {
                out.push(game);
            }
        }
    }
    out
}

fn parse_appmanifest(path: &Path, library_root: &Path) -> Option<SteamGame> {
    let text = fs::read_to_string(path).ok()?;
    let app_id: u32 = vdf_string(&text, "appid")?.parse().ok()?;
    let name = vdf_string(&text, "name")?;
    let state: u32 = vdf_string(&text, "StateFlags")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // Bit 2 (value 4) = Fully Installed in Steam's StateFlags.
    if state & 4 == 0 {
        return None;
    }
    let install_dir = vdf_string(&text, "installdir");
    let banner = banner_art_path(app_id);
    Some(SteamGame {
        app_id,
        name,
        install_dir,
        library_path: library_root.to_path_buf(),
        banner,
    })
}

/// Top-level `"key" "value"` pairs in ACF (Steam's simplified VDF).
fn vdf_string(text: &str, key: &str) -> Option<String> {
    // "key"\t\t"value"  or  "key""value"  or  "key" "value"
    let needle = format!("\"{key}\"");
    let mut rest = text;
    while let Some(idx) = rest.find(&needle) {
        let after = &rest[idx + needle.len()..];
        // Skip whitespace / tabs between key and value.
        let after = after.trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                return Some(stripped[..end].to_string());
            }
        }
        rest = &rest[idx + needle.len()..];
    }
    None
}

/// `libraryfolders.vdf` → library root paths.
fn parse_library_folders(path: &Path) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // Each library block has "path" "…".
    let mut rest = text.as_str();
    let key = "\"path\"";
    while let Some(idx) = rest.find(key) {
        let after = rest[idx + key.len()..].trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                let p = stripped[..end].replace("\\\\", "\\");
                out.push(PathBuf::from(p));
            }
        }
        rest = &rest[idx + key.len()..];
    }
    out
}

fn is_playable(game: &SteamGame) -> bool {
    if TOOL_APP_IDS.contains(&game.app_id) {
        return false;
    }
    let low = game.name.to_ascii_lowercase();
    !TOOL_NAME_MARKERS.iter().any(|m| low.contains(m))
}

/// Known non-game Steam appids (Proton, runtimes, redistributables).
const TOOL_APP_IDS: &[u32] = &[
    228980,  // Steamworks Common Redistributables
    1070560, // Steam Linux Runtime 1.0 (scout)
    1391110, // Steam Linux Runtime 2.0 (soldier)
    1628350, // Steam Linux Runtime 3.0 (sniper) — common
    1493710, // Proton Experimental
    1580130, // Proton Hotfix
    1887720, // Proton 8.0 (example; name filter covers others)
    2805730, // Proton 9.0
    3658110, // Proton 10.0-ish
    4628710, // Proton 11.0
    3086180, // Proton Voice Files
    4183110, // Steam Linux Runtime 4.0
];

const TOOL_NAME_MARKERS: &[&str] = &[
    "proton",
    "steam linux runtime",
    "steamworks common redistribut",
    "dedicated server",
    " server",
    "sdk",
    "redistributable",
    "steamworks sdk",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn vdf_string_reads_acf_style() {
        let sample = r#""AppState"
{
"appid""400"
"name""Portal"
"StateFlags""4"
"installdir""Portal"
}
"#;
        assert_eq!(vdf_string(sample, "appid").as_deref(), Some("400"));
        assert_eq!(vdf_string(sample, "name").as_deref(), Some("Portal"));
        assert_eq!(vdf_string(sample, "StateFlags").as_deref(), Some("4"));
    }

    #[test]
    fn parse_appmanifest_filters_uninstalled() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path();
        let apps = lib.join("steamapps");
        fs::create_dir_all(&apps).unwrap();
        let path = apps.join("appmanifest_1.acf");
        let mut f = fs::File::create(&path).unwrap();
        write!(
            f,
            r#""AppState" {{ "appid" "1" "name" "No" "StateFlags" "0" }}"#
        )
        .unwrap();
        assert!(parse_appmanifest(&path, lib).is_none());
    }

    #[test]
    fn is_playable_drops_proton_and_runtimes() {
        assert!(!is_playable(&SteamGame {
            app_id: 1493710,
            name: "Proton Experimental".into(),
            install_dir: None,
            library_path: PathBuf::new(),
            banner: None,
        }));
        assert!(!is_playable(&SteamGame {
            app_id: 999,
            name: "Something Dedicated Server".into(),
            install_dir: None,
            library_path: PathBuf::new(),
            banner: None,
        }));
        assert!(is_playable(&SteamGame {
            app_id: 400,
            name: "Portal".into(),
            install_dir: Some("Portal".into()),
            library_path: PathBuf::new(),
            banner: None,
        }));
    }

    #[test]
    fn banner_art_path_prefers_library_hero() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cache = root.join("appcache/librarycache/400");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("header.jpg"), b"hdr").unwrap();
        fs::write(cache.join("library_600x900.jpg"), b"portrait").unwrap();
        fs::write(cache.join("library_hero.jpg"), b"hero").unwrap();
        let prev = std::env::var_os("STEAM_DIR");
        // SAFETY: serial test process env for fixture path only.
        unsafe {
            std::env::set_var("STEAM_DIR", root);
        }
        let path = banner_art_path(400).expect("banner");
        assert!(path.ends_with("library_hero.jpg"), "{path:?}");
        unsafe {
            match prev {
                Some(v) => std::env::set_var("STEAM_DIR", v),
                None => std::env::remove_var("STEAM_DIR"),
            }
        }
    }

    #[test]
    fn scan_library_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path();
        let apps = lib.join("steamapps");
        fs::create_dir_all(&apps).unwrap();
        fs::write(
            apps.join("appmanifest_400.acf"),
            r#""AppState"
{
"appid""400"
"name""Portal"
"StateFlags""4"
"installdir""Portal"
}
"#,
        )
        .unwrap();
        fs::write(
            apps.join("appmanifest_228980.acf"),
            r#""AppState"
{
"appid""228980"
"name""Steamworks Common Redistributables"
"StateFlags""4"
}
"#,
        )
        .unwrap();
        let games = scan_library(lib);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].app_id, 400);
        assert_eq!(games[0].name, "Portal");
    }
}
