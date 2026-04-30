//! `solactl click | move | scroll | key` — synthesized input.
//!
//! Emits `SimulatePointer` / `SimulateKey` topics; sola-river drives the
//! compositor's wlr-virtual-pointer / virtual-keyboard.

use sola_bus::topics::{
    PointerAction, PointerButton, SimulateKeyPayload, SimulatePointerPayload, Topic,
};
use sola_core::{KeyChord, KeyCode};

use crate::bus;

pub fn click(x: i32, y: i32, button: &str) -> i32 {
    let Some(button) = parse_button(button) else {
        eprintln!("solactl: unknown button '{button}'. Use left, right, or middle.");
        return 3;
    };
    emit_pointer(PointerAction::Click { button, x, y })
}

pub fn move_to(x: i32, y: i32) -> i32 {
    emit_pointer(PointerAction::Move { x, y })
}

pub fn scroll(dx: f64, dy: f64) -> i32 {
    emit_pointer(PointerAction::Scroll { dx, dy })
}

pub fn key(chord_str: &str) -> i32 {
    match parse_chord(chord_str) {
        Ok(chord) => {
            let mut client = bus::connect_or_exit();
            bus::emit(&mut client, Topic::SimulateKey(SimulateKeyPayload { chord }));
            std::thread::sleep(std::time::Duration::from_millis(50));
            println!("sent {}", chord.display());
            0
        }
        Err(e) => {
            eprintln!("solactl: {e}");
            3
        }
    }
}

fn emit_pointer(action: PointerAction) -> i32 {
    let mut client = bus::connect_or_exit();
    bus::emit(
        &mut client,
        Topic::SimulatePointer(SimulatePointerPayload { action }),
    );
    std::thread::sleep(std::time::Duration::from_millis(50));
    0
}

fn parse_button(s: &str) -> Option<PointerButton> {
    match s.to_ascii_lowercase().as_str() {
        "left" | "l" => Some(PointerButton::Left),
        "right" | "r" => Some(PointerButton::Right),
        "middle" | "m" => Some(PointerButton::Middle),
        _ => None,
    }
}

/// Parse a chord string like "Meta+Tab", "Ctrl+Shift+A", or just "Esc"
/// into a `KeyChord`. Case-insensitive on modifier names; key name is
/// case-insensitive too.
fn parse_chord(s: &str) -> Result<KeyChord, String> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("empty chord".into());
    }
    let key_str = parts[parts.len() - 1];
    let keycode = parse_keycode(key_str).ok_or_else(|| {
        format!("unknown key '{key_str}'. Try Tab, Esc, Enter, A-Z, 0-9, or a numpad key.")
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
