//! Pure keyboard/mouse → PTY byte encoder.
//!
//! No I/O, no iced runtime, no terminal-state mutation.  Every public
//! function is a deterministic mapping:
//!
//!   `event + modifiers (+ terminal mode) → Option<Vec<u8>>`
//!
//! The caller (Task 2.5 renderer) reads `term.lock().mode()`, extracts the
//! relevant [`alacritty_terminal::term::TermMode`] bits, destructs the iced
//! keyboard event, and delegates here.  This design makes the encoder fully
//! unit-testable without a display or PTY.
//!
//! # Mode-aware key encoding
//!
//! [`encode_key`] takes an explicit `mode: TermMode` so the caller doesn't
//! have to know which bits matter — the encoder inspects them directly.
//! Relevant bits used here:
//!
//! - `TermMode::APP_CURSOR`      — DECCKM: arrows/Home/End use `ESC O x`
//!   instead of `ESC [ x`.
//! - `TermMode::BRACKETED_PASTE` — checked by [`paste`], not by
//!   `encode_key`.

use alacritty_terminal::term::TermMode;
use iced::keyboard::{self, key::Named};

// ── Modifiers ──────────────────────────────────────────────────────────────

/// Modifier-key bitmask, wrapping [`iced::keyboard::Modifiers`].
///
/// Provides named constants (`NONE`, `CTRL`, `ALT`, `SHIFT`) and a
/// `From<iced::keyboard::Modifiers>` impl so callers can pass iced's type
/// directly or build one from scratch for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods(keyboard::Modifiers);

impl Mods {
    // Reserved for mouse-mode SGR encoding (Task 2.5 / future mouse reporting).
    #[allow(dead_code)]
    pub const NONE: Mods = Mods(keyboard::Modifiers::empty());
    #[allow(dead_code)]
    pub const CTRL: Mods = Mods(keyboard::Modifiers::CTRL);
    #[allow(dead_code)]
    pub const ALT: Mods = Mods(keyboard::Modifiers::ALT);
    #[allow(dead_code)]
    pub const SHIFT: Mods = Mods(keyboard::Modifiers::SHIFT);

    pub fn ctrl(self) -> bool {
        self.0.control()
    }
    pub fn alt(self) -> bool {
        self.0.alt()
    }
    pub fn shift(self) -> bool {
        self.0.shift()
    }
}

impl From<keyboard::Modifiers> for Mods {
    fn from(m: keyboard::Modifiers) -> Self {
        Mods(m)
    }
}

// ── MouseButton ───────────────────────────────────────────────────────────

/// Terminal mouse button identity for [`encode_mouse_sgr`].
// Reserved for mouse-mode SGR reporting (Task 2.5).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

// ── encode_key ────────────────────────────────────────────────────────────

/// Encode a named (non-printable) key into the byte sequence a PTY expects.
///
/// Returns `None` for keys the encoder doesn't handle (caller should fall
/// through to [`encode_char`] / the iced `text` field).
///
/// `mode` is the current [`TermMode`]; the encoder uses it to select between
/// normal-cursor sequences (`ESC [ …`) and application-cursor sequences
/// (`ESC O …`) when `TermMode::APP_CURSOR` is set (DECCKM).
///
/// For `Key::Character` with `Mods::CTRL` (incl. Ctrl+symbol like Ctrl-[),
/// this returns `None` for symbol keys; the caller MUST fall through to
/// [`encode_char`] so those are encoded correctly.
pub fn encode_key(key: &keyboard::Key, mods: Mods, mode: TermMode) -> Option<Vec<u8>> {
    // Ctrl-letter on a Character key → control byte (0x01..=0x1a).
    // We check this before the Named arm so Ctrl+Enter etc. still fall
    // through to the Named arm below.
    if mods.ctrl() {
        if let keyboard::Key::Character(s) = key {
            if let Some(encoded) = ctrl_char(s) {
                // Alt+Ctrl: prepend ESC (xterm Meta+Ctrl convention).
                return if mods.alt() {
                    let mut out = vec![0x1b];
                    out.extend_from_slice(&encoded);
                    Some(out)
                } else {
                    Some(encoded)
                };
            }
        }
    }

    // Alt-prefix on a Character key → ESC + char.
    if mods.alt() && !mods.ctrl() {
        if let keyboard::Key::Character(s) = key {
            if let Some(c) = s.chars().next() {
                if !c.is_control() {
                    let mut out = vec![0x1b];
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    return Some(out);
                }
            }
        }
    }

    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    match key {
        keyboard::Key::Named(named) => match named {
            Named::Enter => Some(b"\r".to_vec()),
            Named::Tab => {
                if mods.shift() {
                    // Shift-Tab = CSI Z (back-tab)
                    Some(b"\x1b[Z".to_vec())
                } else {
                    Some(b"\t".to_vec())
                }
            }
            Named::Backspace => {
                if mods.ctrl() {
                    // Ctrl-Backspace → word erase (0x08 = BS)
                    Some(vec![0x08])
                } else {
                    Some(vec![0x7f])
                }
            }
            Named::Escape => Some(vec![0x1b]),
            Named::Space => {
                if mods.ctrl() {
                    // Ctrl-Space → NUL
                    Some(vec![0x00])
                } else {
                    Some(b" ".to_vec())
                }
            }

            // Arrow keys: normal `ESC [ x` vs. application `ESC O x` (DECCKM).
            Named::ArrowUp => Some(if app_cursor {
                b"\x1bOA".to_vec()
            } else {
                b"\x1b[A".to_vec()
            }),
            Named::ArrowDown => Some(if app_cursor {
                b"\x1bOB".to_vec()
            } else {
                b"\x1b[B".to_vec()
            }),
            Named::ArrowRight => Some(if app_cursor {
                b"\x1bOC".to_vec()
            } else {
                b"\x1b[C".to_vec()
            }),
            Named::ArrowLeft => Some(if app_cursor {
                b"\x1bOD".to_vec()
            } else {
                b"\x1b[D".to_vec()
            }),

            // Home / End: also flip to application form under DECCKM.
            Named::Home => Some(if app_cursor {
                b"\x1bOH".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }),
            Named::End => Some(if app_cursor {
                b"\x1bOF".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }),

            // Other navigation keys use tilde-form regardless of cursor mode.
            Named::PageUp => Some(b"\x1b[5~".to_vec()),
            Named::PageDown => Some(b"\x1b[6~".to_vec()),
            Named::Insert => Some(b"\x1b[2~".to_vec()),
            Named::Delete => Some(b"\x1b[3~".to_vec()),

            // F-keys.
            Named::F1 => Some(b"\x1bOP".to_vec()),
            Named::F2 => Some(b"\x1bOQ".to_vec()),
            Named::F3 => Some(b"\x1bOR".to_vec()),
            Named::F4 => Some(b"\x1bOS".to_vec()),
            Named::F5 => Some(b"\x1b[15~".to_vec()),
            Named::F6 => Some(b"\x1b[17~".to_vec()),
            Named::F7 => Some(b"\x1b[18~".to_vec()),
            Named::F8 => Some(b"\x1b[19~".to_vec()),
            Named::F9 => Some(b"\x1b[20~".to_vec()),
            Named::F10 => Some(b"\x1b[21~".to_vec()),
            Named::F11 => Some(b"\x1b[23~".to_vec()),
            Named::F12 => Some(b"\x1b[24~".to_vec()),

            // Anything else: not handled here.
            _ => None,
        },

        // Character keys with Ctrl are handled above; fall through for plain
        // and Alt combos — the caller should use encode_char / text field.
        keyboard::Key::Character(_) => None,

        // Unidentified / Dead keys.
        _ => None,
    }
}

// ── encode_char ───────────────────────────────────────────────────────────

/// Encode a printable character (with modifiers) into PTY bytes.
///
/// - **Ctrl-letter** → control byte (0x01..=0x1a for a-z; extended symbols).
/// - **Alt+Ctrl-char** → `ESC` prefix + the Ctrl control byte (xterm Meta+Ctrl).
/// - **Alt-char** → `ESC` prefix + the UTF-8 bytes of the char.
/// - **Plain char** → its UTF-8 bytes.
///
/// Returns `None` only when there is genuinely nothing to send (e.g., a
/// control character that arrives without a meaningful encoding).
pub fn encode_char(c: char, mods: Mods) -> Option<Vec<u8>> {
    if mods.ctrl() {
        // Ctrl-letter → control code.
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphabetic() {
            let code = (lc as u8) - b'a' + 1; // a=0x01 … z=0x1a
            // Alt+Ctrl: prepend ESC (xterm Meta+Ctrl convention).
            return if mods.alt() {
                Some(vec![0x1b, code])
            } else {
                Some(vec![code])
            };
        }
        // Ctrl-symbol cases (common subset xterm honours).
        let code = match c {
            ' ' => Some(0x00u8),        // Ctrl-Space = NUL
            '[' | '{' => Some(0x1b),    // Ctrl-[ = ESC
            '\\' | '|' => Some(0x1c),   // Ctrl-\ = FS
            ']' | '}' => Some(0x1d),    // Ctrl-] = GS
            '^' | '~' => Some(0x1e),    // Ctrl-^ = RS  (0x5e & 0x1f = 0x1e)
            '`' => Some(0x00),          // Ctrl-` = NUL (0x60 & 0x1f = 0x00)
            '_' => Some(0x1f),          // Ctrl-_ = US
            '?' => Some(0x7f),          // Ctrl-? = DEL (readline backward-delete-char)
            _ => None,
        };
        if let Some(b) = code {
            // Alt+Ctrl: prepend ESC (xterm Meta+Ctrl convention).
            return if mods.alt() {
                Some(vec![0x1b, b])
            } else {
                Some(vec![b])
            };
        }
        // Fall through for unrecognised Ctrl combos.
    }

    if mods.alt() {
        // Alt-char → ESC prefix + UTF-8 bytes.
        let mut out = vec![0x1b];
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        return Some(out);
    }

    // Plain character — emit UTF-8.
    if !c.is_control() {
        let mut buf = [0u8; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes();
        return Some(bytes.to_vec());
    }

    None
}

// ── bracketed paste ───────────────────────────────────────────────────────

/// Wrap `text` with the bracketed-paste markers `ESC[200~` … `ESC[201~`.
///
/// The caller decides whether bracketed-paste mode is active by inspecting
/// `TermMode::BRACKETED_PASTE`; this function only does the wrapping.
/// Newlines inside `text` are NOT normalised here — use [`paste`] for that.
pub fn bracketed_paste(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

/// Send paste text, wrapping with bracketed-paste markers when the mode bit
/// is set.  Normalises line endings to `\r` as PTYs expect carriage-return.
/// CRLF (`\r\n`) is collapsed to a single `\r` before any stray `\n` → `\r`
/// substitution, preventing double-CR on Windows-style clipboard text.
pub fn paste(text: &str, mode: TermMode) -> Vec<u8> {
    // Normalise newlines: collapse CRLF first, then convert remaining bare LF.
    let normalised: std::borrow::Cow<str> = if text.contains('\n') {
        text.replace("\r\n", "\r").replace('\n', "\r").into()
    } else {
        text.into()
    };

    if mode.contains(TermMode::BRACKETED_PASTE) {
        bracketed_paste(&normalised)
    } else {
        normalised.as_bytes().to_vec()
    }
}

// ── mouse SGR ─────────────────────────────────────────────────────────────

/// Encode a mouse event as an SGR sequence: `ESC [ < b ; col ; row M/m`.
///
/// `col` and `row` are 1-based cell coordinates (the caller maps pixel
/// position → cell).  `pressed` is `true` for button-down / wheel events,
/// `false` for button-up.
///
/// The renderer (Task 2.5) decides whether mouse reporting is active by
/// checking `TermMode::MOUSE_REPORT_CLICK` / `TermMode::SGR_MOUSE` etc.;
/// this function is the pure encoder and does not inspect mode bits.
// Reserved for mouse-mode SGR reporting (Task 2.5).
#[allow(dead_code)]
pub fn encode_mouse_sgr(
    button: MouseButton,
    col: u16,
    row: u16,
    pressed: bool,
    mods: Mods,
) -> Vec<u8> {
    // SGR button code:
    //   0 = left, 1 = middle, 2 = right
    //   64 = wheel-up, 65 = wheel-down
    // Modifier bits added on top:
    //   +4  shift, +8  alt, +16 ctrl
    let base: u8 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    let mut code = base as u16;
    if mods.shift() {
        code += 4;
    }
    if mods.alt() {
        code += 8;
    }
    if mods.ctrl() {
        code += 16;
    }

    // Wheel events are always "press"; for normal buttons M=press, m=release.
    let action = match button {
        MouseButton::WheelUp | MouseButton::WheelDown => b'M',
        _ => if pressed { b'M' } else { b'm' },
    };

    format!("\x1b[<{};{};{}{}", code, col, row, action as char)
        .into_bytes()
}

// ── internal helpers ──────────────────────────────────────────────────────

/// Map a [`keyboard::Key::Character`] string under Ctrl to its control byte,
/// if the character is an ASCII letter.
fn ctrl_char(s: &str) -> Option<Vec<u8>> {
    let c = s.chars().next()?;
    let lc = c.to_ascii_lowercase();
    if lc.is_ascii_alphabetic() {
        return Some(vec![(lc as u8) - b'a' + 1]);
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{key::Named, Key};

    // Helper: normal mode (no special bits set).
    fn normal() -> TermMode {
        TermMode::empty()
    }

    // Helper: application-cursor mode (DECCKM).
    fn app_cursor() -> TermMode {
        TermMode::APP_CURSOR
    }

    // ── encode_key: basic named keys ──────────────────────────────────

    #[test]
    fn enter_is_cr() {
        assert_eq!(
            encode_key(&Key::Named(Named::Enter), Mods::NONE, normal()),
            Some(b"\r".to_vec())
        );
    }

    #[test]
    fn tab_is_ht() {
        assert_eq!(
            encode_key(&Key::Named(Named::Tab), Mods::NONE, normal()),
            Some(b"\t".to_vec())
        );
    }

    #[test]
    fn shift_tab_is_csi_z() {
        assert_eq!(
            encode_key(&Key::Named(Named::Tab), Mods::SHIFT, normal()),
            Some(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn backspace_is_del() {
        assert_eq!(
            encode_key(&Key::Named(Named::Backspace), Mods::NONE, normal()),
            Some(vec![0x7f])
        );
    }

    #[test]
    fn escape_is_esc() {
        assert_eq!(
            encode_key(&Key::Named(Named::Escape), Mods::NONE, normal()),
            Some(vec![0x1b])
        );
    }

    #[test]
    fn delete_is_csi_3_tilde() {
        assert_eq!(
            encode_key(&Key::Named(Named::Delete), Mods::NONE, normal()),
            Some(b"\x1b[3~".to_vec())
        );
    }

    #[test]
    fn insert_is_csi_2_tilde() {
        assert_eq!(
            encode_key(&Key::Named(Named::Insert), Mods::NONE, normal()),
            Some(b"\x1b[2~".to_vec())
        );
    }

    #[test]
    fn page_up_is_csi_5_tilde() {
        assert_eq!(
            encode_key(&Key::Named(Named::PageUp), Mods::NONE, normal()),
            Some(b"\x1b[5~".to_vec())
        );
    }

    #[test]
    fn page_down_is_csi_6_tilde() {
        assert_eq!(
            encode_key(&Key::Named(Named::PageDown), Mods::NONE, normal()),
            Some(b"\x1b[6~".to_vec())
        );
    }

    // ── encode_key: arrow keys, normal cursor mode ─────────────────────

    #[test]
    fn up_arrow_csi() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowUp), Mods::NONE, normal()),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn down_arrow_csi() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowDown), Mods::NONE, normal()),
            Some(b"\x1b[B".to_vec())
        );
    }

    #[test]
    fn right_arrow_csi() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowRight), Mods::NONE, normal()),
            Some(b"\x1b[C".to_vec())
        );
    }

    #[test]
    fn left_arrow_csi() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowLeft), Mods::NONE, normal()),
            Some(b"\x1b[D".to_vec())
        );
    }

    #[test]
    fn home_csi_normal() {
        assert_eq!(
            encode_key(&Key::Named(Named::Home), Mods::NONE, normal()),
            Some(b"\x1b[H".to_vec())
        );
    }

    #[test]
    fn end_csi_normal() {
        assert_eq!(
            encode_key(&Key::Named(Named::End), Mods::NONE, normal()),
            Some(b"\x1b[F".to_vec())
        );
    }

    // ── encode_key: application-cursor mode (DECCKM) ──────────────────

    #[test]
    fn up_arrow_app_cursor() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowUp), Mods::NONE, app_cursor()),
            Some(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn down_arrow_app_cursor() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowDown), Mods::NONE, app_cursor()),
            Some(b"\x1bOB".to_vec())
        );
    }

    #[test]
    fn right_arrow_app_cursor() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowRight), Mods::NONE, app_cursor()),
            Some(b"\x1bOC".to_vec())
        );
    }

    #[test]
    fn left_arrow_app_cursor() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowLeft), Mods::NONE, app_cursor()),
            Some(b"\x1bOD".to_vec())
        );
    }

    #[test]
    fn home_app_cursor() {
        assert_eq!(
            encode_key(&Key::Named(Named::Home), Mods::NONE, app_cursor()),
            Some(b"\x1bOH".to_vec())
        );
    }

    #[test]
    fn end_app_cursor() {
        assert_eq!(
            encode_key(&Key::Named(Named::End), Mods::NONE, app_cursor()),
            Some(b"\x1bOF".to_vec())
        );
    }

    // ── encode_key: Ctrl on Character keys ────────────────────────────

    #[test]
    fn ctrl_letter_via_encode_key() {
        // Ctrl-C through encode_key (Character variant) → ETX.
        assert_eq!(
            encode_key(
                &Key::Character("c".into()),
                Mods::CTRL,
                normal()
            ),
            Some(vec![0x03])
        );
    }

    // ── encode_char ───────────────────────────────────────────────────

    #[test]
    fn ctrl_a_is_soh() {
        assert_eq!(encode_char('a', Mods::CTRL), Some(vec![0x01]));
    }

    #[test]
    fn ctrl_c_is_etx() {
        assert_eq!(encode_char('c', Mods::CTRL), Some(vec![0x03]));
    }

    #[test]
    fn ctrl_z_is_sub() {
        assert_eq!(encode_char('z', Mods::CTRL), Some(vec![0x1a]));
    }

    #[test]
    fn ctrl_bracket_is_esc() {
        assert_eq!(encode_char('[', Mods::CTRL), Some(vec![0x1b]));
    }

    #[test]
    fn ctrl_backslash_is_fs() {
        assert_eq!(encode_char('\\', Mods::CTRL), Some(vec![0x1c]));
    }

    #[test]
    fn ctrl_caret_is_rs() {
        assert_eq!(encode_char('^', Mods::CTRL), Some(vec![0x1e]));
    }

    #[test]
    fn ctrl_underscore_is_us() {
        assert_eq!(encode_char('_', Mods::CTRL), Some(vec![0x1f]));
    }

    #[test]
    fn alt_x_is_esc_prefix() {
        assert_eq!(encode_char('x', Mods::ALT), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn plain_char_utf8() {
        assert_eq!(encode_char('a', Mods::NONE), Some(b"a".to_vec()));
        assert_eq!(encode_char('Z', Mods::NONE), Some(b"Z".to_vec()));
    }

    #[test]
    fn plain_unicode_char() {
        // Multi-byte UTF-8: U+00E9 LATIN SMALL LETTER E WITH ACUTE = 0xC3 0xA9
        let bytes = encode_char('é', Mods::NONE).unwrap();
        assert_eq!(bytes, "é".as_bytes());
    }

    // ── bracketed_paste / paste ────────────────────────────────────────

    #[test]
    fn bracketed_paste_wraps() {
        let result = bracketed_paste("hello");
        assert_eq!(result, b"\x1b[200~hello\x1b[201~".to_vec());
    }

    #[test]
    fn bracketed_paste_empty() {
        let result = bracketed_paste("");
        assert_eq!(result, b"\x1b[200~\x1b[201~".to_vec());
    }

    #[test]
    fn paste_with_mode_wraps() {
        let mode = TermMode::BRACKETED_PASTE;
        let result = paste("hello", mode);
        assert_eq!(result, b"\x1b[200~hello\x1b[201~".to_vec());
    }

    #[test]
    fn paste_without_mode_plain() {
        let result = paste("hello", TermMode::empty());
        assert_eq!(result, b"hello".to_vec());
    }

    #[test]
    fn paste_normalises_newlines() {
        // \n → \r in both bracketed and plain modes.
        let result = paste("foo\nbar", TermMode::empty());
        assert_eq!(result, b"foo\rbar".to_vec());
    }

    #[test]
    fn paste_bracketed_normalises_newlines() {
        let result = paste("foo\nbar", TermMode::BRACKETED_PASTE);
        assert_eq!(result, b"\x1b[200~foo\rbar\x1b[201~".to_vec());
    }

    // ── encode_mouse_sgr ──────────────────────────────────────────────

    #[test]
    fn mouse_sgr_left_press() {
        // Left button press at col=1 row=1, no modifiers → ESC[<0;1;1M
        let result = encode_mouse_sgr(MouseButton::Left, 1, 1, true, Mods::NONE);
        assert_eq!(result, b"\x1b[<0;1;1M".to_vec());
    }

    #[test]
    fn mouse_sgr_left_release() {
        let result = encode_mouse_sgr(MouseButton::Left, 5, 3, false, Mods::NONE);
        assert_eq!(result, b"\x1b[<0;5;3m".to_vec());
    }

    #[test]
    fn mouse_sgr_right_press() {
        let result = encode_mouse_sgr(MouseButton::Right, 10, 20, true, Mods::NONE);
        assert_eq!(result, b"\x1b[<2;10;20M".to_vec());
    }

    #[test]
    fn mouse_sgr_wheel_up() {
        let result = encode_mouse_sgr(MouseButton::WheelUp, 1, 1, true, Mods::NONE);
        assert_eq!(result, b"\x1b[<64;1;1M".to_vec());
    }

    #[test]
    fn mouse_sgr_wheel_down() {
        let result = encode_mouse_sgr(MouseButton::WheelDown, 1, 1, true, Mods::NONE);
        assert_eq!(result, b"\x1b[<65;1;1M".to_vec());
    }

    #[test]
    fn mouse_sgr_ctrl_modifier() {
        // Ctrl adds +16 to button code.
        let result = encode_mouse_sgr(MouseButton::Left, 1, 1, true, Mods::CTRL);
        assert_eq!(result, b"\x1b[<16;1;1M".to_vec());
    }

    #[test]
    fn mouse_sgr_shift_modifier() {
        // Shift adds +4 to button code.
        let result = encode_mouse_sgr(MouseButton::Left, 1, 1, true, Mods::SHIFT);
        assert_eq!(result, b"\x1b[<4;1;1M".to_vec());
    }

    #[test]
    fn mouse_sgr_alt_modifier() {
        // Alt adds +8 to button code.
        let result = encode_mouse_sgr(MouseButton::Middle, 2, 3, true, Mods::ALT);
        assert_eq!(result, b"\x1b[<9;2;3M".to_vec());
    }

    // ── Fix 1: Ctrl-? = DEL (0x7f), Ctrl-/ has no special mapping ────

    #[test]
    fn ctrl_question_is_del() {
        // Ctrl-? maps to DEL (0x7f) — readline backward-delete-char.
        assert_eq!(encode_char('?', Mods::CTRL), Some(vec![0x7f]));
    }

    #[test]
    fn ctrl_slash_has_no_special_mapping() {
        // '/' (0x2f) has no Ctrl-symbol mapping, so the Ctrl path falls
        // through.  '/' is not a control character, so the plain-char path
        // emits its UTF-8 byte unchanged (0x2f).  This confirms the old
        // '/' => 0x7f (DEL) arm is gone.
        assert_eq!(encode_char('/', Mods::CTRL), Some(vec![0x2f]));
    }

    // ── Fix 2: Alt+Ctrl combos get ESC prefix ─────────────────────────

    fn ctrl_alt() -> Mods {
        Mods(iced::keyboard::Modifiers::CTRL | iced::keyboard::Modifiers::ALT)
    }

    #[test]
    fn alt_ctrl_a_is_esc_soh() {
        // Alt+Ctrl+A → ESC 0x01 (Meta+Ctrl-A, xterm convention).
        assert_eq!(encode_char('a', ctrl_alt()), Some(vec![0x1b, 0x01]));
    }

    #[test]
    fn alt_ctrl_bracket_is_esc_esc() {
        // Alt+Ctrl+[ → ESC ESC (Meta+Ctrl-[).
        assert_eq!(encode_char('[', ctrl_alt()), Some(vec![0x1b, 0x1b]));
    }

    #[test]
    fn alt_ctrl_letter_via_encode_key() {
        // Alt+Ctrl+C through encode_key → ESC ETX.
        assert_eq!(
            encode_key(
                &Key::Character("c".into()),
                ctrl_alt(),
                normal()
            ),
            Some(vec![0x1b, 0x03])
        );
    }

    // ── Fix 3: paste() CRLF normalisation — no double-CR ─────────────

    #[test]
    fn paste_crlf_no_double_cr() {
        // "foo\r\nbar" must produce "foo\rbar", not "foo\r\rbar".
        let result = paste("foo\r\nbar", TermMode::empty());
        assert_eq!(result, b"foo\rbar".to_vec());
    }

    #[test]
    fn paste_bracketed_crlf_no_double_cr() {
        // Same check with bracketed-paste mode active.
        let result = paste("foo\r\nbar", TermMode::BRACKETED_PASTE);
        assert_eq!(result, b"\x1b[200~foo\rbar\x1b[201~".to_vec());
    }

    // ── Fix 4: Ctrl-backtick = NUL (0x00), not RS (0x1e) ─────────────

    #[test]
    fn ctrl_backtick_is_nul() {
        // '`' (0x60) & 0x1f = 0x00 (NUL).
        assert_eq!(encode_char('`', Mods::CTRL), Some(vec![0x00]));
    }

    #[test]
    fn ctrl_caret_is_rs_still() {
        // '^' (0x5e) & 0x1f = 0x1e (RS) — unchanged.
        assert_eq!(encode_char('^', Mods::CTRL), Some(vec![0x1e]));
    }

    #[test]
    fn ctrl_tilde_is_rs_still() {
        // '~' (0x7e) & 0x1f = 0x1e (RS) — unchanged.
        assert_eq!(encode_char('~', Mods::CTRL), Some(vec![0x1e]));
    }

    // ── Fix 5: F-key encoding coverage ───────────────────────────────
    // F1–F4 use SS3 form (ESC O x); F5–F12 use CSI tilde form.
    // F6 skips 16→17~, F11 skips 22→23~.

    #[test]
    fn f1_is_esc_op() {
        assert_eq!(
            encode_key(&Key::Named(Named::F1), Mods::NONE, normal()),
            Some(b"\x1bOP".to_vec())
        );
    }

    #[test]
    fn f2_is_esc_oq() {
        assert_eq!(
            encode_key(&Key::Named(Named::F2), Mods::NONE, normal()),
            Some(b"\x1bOQ".to_vec())
        );
    }

    #[test]
    fn f3_is_esc_or() {
        assert_eq!(
            encode_key(&Key::Named(Named::F3), Mods::NONE, normal()),
            Some(b"\x1bOR".to_vec())
        );
    }

    #[test]
    fn f4_is_esc_os() {
        assert_eq!(
            encode_key(&Key::Named(Named::F4), Mods::NONE, normal()),
            Some(b"\x1bOS".to_vec())
        );
    }

    #[test]
    fn f5_is_csi_15_tilde() {
        assert_eq!(
            encode_key(&Key::Named(Named::F5), Mods::NONE, normal()),
            Some(b"\x1b[15~".to_vec())
        );
    }

    #[test]
    fn f6_is_csi_17_tilde() {
        // Skips 16~ (not assigned in xterm ctlseqs).
        assert_eq!(
            encode_key(&Key::Named(Named::F6), Mods::NONE, normal()),
            Some(b"\x1b[17~".to_vec())
        );
    }

    #[test]
    fn f11_is_csi_23_tilde() {
        // Skips 22~ (not assigned in xterm ctlseqs).
        assert_eq!(
            encode_key(&Key::Named(Named::F11), Mods::NONE, normal()),
            Some(b"\x1b[23~".to_vec())
        );
    }

    #[test]
    fn f12_is_csi_24_tilde() {
        assert_eq!(
            encode_key(&Key::Named(Named::F12), Mods::NONE, normal()),
            Some(b"\x1b[24~".to_vec())
        );
    }
}
