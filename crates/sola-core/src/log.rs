//! Shared tracing setup for every Sola binary.
//!
//! All logs — process manager, bus, session, river, app — land in
//! `/opt/sola/log/sola.log` plus stderr. Each binary just calls
//! `sola_core::log::init("<name>")` at startup.
//!
//! ## Format
//!
//! stderr (colored, local time):
//! ```text
//! 21:46:29  INFO [sola]      sola::core     watching for binary changes path=/opt/sola/bin
//! 21:46:30  INFO [bus]       sola::bus      bus listening path=/run/user/1000/sola-bus
//! ```
//!
//! file (plain, UTC with full timestamp):
//! ```text
//! 2026-04-23T21:46:29Z  INFO [sola]      sola::core     watching for binary changes
//! ```

use std::fmt::Write;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// Directory on disk where all Sola logs are written.
pub const LOG_DIR: &str = "/opt/sola/log";

/// Single log file shared by every Sola binary.
pub const LOG_FILE: &str = "sola.log";

/// Maximum size of `sola.log` before rotation (bytes).
const MAX_LOG_SIZE: u64 = 100_000;

/// Number of rotated log files to keep (`sola.log.1` … `sola.log.N`).
const MAX_LOG_FILES: u32 = 10;

/// Fixed width for `[process]` column (covers `[terminal]`, `[settings]`).
const PROCESS_WIDTH: usize = 10;

/// Fixed width for target label column (covers `sola::terminal`, `sola::settings`).
const LABEL_WIDTH: usize = 14;

/// Process name set by `init()`, read by the formatter.
static PROCESS_NAME: OnceLock<String> = OnceLock::new();

/// Initialize tracing for a Sola binary.
///
/// * Stores the process name (stripped of `sola-` prefix) for log output.
/// * Creates `/opt/sola/log` if missing.
/// * Writes to `/opt/sola/log/sola.log` (append, no rotation) and stderr.
/// * Falls back to stderr-only if the log file can't be opened (e.g. in
///   tests or local dev where /opt/sola/log may not be writable).
/// * Default `RUST_LOG` filter: binary's own crate + shared sola libs at info.
/// * Installs a panic hook that routes panics through tracing so they
///   end up in the log file, not just on stderr.
pub fn init(name: &str) {
    let short = name.strip_prefix("sola-").unwrap_or(name);
    PROCESS_NAME.get_or_init(|| short.to_string());

    // Include the binary's own crate + all shared sola libraries.
    // Third-party crates (gtk, calloop, etc.) are excluded by default.
    // Override with RUST_LOG for debug sessions.
    let own_crate = name.replace('-', "_");
    let default_filter =
        format!("{own_crate}=info,sola_core=info,sola_bus=info,sola_app=info,sola_assets=info");
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into());

    let stderr_layer = tracing_subscriber::fmt::layer()
        .event_format(SolaFormat { ansi: true })
        .with_writer(io::stderr);
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

/// Rotate `sola.log` if it exceeds [`MAX_LOG_SIZE`].
///
/// Renames `sola.log` → `sola.log.1`, `sola.log.1` → `sola.log.2`, etc.,
/// deleting anything beyond [`MAX_LOG_FILES`]. Intended to be called once
/// at startup by the process manager, before any children are spawned.
///
/// Errors are silently ignored — rotation is best-effort.
pub fn rotate() {
    let dir = Path::new(LOG_DIR);
    let current = dir.join(LOG_FILE);

    let size = match std::fs::metadata(&current) {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    if size < MAX_LOG_SIZE {
        return;
    }

    // Delete the oldest if it exists.
    let oldest = dir.join(format!("{LOG_FILE}.{MAX_LOG_FILES}"));
    let _ = std::fs::remove_file(oldest);

    // Shift N → N+1, starting from the highest.
    for i in (1..MAX_LOG_FILES).rev() {
        let from = dir.join(format!("{LOG_FILE}.{i}"));
        let to = dir.join(format!("{LOG_FILE}.{}", i + 1));
        let _ = std::fs::rename(from, to);
    }

    // Current → .1
    let _ = std::fs::rename(&current, dir.join(format!("{LOG_FILE}.1")));
}

/// Try to open the shared log file with a plain (no-color) formatter.
/// Returns `None` if the directory can't be created or the file can't be
/// opened — callers should still emit to stderr in that case.
fn open_file_layer<S>() -> Option<
    tracing_subscriber::fmt::Layer<
        S,
        tracing_subscriber::fmt::format::DefaultFields,
        SolaFormat,
        tracing_appender::rolling::RollingFileAppender,
    >,
>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let _ = std::fs::create_dir_all(LOG_DIR);
    // Probe writability before handing the path to tracing-appender —
    // `rolling::never` panics on permission errors, which is fatal for
    // tests and local dev where /opt/sola/log may not be writable.
    let probe = Path::new(LOG_DIR).join(LOG_FILE);
    if OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe)
        .is_err()
    {
        return None;
    }
    let appender = tracing_appender::rolling::never(LOG_DIR, LOG_FILE);
    Some(
        tracing_subscriber::fmt::layer()
            .event_format(SolaFormat { ansi: false })
            .with_ansi(false)
            .with_writer(appender),
    )
}

// ---------------------------------------------------------------------------
// Custom formatter
// ---------------------------------------------------------------------------

/// Custom log event formatter for Sola.
///
/// Each line includes: timestamp, level, `[process]`, target label, message.
/// When `ansi` is true (stderr), uses short local time and ANSI colors.
/// When false (file), uses full ISO 8601 UTC timestamps with no color.
struct SolaFormat {
    ansi: bool,
}

impl<S, N> FormatEvent<S, N> for SolaFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let level = *meta.level();
        let target = meta.target();
        let label = target_label(target);
        let process = PROCESS_NAME.get().map(|s| s.as_str()).unwrap_or("?");
        let bracketed = format!("[{process}]");

        if self.ansi {
            // Dim timestamp
            write!(writer, "\x1b[2m")?;
            format_short_time(&mut writer)?;
            write!(writer, "\x1b[0m ")?;

            // Colored level
            let (color, text) = level_style(level);
            write!(writer, "{color}{text}\x1b[0m ")?;

            // Colored process bracket
            let pcolor = component_color(process);
            write!(writer, "{pcolor}{bracketed:<PROCESS_WIDTH$}\x1b[0m ")?;

            // Colored target label
            let lcolor = component_color(&label);
            write!(writer, "{lcolor}{label:<LABEL_WIDTH$}\x1b[0m ")?;
        } else {
            format_iso_time(&mut writer)?;
            write!(
                writer,
                " {:>5} {bracketed:<PROCESS_WIDTH$} {label:<LABEL_WIDTH$} ",
                level
            )?;
        }

        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

// ---------------------------------------------------------------------------
// Label mapping
// ---------------------------------------------------------------------------

/// Map a tracing target (Rust module path) to a short component label.
///
/// - `sola` → `sola`
/// - `sola::river` → `sola::river` (submodule within process manager)
/// - `sola_bus::client` → `sola::bus`
/// - `sola_session::session` → `sola::session`
fn target_label(target: &str) -> String {
    let crate_name = target.split("::").next().unwrap_or(target);

    if crate_name == "sola" {
        // Keep up to two path segments for the sola crate's submodules.
        let mut parts = target.splitn(3, "::");
        let first = parts.next().unwrap();
        match parts.next() {
            Some(second) => format!("{first}::{second}"),
            None => first.to_string(),
        }
    } else if let Some(suffix) = crate_name.strip_prefix("sola_") {
        format!("sola::{suffix}")
    } else {
        crate_name.to_string()
    }
}

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// ANSI color and right-aligned text for a log level.
fn level_style(level: Level) -> (&'static str, &'static str) {
    match level {
        Level::ERROR => ("\x1b[1;31m", "ERROR"),
        Level::WARN => ("\x1b[1;33m", " WARN"),
        Level::INFO => ("\x1b[1;32m", " INFO"),
        Level::DEBUG => ("\x1b[1;34m", "DEBUG"),
        Level::TRACE => ("\x1b[1;35m", "TRACE"),
    }
}

/// ANSI color code for a component name (used for both process brackets
/// and target labels).
fn component_color(name: &str) -> &'static str {
    match name {
        "sola" => "\x1b[1;36m",                      // bold cyan
        "bus" | "sola::bus" => "\x1b[1;35m",         // bold magenta
        "river" | "sola::river" => "\x1b[1;34m",     // bold blue
        "session" | "sola::session" => "\x1b[1;32m", // bold green
        "shell" | "sola::shell" => "\x1b[1;33m",     // bold yellow
        "core" | "sola::core" => "\x1b[37m",         // white
        _ => "\x1b[36m",                             // cyan (apps and other)
    }
}

// ---------------------------------------------------------------------------
// Time formatting
// ---------------------------------------------------------------------------

/// Write `HH:MM:SS` in local time.
fn format_short_time(w: &mut impl Write) -> std::fmt::Result {
    let secs = epoch_secs();
    let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    write!(w, "{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// Write full ISO 8601 timestamp in UTC with microsecond precision.
fn format_iso_time(w: &mut impl Write) -> std::fmt::Result {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as libc::time_t;
    let micros = now.subsec_micros();
    let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::gmtime_r(&secs, &mut tm) };
    write!(
        w,
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        micros,
    )
}

fn epoch_secs() -> libc::time_t {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t
}
