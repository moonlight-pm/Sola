//! sola-browser — CEF engine + iced chrome.
use sola_browser::CefEngine;

fn main() -> std::process::ExitCode {
    sola_browser::run::<CefEngine>("sola-browser")
}
