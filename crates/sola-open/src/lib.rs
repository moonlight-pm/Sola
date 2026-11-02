//! Opening links and files: the handlers Sola keeps
//! (`docs/specs/2026-10-31-sola-open-design.md`).
//!
//! Apps claim link schemes and file types ([`claims`]); you choose among
//! them ([`defaults`]); [`resolve`] picks the handler and the target goes to
//! it — a Sola app through its `open` method, a `.desktop` handler by its
//! command. `/bin/xdg-open` and `/bin/x-www-browser` (this crate's
//! binaries), `solactl open` and
//! `sola_kit::open` all come here.

pub mod claims;
pub mod cli;
pub mod defaults;
mod deliver;
pub mod desktop;
pub mod resolve;
pub mod target;

use std::path::PathBuf;

pub use deliver::{OPEN_METHOD, open_method, to_app};
pub use target::{Kind, Target};

#[derive(Debug)]
pub enum OpenError {
    /// Not a link or a path.
    Usage(String),
    /// A file that is not there.
    NotFound(PathBuf),
    /// Nothing opens it.
    NoHandler(String),
    /// Its handler did not open it.
    Failed(String),
}

impl OpenError {
    /// xdg-open's exit status: 1 usage, 2 no such file, 3 nothing opens it,
    /// 4 the handler failed.
    pub fn exit_code(&self) -> i32 {
        match self {
            OpenError::Usage(_) => 1,
            OpenError::NotFound(_) => 2,
            OpenError::NoHandler(_) => 3,
            OpenError::Failed(_) => 4,
        }
    }
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Usage(s) => write!(f, "{s}"),
            OpenError::NotFound(p) => write!(f, "{}: no such file", p.display()),
            OpenError::NoHandler(what) => write!(f, "nothing opens {what}"),
            OpenError::Failed(s) => write!(f, "{s}"),
        }
    }
}

/// Open `raw` (a link, or a path taken from this process's working
/// directory) with its handler. Blocks until the handler has it, which can
/// take the time an app needs to start: call it off a UI thread.
pub fn open(raw: &str) -> Result<(), OpenError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let target = Target::parse(raw, &cwd).ok_or_else(|| OpenError::Usage("nothing to open".into()))?;
    if let Target::File { path, .. } = &target {
        if !path.exists() {
            return Err(OpenError::NotFound(path.clone()));
        }
    }
    let kind = target
        .kind()
        .ok_or_else(|| OpenError::NoHandler(format!("{} (a file with no type)", target.as_arg())))?;
    let handler = resolve::resolve(&kind, &resolve::Sources::live())
        .ok_or_else(|| OpenError::NoHandler(kind.to_string()))?;
    tracing::info!(target = %target.as_arg(), ?kind, ?handler, "open");
    match handler {
        resolve::Handler::App(app_id) => deliver::to_app(&app_id, &target.as_arg()),
        resolve::Handler::Desktop(entry) => deliver::to_desktop(&entry, &target.as_arg()),
    }
}

/// [`open`] on a thread of its own, its failure logged: for UI handlers.
pub fn open_detached(raw: &str) {
    let raw = raw.to_string();
    std::thread::spawn(move || {
        if let Err(e) = open(&raw) {
            tracing::warn!(target = %raw, error = %e, "open failed");
        }
    });
}
