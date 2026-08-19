//! solactl — operator CLI for Sola.
//!
//! Compiled owners (`compositor`, `session`, `workspaces`) are a real clap
//! tree. Other live owners appear as `solactl <app-id>` from the call registry.

use clap::{Parser, Subcommand};

mod bus;
mod call;
mod compositor;
mod dynamic;
mod emit;
mod logs;
mod media;
mod open;
mod session;

#[derive(Parser, Debug)]
#[command(name = "solactl", about = "Sola control + introspection CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compositor (sola-river): screenshot, windows, input.
    Compositor {
        #[command(subcommand)]
        cmd: compositor::Command,
    },
    /// Session manager: launch and close user apps.
    Session {
        #[command(subcommand)]
        cmd: session::Command,
    },
    /// Workspaces: projects, worktrees, panes (app must be running).
    #[command(disable_help_flag = true)]
    Workspaces {
        /// Method and flags. Omit to list advertised methods.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Tail an app's log file at `/opt/sola/log/<app>.log`.
    Logs {
        app: Option<String>,
        #[arg(short, long)]
        follow: bool,
    },

    /// Emit a bus topic with a JSON payload (developer escape hatch).
    Emit {
        kind: String,
        payload: String,
    },

    /// Open a URL in sola-browser, or an image path in sola-paint.
    Open { target: String },

    /// Global media-key action (MPRIS / wpctl). Invoked by the shell.
    Media {
        #[arg(value_enum)]
        action: media::MediaAction,
    },

    /// Live owner not compiled into this CLI (`solactl <app-id> …`).
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() {
    let cli = Cli::parse();
    let exit = match cli.command {
        Command::Compositor { cmd } => compositor::run(cmd),
        Command::Session { cmd } => session::run(cmd),
        Command::Workspaces { args } => {
            let mut all = vec!["workspaces".into()];
            all.extend(args);
            dynamic::run(all)
        },
        Command::Logs { app, follow } => logs::run(app.as_deref(), follow),
        Command::Emit { kind, payload } => emit::run(&kind, &payload),
        Command::Open { target } => open::run(&target),
        Command::Media { action } => media::run(action),
        Command::External(args) => dynamic::run(args),
    };
    std::process::exit(exit);
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn help_lists_workspaces() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        cmd.write_long_help(&mut buf).unwrap();
        let help = String::from_utf8(buf).unwrap();
        assert!(
            help.contains("workspaces"),
            "solactl help must list workspaces:\n{help}"
        );
    }
}
