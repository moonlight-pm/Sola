//! CEF binary distribution: probe + download + path resolution.
//!
//! Single source of truth for CEF version. Bumping is a one-character
//! edit; the cache directory is version-suffixed so multiple versions
//! coexist safely.

use std::path::PathBuf;

/// Pinned CEF release. Update this constant to bump the engine version.
/// Match the binary tarball naming on https://cef-builds.spotifycdn.com/.
pub const CEF_VERSION: &str = "147.0.10+gd58e84d+chromium-147.0.7727.118";

/// Directory name used inside the cache. Stable across version bumps
/// only via the version-suffixed subdirectory.
const CACHE_PREFIX: &str = "cef-";

/// Resolve `~/.cache/sola/cef-<CEF_VERSION>/`.
pub fn cef_path() -> PathBuf {
    let base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"))
        .join("sola");
    base.join(format!("{CACHE_PREFIX}{CEF_VERSION}"))
}

/// Path to the `Release/` subdirectory containing libcef.so + binaries.
pub fn release_dir() -> PathBuf {
    cef_path().join("Release")
}

/// Path to the `Resources/` subdirectory containing icudtl.dat + .pak files.
pub fn resources_dir() -> PathBuf {
    cef_path().join("Resources")
}

/// Path to the `Resources/locales/` subdirectory.
pub fn locales_dir() -> PathBuf {
    resources_dir().join("locales")
}

/// True if a usable CEF tree is present at the cache location.
pub fn is_present() -> bool {
    release_dir().join("libcef.so").exists()
}

use std::fs;
use std::io;

/// URL for the official Spotify-hosted CEF tarball matching `CEF_VERSION`.
/// Variant is `_linux64_minimal` (drops the C++ wrapper static lib and the
/// example binaries we don't ship).
fn tarball_url() -> String {
    // Spotify URL-encodes the '+' in the version as '%2B'.
    let encoded = CEF_VERSION.replace('+', "%2B");
    format!("https://cef-builds.spotifycdn.com/cef_binary_{encoded}_linux64_minimal.tar.bz2")
}

/// Ensure CEF is present at `cef_path()`. If not, download and extract.
/// Idempotent — short-circuits when libcef.so exists.
pub fn ensure_cef() -> io::Result<PathBuf> {
    let dir = cef_path();
    if is_present() {
        return Ok(dir);
    }
    eprintln!("[cef] not found at {} — downloading {}", dir.display(), CEF_VERSION);
    download_and_extract(&dir)?;
    if !is_present() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("CEF download completed but libcef.so missing under {}", dir.display()),
        ));
    }
    eprintln!("[cef] installed to {}", dir.display());
    Ok(dir)
}

fn download_and_extract(dir: &Path) -> io::Result<()> {
    let parent = dir.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cef_path has no parent"))?;
    fs::create_dir_all(parent)?;

    let url = tarball_url();
    eprintln!("[cef] GET {url}");
    let response = ureq::get(&url)
        .call()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("download failed: {e}")))?;
    let reader = response.into_reader();
    let bz2 = bzip2::read::BzDecoder::new(reader);
    let mut archive = tar::Archive::new(bz2);

    // The tarball contains a top-level dir like `cef_binary_<ver>_linux64_minimal/`.
    // We want its contents, not the dir itself, placed at `dir/`. Easiest:
    // extract to a tmp staging directory, then rename the inner directory.
    let staging = parent.join(format!(".cef-staging-{}", std::process::id()));
    if staging.exists() { fs::remove_dir_all(&staging)?; }
    fs::create_dir_all(&staging)?;

    archive.unpack(&staging)?;

    // Find the single top-level directory inside staging.
    let inner = fs::read_dir(&staging)?
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no top-level dir in CEF tarball"))?;

    fs::rename(inner.path(), dir)?;
    fs::remove_dir_all(&staging)?;
    Ok(())
}

use std::path::Path;
