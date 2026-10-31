//! A computer's Projects sessions, as its tmux tells them: any computer
//! can list them and open one (sessions anywhere, 2026-10-30). The listing
//! is shared with the launcher's chooser ([`sola_core::project_sessions`]);
//! here a session is stamped with what it is when Projects makes it.

use std::path::Path;

pub use sola_core::project_sessions::{HostSession, list_local, list_remote};
use sola_core::project_sessions::{KIND, NAME, PROJECT, TITLE};

/// The tmux options a new session carries (see the module docs).
pub fn options(project: &Path, name: &str, title: Option<&str>, kind: &str) -> Vec<(String, String)> {
    vec![
        (PROJECT.into(), project.to_string_lossy().into_owned()),
        (NAME.into(), name.into()),
        (TITLE.into(), title.unwrap_or("").into()),
        (KIND.into(), kind.into()),
    ]
}

/// Set `opts` on local session `session`. `set-option` takes a pane
/// target, so the exact session is `=name:`.
pub fn stamp(session: &str, opts: &[(String, String)]) {
    let target = format!("={session}:");
    let mut cmd = sola_terminal::tmux::tmux_cmd();
    for (i, (k, v)) in opts.iter().enumerate() {
        if i > 0 {
            cmd.arg(";");
        }
        cmd.args(["set-option", "-t", &target, k, v]);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => tracing::warn!(%session, "stamping session options: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => tracing::warn!(%session, "stamping session options: {e}"),
    }
}
