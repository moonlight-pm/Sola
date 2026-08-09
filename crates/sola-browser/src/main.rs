//! sola-browser — WPE WebKit + iced chrome.
use sola_browser::WpeEngine;

fn main() -> std::process::ExitCode {
    sola_browser::run::<WpeEngine>("sola-browser")
}
