//! `solactl session …`

use clap::Subcommand;
use sola_call::methods::OWNER_SESSION;

use crate::call;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Spawn an app. Default command is `/opt/sola/bin/<app_id>` when that exists.
    Launch {
        app_id: String,
        #[arg(short, long)]
        command: Option<String>,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    /// Close a session-tracked app.
    Close {
        app_id: String,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
}

pub fn run(cmd: Command) -> i32 {
    match cmd {
        Command::Launch {
            app_id,
            command,
            timeout,
        } => {
            let mut params = serde_json::json!({ "app_id": app_id });
            if let Some(c) = command {
                params["command"] = serde_json::Value::String(c);
            }
            call::run(OWNER_SESSION, "launch", params, timeout)
        }
        Command::Close { app_id, timeout } => call::run(
            OWNER_SESSION,
            "close",
            serde_json::json!({ "app_id": app_id }),
            timeout,
        ),
    }
}
