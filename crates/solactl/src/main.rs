//! solactl — control + introspection CLI for Sola.
//!
//! Subcommands:
//!   apps        — list running apps and their windows
//!   eval        — evaluate a JS expression in a WebView, print the result
//!   logs        — tail an app's log file
//!   emit        — emit any bus topic with a JSON payload
//!   open        — open a URL in Helium (system browser / xdg-open path)
//!   click/move/scroll/key — synthesized input
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
mod media;
mod open;
mod screenshot;

#[derive(Parser, Debug)]
#[command(name = "solactl", about = "Sola control + introspection CLI")]
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
    ///   solactl emit Shutdown null
    ///   solactl emit LaunchApp '{"app_id":"foo","command":"/opt/sola/bin/foo"}'
    ///   solactl emit Frame '{"window_id":1,"x":0,"y":0,"width":800,"height":600}'
    Emit {
        /// Topic kind name (e.g. `Shutdown`, `Frame`, `LaunchApp`). Same
        /// names as the `Topic` variants in sola-bus.
        kind: String,
        /// JSON payload. Use `null` or `{}` for unit topics.
        payload: String,
    },

    /// Open a URL in sola-browser. Activates the new tab. Used as the
    /// http/https scheme handler from `sola-browser.desktop`, so any
    /// xdg-open / GIO / `BROWSER=solactl open` caller routes here.
    Open {
        /// URL to open.
        url: String,
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

    /// Global media-key action: control the active MPRIS player
    /// (play-pause/next/prev) or the default audio sink (mute/vol-up/
    /// vol-down). Invoked per keypress by sola-shell when an XF86Audio*
    /// chord fires; focus-independent.
    Media {
        /// Action to perform.
        #[arg(value_enum)]
        action: media::MediaAction,
    },

    /// Synthesize a single keystroke. Chord syntax: `Meta+Tab`, `Ctrl+A`,
    /// `Shift+Esc`, `Escape`, `Tab`, single letters/digits, etc.
    Key {
        /// Key chord, e.g. "Meta+Tab" or "A".
        chord: String,
    },

    /// Capture the compositor output to a PNG. With `--app`, captures
    /// only the region currently occupied by that app's window. The
    /// region capture takes whatever is visually at those screen
    /// coordinates — if another window overlaps the target you'll see
    /// the overlap; raise the window first if you want a clean shot.
    Screenshot {
        /// Path to write the PNG. Defaults to `/tmp/sola/screenshots/<unix-ms>.png`.
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        /// Capture only this app's window region.
        #[arg(short, long)]
        app: Option<String>,
        /// Window title within the app. Defaults to the first window.
        #[arg(short, long)]
        window: Option<String>,
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
        Command::Open { url } => open::run(&url),
        Command::Media { action } => media::run(action),
        Command::Click { x, y, button } => input::click(x, y, &button),
        Command::Move { x, y } => input::move_to(x, y),
        Command::Scroll { dx, dy } => input::scroll(dx, dy),
        Command::Key { chord } => input::key(&chord),
        Command::Screenshot {
            output,
            app,
            window,
            timeout,
        } => screenshot::run(output.as_deref(), app.as_deref(), window.as_deref(), timeout),
    };
    std::process::exit(exit);
}
