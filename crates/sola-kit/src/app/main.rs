mod app;
mod catalog;
mod fonts;

use std::process::ExitCode;

fn main() -> ExitCode {
    // Subprocess gate. CEF re-execs this binary as the renderer, GPU,
    // utility, and zygote workers — `--type=...` argv distinguishes them.
    // `cef::execute_process` runs the worker's main loop (never returns
    // for workers); the main browser process falls through with `None`.
    if let Some(code) = sola_kit::cef::short_circuit_if_subprocess() {
        return code;
    }

    sola_kit::run::<app::KitApp>();
    ExitCode::SUCCESS
}
