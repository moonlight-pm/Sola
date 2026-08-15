//! `solactl open <URL>` — open a URL in sola-browser.
//!
//! Used as the desktop http/https handler when MIME defaults point at
//! `sola-browser.desktop` (or a `.desktop` that execs `solactl open`), and
//! as a CLI for scripts. Spawns sola-browser with the URL (same path as
//! terminal/mail link clicks and shell's bus `OpenUrl` handler).

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
