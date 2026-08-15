//! `sat` — talk to a running sola-agent-terminal. Does not launch it.

use clap::{Parser, Subcommand};
use sola_agent_terminal::cli::{self, Request};

#[derive(Parser, Debug)]
#[command(
    name = "sat",
    about = "Workspaces CLI. Fails if the app is not running."
)]
struct Cli {
    /// Print JSON instead of the scan table / names.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Project → workspace → state table.
    Ps,
    #[command(subcommand)]
    Project(ProjectCmd),
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
    #[command(subcommand)]
    Pane(PaneCmd),
}

#[derive(Subcommand, Debug)]
enum ProjectCmd {
    List,
}

#[derive(Subcommand, Debug)]
enum WorkspaceCmd {
    List {
        #[arg(long)]
        project: Option<String>,
    },
    Spawn {
        #[arg(long)]
        project: String,
        #[arg(long)]
        name: String,
        /// Only `grok` in v1.
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        #[arg(long)]
        prompt_file: Option<String>,
        /// Parent workspace. Bare `--parent` uses $SOLA_PANE_ID.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        parent: Option<String>,
    },
    Rm {
        #[arg(long)]
        workspace: String,
    },
}

#[derive(Subcommand, Debug)]
enum PaneCmd {
    List {
        #[arg(long)]
        workspace: Option<String>,
    },
    Send {
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        text: String,
        #[arg(long)]
        enter: bool,
    },
    Read {
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        lines: Option<u32>,
    },
}

fn main() {
    let cli = Cli::parse();
    let req = match build_request(cli.command) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sat: {e}");
            std::process::exit(1);
        }
    };
    match cli::call(&req) {
        Ok(resp) if resp.ok => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(resp.data.as_ref().unwrap_or(&serde_json::Value::Null))
                        .unwrap_or_else(|_| "{}".into())
                );
            } else {
                print_text(&req, resp.data.as_ref());
            }
        }
        Ok(resp) => {
            eprintln!("sat: {}", resp.error.as_deref().unwrap_or("failed"));
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("sat: {e}");
            std::process::exit(2);
        }
    }
}

fn build_request(cmd: Command) -> Result<Request, String> {
    Ok(match cmd {
        Command::Ps => Request::Ps,
        Command::Project(ProjectCmd::List) => Request::ProjectList,
        Command::Workspace(WorkspaceCmd::List { project }) => {
            Request::WorkspaceList { project }
        }
        Command::Workspace(WorkspaceCmd::Spawn {
            project,
            name,
            agent,
            prompt,
            prompt_file,
            parent,
        }) => {
            let prompt = match (prompt, prompt_file) {
                (Some(p), _) => Some(p),
                (_, Some(path)) => Some(
                    std::fs::read_to_string(&path)
                        .map_err(|e| format!("prompt-file: {e}"))?,
                ),
                _ => None,
            };
            let parent = match parent.as_deref() {
                None => None,
                Some("") => std::env::var("SOLA_PANE_ID").ok(),
                Some(id) => Some(id.to_string()),
            };
            Request::WorkspaceSpawn {
                project,
                name,
                agent,
                prompt,
                parent,
            }
        }
        Command::Workspace(WorkspaceCmd::Rm { workspace }) => {
            Request::WorkspaceRm { workspace }
        }
        Command::Pane(PaneCmd::List { workspace }) => Request::PaneList { workspace },
        Command::Pane(PaneCmd::Send { pane, text, enter }) => {
            Request::PaneSend { pane, text, enter }
        }
        Command::Pane(PaneCmd::Read { pane, lines }) => Request::PaneRead { pane, lines },
    })
}

fn print_text(req: &Request, data: Option<&serde_json::Value>) {
    let Some(data) = data else {
        return;
    };
    match req {
        Request::Ps => print!("{}", cli::format_ps(data)),
        Request::ProjectList => {
            if let Some(arr) = data.get("projects").and_then(|v| v.as_array()) {
                for p in arr {
                    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    println!("{id}\t{name}");
                }
            }
        }
        Request::WorkspaceList { .. } => {
            if let Some(arr) = data.get("workspaces").and_then(|v| v.as_array()) {
                for w in arr {
                    let id = w.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = w.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let status = w.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    println!("{id}\t{name}\t{status}");
                }
            }
        }
        Request::PaneList { .. } => {
            if let Some(arr) = data.get("panes").and_then(|v| v.as_array()) {
                for p in arr {
                    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let status = p.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    println!("{id}\t{status}");
                }
            }
        }
        Request::PaneRead { .. } => {
            if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
                print!("{text}");
                if !text.ends_with('\n') {
                    println!();
                }
            }
        }
        Request::WorkspaceSpawn { .. } => {
            if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                println!("{id}");
            }
        }
        _ => {}
    }
}
