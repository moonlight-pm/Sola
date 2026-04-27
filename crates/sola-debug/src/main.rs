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
mod emit;
mod eval;
mod input;
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

    /// Emit a bus topic with a JSON payload. The payload is deserialized
    /// via the topic's payload type — same shape as `sola-monitor` shows.
    /// Unit topics (no payload) accept any value, including `null` or `{}`.
    ///
    /// Examples:
    ///   sola-debug emit Shutdown null
    ///   sola-debug emit LaunchApp '{"app_id":"foo","command":"/opt/sola/bin/foo"}'
    ///   sola-debug emit Frame '{"window_id":1,"x":0,"y":0,"width":800,"height":600}'
    Emit {
        /// Topic kind name (e.g. `Shutdown`, `Frame`, `LaunchApp`). Same
        /// names as the `Topic` variants in sola-bus.
        kind: String,
        /// JSON payload. Use `null` or `{}` for unit topics.
        payload: String,
    },

    /// Move the pointer and click at absolute output coordinates.
    Click {
        x: i32,
        y: i32,
        /// Mouse button: left (default), right, middle.
        #[arg(short, long, default_value = "left")]
        button: String,
    },

    /// Move the pointer to absolute output coordinates.
    Move { x: i32, y: i32 },

    /// Scroll the pointer wheel. Positive `dy` = scroll down.
    Scroll {
        #[arg(short = 'x', long, default_value_t = 0.0)]
        dx: f64,
        #[arg(short = 'y', long, default_value_t = 5.0)]
        dy: f64,
    },

    /// Synthesize a single keystroke. Chord syntax: `Meta+Tab`, `Ctrl+A`,
    /// `Shift+Esc`, `Escape`, `Tab`, single letters/digits, etc.
    Key {
        /// Key chord, e.g. "Meta+Tab" or "A".
        chord: String,
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
        Command::Emit { kind, payload } => emit::run(&kind, &payload),
        Command::Click { x, y, button } => input::click(x, y, &button),
        Command::Move { x, y } => input::move_to(x, y),
        Command::Scroll { dx, dy } => input::scroll(dx, dy),
        Command::Key { chord } => input::key(&chord),
        Command::Screenshot { output, timeout } => screenshot::run(output.as_deref(), timeout),
    };
    std::process::exit(exit);
}
