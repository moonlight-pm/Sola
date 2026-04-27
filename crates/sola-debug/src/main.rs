//! sola-debug — runtime introspection CLI for Sola apps.
//!
//! Subcommands:
//!   apps        — list running apps and their windows
//!   eval        — evaluate a JS expression in a WebView, print the result
//!   logs        — tail an app's log file
//!   screenshot  — capture the compositor output to a PNG
//!
//! All subcommands print JSON or plain text to stdout. Exit codes:
//!   0 — success
//!   1 — remote error returned by the responder
//!   2 — timeout waiting for a response
//!   3 — local error (bus connect, missing app, IO, etc.)

use clap::{Parser, Subcommand};

mod apps;
mod bus;
mod eval;
mod logs;
mod screenshot;

#[derive(Parser, Debug)]
#[command(name = "sola-debug", about = "Runtime introspection for Sola apps")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List running apps and their windows.
    Apps,

    /// Evaluate a JavaScript expression in a Sola app's WebView and print
    /// the JSON-encoded result. The expression is awaited (Promises are
    /// resolved) and JSON-serialized; non-serializable values like DOM
    /// elements collapse to `{}`, so wrap them in a projection like
    /// `{tag: el.tagName, rect: el.getBoundingClientRect()}` before
    /// returning.
    Eval {
        /// Target app id (e.g. `sola-shell`).
        app: String,
        /// JS expression to evaluate.
        expression: String,
        /// Window title within the app. Defaults to the first window.
        #[arg(short, long)]
        window: Option<String>,
        /// Timeout in seconds.
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },

    /// Tail an app's log file at `/opt/sola/log/<app>.log`.
    Logs {
        /// App id; if omitted, tails the supervisor's `sola.log`.
        app: Option<String>,
        /// Follow new lines (like `tail -f`).
        #[arg(short, long)]
        follow: bool,
    },

    /// Capture the compositor output to a PNG.
    Screenshot {
        /// Path to write the PNG. Defaults to `/tmp/sola/screenshots/<unix-ms>.png`.
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        /// Timeout in seconds.
        #[arg(short, long, default_value_t = 10)]
        timeout: u64,
    },
}

fn main() {
    let cli = Cli::parse();
    let exit = match cli.command {
        Command::Apps => apps::run(),
        Command::Eval {
            app,
            expression,
            window,
            timeout,
        } => eval::run(&app, window.as_deref(), &expression, timeout),
        Command::Logs { app, follow } => logs::run(app.as_deref(), follow),
        Command::Screenshot { output, timeout } => screenshot::run(output.as_deref(), timeout),
    };
    std::process::exit(exit);
}
