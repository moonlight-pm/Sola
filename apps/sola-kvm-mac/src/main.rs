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
/// The server's half (the initiator) is unused here.
#[allow(dead_code)]
#[path = "../../../crates/sola-kvm/src/link.rs"]
mod link;

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
    /// Pairing by public key (`sola-kvm pair <mac> --remote-bin
    /// sola-kvm-mac` on the server runs `--accept` over ssh). Restart the
    /// agent after.
    Pair {
        /// Read the server's public key on stdin, keep it, and print this
        /// Mac's.
        #[arg(long, conflicts_with_all = ["forget", "public"])]
        accept: bool,
        /// Print this Mac's public key (made the first time).
        #[arg(long, conflicts_with = "forget")]
        public: bool,
        /// Forget the server's key: unpaired, and the agent will not start.
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
        Some(Command::Pair {
            accept,
            public,
            forget,
        }) => {
            pair(*accept, *public, *forget);
            return;
        }
    };

    info!(
        bind = %bind,
        os = std::env::consts::OS,
        "starting sola-kvm-mac"
    );

    priority::boost_process();

    let dir = link::dir();
    let keys = match link::load(&dir) {
        Ok(Some(keys)) => {
            info!(
                server = %link::hex(&keys.peer),
                "paired: input and clipboard only from the paired server"
            );
            keys
        }
        Ok(None) => {
            eprintln!("sola-kvm-mac: not paired, not starting (`sola-kvm pair <this mac> --remote-bin sola-kvm-mac` on the server)");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("sola-kvm-mac: pairing unreadable, not starting: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = agent::run(&bind, keys) {
        eprintln!("sola-kvm-mac failed: {e}");
        std::process::exit(1);
    }
}

fn pair(accept: bool, public: bool, forget: bool) {
    let dir = link::dir();
    let fail = |msg: String| -> ! {
        eprintln!("sola-kvm-mac pair: {msg}");
        std::process::exit(1);
    };
    if forget {
        match link::forget_peer(&dir) {
            Ok(true) => println!("Unpaired: forgot the server's key. Restart the agent."),
            Ok(false) => println!("Not paired."),
            Err(e) => fail(format!("forget the server in {}: {e}", dir.display())),
        }
        return;
    }
    let (_, own) = link::own_or_new(&dir)
        .unwrap_or_else(|e| fail(format!("this Mac's key in {}: {e}", dir.display())));
    if public {
        println!("{}", link::hex(&own));
        return;
    }
    if !accept {
        fail("pair from the server (`sola-kvm pair <this mac> --remote-bin sola-kvm-mac`), or --public / --forget".into());
    }
    // Standard output carries only this Mac's public key: the server reads
    // it back over ssh.
    let mut line = String::new();
    let theirs = std::io::stdin()
        .read_line(&mut line)
        .ok()
        .and_then(|_| link::parse_hex(&line));
    let Some(theirs) = theirs else {
        fail("stdin is not a public key (64 hex digits)".into());
    };
    if let Err(e) = link::save_peer(&dir, &theirs) {
        fail(format!("save the server's key in {}: {e}", dir.display()));
    }
    println!("{}", link::hex(&own));
    eprintln!("Paired with {}.", link::hex(&theirs));
}

fn init_log() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
