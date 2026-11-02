//! `x-www-browser`: Sola's opener under the one name Rust's `webbrowser` crate
//! would try here. That crate (Grok's) never calls `xdg-open`: it reads
//! `$BROWSER`, asks `xdg-settings`, tries a known desktop's tools, then this.

fn main() -> std::process::ExitCode {
    sola_open::cli::run()
}
