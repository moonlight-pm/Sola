//! `solactl open <URL>` — emit `Topic::OpenUrl` so `sola-browser`
//! creates a new tab.
//!
//! Wired into the desktop as the default http/https handler via
//! `crates/sola-browser/dist/applications/sola-browser.desktop`. Stays
//! silent on success so it can be invoked from `xdg-open` without
//! polluting the caller's stdout.

use sola_bus::topics::{OpenUrlRequest, Topic};

use crate::bus;

pub fn run(url: &str) -> i32 {
    if url.is_empty() {
        eprintln!("solactl: open requires a URL");
        return 3;
    }
    let mut client = bus::connect_or_exit();
    bus::emit(
        &mut client,
        Topic::OpenUrl(OpenUrlRequest {
            url: url.to_string(),
            activate: true,
        }),
    );
    // Bus writes are async — give the writer thread a moment to flush
    // before we exit, otherwise the message may never reach the bus.
    std::thread::sleep(std::time::Duration::from_millis(50));
    0
}
