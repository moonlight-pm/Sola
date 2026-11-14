//! `solactl badge set <id> reload|restart` / `solactl badge clear <id>` — a
//! badge in the screen's top-left corner while a window fills the screen
//! (a game in Cinema). `set` emits the persistent `Topic::Badge`, keyed by
//! id; sola-river draws it. `clear` retracts it.

use clap::{Subcommand, ValueEnum};
use sola_bus::topics::{Badge, BadgeKind, Topic};

use crate::bus;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show badge ID: `reload` (↻) or `restart` (⏻). Setting it again
    /// replaces it.
    Set {
        id: String,
        #[arg(value_enum)]
        kind: Kind,
    },
    /// Take badge ID away.
    Clear { id: String },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Kind {
    Reload,
    Restart,
}

pub fn run(cmd: Command) -> i32 {
    let mut client = bus::connect_or_exit();
    match cmd {
        Command::Set { id, kind } => {
            let set_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            let kind = match kind {
                Kind::Reload => BadgeKind::Reload,
                Kind::Restart => BadgeKind::Restart,
            };
            bus::emit(&mut client, Topic::Badge(Badge { id, kind, set_at }));
        }
        Command::Clear { id } => {
            let topic = Topic::Badge(Badge {
                id,
                kind: BadgeKind::Reload,
                set_at: 0,
            });
            if let Err(e) = client.retract(topic) {
                eprintln!("solactl: bus retract failed: {e}");
                return 3;
            }
        }
    }
    // The bus is async-write; let the writer flush before we exit.
    std::thread::sleep(std::time::Duration::from_millis(50));
    0
}
