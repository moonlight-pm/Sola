//! sola-wrapper — websites as first-class Sola apps.
//!
//! Identity is `sola-wrapper <id>`. The URL lives on the Application
//! catalog. CEF subprocesses and `--engine` helpers are the same binary.

mod app;
mod argv;
mod catalog;
mod instance;
mod links;
mod menu;
mod profile;

use std::process::ExitCode;

use sola_browser::{CefEngine, Engine};

fn main() -> ExitCode {
    // CEF renderer / GPU / utility workers — before log, Wayland, or iced.
    if let Some(code) = CefEngine::dispatch_subprocess("sola-wrapper") {
        return code;
    }

    match argv::parse(std::env::args().skip(1)) {
        Ok(argv::Args::Engine { id }) => run_engine(&id),
        Ok(argv::Args::Chrome { id }) => run_chrome(&id),
        Err(e) => {
            eprintln!("sola-wrapper: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_engine(id: &str) -> ExitCode {
    let app_id = leak(id);
    sola_core::log::init(app_id);
    if let Err(e) = profile::bind(id) {
        eprintln!("sola-wrapper: bind profile: {e}");
        return ExitCode::FAILURE;
    }
    // `try_run` also calls `log::init` — must stay idempotent (helper panic
    // used to kill CEF before the first paint; black window).
    sola_browser::cef::host::try_run(app_id).unwrap_or(ExitCode::FAILURE)
}

fn run_chrome(id: &str) -> ExitCode {
    let spec = match catalog::lookup(id) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("sola-wrapper: {e}");
            return ExitCode::FAILURE;
        }
    };

    match instance::claim(id) {
        instance::Claim::Handoff => {
            return match instance::handoff(id) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("sola-wrapper: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        instance::Claim::Primary => {}
    }

    let app_id = leak(id);
    sola_kit::app::startup(app_id);
    if let Err(e) = profile::bind(id) {
        tracing::error!(error = %e, "bind wrapper profile");
        return ExitCode::FAILURE;
    }
    match app::run(app_id, spec) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "iced");
            ExitCode::FAILURE
        }
    }
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}
