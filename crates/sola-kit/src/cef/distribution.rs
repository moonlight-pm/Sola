//! Runtime CEF path resolution. Mirrors what `crates/sola-make/src/cef.rs`
//! resolves at build time, but reads from the env var that build.rs
//! embedded so the binary doesn't need to recompute it.

use std::path::PathBuf;

pub fn cef_dir() -> PathBuf {
    PathBuf::from(env!("SOLA_KIT_CEF_DIR"))
}

pub fn release_dir() -> PathBuf {
    cef_dir().join("Release")
}

pub fn resources_dir() -> PathBuf {
    cef_dir().join("Resources")
}

pub fn locales_dir() -> PathBuf {
    resources_dir().join("locales")
}
