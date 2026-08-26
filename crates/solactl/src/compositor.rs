//! `solactl compositor …`

use clap::Subcommand;
use sola_call::methods::OWNER_COMPOSITOR;
use sola_core::{KeyChord, KeyCode};

use crate::call;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Capture a PNG of the output, or of one window with `--app`.
    Screenshot {
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        #[arg(short, long)]
        app: Option<String>,
        #[arg(short, long)]
        window: Option<String>,
        #[arg(short, long, default_value_t = 10)]
        timeout: u64,
    },
    /// RGBA patch around the current pointer (for sola-scope).
    Sample {
        #[arg(short, long, default_value_t = 15)]
        size: i32,
        #[arg(short, long, default_value_t = 2)]
        timeout: u64,
    },
    /// List windows grouped by app id.
    Windows,
    /// Synthesize pointer or key events.
    #[command(subcommand)]
    Input(InputCmd),
}

#[derive(Subcommand, Debug)]
pub enum InputCmd {
    Click {
        x: i32,
        y: i32,
        #[arg(short, long, default_value = "left")]
        button: String,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    Move {
        x: i32,
        y: i32,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    Scroll {
        #[arg(short = 'x', long, default_value_t = 0.0)]
        dx: f64,
        #[arg(short = 'y', long, default_value_t = 5.0)]
        dy: f64,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    Key {
        chord: String,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
}

pub fn run(cmd: Command) -> i32 {
    match cmd {
        Command::Screenshot {
            output,
            app,
            window,
            timeout,
        } => {
            let mut params = serde_json::Map::new();
            if let Some(p) = output {
                params.insert(
                    "output".into(),
                    serde_json::Value::String(p.display().to_string()),
                );
            }
            if let Some(a) = app {
                params.insert("app".into(), serde_json::Value::String(a));
            }
            if let Some(w) = window {
                params.insert("window".into(), serde_json::Value::String(w));
            }
            call::run(
                OWNER_COMPOSITOR,
                "screenshot",
                serde_json::Value::Object(params),
                timeout,
            )
        }
        Command::Sample { size, timeout } => call::run(
            OWNER_COMPOSITOR,
            "sample",
            serde_json::json!({ "size": size }),
            timeout,
        ),
        Command::Windows => call::run(OWNER_COMPOSITOR, "windows", serde_json::json!({}), 5),
        Command::Input(input) => run_input(input),
    }
}

fn run_input(cmd: InputCmd) -> i32 {
    match cmd {
        InputCmd::Click {
            x,
            y,
            button,
            timeout,
        } => call::run(
            OWNER_COMPOSITOR,
            "input.click",
            serde_json::json!({ "x": x, "y": y, "button": button }),
            timeout,
        ),
        InputCmd::Move { x, y, timeout } => call::run(
            OWNER_COMPOSITOR,
            "input.move",
            serde_json::json!({ "x": x, "y": y }),
            timeout,
        ),
        InputCmd::Scroll { dx, dy, timeout } => call::run(
            OWNER_COMPOSITOR,
            "input.scroll",
            serde_json::json!({ "dx": dx, "dy": dy }),
            timeout,
        ),
        InputCmd::Key { chord, timeout } => match parse_chord(&chord) {
            Ok(c) => call::run(
                OWNER_COMPOSITOR,
                "input.key",
                serde_json::json!({ "chord": c }),
                timeout,
            ),
            Err(e) => {
                eprintln!("solactl: {e}");
                3
            }
        },
    }
}

fn parse_chord(s: &str) -> Result<KeyChord, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("empty chord".into());
    }
    let key_str = parts[parts.len() - 1];
    let keycode = parse_keycode(key_str)
        .ok_or_else(|| format!("unknown key '{key_str}'. Try Tab, Esc, Enter, A-Z, 0-9."))?;
    let mut chord = KeyChord::new(keycode);
    for m in &parts[..parts.len() - 1] {
        match m.to_ascii_lowercase().as_str() {
            "meta" | "super" | "win" | "cmd" => chord = chord.meta(),
            "alt" | "option" => chord = chord.alt(),
            "ctrl" | "control" => chord = chord.ctrl(),
            "shift" => chord = chord.shift(),
            other => return Err(format!("unknown modifier '{other}'")),
        }
    }
    Ok(chord)
}

fn parse_keycode(s: &str) -> Option<KeyCode> {
    let upper = s.to_ascii_uppercase();
    match upper.as_str() {
        "TAB" => Some(KeyCode::TAB),
        "ESC" | "ESCAPE" => Some(KeyCode::ESCAPE),
        "ENTER" | "RETURN" => Some(KeyCode::ENTER),
        "BACKSPACE" | "BS" => Some(KeyCode::BACKSPACE),
        "SPACE" => Some(KeyCode::SPACE),
        "LEFT" => Some(KeyCode::LEFT),
        "RIGHT" => Some(KeyCode::RIGHT),
        "A" => Some(KeyCode::A),
        "B" => Some(KeyCode::B),
        "C" => Some(KeyCode::C),
        "D" => Some(KeyCode::D),
        "E" => Some(KeyCode::E),
        "F" => Some(KeyCode::F),
        "G" => Some(KeyCode::G),
        "H" => Some(KeyCode::H),
        "I" => Some(KeyCode::I),
        "J" => Some(KeyCode::J),
        "K" => Some(KeyCode::K),
        "L" => Some(KeyCode::L),
        "M" => Some(KeyCode::M),
        "N" => Some(KeyCode::N),
        "O" => Some(KeyCode::O),
        "P" => Some(KeyCode::P),
        "Q" => Some(KeyCode::Q),
        "R" => Some(KeyCode::R),
        "S" => Some(KeyCode::S),
        "T" => Some(KeyCode::T),
        "U" => Some(KeyCode::U),
        "V" => Some(KeyCode::V),
        "W" => Some(KeyCode::W),
        "X" => Some(KeyCode::X),
        "Y" => Some(KeyCode::Y),
        "Z" => Some(KeyCode::Z),
        "0" => Some(KeyCode::KEY_0),
        "1" => Some(KeyCode::KEY_1),
        "2" => Some(KeyCode::KEY_2),
        "3" => Some(KeyCode::KEY_3),
        "4" => Some(KeyCode::KEY_4),
        "5" => Some(KeyCode::KEY_5),
        "6" => Some(KeyCode::KEY_6),
        "7" => Some(KeyCode::KEY_7),
        "8" => Some(KeyCode::KEY_8),
        "9" => Some(KeyCode::KEY_9),
        _ => None,
    }
}
