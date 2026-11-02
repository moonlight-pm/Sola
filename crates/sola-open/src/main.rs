//! `xdg-open`: Sola's opener under the name every program calls.

fn main() -> std::process::ExitCode {
    sola_open::cli::run()
}
