mod app;
mod keys;
mod launcher;
mod menu;
mod menubar;
mod switcher;
pub mod theme;
mod zoning;

use std::process::ExitCode;

use sola_kit::SolaApp;

fn main() -> ExitCode {
    // Subprocess gate — CEF re-execs this binary as renderer/GPU/util/zygote.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess(app::ShellApp::APP_ID) {
        return code;
    }
    sola_kit::run::<app::ShellApp>();
    ExitCode::SUCCESS
}
