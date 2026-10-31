//! `solactl compositor …`

use clap::Subcommand;
use sola_call::methods::OWNER_COMPOSITOR;
use sola_core::{KeyChord, KeyCode};

use crate::call;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Capture a PNG of the output, or of one window with `--app`.
    ///
    /// `--app` copies that window's own buffer. The window does not need
    /// to be on top and is not raised.
    Screenshot {
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        #[arg(short, long)]
        app: Option<String>,
        #[arg(short, long)]
        window: Option<String>,
        /// `png` (default) or `rgba` (packed RGBA8, no PNG encode).
        #[arg(long)]
        format: Option<String>,
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
    /// Press a button and hold it (moving to X Y first when given).
    Press {
        x: Option<i32>,
        y: Option<i32>,
        #[arg(short, long, default_value = "left")]
        button: String,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    /// Release a held button (moving to X Y first when given).
    Release {
        x: Option<i32>,
        y: Option<i32>,
        #[arg(short, long, default_value = "left")]
        button: String,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
    /// Press at X1 Y1, move to X2 Y2 in steps, release there.
    Drag {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        #[arg(short, long, default_value = "left")]
        button: String,
        /// Pointer moves between press and release.
        #[arg(short, long, default_value_t = 12)]
        steps: u32,
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,
    },
}

fn is_sola_bot() -> bool {
    matches!(
        std::env::var("SOLA_BOT").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

fn refuse_bot_compositor(what: &str) -> i32 {
    eprintln!(
        "solactl: SOLA_BOT=1: compositor {what} is refused (use solactl browser; do not steal the seat)"
    );
    3
}

pub fn run(cmd: Command) -> i32 {
    if is_sola_bot() {
        match &cmd {
            Command::Screenshot { .. } => return refuse_bot_compositor("screenshot"),
            Command::Sample { .. } => return refuse_bot_compositor("sample"),
            Command::Input(_) => return refuse_bot_compositor("input"),
            Command::Windows => {}
        }
    }
    match cmd {
        Command::Screenshot {
            output,
            app,
            window,
            format,
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
            if let Some(f) = format {
                params.insert("format".into(), serde_json::Value::String(f));
            }
            call::run(
                OWNER_COMPOSITOR,
                "screenshot",
                serde_json::Value::Object(params),
                timeout,
            )
        }
        Command::Sample { size, timeout } => {
            call::run(
                OWNER_COMPOSITOR,
                "sample",
                serde_json::json!({ "size": size }),
                timeout,
            )
        }
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
        InputCmd::Press {
            x,
            y,
            button,
            timeout,
        } => call::run(
            OWNER_COMPOSITOR,
            "input.press",
            serde_json::json!({ "x": x, "y": y, "button": button }),
            timeout,
        ),
        InputCmd::Release {
            x,
            y,
            button,
            timeout,
        } => call::run(
            OWNER_COMPOSITOR,
            "input.release",
            serde_json::json!({ "x": x, "y": y, "button": button }),
            timeout,
        ),
        InputCmd::Drag {
            x1,
            y1,
            x2,
            y2,
            button,
            steps,
            timeout,
        } => match drag((x1, y1), (x2, y2), &button, steps, timeout) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("solactl: {e}");
                1
            }
        },
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

/// A drag: press at `from`, `steps` moves ~16 ms apart, release at `to`.
fn drag(
    from: (i32, i32),
    to: (i32, i32),
    button: &str,
    steps: u32,
    timeout: u64,
) -> Result<(), String> {
    let wait = std::time::Duration::from_secs(timeout);
    let call = |method: &str, params: serde_json::Value| {
        sola_call::invoke(OWNER_COMPOSITOR, method, params, wait)
            .map(drop)
            .map_err(|e| format!("{method}: {e}"))
    };
    let at = |x: i32, y: i32| serde_json::json!({ "x": x, "y": y, "button": button });
    call("input.press", at(from.0, from.1))?;
    let steps = steps.max(1) as i32;
    for i in 1..=steps {
        let x = from.0 + (to.0 - from.0) * i / steps;
        let y = from.1 + (to.1 - from.1) * i / steps;
        std::thread::sleep(std::time::Duration::from_millis(16));
        call("input.move", serde_json::json!({ "x": x, "y": y }))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(16));
    call("input.release", at(to.0, to.1))
}

fn parse_chord(s: &str) -> Result<KeyChord, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("empty chord".into());
    }
    let key_str = parts[parts.len() - 1];
    let keycode = parse_keycode(key_str)
        .ok_or_else(|| {
            format!("unknown key '{key_str}'. Try Tab, Esc, Enter, A-Z, 0-9, `, KP0-KP8, KPEnter, KP*.")
        })?;
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
        "`" | "GRAVE" | "BACKTICK" => Some(KeyCode::GRAVE),
        "KP0" => Some(KeyCode::KP_0),
        "KP2" => Some(KeyCode::KP_2),
        "KP4" => Some(KeyCode::KP_4),
        "KP5" => Some(KeyCode::KP_5),
        "KP6" => Some(KeyCode::KP_6),
        "KP8" => Some(KeyCode::KP_8),
        "KPENTER" => Some(KeyCode::KP_ENTER),
        "KPMULTIPLY" | "KP*" => Some(KeyCode::KP_MULTIPLY),
        "KPADD" => Some(KeyCode::KP_ADD),
        "KPEQUAL" | "KP=" => Some(KeyCode::KP_EQUAL),
        "KPDECIMAL" | "KP." => Some(KeyCode::KP_DECIMAL),
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
