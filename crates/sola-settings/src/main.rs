mod app;
mod procfs;

use std::process::ExitCode;

use sola_kit::SolaApp;

fn main() -> ExitCode {
    // Subprocess gate — CEF re-execs this binary as renderer/GPU/util/zygote.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess(app::SettingsApp::APP_ID) {
        return code;
    }
    sola_kit::run::<app::SettingsApp>();
    ExitCode::SUCCESS
}
