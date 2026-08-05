//! sola-kvm-mac — ember (macOS) UDP agent for sola-kvm.
//!
//! Listens for KVM1 packets from novus and injects pointer/keyboard via
//! CoreGraphics (macOS). On non-macOS hosts, inject is a logging stub so
//! decode + keymap unit tests still run on Linux.

mod agent;
mod click;
mod clip;
mod inject;
mod keymap;
mod metrics;
mod priority;
mod protocol;

use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "sola-kvm-mac",
    about = "ember macOS agent for sola-kvm (UDP receive + CGEvent inject)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Bind address (default when no subcommand: 0.0.0.0:4242).
    #[arg(long, global = true, default_value = "0.0.0.0:4242")]
    bind: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Listen for UDP packets and inject (default).
    Listen {
        /// Bind address override.
        #[arg(long)]
        bind: Option<String>,
    },
    /// Print version / platform and exit.
    Version,
}

fn main() {
    init_log();

    let cli = Cli::parse();
    let bind = match &cli.command {
        Some(Command::Listen { bind: Some(b) }) => b.clone(),
        Some(Command::Listen { bind: None }) | None => cli.bind.clone(),
        Some(Command::Version) => {
            println!(
                "sola-kvm-mac {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            );
            #[cfg(target_os = "macos")]
            println!("inject: CoreGraphics CGEvent");
            #[cfg(not(target_os = "macos"))]
            println!("inject: stub (build on macOS for real CGEvent inject)");
            return;
        }
    };

    info!(
        bind = %bind,
        os = std::env::consts::OS,
        "starting sola-kvm-mac"
    );

    priority::boost_process();

    if let Err(e) = agent::run(&bind) {
        eprintln!("sola-kvm-mac failed: {e}");
        std::process::exit(1);
    }
}

fn init_log() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
