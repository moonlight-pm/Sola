//! `solactl background set <image>` / `solactl background clear` — the
//! picture behind the windows. `set` copies the image into Sola's own
//! store (`sola_core::background`) and emits the persistent
//! `Topic::Background`; sola-river draws it. `clear` retracts it: the
//! plain black ground.

use std::path::PathBuf;

use clap::Subcommand;
use sola_bus::topics::{Background, Topic};

use crate::bus;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Copy an image into ~/.config/sola/background/ and show it.
    Set { image: PathBuf },
    /// Show the plain black ground again.
    Clear,
}

pub fn run(cmd: Command) -> i32 {
    let mut client = bus::connect_or_exit();
    match cmd {
        Command::Set { image } => {
            let path = match sola_core::background::adopt(&image) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("solactl: {}: {e}", image.display());
                    return 3;
                }
            };
            let set_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            bus::emit(
                &mut client,
                Topic::Background(Background {
                    path: path.to_string_lossy().into_owned(),
                    set_at,
                }),
            );
            println!("{}", path.display());
        }
        Command::Clear => {
            let topic = Topic::Background(Background {
                path: String::new(),
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
