//! `solactl open <target>` — URL → sola-browser, image path → sola-paint.
//!
//! Used as the desktop http/https handler when MIME defaults point at
//! `sola-browser.desktop`, as the image handler via `sola-paint.desktop`,
//! and as a CLI for scripts.

use sola_core::{open_image, open_url};

pub fn run(target: &str) -> i32 {
    if target.is_empty() {
        eprintln!("solactl: open requires a URL or image path");
        return 3;
    }
    let result = if open_image::looks_like_image(target) {
        open_image::open(target)
    } else {
        open_url::open(target)
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("solactl: open failed: {e}");
            3
        }
    }
}
