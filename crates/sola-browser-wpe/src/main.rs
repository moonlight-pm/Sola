//! sola-browser-wpe — WPE engine over the shared sola-browser-core chrome.
use sola_browser_wpe::engine::WpeEngine;

fn main() -> std::process::ExitCode {
    sola_browser_core::run::<WpeEngine>("sola-browser")
}
