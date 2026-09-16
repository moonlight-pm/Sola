//! Parse `solactl browser key --chord` into CDP `Input.dispatchKeyEvent`s.
//!
//! Coordinates and keys stay in the tab. This is not a compositor chord.

use serde::{Deserialize, Serialize};

/// One CDP `Input.dispatchKeyEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CdpKeyEvent {
    pub ty: String,
    pub key: String,
    pub code: String,
    pub vk: i32,
    pub modifiers: i32,
    pub text: String,
}

/// CDP modifier bits: Alt=1, Ctrl=2, Meta=4, Shift=8.
const ALT: i32 = 1;
const CTRL: i32 = 2;
const META: i32 = 4;
const SHIFT: i32 = 8;

pub fn parse_chord(raw: &str) -> Result<Vec<CdpKeyEvent>, String> {
    let parts: Vec<&str> = raw
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("empty chord".into());
    }
    let key_str = parts[parts.len() - 1];
    let mut modifiers = 0i32;
    for m in &parts[..parts.len() - 1] {
        match m.to_ascii_lowercase().as_str() {
            "meta" | "super" | "win" | "cmd" | "command" => modifiers |= META,
            "alt" | "option" => modifiers |= ALT,
            "ctrl" | "control" => modifiers |= CTRL,
            "shift" => modifiers |= SHIFT,
            other => return Err(format!("unknown modifier '{other}'")),
        }
    }
    let (key, code, vk, printable) =
        map_key(key_str).ok_or_else(|| format!("unknown key '{key_str}'"))?;
    let text = if modifiers & CTRL != 0 || modifiers & META != 0 || modifiers & ALT != 0 {
        String::new()
    } else {
        printable
    };
    let mut out = Vec::with_capacity(3);
    out.push(CdpKeyEvent {
        ty: "keyDown".into(),
        key: key.clone(),
        code: code.clone(),
        vk,
        modifiers,
        text: text.clone(),
    });
    if !text.is_empty() {
        out.push(CdpKeyEvent {
            ty: "char".into(),
            key: key.clone(),
            code: code.clone(),
            vk,
            modifiers,
            text: text.clone(),
        });
    }
    out.push(CdpKeyEvent {
        ty: "keyUp".into(),
        key,
        code,
        vk,
        modifiers,
        text: String::new(),
    });
    Ok(out)
}

fn map_key(s: &str) -> Option<(String, String, i32, String)> {
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "enter" | "return" => Some(("Enter".into(), "Enter".into(), 0x0D, "\r".into())),
        "escape" | "esc" => Some(("Escape".into(), "Escape".into(), 0x1B, String::new())),
        "tab" => Some(("Tab".into(), "Tab".into(), 0x09, String::new())),
        "backspace" | "bs" => Some(("Backspace".into(), "Backspace".into(), 0x08, String::new())),
        "space" | " " => Some((" ".into(), "Space".into(), 0x20, " ".into())),
        "delete" | "del" => Some(("Delete".into(), "Delete".into(), 0x2E, String::new())),
        "arrowup" | "up" => Some(("ArrowUp".into(), "ArrowUp".into(), 0x26, String::new())),
        "arrowdown" | "down" => Some(("ArrowDown".into(), "ArrowDown".into(), 0x28, String::new())),
        "arrowleft" | "left" => Some(("ArrowLeft".into(), "ArrowLeft".into(), 0x25, String::new())),
        "arrowright" | "right" => Some((
            "ArrowRight".into(),
            "ArrowRight".into(),
            0x27,
            String::new(),
        )),
        "home" => Some(("Home".into(), "Home".into(), 0x24, String::new())),
        "end" => Some(("End".into(), "End".into(), 0x23, String::new())),
        "pageup" | "pgup" => Some(("PageUp".into(), "PageUp".into(), 0x21, String::new())),
        "pagedown" | "pgdn" => Some(("PageDown".into(), "PageDown".into(), 0x22, String::new())),
        _ if s.chars().count() == 1 => {
            let c = s.chars().next()?;
            if c.is_ascii_alphabetic() {
                let u = c.to_ascii_uppercase();
                let vk = u as i32;
                let ch = c.to_ascii_lowercase().to_string();
                Some((ch.clone(), format!("Key{u}"), vk, ch))
            } else if c.is_ascii_digit() {
                Some((
                    c.to_string(),
                    format!("Digit{c}"),
                    0x30 + (c as i32 - '0' as i32),
                    c.to_string(),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_is_enter() {
        let ev = parse_chord("Return").unwrap();
        assert_eq!(ev[0].ty, "keyDown");
        assert_eq!(ev[0].key, "Enter");
        assert_eq!(ev[0].vk, 0x0D);
        assert_eq!(ev[1].ty, "char");
        assert_eq!(ev[2].ty, "keyUp");
    }

    #[test]
    fn control_enter_has_no_char() {
        let ev = parse_chord("Control+Enter").unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].modifiers, CTRL);
        assert_eq!(ev[0].key, "Enter");
        assert_eq!(ev[1].ty, "keyUp");
    }

    #[test]
    fn escape_chord() {
        let ev = parse_chord("Escape").unwrap();
        assert_eq!(ev[0].vk, 0x1B);
        assert!(ev.iter().all(|e| e.ty != "char"));
    }
}
