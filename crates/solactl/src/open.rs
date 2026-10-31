//! `solactl open <target>` — a link or a file to the app Sola keeps for its
//! scheme or type ([`sola_open`]), as `/bin/xdg-open` does: the Browser for
//! the web, Paint for images, Calendar for `.ics`, Mail for `mailto:`, a
//! `.desktop` handler for a scheme no Sola app claims.

pub fn run(target: &str) -> i32 {
    if target.is_empty() {
        eprintln!("solactl: open requires a URL or a file path");
        return 3;
    }
    match sola_open::open(target) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("solactl: open failed: {e}");
            3
        }
    }
}
