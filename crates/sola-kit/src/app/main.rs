mod app;
mod catalog;
mod fonts;

use std::process::ExitCode;

use sola_kit::SolaApp;

fn main() -> ExitCode {
    // Subprocess gate. CEF re-execs this binary as the renderer, GPU,
    // utility, and zygote workers — `--type=...` argv distinguishes them.
    // `cef::execute_process` runs the worker's main loop (never returns
    // for workers); the main browser process falls through with `None`.
    // The app_id flows into Chromium's command line so the secondary
    // windows CEF creates (DevTools etc.) report the same Wayland
    // xdg_toplevel.app_id as the primary surface.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess(app::KitApp::APP_ID) {
        return code;
    }

    sola_kit::run::<app::KitApp>();
    ExitCode::SUCCESS
}
