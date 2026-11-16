//! What a build of `sola-signal` needs beside cargo: libsignal's build
//! scripts run `protoc` and bindgen.
//!
//! `protoc` is ours (`crates/sola-protoc`, on protox): built first, copied to
//! `target/tools/` and put on PATH, because rust-protobuf finds it there and
//! prost-build by `PROTOC`. bindgen loads libclang; on Oath that is
//! `pkg:libclang` (Oath R91), a library with no `clang` beside it, so bindgen
//! is told where it and the C headers are.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

const CRATE: &str = "sola-signal";
const TOOLS: &str = "target/tools";
const OATH_LIB: &str = "/lib";
const OATH_LIBC_HEADERS: &str = "/oath/store/pkg/glibc/live/include";

pub type Vars = Vec<(&'static str, OsString)>;

/// The environment for a cargo build of `packages` (none: the workspace).
/// Empty when the build does not reach `sola-signal`.
pub fn vars_or_exit(packages: &[String]) -> Vars {
    if !packages.is_empty() && !packages.iter().any(|p| p == CRATE) {
        return Vec::new();
    }
    let protoc = protoc().unwrap_or_else(|e| {
        eprintln!("error: protoc for {CRATE}: {e}");
        exit(1);
    });
    let mut vars: Vars = vec![("PATH", path_with(protoc.parent().unwrap())), ("PROTOC", protoc.into())];
    if crate::oath::on_oath() {
        vars.extend(bindgen_on_oath());
    }
    vars.retain(|(key, _)| *key == "PATH" || std::env::var_os(key).is_none());
    vars
}

/// Build `sola-protoc` and place its `protoc` alone in a directory.
fn protoc() -> Result<PathBuf, String> {
    let status = Command::new("cargo")
        .args(["build", "--release", "-q", "-p", "sola-protoc"])
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err("sola-protoc did not build".into());
    }
    let tools = std::env::current_dir().map_err(|e| e.to_string())?.join(TOOLS);
    std::fs::create_dir_all(&tools).map_err(|e| format!("{}: {e}", tools.display()))?;
    let placed = tools.join("protoc");
    // Remove first: a build script may still be running the old one.
    let _ = std::fs::remove_file(&placed);
    std::fs::copy("target/release/protoc", &placed).map_err(|e| format!("{}: {e}", placed.display()))?;
    Ok(placed)
}

fn path_with(dir: &Path) -> OsString {
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&rest)))
        .unwrap_or(rest)
}

fn bindgen_on_oath() -> Vars {
    // `/lib/clang/<major>` holds clang's own headers (stddef.h, stdarg.h).
    let resource = std::fs::read_dir(Path::new(OATH_LIB).join("clang"))
        .ok()
        .and_then(|dir| dir.flatten().map(|e| e.path()).max());
    let Some(resource) = resource else {
        eprintln!("warning: no {OATH_LIB}/clang: is pkg:libclang applied? bindgen will fail");
        return Vec::new();
    };
    let args = format!("-resource-dir={} -idirafter {OATH_LIBC_HEADERS}", resource.display());
    vec![("LIBCLANG_PATH", OATH_LIB.into()), ("BINDGEN_EXTRA_CLANG_ARGS", args.into())]
}
