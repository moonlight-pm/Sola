//! sola-kvm — Sola-native software KVM (novus server).
//!
//! Phase A: config + layout + UDP spray / dump tools.
//! Phase C: edge enter/leave state machine, UDP emit, feed/demo/evdev input.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tracing::{error, info, warn};

use sola_kvm::config::Config;
use sola_kvm::input::InputBackendKind;
use sola_kvm::protocol::{Edge, Packet};
use sola_kvm::link::{self, Keys};
use sola_kvm::run;
use sola_kvm::udp::Sender;

#[derive(Parser, Debug)]
#[command(
    name = "sola-kvm",
    about = "Sola-native software KVM (novus server → Linux or Mac client UDP)"
)]
struct Cli {
    /// Config path (default: ~/.config/sola-kvm/config.toml).
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
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

    /// Run the server: edge enter/leave + virtual cursor + UDP emit.
    ///
    /// Input backends:
    /// - `wayland` (default): through the compositor — a layer-shell strip on
    ///   the shared edge; while remote it holds the keyboard, a pointer lock
    ///   and a shortcuts inhibitor. Ctrl+Alt+Esc comes back.
    /// - `feed`: stdin line protocol (`rel`/`abs`/`btn`/`key`/`scroll`/`leave`)
    /// - `demo`: scripted smoke sequence then idle
    ///
    /// Bare `sola-kvm` (no subcommand) also starts the server.
    Server {
        /// Input backend: wayland | feed | demo (`evdev` means wayland).
        #[arg(long, default_value = "wayland")]
        input: String,
    },

    /// Listen for UDP packets and inject them (Linux client).
    ///
    /// Default: Wayland virtual pointer + virtual keyboard (River).
    /// `--dump` prints packets instead (debug stand-in).
    Listen {
        /// Bind address (default: 0.0.0.0:<config peer.port>).
        #[arg(long)]
        bind: Option<String>,
        /// Log packets only; do not inject.
        #[arg(long)]
        dump: bool,
    },

    /// Pair this machine (the server) with its peer by public key, as SSH
    /// swaps host keys: each keeps its own key pair and the other's public
    /// key, swapped over ssh. From then on each side takes only the other's
    /// input and clipboard, and neither starts unpaired. Restart sola-kvm on
    /// both after.
    Pair {
        /// The peer's ssh host (default: the config's peer host).
        peer: Option<String>,
        /// The KVM binary to run on the peer (`sola-kvm-mac` on a Mac).
        #[arg(long, default_value = "sola-kvm")]
        remote_bin: String,
        /// Read the other machine's public key on stdin, keep it, and print
        /// this machine's (what `pair` runs on the peer; also pairs by hand).
        #[arg(long, conflicts_with_all = ["peer", "forget", "public"])]
        accept: bool,
        /// Print this machine's public key (made the first time).
        #[arg(long, conflicts_with_all = ["peer", "forget"])]
        public: bool,
        /// Forget the peer's key: unpaired, and sola-kvm will not start.
        #[arg(long, conflicts_with = "peer")]
        forget: bool,
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
        None => cmd_server(&config_path, "wayland"),
        Some(Command::Show) => cmd_show(&config_path),
        Some(Command::Init { force }) => cmd_init(&config_path, force),
        Some(Command::Server { input }) => cmd_server(&config_path, &input),
        Some(Command::Listen { bind, dump }) => cmd_listen(&config_path, bind, dump),
        Some(Command::SendTest { to, x, y }) => cmd_send_test(&config_path, to, x, y),
        Some(Command::Pair {
            peer,
            remote_bin,
            accept,
            public,
            forget,
        }) => cmd_pair(&config_path, peer, &remote_bin, accept, public, forget),
    }
}

/// The pairing. Unpaired, or keys that cannot be read, stop the process:
/// sola-kvm sends and takes only the paired machine's traffic.
fn paired() -> Keys {
    let dir = link::dir();
    match link::load(&dir) {
        Ok(Some(keys)) => {
            info!(
                peer = %link::hex(&keys.peer),
                "paired: input and clipboard go only to and from the paired machine"
            );
            keys
        }
        Ok(None) => {
            error!("not paired, not starting: run `sola-kvm pair <peer>` on the server");
            std::process::exit(1);
        }
        Err(e) => {
            error!(dir = %dir.display(), "pairing unreadable, not starting: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_pair(
    path: &PathBuf,
    peer: Option<String>,
    remote_bin: &str,
    accept: bool,
    public: bool,
    forget: bool,
) {
    let dir = link::dir();
    let fail = |msg: String| -> ! {
        eprintln!("sola-kvm pair: {msg}");
        std::process::exit(1);
    };
    if forget {
        match link::forget_peer(&dir) {
            Ok(true) => println!("Unpaired: forgot the peer's key. sola-kvm will not start until paired again."),
            Ok(false) => println!("Not paired."),
            Err(e) => fail(format!("forget the peer in {}: {e}", dir.display())),
        }
        return;
    }
    let (_, own) = link::own_or_new(&dir)
        .unwrap_or_else(|e| fail(format!("this machine's key in {}: {e}", dir.display())));
    if public {
        println!("{}", link::hex(&own));
        return;
    }
    if accept {
        // Standard output carries only this machine's public key: the server
        // reads it back over ssh.
        let mut line = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut line) {
            fail(format!("read the key: {e}"));
        }
        let Some(theirs) = link::parse_hex(&line) else {
            fail("stdin is not a public key (64 hex digits)".into());
        };
        if let Err(e) = link::save_peer(&dir, &theirs) {
            fail(format!("save the peer's key in {}: {e}", dir.display()));
        }
        println!("{}", link::hex(&own));
        eprintln!("Paired with {}.", link::hex(&theirs));
        return;
    }
    let host = peer.unwrap_or_else(|| load(path).peer.host);
    // The peer first: it keeps this machine's key and answers with its own.
    let mut child = std::process::Command::new("ssh")
        .args(["-o", "BatchMode=yes", &host, remote_bin, "pair", "--accept"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| fail(format!("ssh {host}: {e}")));
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("piped stdin");
        if let Err(e) = writeln!(stdin, "{}", link::hex(&own)) {
            fail(format!("send this machine's key to {host}: {e}"));
        }
    }
    let out = child
        .wait_with_output()
        .unwrap_or_else(|e| fail(format!("ssh {host}: {e}")));
    if !out.status.success() {
        fail(format!(
            "`{remote_bin} pair --accept` on {host} failed ({}); nothing changed here",
            out.status
        ));
    }
    let answer = String::from_utf8_lossy(&out.stdout);
    let Some(theirs) = answer.lines().rev().find_map(link::parse_hex) else {
        fail(format!("{host} answered with no public key; nothing changed here"));
    };
    if let Err(e) = link::save_peer(&dir, &theirs) {
        fail(format!(
            "{host} keeps this machine's key, but keeping its key here failed ({}: {e}); run pair again",
            dir.display()
        ));
    }
    println!(
        "Paired with {host}.\n  this machine: {}\n  {host}: {}\n\
         Restart sola-kvm on both machines (it reads the keys when it starts).",
        link::hex(&own),
        link::hex(&theirs)
    );
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
    println!("primary: {}×{}", cfg.primary.width, cfg.primary.height);
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

fn cmd_server(path: &PathBuf, input: &str) {
    // Prefer this process under desktop load (best-effort; may need CAP_SYS_NICE).
    sola_kvm::priority::boost_process();
    sola_kvm::priority::warn_if_still_default();

    // When managed by sola (or launched from a bare TTY), pick up the
    // River Wayland socket so layer-shell edge barriers can bind.
    let wayland_display = sola_core::env::activate_wayland_session(30_000);
    if !sola_core::env::wait_for_wayland_socket(&wayland_display, 30_000) {
        warn!(
            wayland_display = %wayland_display,
            "wayland socket not ready after 30s — barrier may fail until River is up"
        );
    } else {
        info!(
            wayland_display = %wayland_display,
            "WAYLAND_DISPLAY ready for layer-shell barrier"
        );
    }

    let cfg = load(path);
    let backend = match InputBackendKind::parse(input) {
        Some(b) => b,
        None => {
            error!(input, "unknown --input backend (want wayland | feed | demo)");
            std::process::exit(2);
        }
    };
    if let Err(e) = run::run_server(&cfg, backend, paired()) {
        error!("{e}");
        std::process::exit(1);
    }
}

fn cmd_listen(path: &PathBuf, bind: Option<String>, dump: bool) {
    let cfg = load(path);
    let keys = paired();
    let addr = bind.unwrap_or_else(|| format!("0.0.0.0:{}", cfg.peer.port));
    let result = if dump {
        sola_kvm::inject::run_dump(&addr, keys)
    } else {
        let clip = if cfg.clipboard.enable {
            Some(sola_kvm::clip::ClipConfig {
                peer_host: cfg.peer.host.clone(),
                peer_port: cfg.peer.port,
                max_bytes: cfg.clipboard.max_bytes,
                sync_on_enter: cfg.clipboard.sync_on_enter,
                sync_on_leave: cfg.clipboard.sync_on_leave,
                keys: keys.clone(),
            })
        } else {
            None
        };
        sola_kvm::inject::run_listen(
            &addr,
            cfg.layout.mac_width,
            cfg.layout.mac_height,
            clip,
            keys,
        )
    };
    if let Err(e) = result {
        error!("{e}");
        std::process::exit(1);
    }
}

fn cmd_send_test(path: &PathBuf, to: Option<String>, x: i32, y: i32) {
    let cfg = load(path);
    let peer = to.unwrap_or_else(|| cfg.peer_addr());
    let mut sender = match Sender::connect(&peer, paired()) {
        Ok(s) => s,
        Err(e) => {
            error!("connect {peer}: {e}");
            std::process::exit(1);
        }
    };
    if !sender.wait_up(Duration::from_secs(3)) {
        error!(peer = %sender.peer(), "the paired peer did not answer the handshake");
        std::process::exit(1);
    }
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
        Packet::Scroll { dx: 0.0, dy: -1.0 },
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
