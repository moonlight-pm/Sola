use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Sola desktop shell — a Wayland compositor with WebView-based UI.
///
/// This is the main entry point. It configures logging and delegates
/// to `sola_compositor::run()` for the actual compositor lifecycle.
#[derive(Parser)]
#[command(name = "sola", about = "Sola desktop shell")]
struct Cli {}

fn main() -> anyhow::Result<()> {
    // Log to both stderr (for TTY/SSH visibility) AND a persistent file
    // at /opt/sola/log/sola.log (so logs survive after the process exits).
    // Override filter at runtime: RUST_LOG=debug /opt/sola/bin/sola
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            // Default: show info from sola crates, error-only from smithay.
            // Smithay is chatty at info (device discovery, EGL extensions) and
            // logs harmless warnings/errors on startup and shutdown (e.g.,
            // "Failed to restore previous state" during DrmDevice drop).
            // Use RUST_LOG=smithay=debug to see smithay internals when needed.
            "sola=info,sola_compositor=info,smithay=error".into()
        });

    // File appender — writes to /opt/sola/log/sola.log, non-rotating.
    // Creates the directory if needed. Falls back gracefully if it can't write.
    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

    // Two output layers: stderr (with ANSI colors) + file (plain text).
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    let _cli = Cli::parse();

    tracing::info!("sola starting (logs → stderr + {log_dir}/sola.log)");
    sola_compositor::run()
}
