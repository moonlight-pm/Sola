//! Open iCalendar files in **sola-calendar** (default `text/calendar` app).
//!
//! Surfaces that open a `.ics` path should call [`open`] so behaviour
//! stays consistent:
//!
//! - mail attachment **Open**
//! - `solactl open` (xdg-open / MIME path via `sola-calendar.desktop`)
//!
//! Spawns `paths::bin_path("sola-calendar") <path>` detached. A running
//! calendar keeps its window: the new process hands the path over and
//! exits. Calendar drafts the event; you pick the calendar and save.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::open_image::strip_file_url;

/// Extensions sola-calendar opens.
pub const CALENDAR_EXTENSIONS: &[&str] = &["ics", "ical", "icalendar"];

/// True when `target` is a filesystem path (or `file://` URL) whose
/// extension is an iCalendar file.
pub fn looks_like_calendar(target: &str) -> bool {
    let path = strip_file_url(target);
    if path.is_empty() || path.contains("://") {
        return false;
    }
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            CALENDAR_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Open `path` (or a `file://` URL) in sola-calendar. Spawns detached.
pub fn open(path: &str) -> Result<(), String> {
    let path = strip_file_url(path);
    if path.trim().is_empty() {
        return Err("empty calendar file path".into());
    }
    let bin = crate::paths::bin_path("sola-calendar");
    if !bin.is_file() {
        return Err(format!("sola-calendar not found at {}", bin.display()));
    }
    Command::new(&bin)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", bin.display()))?;
    Ok(())
}

/// Best-effort open: log failures, never panic.
pub fn open_logged(path: &str) {
    match open(path) {
        Ok(()) => tracing::info!(%path, "opened calendar file in sola-calendar"),
        Err(e) => tracing::warn!(%path, error = %e, "failed to open calendar file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_calendar_paths() {
        assert!(looks_like_calendar("/tmp/sola-mail/1-2-invite.ics"));
        assert!(looks_like_calendar("file:///home/me/Trip.ICS"));
        assert!(!looks_like_calendar("https://example.com/feed.ics"));
        assert!(!looks_like_calendar("/tmp/notes.txt"));
    }
}
