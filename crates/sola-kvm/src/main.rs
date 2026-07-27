//! sola-kvm — Sola-native software KVM (novus server).
//!
//! Phase A: config + layout + UDP spray / dump tools.
//! Phase C will add edge capture and exclusive grab.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tracing::{error, info, warn};

use sola_kvm::config::Config;
use sola_kvm::protocol::{Edge, Packet};
use sola_kvm::udp::{Listener, Sender};

#[derive(Parser, Debug)]
#[command(
    name = "sola-kvm",
    about = "Sola-native software KVM (novus → ember UDP)"
)]
struct Cli {
    /// Config path (default: ~/.config/sola-kvm/config.toml).
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print resolved config + layout and exit.
    Show,

    /// Write a default config file (creates parent dirs).
    Init {
        /// Overwrite if the file already exists.
        #[arg(long)]
        force: bool,
    },

    /// Run the server loop (Phase A: layout + idle; no edge capture yet).
    Server,

    /// Listen for UDP packets and print them (debug / Mac-side stand-in).
    Listen {
        /// Bind address (default: 0.0.0.0:<config peer.port>).
        #[arg(long)]
        bind: Option<String>,
    },

    /// Send a short test sequence to the configured peer (or --to).
    SendTest {
        /// Override peer `host:port`.
        #[arg(long)]
        to: Option<String>,
        /// Mac-local enter X.
        #[arg(long, default_value_t = 100)]
        x: i32,
        /// Mac-local enter Y.
        #[arg(long, default_value_t = 200)]
        y: i32,
    },
}

fn main() {
    sola_core::log::init("sola-kvm");

    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(Config::default_path);

    match cli.command {
        Command::Show => cmd_show(&config_path),
        Command::Init { force } => cmd_init(&config_path, force),
        Command::Server => cmd_server(&config_path),
        Command::Listen { bind } => cmd_listen(&config_path, bind),
        Command::SendTest { to, x, y } => cmd_send_test(&config_path, to, x, y),
    }
}

fn load(path: &PathBuf) -> Config {
    match Config::load(path) {
        Ok(cfg) => {
            if path.exists() {
                info!(path = %path.display(), "loaded config");
            } else {
                warn!(
                    path = %path.display(),
                    "config missing; using defaults (run `sola-kvm init`)"
                );
            }
            cfg
        }
        Err(e) => {
            error!("failed to load config: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_show(path: &PathBuf) {
    let cfg = load(path);
    let layout = cfg.layout();
    println!("config:  {}", path.display());
    println!("peer:    {}", cfg.peer_addr());
    println!(
        "primary: {}×{}",
        cfg.primary.width, cfg.primary.height
    );
    println!(
        "mac:     {}×{}  side={:?} align={:?}",
        layout.mac_w, layout.mac_h, layout.side, layout.align
    );
    println!(
        "origin:  ({}, {})  →  right={} bottom={}",
        layout.origin_x,
        layout.origin_y,
        layout.mac_right(),
        layout.mac_bottom()
    );
    println!("scale:   {}", layout.scale);
    println!("release: {:?}", cfg.bind.release);
}

fn cmd_init(path: &PathBuf, force: bool) {
    if path.exists() && !force {
        error!(
            path = %path.display(),
            "already exists; pass --force to overwrite"
        );
        std::process::exit(1);
    }
    if let Err(e) = Config::write_example(path) {
        error!("write failed: {e}");
        std::process::exit(1);
    }
    info!(path = %path.display(), "wrote default config");
    println!("wrote {}", path.display());
}

fn cmd_server(path: &PathBuf) {
    let cfg = load(path);
    let layout = cfg.layout();
    info!(
        peer = %cfg.peer_addr(),
        origin_x = layout.origin_x,
        origin_y = layout.origin_y,
        mac_w = layout.mac_w,
        mac_h = layout.mac_h,
        scale = layout.scale,
        "sola-kvm server (Phase A stub — no edge capture yet)"
    );
    info!(
        "layout bottoms meet at y={} (mac_bottom); primary {}×{}",
        layout.mac_bottom(),
        layout.primary_w,
        layout.primary_h
    );

    // Keep process alive so a future user unit / sola MANAGED entry can
    // supervise it. Phase C replaces this sleep loop with the capture path.
    loop {
        thread::sleep(Duration::from_secs(60));
        tracing::debug!("server idle tick");
    }
}

fn cmd_listen(path: &PathBuf, bind: Option<String>) {
    let cfg = load(path);
    let addr = bind.unwrap_or_else(|| format!("0.0.0.0:{}", cfg.peer.port));
    let listener = match Listener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            error!("bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    let local = listener.local_addr().ok();
    info!(?local, "listening for sola-kvm UDP packets");

    loop {
        match listener.recv() {
            Ok((src, seq, packet)) => {
                info!(%src, seq, ?packet, "recv");
            }
            Err(e) => {
                error!("recv: {e}");
                // Brief pause on hard errors so we don't spin if the
                // socket is wedged; timeouts would also land here if set.
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn cmd_send_test(path: &PathBuf, to: Option<String>, x: i32, y: i32) {
    let cfg = load(path);
    let peer = to.unwrap_or_else(|| cfg.peer_addr());
    let mut sender = match Sender::connect(&peer) {
        Ok(s) => s,
        Err(e) => {
            error!("connect {peer}: {e}");
            std::process::exit(1);
        }
    };
    info!(peer = %sender.peer(), "sending test sequence");

    let packets = [
        Packet::Enter {
            edge: Edge::Right,
            x,
            y,
        },
        Packet::Motion {
            x: x + 50,
            y: y + 20,
        },
        Packet::Button {
            button: 0,
            pressed: 1,
        },
        Packet::Button {
            button: 0,
            pressed: 0,
        },
        Packet::Key {
            keycode: 30, // KEY_A
            pressed: 1,
        },
        Packet::Key {
            keycode: 30,
            pressed: 0,
        },
        Packet::Scroll {
            dx: 0.0,
            dy: -1.0,
        },
        Packet::Leave,
    ];

    for p in &packets {
        match sender.send(p) {
            Ok(seq) => info!(seq, ?p, "sent"),
            Err(e) => {
                error!(?p, "send failed: {e}");
                std::process::exit(1);
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    info!("test sequence complete ({} packets)", packets.len());
}
