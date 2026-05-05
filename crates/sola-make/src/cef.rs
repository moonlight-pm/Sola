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
/// Currently unused inside this build crate — `crates/sola-kit/src/cef/distribution.rs`
/// reads the same path from the `SOLA_KIT_CEF_DIR` env var at runtime — but it
/// rounds out the cache-path API alongside `release_dir` and is cheap to keep.
#[allow(dead_code)]
pub fn resources_dir() -> PathBuf {
    cef_path().join("Resources")
}

/// Path to the `Resources/locales/` subdirectory.
/// Same status as `resources_dir`: round-out of the path API; the runtime
/// resolves locales from the env var.
#[allow(dead_code)]
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

/// Ensure CEF is present at `cef_path()`. If not, download, extract,
/// patch libcef.so for NixOS, and symlink Resources/* next to
/// libcef.so. Idempotent — short-circuits when libcef.so already
/// exists (assumes the patch + symlink steps have run if so).
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
    patch_cef_libs_for_nix_ld(&release_dir())?;
    expose_resources_next_to_libcef(&release_dir(), &resources_dir())?;
    eprintln!("[cef] installed to {}", dir.display());
    Ok(dir)
}

/// Stable NixOS path that nix-ld populates from `programs.nix-ld.libraries`.
/// Adding this to libcef.so's RUNPATH lets the dynamic linker resolve CEF's
/// transitive system deps (glib, nss, atk, dbus, cups, X11/X*, gbm, expat,
/// xkbcommon, cairo, pango, udev, alsa, atspi, …) without LD_LIBRARY_PATH
/// or any wrapper script. See `docs/vault/Distribution.md` →
/// "CEF runtime libraries (sola-kit)".
const NIX_LD_LIB_DIR: &str = "/run/current-system/sw/share/nix-ld/lib";

/// Symlink each top-level entry from `Resources/` into `Release/`.
/// Recent CEF / Chromium versions look up `icudtl.dat`, `*.pak`, and
/// `locales/` relative to `libcef.so` (DIR_MODULE) rather than honoring
/// `Settings::resources_dir_path` for every consumer — notably the
/// subprocess ICU init, which crashes with `Invalid file descriptor to
/// ICU data received` otherwise. Symlinks keep the original tarball
/// layout intact while exposing the data next to the engine.
///
/// Idempotent: skips entries that already exist (e.g. on a re-extraction
/// where libcef.so happened to be present).
fn expose_resources_next_to_libcef(release: &Path, resources: &Path) -> io::Result<()> {
    if !resources.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Resources dir missing at {}", resources.display()),
        ));
    }
    for entry in fs::read_dir(resources)? {
        let entry = entry?;
        let name = entry.file_name();
        let dest = release.join(&name);
        if dest.exists() || dest.is_symlink() {
            continue;
        }
        // Use a relative target — `../Resources/<name>` — so the link
        // survives if the cache directory is moved (the relative
        // structure of Release/ next to Resources/ is invariant).
        let rel_target = PathBuf::from("..").join("Resources").join(&name);
        std::os::unix::fs::symlink(&rel_target, &dest)?;
    }
    eprintln!("[cef] symlinked Resources/* into {} for DIR_MODULE lookup", release.display());
    Ok(())
}

/// Append `NIX_LD_LIB_DIR` to the RUNPATH of every CEF shared library
/// in `release/`. Just patching `libcef.so` is not enough: ANGLE's
/// `dlopen("libGL.so.1")` from inside `libEGL.so` consults libEGL.so's
/// own runpath (not libcef.so's), and libEGL.so / libGLESv2.so /
/// libvk_swiftshader.so / libvulkan.so.1 all ship with no runpath at
/// all. We add `$ORIGIN` so they find each other AND `NIX_LD_LIB_DIR`
/// so dlopened system libs (libGL, libudev, libX*, …) resolve.
///
/// No-op on non-NixOS hosts (the path won't exist, but that's harmless —
/// the dynamic linker silently skips missing RUNPATH entries).
fn patch_cef_libs_for_nix_ld(release: &Path) -> io::Result<()> {
    let mut patched = Vec::new();
    for entry in fs::read_dir(release)? {
        let entry = entry?;
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // Match libcef.so, libEGL.so, libGLESv2.so, libvk_swiftshader.so, libvulkan.so.1.
        let is_cef_so = name.starts_with("lib") && (name.ends_with(".so") || name.contains(".so."));
        if !is_cef_so {
            continue;
        }
        // `--add-rpath $ORIGIN:NIX_LD_LIB_DIR` keeps each library's
        // existing runpath (if any — `$ORIGIN` is already there for
        // libcef.so) and appends both. Idempotency on second runs is
        // handled by `is_present()` short-circuiting `ensure_cef`.
        let new_path = format!("$ORIGIN:{NIX_LD_LIB_DIR}");
        let status = std::process::Command::new("patchelf")
            .args(["--add-rpath", &new_path])
            .arg(&path)
            .status()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("patchelf not on PATH: {e}")))?;
        if !status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("patchelf failed on {}: exit {status}", path.display()),
            ));
        }
        patched.push(name.to_string());
    }
    eprintln!("[cef] patched RUNPATH on {} libs in {}: {}",
        patched.len(), release.display(), patched.join(", "));
    Ok(())
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
