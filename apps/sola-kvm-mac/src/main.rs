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
/// Paired traffic, the same file as sola-kvm's (built from a full checkout).
#[path = "../../../crates/sola-kvm/src/seal.rs"]
mod seal;

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
    /// Take the pair key on stdin (`sola-kvm pair <mac> --remote-bin
    /// sola-kvm-mac` runs this over ssh), or `--forget` it. Restart the
    /// agent after.
    Pair {
        #[arg(long, conflicts_with = "forget")]
        accept: bool,
        #[arg(long)]
        forget: bool,
    },
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
        Some(Command::Pair { accept, forget }) => {
            pair(*accept, *forget);
            return;
        }
    };

    info!(
        bind = %bind,
        os = std::env::consts::OS,
        "starting sola-kvm-mac"
    );

    priority::boost_process();

    let key = match seal::load_key(&seal::key_path()) {
        Ok(Some(key)) => {
            info!("paired: input and clipboard sealed with the pair key");
            Some(key)
        }
        Ok(None) => {
            tracing::warn!(
                "not paired: input and clipboard are open to the network (`sola-kvm pair`)"
            );
            None
        }
        Err(e) => {
            eprintln!("sola-kvm-mac: pair key unreadable, not starting: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = agent::run(&bind, key) {
        eprintln!("sola-kvm-mac failed: {e}");
        std::process::exit(1);
    }
}

fn pair(accept: bool, forget: bool) {
    let path = seal::key_path();
    if forget {
        match std::fs::remove_file(&path) {
            Ok(()) => println!("Unpaired: removed {}. Restart the agent.", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("Not paired."),
            Err(e) => {
                eprintln!("sola-kvm-mac pair: remove {}: {e}", path.display());
                std::process::exit(1);
            }
        }
        return;
    }
    if !accept {
        eprintln!("sola-kvm-mac pair: pair from the server (`sola-kvm pair <this mac> --remote-bin sola-kvm-mac`), or --forget");
        std::process::exit(2);
    }
    let mut line = String::new();
    let key = std::io::stdin()
        .read_line(&mut line)
        .ok()
        .and_then(|_| seal::parse_key(&line));
    let Some(key) = key else {
        eprintln!("sola-kvm-mac pair: stdin is not a pair key (64 hex digits)");
        std::process::exit(1);
    };
    if let Err(e) = seal::save_key(&path, &key) {
        eprintln!("sola-kvm-mac pair: save {}: {e}", path.display());
        std::process::exit(1);
    }
    println!("Paired: key saved at {}.", path.display());
}

fn init_log() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
