const X_UNIX_DIR: &str = "/tmp/.X11-unix";

/// Read the wayland socket name that sola-river published to
/// `$XDG_RUNTIME_DIR/sola-wayland`. Returns `None` if the file isn't
/// there yet (sola-river still starting, or not running).
pub fn wayland_socket() -> Option<String> {
    read_runtime_name("sola-wayland")
}

/// Resolve the X11 display user apps should target. Prefers the value
/// sola-river published to `$XDG_RUNTIME_DIR/sola-display`; if absent
/// (XWayland started lazily or our startup probe missed it), falls back
/// to a live probe of `/tmp/.X11-unix/X*` at the time of the call.
pub fn x_display() -> Option<String> {
    if let Some(name) = read_runtime_name("sola-display") {
        return Some(name);
    }
    probe_live_x_display()
}

fn read_runtime_name(file: &str) -> Option<String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let path = std::path::Path::new(&runtime_dir).join(file);
    let raw = std::fs::read_to_string(&path).ok()?;
    let name = raw.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn probe_live_x_display() -> Option<String> {
    use std::path::PathBuf;
    let dir = std::fs::read_dir(X_UNIX_DIR).ok()?;
    let mut candidates: Vec<(u32, PathBuf)> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            let n = name.strip_prefix('X')?.parse::<u32>().ok()?;
            Some((n, e.path()))
        })
        .collect();
    candidates.sort_by_key(|(n, _)| *n);
    for (n, path) in candidates {
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Some(format!(":{n}"));
        }
    }
    None
}
