//! `solactl badge set <id> reload|restart [--corner …] [--offset X,Y]` /
//! `solactl badge clear <id>` — a badge in a corner of the screen while a
//! window fills the screen (a game in Cinema). `set` emits the persistent
//! `Topic::Badge`, keyed by id; sola-river draws it. `clear` retracts it.

use clap::{Subcommand, ValueEnum};
use sola_bus::topics::{Badge, BadgeCorner, BadgeKind, Topic};

use crate::bus;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show badge ID: `reload` (↻) or `restart` (⏻). Setting it again
    /// replaces it.
    Set {
        id: String,
        #[arg(value_enum)]
        kind: Kind,
        /// The screen's corner it is placed from.
        #[arg(long, value_enum, default_value = "top-left")]
        corner: Corner,
        /// Logical pixels in from that corner, across and down (or up).
        #[arg(long, value_name = "X,Y", default_value = "4,4", value_parser = offset)]
        offset: (i32, i32),
    },
    /// Take badge ID away.
    Clear { id: String },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Kind {
    Reload,
    Restart,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// `X,Y`, whole logical pixels.
fn offset(s: &str) -> Result<(i32, i32), String> {
    let (x, y) = s.split_once(',').ok_or("expected X,Y")?;
    let n = |v: &str| v.trim().parse::<i32>().map_err(|e| format!("{v:?}: {e}"));
    Ok((n(x)?, n(y)?))
}

pub fn run(cmd: Command) -> i32 {
    let mut client = bus::connect_or_exit();
    match cmd {
        Command::Set { id, kind, corner, offset } => {
            let set_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            let kind = match kind {
                Kind::Reload => BadgeKind::Reload,
                Kind::Restart => BadgeKind::Restart,
            };
            let corner = match corner {
                Corner::TopLeft => BadgeCorner::TopLeft,
                Corner::TopRight => BadgeCorner::TopRight,
                Corner::BottomLeft => BadgeCorner::BottomLeft,
                Corner::BottomRight => BadgeCorner::BottomRight,
            };
            let badge = Badge { id, kind, set_at, corner, offset };
            bus::emit(&mut client, Topic::Badge(badge));
        }
        Command::Clear { id } => {
            let topic = Topic::Badge(Badge {
                id,
                kind: BadgeKind::Reload,
                set_at: 0,
                corner: BadgeCorner::TopLeft,
                offset: (0, 0),
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

#[cfg(test)]
mod tests {
    use super::offset;

    #[test]
    fn an_offset_is_two_whole_numbers() {
        assert_eq!(offset("112,7"), Ok((112, 7)));
        assert_eq!(offset(" 4 , 4 "), Ok((4, 4)));
        assert!(offset("112").is_err());
        assert!(offset("1.5,2").is_err());
    }
}
