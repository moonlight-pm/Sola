//! `solactl dropdown set <app-id> --command CMD [--height PCT] [--key CHORD]`
//! / `solactl dropdown clear <app-id>` — a drop-down terminal over an app
//! while it fills the screen. `set` emits the persistent `Topic::DropDown`,
//! keyed by app id; the shell grabs its key while that app fills the screen
//! and runs `CMD` in `sola-dropdown` over it. `clear` retracts it.

use clap::Subcommand;
use sola_bus::topics::{DropDown, Topic};
use sola_core::KeyCode;

use crate::bus;
use crate::compositor::parse_chord;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Give APP_ID a drop-down. Setting it again replaces it.
    Set {
        app_id: String,
        /// What it shows, run with `sh -c` each time it comes down.
        #[arg(long)]
        command: String,
        /// How much of the screen's height it takes, in percent.
        #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u8).range(10..=100))]
        height: u8,
        /// The key that brings it down and sends it away.
        #[arg(long, default_value = "super+j", value_parser = parse_chord)]
        key: sola_core::KeyChord,
    },
    /// Take APP_ID's drop-down away.
    Clear { app_id: String },
}

pub fn run(cmd: Command) -> i32 {
    let mut client = bus::connect_or_exit();
    match cmd {
        Command::Set { app_id, command, height, key } => {
            let drop = DropDown { app_id, command, height, key };
            bus::emit(&mut client, Topic::DropDown(drop));
        }
        Command::Clear { app_id } => {
            let topic = Topic::DropDown(DropDown {
                app_id,
                command: String::new(),
                height: 0,
                key: KeyCode::J.meta(),
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
