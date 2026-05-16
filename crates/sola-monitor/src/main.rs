mod app;
mod decode;

use std::process::ExitCode;

use sola_kit::SolaApp;

fn main() -> ExitCode {
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess(app::MonitorApp::APP_ID) {
        return code;
    }
    sola_kit::run::<app::MonitorApp>();
    ExitCode::SUCCESS
}
