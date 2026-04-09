use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Sola desktop shell — a Wayland compositor with WebView-based UI.
#[derive(Parser)]
#[command(name = "sola", about = "Sola desktop shell")]
struct Cli {}

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            // Default: show info from sola crates, error-only from smithay.
            "sola=info,sola_compositor=info,smithay=error".into()
        });

    let log_dir = "/opt/sola/log";
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::never(log_dir, "sola.log");

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

    if let Err(err) = sola_compositor::run() {
        tracing::error!(%err, "compositor exited with error");
        std::process::exit(1);
    }
}
