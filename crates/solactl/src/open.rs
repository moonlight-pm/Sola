//! `solactl open <URL>` — open a URL in Helium (system browser).
//!
//! Used as the desktop http/https handler when MIME defaults point at a
//! `.desktop` that execs `solactl open`, and as a CLI for scripts. Does
//! **not** emit `Topic::OpenUrl` (sola-browser is not the day-to-day
//! browser); sola-shell also handles bus `OpenUrl` → Helium for in-Sola
//! emitters.

use sola_core::open_url;

pub fn run(url: &str) -> i32 {
    if url.is_empty() {
        eprintln!("solactl: open requires a URL");
        return 3;
    }
    match open_url::open(url) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("solactl: open failed: {e}");
            3
        }
    }
}
