//! sola-browser — WPE WebKit engine over the shared sola-browser-core chrome.
use sola_browser::engine::WpeEngine;

fn main() -> std::process::ExitCode {
    sola_browser_core::run::<WpeEngine>("sola-browser")
}
