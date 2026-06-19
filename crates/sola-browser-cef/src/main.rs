//! sola-browser-cef — CEF engine over the shared sola-browser-core chrome.
use sola_browser_cef::engine::CefEngine;

fn main() -> std::process::ExitCode {
    sola_browser_core::run::<CefEngine>("sola-browser")
}
