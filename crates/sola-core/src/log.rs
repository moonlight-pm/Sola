//! Shared tracing setup for every Sola binary.
//!
//! All logs — process manager, bus, session, river, app — land in
//! `/opt/sola/log/sola.log` plus stderr. Each binary just calls
//! `sola_core::log::init("<name>")` at startup.

use std::fs::OpenOptions;
use std::path::Path;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Directory on disk where all Sola logs are written.
pub const LOG_DIR: &str = "/opt/sola/log";

/// Single log file shared by every Sola binary.
pub const LOG_FILE: &str = "sola.log";

/// Initialize tracing for a Sola binary.
///
/// * Creates `/opt/sola/log` if missing.
/// * Writes to `/opt/sola/log/sola.log` (append, no rotation) and stderr.
/// * Falls back to stderr-only if the log file can't be opened (e.g. in
///   tests or local dev where /opt/sola/log may not be writable).
/// * Default `RUST_LOG` filter: `<name_with_underscores>=info`.
/// * Installs a panic hook that routes panics through tracing so they
///   end up in the log file, not just on stderr.
pub fn init(name: &str) {
    let default_filter = format!("{}=info", name.replace('-', "_"));
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into());

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let file_layer = open_file_layer();

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    let name = name.to_string();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(%info, "{name} panicked\n{backtrace}");
    }));
}

/// Try to open the shared log file as a non-rotating file appender.
/// Returns `None` if the directory can't be created or the file can't be
/// opened — callers should still emit to stderr in that case.
fn open_file_layer<S>() -> Option<tracing_subscriber::fmt::Layer<S, tracing_subscriber::fmt::format::DefaultFields, tracing_subscriber::fmt::format::Format, tracing_appender::rolling::RollingFileAppender>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let _ = std::fs::create_dir_all(LOG_DIR);
    // Probe writability before handing the path to tracing-appender —
    // `rolling::never` panics on permission errors, which is fatal for
    // tests and local dev where /opt/sola/log may not be writable.
    let probe = Path::new(LOG_DIR).join(LOG_FILE);
    if OpenOptions::new().create(true).append(true).open(&probe).is_err() {
        return None;
    }
    let appender = tracing_appender::rolling::never(LOG_DIR, LOG_FILE);
    Some(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(appender),
    )
}
