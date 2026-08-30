//! Steam library discovery from on-disk VDF / ACF / appinfo files.
//!
//! No Steam API; no Settings catalog. Reads `libraryfolders.vdf`, each
//! library's `appmanifest_*.acf`, per-user `localconfig.vdf` (LastPlayed),
//! and `appcache/appinfo.vdf` (names + type for uninstalled titles).
//! Filters tools/runtimes so the arcade list is playable titles only.
//!
//! Scan results are cached under `~/.config/sola/arcade-library.json` so the
//! Arcade UI can open instantly and refresh in the background.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sola_core::config::JsonConfig;
use tracing::{info, warn};

/// One Steam title (installed or owned-but-uninstalled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    pub install_dir: Option<String>,
    #[serde(default)]
    pub library_path: PathBuf,
    /// Banner art path when known. May be empty after a fast scan — resolve
    /// with [`banner_art_path`] when the row becomes visible.
    #[serde(default)]
    pub banner: Option<PathBuf>,
    /// Fully installed on disk (`StateFlags` bit 2 / value 4).
    pub installed: bool,
    /// Most recent player activity (unix seconds): max of LastPlayed and
    /// manifest `LastUpdated`. Zero when unknown — sorts last under recency.
    #[serde(default)]
    pub last_activity: u64,
}

/// On-disk library cache written after each successful scan.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LibraryCache {
    /// Bump when the JSON shape changes incompatibly.
    #[serde(default)]
    version: u32,
    /// Unix seconds when the scan finished.
    #[serde(default)]
    scanned_at: u64,
    #[serde(default)]
    games: Vec<SteamGame>,
}

impl JsonConfig for LibraryCache {
    const FILE_NAME: &'static str = "arcade-library.json";
}

const LIBRARY_CACHE_VERSION: u32 = 1;

/// Load the last successful library scan, if any.
///
/// Returns `None` when the file is missing or unreadable so callers can show
/// a first-time scan status. An empty `games` list with a valid file is still
/// `Some` (user may truly have no titles).
pub fn load_library_cache() -> Option<Vec<SteamGame>> {
    match LibraryCache::try_load() {
        Ok(cache) if cache.version == LIBRARY_CACHE_VERSION => {
            info!(
                n = cache.games.len(),
                scanned_at = cache.scanned_at,
                "loaded arcade library cache"
            );
            Some(cache.games)
        }
        Ok(cache) => {
            warn!(
                version = cache.version,
                expected = LIBRARY_CACHE_VERSION,
                "arcade library cache version mismatch — rescanning"
            );
            None
        }
        Err(_) => None,
    }
}

/// Persist a completed scan for the next cold start.
pub fn save_library_cache(games: &[SteamGame]) {
    let scanned_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache = LibraryCache {
        version: LIBRARY_CACHE_VERSION,
        scanned_at,
        games: games.to_vec(),
    };
    cache.save();
    info!(n = games.len(), "saved arcade library cache");
}

impl SteamGame {
    /// Wide banner art for full-width list rows (faded background).
    #[allow(dead_code)]
    pub fn banner_path(&self) -> Option<PathBuf> {
        self.banner.clone().or_else(|| banner_art_path(self.app_id))
    }
}

/// How the gallery orders rows. Persisted in [`crate::prefs::ArcadePrefs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    #[default]
    Alphabetical,
    /// Most recent player activity first (play / install / update).
    Recency,
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
#[allow(dead_code)]
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

/// Full library scan (installed + uninstalled games known from localconfig
/// / appinfo). Default sort is alphabetical; callers re-sort for recency.
pub fn scan_library_games() -> Vec<SteamGame> {
    let activity = scan_activity_map();
    let mut by_id: BTreeMap<u32, SteamGame> = BTreeMap::new();

    // 1) Installed (and partially-present) manifests.
    for lib in steam_library_roots() {
        for g in scan_library(&lib) {
            let act = activity.get(&g.app_id).copied().unwrap_or(0);
            let last_activity = act.max(g.last_activity);
            by_id
                .entry(g.app_id)
                .and_modify(|existing| {
                    // Prefer installed record if we already saw a stub.
                    if g.installed || !existing.installed {
                        existing.installed = existing.installed || g.installed;
                        if g.installed {
                            existing.install_dir = g.install_dir.clone();
                            existing.library_path = g.library_path.clone();
                            if !g.name.is_empty() {
                                existing.name = g.name.clone();
                            }
                        }
                        existing.last_activity = existing.last_activity.max(last_activity);
                        if existing.banner.is_none() {
                            existing.banner = g.banner.clone();
                        }
                    }
                })
                .or_insert_with(|| SteamGame { last_activity, ..g });
        }
    }

    // 2) Uninstalled games: type=game in appinfo that appear in localconfig
    //    activity (played / launched) but have no fully-installed manifest.
    let need_meta: Vec<u32> = activity
        .keys()
        .copied()
        .filter(|id| {
            by_id
                .get(id)
                .map(|g| !g.installed || g.name.is_empty())
                .unwrap_or(true)
        })
        .collect();
    let meta = load_appinfo_meta(&need_meta);

    for (app_id, last) in &activity {
        if let Some(existing) = by_id.get_mut(app_id) {
            existing.last_activity = existing.last_activity.max(*last);
            if existing.name.is_empty() {
                if let Some(m) = meta.get(app_id) {
                    existing.name = m.name.clone();
                }
            }
            continue;
        }
        let Some(m) = meta.get(app_id) else {
            continue;
        };
        // Uninstalled list: real games only (DLC/tools already excluded by type).
        if !m.is_game {
            continue;
        }
        if !is_playable_name_id(*app_id, &m.name) {
            continue;
        }
        // Defer banner path walks until a row is visible — hundreds of
        // librarycache lookups dominate cold scan time.
        by_id.insert(
            *app_id,
            SteamGame {
                app_id: *app_id,
                name: m.name.clone(),
                install_dir: None,
                library_path: PathBuf::new(),
                banner: None,
                installed: false,
                last_activity: *last,
            },
        );
    }

    let mut games: Vec<SteamGame> = by_id.into_values().collect();
    // Drop non-playable installed tools that slipped past (name markers).
    games.retain(|g| is_playable(g));
    sort_games(&mut games, SortMode::Alphabetical);
    games
}

/// Sort `games` in place by the requested mode.
pub fn sort_games(games: &mut [SteamGame], mode: SortMode) {
    match mode {
        SortMode::Alphabetical => {
            games.sort_by(|a, b| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
                    .then_with(|| a.app_id.cmp(&b.app_id))
            });
        }
        SortMode::Recency => {
            // Newest activity first; unknown (0) last; tie-break by name.
            games.sort_by(|a, b| {
                b.last_activity
                    .cmp(&a.last_activity)
                    .then_with(|| {
                        a.name
                            .to_ascii_lowercase()
                            .cmp(&b.name.to_ascii_lowercase())
                    })
                    .then_with(|| a.app_id.cmp(&b.app_id))
            });
        }
    }
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
        || library_root.read_dir().ok().into_iter().flatten().any(|e| {
            e.ok()
                .map(|e| e.file_name().to_string_lossy().starts_with("appmanifest_"))
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
    let installed = state & 4 != 0;
    // Keep uninstalled ACF stubs only when still present (rare); the main
    // uninstalled path is localconfig + appinfo.
    if !installed {
        return None;
    }
    let install_dir = vdf_string(&text, "installdir");
    let last_updated: u64 = vdf_string(&text, "LastUpdated")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // Banner paths resolved lazily when the row scrolls into view.
    Some(SteamGame {
        app_id,
        name,
        install_dir,
        library_path: library_root.to_path_buf(),
        banner: None,
        installed: true,
        last_activity: last_updated,
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

/// Per-app max activity time from every userdata `localconfig.vdf`.
fn scan_activity_map() -> HashMap<u32, u64> {
    let mut out: HashMap<u32, u64> = HashMap::new();
    for root in steam_client_roots() {
        let userdata = root.join("userdata");
        let Ok(users) = fs::read_dir(&userdata) else {
            continue;
        };
        for user in users.flatten() {
            let path = user.path().join("config/localconfig.vdf");
            if !path.is_file() {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            merge_localconfig_activity(&text, &mut out);
        }
    }
    out
}

/// Parse `UserLocalConfigStore` → `apps` → per-app `LastPlayed` (and
/// autocloud launch/exit as a weaker signal).
fn merge_localconfig_activity(text: &str, out: &mut HashMap<u32, u64>) {
    // Walk every `"<appid>" { ... }` block that contains LastPlayed / autocloud.
    // Nested braces inside cloud blocks are handled with a simple depth scan.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Find a quoted numeric key.
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let key_start = i + 1;
        let mut key_end = key_start;
        while key_end < bytes.len() && bytes[key_end] != b'"' {
            key_end += 1;
        }
        if key_end >= bytes.len() {
            break;
        }
        let key = &text[key_start..key_end];
        let app_id: u32 = match key.parse() {
            Ok(id) if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) => id,
            _ => {
                i = key_end + 1;
                continue;
            }
        };
        // Skip past key and whitespace to `{`.
        let mut j = key_end + 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'{' {
            i = key_end + 1;
            continue;
        }
        // Extract brace-balanced body.
        let body_start = j + 1;
        let mut depth = 1usize;
        j += 1;
        while j < bytes.len() && depth > 0 {
            match bytes[j] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let body = &text[body_start..j.saturating_sub(1)];
        // Only treat as an app block if it looks like localconfig apps entry.
        let mut activity = 0u64;
        if let Some(lp) = vdf_string(body, "LastPlayed").and_then(|s| s.parse().ok()) {
            activity = activity.max(lp);
        }
        if let Some(ll) = vdf_string(body, "lastlaunch").and_then(|s| s.parse().ok()) {
            activity = activity.max(ll);
        }
        if let Some(le) = vdf_string(body, "lastexit").and_then(|s| s.parse().ok()) {
            activity = activity.max(le);
        }
        // Playtime alone (no LastPlayed) still means the app is known — keep
        // a minimal non-zero so recency places it above pure unknowns when
        // we later max with install time; value 1 sorts near the bottom.
        if activity == 0 {
            if vdf_string(body, "Playtime").is_some() {
                activity = 1;
            } else {
                i = j;
                continue;
            }
        }
        out.entry(app_id)
            .and_modify(|t| *t = (*t).max(activity))
            .or_insert(activity);
        i = j;
    }
}

#[derive(Debug, Clone)]
struct AppMeta {
    name: String,
    is_game: bool,
}

/// Load names + type for the given app ids from `appcache/appinfo.vdf` (v41).
fn load_appinfo_meta(want: &[u32]) -> HashMap<u32, AppMeta> {
    if want.is_empty() {
        return HashMap::new();
    }
    let want_set: BTreeSet<u32> = want.iter().copied().collect();
    let mut out = HashMap::new();
    for root in steam_client_roots() {
        let path = root.join("appcache/appinfo.vdf");
        if !path.is_file() {
            continue;
        }
        let Ok(data) = fs::read(&path) else {
            continue;
        };
        parse_appinfo_v41(&data, &want_set, &mut out);
        if out.len() >= want_set.len() {
            break;
        }
    }
    out
}

/// Steam appinfo.vdf magic for format version 41 (string-table keys).
const APPINFO_MAGIC_V41: u32 = 0x0756_4429;

fn parse_appinfo_v41(data: &[u8], want: &BTreeSet<u32>, out: &mut HashMap<u32, AppMeta>) {
    if data.len() < 16 {
        return;
    }
    let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if magic != APPINFO_MAGIC_V41 {
        // Older formats omit the string table — skip for now (v41 is current).
        return;
    }
    let string_table_offset = i64::from_le_bytes(data[8..16].try_into().unwrap());
    if string_table_offset < 0 {
        return;
    }
    let sto = string_table_offset as usize;
    if sto + 4 > data.len() {
        return;
    }
    let string_count = u32::from_le_bytes(data[sto..sto + 4].try_into().unwrap()) as usize;
    let mut strings = Vec::with_capacity(string_count.min(16_384));
    let mut sp = sto + 4;
    for _ in 0..string_count {
        if sp >= data.len() {
            break;
        }
        let end = data[sp..]
            .iter()
            .position(|&b| b == 0)
            .map(|o| sp + o)
            .unwrap_or(data.len());
        let s = String::from_utf8_lossy(&data[sp..end]).into_owned();
        strings.push(s);
        sp = end.saturating_add(1);
    }

    let mut pos = 16usize;
    while pos + 8 <= data.len() {
        let app_id = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        if app_id == 0 {
            break;
        }
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let entry_start = pos + 8;
        let entry_end = entry_start.saturating_add(size);
        if entry_end > data.len() {
            break;
        }
        if want.contains(&app_id) && !out.contains_key(&app_id) {
            // v40+ payload: info_state(4) last_updated(4) pics(8) sha1(20)
            // change(4) bin_sha1(20) = 60, then binary VDF.
            if size > 60 {
                let vdf = &data[entry_start + 60..entry_end];
                if let Some(meta) = meta_from_appinfo_vdf(vdf, &strings) {
                    out.insert(app_id, meta);
                }
            }
        }
        pos = entry_end;
    }
}

fn meta_from_appinfo_vdf(vdf: &[u8], strings: &[String]) -> Option<AppMeta> {
    let (root, _) = parse_bin_vdf(vdf, 0, strings)?;
    // Steam appinfo VDF is usually `{ appinfo { common { name type … } } }`;
    // older/test blobs may put `common` at the root.
    let common = match bin_map_get(&root, "common") {
        Some(m) => m,
        None => match bin_map_get(&root, "appinfo") {
            Some(appinfo) => bin_map_get(appinfo, "common")?,
            None => return None,
        },
    };
    let name = match common.get("name") {
        Some(BinVal::Str(s)) if !s.is_empty() => s.clone(),
        _ => return None,
    };
    let is_game = match common.get("type") {
        Some(BinVal::Str(t)) => t.eq_ignore_ascii_case("game"),
        _ => false,
    };
    Some(AppMeta { name, is_game })
}

fn bin_map_get<'a>(
    map: &'a HashMap<String, BinVal>,
    key: &str,
) -> Option<&'a HashMap<String, BinVal>> {
    match map.get(key) {
        Some(BinVal::Map(m)) => Some(m),
        _ => None,
    }
}

#[derive(Debug)]
enum BinVal {
    Str(String),
    Map(HashMap<String, BinVal>),
    #[allow(dead_code)]
    Other,
}

fn parse_bin_vdf(
    buf: &[u8],
    mut i: usize,
    strings: &[String],
) -> Option<(HashMap<String, BinVal>, usize)> {
    let mut map = HashMap::new();
    while i < buf.len() {
        let t = buf[i];
        i += 1;
        if t == 0x08 {
            // End of map.
            return Some((map, i));
        }
        if i + 4 > buf.len() {
            return Some((map, i));
        }
        let key_idx = u32::from_le_bytes(buf[i..i + 4].try_into().ok()?) as usize;
        i += 4;
        let key = strings
            .get(key_idx)
            .cloned()
            .unwrap_or_else(|| key_idx.to_string());
        match t {
            0x00 => {
                let (child, ni) = parse_bin_vdf(buf, i, strings)?;
                i = ni;
                map.insert(key, BinVal::Map(child));
            }
            0x01 => {
                let end = buf[i..].iter().position(|&b| b == 0)? + i;
                let s = String::from_utf8_lossy(&buf[i..end]).into_owned();
                i = end + 1;
                map.insert(key, BinVal::Str(s));
            }
            0x02 => {
                // int32 — skip
                i = i.saturating_add(4);
                map.insert(key, BinVal::Other);
            }
            0x03 => {
                // float
                i = i.saturating_add(4);
                map.insert(key, BinVal::Other);
            }
            0x07 => {
                // uint64
                i = i.saturating_add(8);
                map.insert(key, BinVal::Other);
            }
            0x0A => {
                // color / int32 variant
                i = i.saturating_add(4);
                map.insert(key, BinVal::Other);
            }
            _ => {
                // Unknown type — abort this branch cleanly.
                return Some((map, i));
            }
        }
    }
    Some((map, i))
}

fn is_playable(game: &SteamGame) -> bool {
    is_playable_name_id(game.app_id, &game.name)
}

fn is_playable_name_id(app_id: u32, name: &str) -> bool {
    if TOOL_APP_IDS.contains(&app_id) {
        return false;
    }
    let low = name.to_ascii_lowercase();
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
            installed: true,
            last_activity: 0,
        }));
        assert!(!is_playable(&SteamGame {
            app_id: 999,
            name: "Something Dedicated Server".into(),
            install_dir: None,
            library_path: PathBuf::new(),
            banner: None,
            installed: true,
            last_activity: 0,
        }));
        assert!(is_playable(&SteamGame {
            app_id: 400,
            name: "Portal".into(),
            install_dir: Some("Portal".into()),
            library_path: PathBuf::new(),
            banner: None,
            installed: true,
            last_activity: 0,
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
"LastUpdated""1700000000"
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
        assert!(games[0].installed);
        assert_eq!(games[0].last_activity, 1_700_000_000);
    }

    #[test]
    fn merge_localconfig_activity_reads_last_played() {
        let sample = r#""UserLocalConfigStore"
{
"Software"
{
"Valve"
{
"Steam"
{
"apps"
{
"400"
{
"LastPlayed""1786223341"
"Playtime""761"
}
"620"
{
"LastPlayed""1329984000"
"Playtime""1703"
"autocloud"
{
"lastlaunch""1329983000"
"lastexit""1329984000"
}
}
"7"
{
"cloud"
{
"last_sync_state""synchronized"
}
}
}
}
}
}
}
"#;
        let mut map = HashMap::new();
        merge_localconfig_activity(sample, &mut map);
        assert_eq!(map.get(&400), Some(&1_786_223_341));
        assert_eq!(map.get(&620), Some(&1_329_984_000));
        // App 7 has no play signal — skipped.
        assert!(!map.contains_key(&7));
    }

    #[test]
    fn sort_games_recency_then_alpha() {
        let mut games = vec![
            SteamGame {
                app_id: 1,
                name: "Zebra".into(),
                install_dir: None,
                library_path: PathBuf::new(),
                banner: None,
                installed: true,
                last_activity: 100,
            },
            SteamGame {
                app_id: 2,
                name: "Alpha".into(),
                install_dir: None,
                library_path: PathBuf::new(),
                banner: None,
                installed: false,
                last_activity: 200,
            },
            SteamGame {
                app_id: 3,
                name: "Beta".into(),
                install_dir: None,
                library_path: PathBuf::new(),
                banner: None,
                installed: true,
                last_activity: 0,
            },
        ];
        sort_games(&mut games, SortMode::Recency);
        assert_eq!(
            games.iter().map(|g| g.app_id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
        sort_games(&mut games, SortMode::Alphabetical);
        assert_eq!(
            games.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["Alpha", "Beta", "Zebra"]
        );
    }

    #[test]
    fn parse_appinfo_v41_name_and_type() {
        // Minimal synthetic v41 blob matching Steam's real layout:
        // { appinfo { common { name "Portal" type "game" } } }
        // String table: 0=appinfo 1=common 2=name 3=type
        let strings = ["appinfo", "common", "name", "type"];
        let mut vdf = Vec::new();
        vdf.push(0x00); // nested appinfo
        vdf.extend_from_slice(&0u32.to_le_bytes());
        vdf.push(0x00); // nested common
        vdf.extend_from_slice(&1u32.to_le_bytes());
        vdf.push(0x01); // string name
        vdf.extend_from_slice(&2u32.to_le_bytes());
        vdf.extend_from_slice(b"Portal\0");
        vdf.push(0x01); // string type
        vdf.extend_from_slice(&3u32.to_le_bytes());
        vdf.extend_from_slice(b"game\0");
        vdf.push(0x08); // end common
        vdf.push(0x08); // end appinfo
        vdf.push(0x08); // end root

        let mut file = Vec::new();
        file.extend_from_slice(&APPINFO_MAGIC_V41.to_le_bytes());
        file.extend_from_slice(&1u32.to_le_bytes()); // universe
        let st_off_pos = file.len();
        file.extend_from_slice(&0i64.to_le_bytes());

        let app_id = 400u32;
        file.extend_from_slice(&app_id.to_le_bytes());
        let payload_size = (60 + vdf.len()) as u32;
        file.extend_from_slice(&payload_size.to_le_bytes());
        file.extend(std::iter::repeat_n(0u8, 60));
        file.extend_from_slice(&vdf);
        file.extend_from_slice(&0u32.to_le_bytes());

        let st_off = file.len() as i64;
        file[st_off_pos..st_off_pos + 8].copy_from_slice(&st_off.to_le_bytes());
        file.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        for s in &strings {
            file.extend_from_slice(s.as_bytes());
            file.push(0);
        }

        let mut out = HashMap::new();
        let want = BTreeSet::from([400u32]);
        parse_appinfo_v41(&file, &want, &mut out);
        let meta = out.get(&400).expect("meta");
        assert_eq!(meta.name, "Portal");
        assert!(meta.is_game);
    }
}
