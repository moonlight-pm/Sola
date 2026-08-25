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
    pub fn logo(self) -> bool {
        self.0.logo()
    }
}

impl From<keyboard::Modifiers> for Mods {
    fn from(m: keyboard::Modifiers) -> Self {
        Mods(m)
    }
}

/// Combine the three modifier sources used by the PTY input path.
///
/// - `event` — the mask on this `KeyPressed`
/// - `snapshot` — last `ModifiersChanged`
/// - `keys_held` — modifier keys seen as `KeyPressed` without `KeyReleased`
///
/// Shift / Ctrl / Alt are unioned from all three: Wayland/winit can deliver
/// Shift+Enter with an empty event mask while Shift is still held.
///
/// Super / Logo is compositor-owned on River. Writing the union back into the
/// snapshot (or trusting a stale `ModifiersChanged` that still has LOGO)
/// latches Super after a ⌘-chord when the Super *release* never arrives.
/// Every later key then looks Super-modified and is dropped until restart.
/// Trust only the current event mask and Super *key* tracking for LOGO.
pub fn merge_modifiers(
    event: keyboard::Modifiers,
    snapshot: keyboard::Modifiers,
    keys_held: keyboard::Modifiers,
) -> keyboard::Modifiers {
    let latchable =
        keyboard::Modifiers::SHIFT | keyboard::Modifiers::CTRL | keyboard::Modifiers::ALT;
    event | keys_held | (snapshot & latchable)
}

// ── MouseButton ───────────────────────────────────────────────────────────

/// Terminal mouse button identity for [`encode_mouse_sgr`] / [`wheel_dispatch`].
/// Click buttons are used by the SGR encoder (and its tests); the wheel path
/// only constructs `WheelUp`/`WheelDown` today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
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
            Named::Enter => Some(encode_enter(mods)),
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
            ' ' => Some(0x00u8),      // Ctrl-Space = NUL
            '[' | '{' => Some(0x1b),  // Ctrl-[ = ESC
            '\\' | '|' => Some(0x1c), // Ctrl-\ = FS
            ']' | '}' => Some(0x1d),  // Ctrl-] = GS
            '^' | '~' => Some(0x1e),  // Ctrl-^ = RS  (0x5e & 0x1f = 0x1e)
            '`' => Some(0x00),        // Ctrl-` = NUL (0x60 & 0x1f = 0x00)
            '_' => Some(0x1f),        // Ctrl-_ = US
            '?' => Some(0x7f),        // Ctrl-? = DEL (readline backward-delete-char)
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

// ── resolve_bytes ─────────────────────────────────────────────────────────

/// Everything a single key press carries, bundled so the resolver and the
/// kitty-protocol builder share one borrow instead of threading many args.
///
/// `key` is iced's base/layout key (Shift NOT applied); `modified_key` is
/// "all modifiers applied except Ctrl" (correct **case**/glyph for
/// Shift/Alt+Shift). `location` distinguishes the numpad (kitty disambiguates
/// numpad keys); `repeat` drives kitty event-type reporting.
pub struct KeyInput<'a> {
    pub key: &'a keyboard::Key,
    pub modified_key: &'a keyboard::Key,
    pub mods: Mods,
    pub mode: TermMode,
    pub location: keyboard::Location,
    pub text: Option<&'a str>,
    pub repeat: bool,
    /// True when the `DISAMBIGUATE_ESC_CODES` bit was SYNTHESISED from tmux's
    /// modifyOtherKeys (see [`crate::extkeys`]) rather than negotiated by a real
    /// kitty-aware app. Under modifyOtherKeys, unmodified Escape / numpad keys
    /// must stay legacy (bare `ESC`, plain digits) — encoding them as CSI-u
    /// leaks a stray `27u` to the app. Modified keys are encoded either way.
    pub modify_other_keys: bool,
}

/// Resolve a key press to the PTY byte sequence.
///
/// When the application has negotiated the **kitty keyboard protocol** (the
/// engine set one of the kitty [`TermMode`] bits — see [`kitty_active`]) and
/// this key is one the protocol wants escaped (see [`should_build_sequence`]),
/// the kitty encoder owns it ([`build_sequence`]); e.g. Shift+Enter →
/// `CSI 13;2u`, distinct from plain Enter's `CR`. This is the mechanism Claude
/// Code (and other TUIs) use to tell Shift+Enter from Enter.
///
/// Otherwise the legacy path runs, in priority order:
///   1. [`encode_key`] — named keys + Ctrl-letter on Character keys.
///   2. [`encode_char`] — printable Character keys (incl. Ctrl+symbol that
///      `encode_key` deliberately returns `None` for).
///   3. the platform `text` field — IME / printable that neither caught.
///
/// The printable legacy path is sourced from `modified_key`, NOT the base
/// `key` — otherwise Shift+a encodes as lowercase `a` (the base key is
/// layout-unshifted). Ctrl is excluded from `modified_key`, so Ctrl-letter
/// still computes the control byte correctly.
pub fn resolve_bytes(input: &KeyInput) -> Option<Vec<u8>> {
    // Modified Enter is forced through `encode_enter` *before* the kitty
    // branch. Two reasons:
    // 1. We always want CSI-u (or ESC+CR for Alt) for Shift/Ctrl/Alt+Enter,
    //    independent of negotiation state.
    // 2. Under a synthesised/real kitty mode, `build_sequence` returns `None`
    //    (and would emit nothing) when `modified_key` lost the Named::Enter
    //    identity — observed on some Wayland/winit paths under Shift.
    if is_enter_key(input) && enter_has_modifiers(input.mods) {
        return Some(encode_enter(input.mods));
    }

    if kitty_active(input.mode) && should_build_sequence(input) {
        // The kitty encoder owns this key. A `None` here (a key it doesn't
        // model, e.g. a bare modifier) means "emit nothing" — falling through
        // to the legacy path would send bytes the app doesn't expect while in
        // kitty mode.
        return build_sequence(input);
    }

    // Plain Enter (no modifiers): prefer the base key if `modified_key` lost
    // Named::Enter, so we still emit CR rather than falling through to text.
    if is_enter_key(input) {
        return Some(encode_enter(input.mods));
    }

    let mk = input.modified_key;
    encode_key(mk, input.mods, input.mode)
        .or_else(|| {
            if let keyboard::Key::Character(s) = mk {
                s.chars().next().and_then(|c| encode_char(c, input.mods))
            } else {
                None
            }
        })
        .or_else(|| {
            input
                .text
                .filter(|t| !t.is_empty())
                .map(|t| t.as_bytes().to_vec())
        })
}

/// True when this press is Enter by any of the identities iced may give us:
/// base key, modified key, or platform text of CR/LF.
fn is_enter_key(input: &KeyInput) -> bool {
    matches!(input.key, keyboard::Key::Named(Named::Enter))
        || matches!(input.modified_key, keyboard::Key::Named(Named::Enter))
        || matches!(input.text, Some("\r") | Some("\n"))
}

fn enter_has_modifiers(mods: Mods) -> bool {
    mods.shift() || mods.alt() || mods.ctrl() || mods.logo()
}

// ── Kitty keyboard protocol ─────────────────────────────────────────────────
//
// Faithful port of alacritty's `build_sequence` / `should_build_sequence`
// (alacritty/src/input/keyboard.rs) adapted from winit to iced key types. The
// engine (alacritty_terminal `Term`, with `Config::kitty_keyboard = true`)
// negotiates the protocol and tracks the mode bits; this is the encoder half.
//
// Scope vs. alacritty: key **release** events are not reported (iced's
// `on_input` only sees presses, so REPORT_EVENT_TYPES carries press/repeat
// only), and the kitty-specific functional codes for F13–F35, media keys, and
// bare modifier keys (CSI 57xxx u) are not emitted. Everything Claude Code and
// common TUIs rely on — Enter/Tab/Esc/Space/Backspace, arrows, nav, F1–F20,
// and modified character keys — is covered.

/// Terminator of a key escape sequence: a legacy final byte (`A`, `~`, …) or
/// the kitty `u`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Terminator {
    Legacy(char),
    Kitty,
}

impl Terminator {
    fn ch(self) -> char {
        match self {
            Terminator::Legacy(c) => c,
            Terminator::Kitty => 'u',
        }
    }
}

/// True when the application has negotiated any level of the kitty keyboard
/// protocol; only then does the kitty encoder run.
pub fn kitty_active(mode: TermMode) -> bool {
    mode.intersects(
        TermMode::REPORT_ALL_KEYS_AS_ESC
            | TermMode::DISAMBIGUATE_ESC_CODES
            | TermMode::REPORT_EVENT_TYPES,
    )
}

/// Kitty modifier bitmask (shift=1, alt=2, ctrl=4, super=8); the value sent in
/// an escape sequence is this `+ 1`.
fn kitty_mod_bits(mods: Mods) -> u8 {
    let mut b = 0u8;
    if mods.shift() {
        b |= 0b0001;
    }
    if mods.alt() {
        b |= 0b0010;
    }
    if mods.ctrl() {
        b |= 0b0100;
    }
    if mods.logo() {
        b |= 0b1000;
    }
    b
}

/// Whether `mods` is *exactly* Shift (no other modifier).
fn shift_only(mods: Mods) -> bool {
    mods.shift() && !mods.ctrl() && !mods.alt() && !mods.logo()
}

/// iced has no `NamedKey::to_text`; this mirrors winit's (Enter→CR,
/// Backspace→BS, Tab→HT, Space→SP, Escape→ESC, else None). Used by
/// [`should_build_sequence`] to decide whether an *unmodified* named key keeps
/// its legacy text form or becomes a kitty sequence.
fn named_to_text(named: &Named) -> Option<&'static str> {
    match named {
        Named::Enter => Some("\r"),
        Named::Backspace => Some("\x08"),
        Named::Tab => Some("\t"),
        Named::Space => Some(" "),
        Named::Escape => Some("\x1b"),
        _ => None,
    }
}

fn should_build_sequence(input: &KeyInput) -> bool {
    let mode = input.mode;
    if mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC) {
        return true;
    }

    let key = input.modified_key;
    let mods = input.mods;
    let any_mods = mods.shift() || mods.ctrl() || mods.alt() || mods.logo();
    let is_tab_enter_bs = matches!(
        key,
        keyboard::Key::Named(Named::Tab | Named::Enter | Named::Backspace)
    );

    // Unmodified Escape and numpad keys are disambiguated only under a REAL
    // kitty negotiation. tmux's modifyOtherKeys synthesises the disambiguate
    // bit but expects unmodified Escape to remain a bare `ESC` (and plain
    // numpad keys to remain their legacy bytes); encoding those as CSI-u is
    // what leaked a stray `27u` to the app. Modified keys are unaffected and
    // still encode as CSI-u under both protocols (so Shift+Enter etc. work).
    let unmodified_disambiguate = !input.modify_other_keys
        && (matches!(key, keyboard::Key::Named(Named::Escape))
            || input.location == keyboard::Location::Numpad);

    let disambiguate = mode.contains(TermMode::DISAMBIGUATE_ESC_CODES)
        && (unmodified_disambiguate || (any_mods && (!shift_only(mods) || is_tab_enter_bs)));

    if disambiguate {
        return true;
    }

    match key {
        keyboard::Key::Named(named) => named_to_text(named).is_none(),
        _ => input.text.map_or(true, |t| t.is_empty()),
    }
}

/// Build the kitty escape sequence for a key. Port of alacritty's
/// `build_sequence` (numpad → named functional → control char → textual),
/// then modifier/event-type/associated-text suffixes and the terminator.
/// Returns `None` for keys the encoder doesn't model.
fn build_sequence(input: &KeyInput) -> Option<Vec<u8>> {
    let mode = input.mode;
    let event_type = mode.contains(TermMode::REPORT_EVENT_TYPES) && input.repeat;
    let assoc_text = input.text.filter(|t| {
        mode.contains(TermMode::REPORT_ASSOCIATED_TEXT) && !t.is_empty() && !is_control_text(t)
    });
    let has_assoc = assoc_text.is_some();

    let (payload, terminator) = build_numpad(input)
        .or_else(|| build_named(input, has_assoc, event_type))
        .or_else(|| build_control_char(input))
        .or_else(|| build_textual(input))?;

    let mut out = format!("\x1b[{payload}");

    let mbits = kitty_mod_bits(input.mods);
    if event_type || mbits != 0 || has_assoc {
        out.push_str(&format!(";{}", mbits + 1));
    }

    // Event type: 1=press (the default, omitted), 2=repeat. Release (3) is
    // never reported — iced delivers only presses to the encode path.
    if event_type {
        out.push_str(":2");
    }

    if let Some(text) = assoc_text {
        let mut cps = text.chars().map(u32::from);
        if let Some(first) = cps.next() {
            out.push_str(&format!(";{first}"));
        }
        for cp in cps {
            out.push_str(&format!(":{cp}"));
        }
    }

    out.push(terminator.ch());
    Some(out.into_bytes())
}

/// Numpad keys get dedicated kitty codes so apps can tell them from the main
/// cluster. Keyed off iced's `Location::Numpad`.
fn build_numpad(input: &KeyInput) -> Option<(String, Terminator)> {
    if input.location != keyboard::Location::Numpad {
        return None;
    }
    let base = match input.modified_key {
        keyboard::Key::Character(s) => match s.as_str() {
            "0" => "57399",
            "1" => "57400",
            "2" => "57401",
            "3" => "57402",
            "4" => "57403",
            "5" => "57404",
            "6" => "57405",
            "7" => "57406",
            "8" => "57407",
            "9" => "57408",
            "." => "57409",
            "/" => "57410",
            "*" => "57411",
            "-" => "57412",
            "+" => "57413",
            "=" => "57415",
            _ => return None,
        },
        keyboard::Key::Named(named) => match named {
            Named::Enter => "57414",
            Named::ArrowLeft => "57417",
            Named::ArrowRight => "57418",
            Named::ArrowUp => "57419",
            Named::ArrowDown => "57420",
            Named::PageUp => "57421",
            Named::PageDown => "57422",
            Named::Home => "57423",
            Named::End => "57424",
            Named::Insert => "57425",
            Named::Delete => "57426",
            _ => return None,
        },
        _ => return None,
    };
    Some((base.to_string(), Terminator::Kitty))
}

/// Named functional keys (arrows, nav, F1–F20) in CSI form. With no modifiers
/// (and no event-type/associated-text) the leading parameter is omitted, so
/// these reproduce the exact legacy sequences; with modifiers they gain the
/// `1;<mods>` parameter (e.g. Shift+Up → `CSI 1;2A`).
fn build_named(
    input: &KeyInput,
    has_assoc: bool,
    event_type: bool,
) -> Option<(String, Terminator)> {
    let named = match input.modified_key {
        keyboard::Key::Named(n) => n,
        _ => return None,
    };
    let one_based = if kitty_mod_bits(input.mods) == 0 && !event_type && !has_assoc {
        ""
    } else {
        "1"
    };
    let (base, term): (&str, Terminator) = match named {
        Named::PageUp => ("5", Terminator::Legacy('~')),
        Named::PageDown => ("6", Terminator::Legacy('~')),
        Named::Insert => ("2", Terminator::Legacy('~')),
        Named::Delete => ("3", Terminator::Legacy('~')),
        Named::Home => (one_based, Terminator::Legacy('H')),
        Named::End => (one_based, Terminator::Legacy('F')),
        Named::ArrowLeft => (one_based, Terminator::Legacy('D')),
        Named::ArrowRight => (one_based, Terminator::Legacy('C')),
        Named::ArrowUp => (one_based, Terminator::Legacy('A')),
        Named::ArrowDown => (one_based, Terminator::Legacy('B')),
        Named::F1 => (one_based, Terminator::Legacy('P')),
        Named::F2 => (one_based, Terminator::Legacy('Q')),
        Named::F3 => (one_based, Terminator::Legacy('R')),
        Named::F4 => (one_based, Terminator::Legacy('S')),
        Named::F5 => ("15", Terminator::Legacy('~')),
        Named::F6 => ("17", Terminator::Legacy('~')),
        Named::F7 => ("18", Terminator::Legacy('~')),
        Named::F8 => ("19", Terminator::Legacy('~')),
        Named::F9 => ("20", Terminator::Legacy('~')),
        Named::F10 => ("21", Terminator::Legacy('~')),
        Named::F11 => ("23", Terminator::Legacy('~')),
        Named::F12 => ("24", Terminator::Legacy('~')),
        Named::F13 => ("25", Terminator::Legacy('~')),
        Named::F14 => ("26", Terminator::Legacy('~')),
        Named::F15 => ("28", Terminator::Legacy('~')),
        Named::F16 => ("29", Terminator::Legacy('~')),
        Named::F17 => ("31", Terminator::Legacy('~')),
        Named::F18 => ("32", Terminator::Legacy('~')),
        Named::F19 => ("33", Terminator::Legacy('~')),
        Named::F20 => ("34", Terminator::Legacy('~')),
        _ => return None,
    };
    Some((base.to_string(), term))
}

/// Control-character named keys → their kitty unicode codepoint (Tab=9,
/// Enter=13, Escape=27, Space=32, Backspace=127). This is where Shift+Enter
/// becomes `CSI 13;2u`. Bare modifier keys (kitty 57xxx) are intentionally not
/// modelled.
fn build_control_char(input: &KeyInput) -> Option<(String, Terminator)> {
    let named = match input.modified_key {
        keyboard::Key::Named(n) => n,
        _ => return None,
    };
    let base = match named {
        Named::Tab => "9",
        Named::Enter => "13",
        Named::Escape => "27",
        Named::Space => "32",
        Named::Backspace => "127",
        _ => return None,
    };
    Some((base.to_string(), Terminator::Kitty))
}

/// Printable character keys → kitty unicode codepoint of the *unshifted* key,
/// so the receiver sees the layout key plus a modifier flag (e.g. Shift+1 →
/// `49` (`1`) with Shift, not `33` (`!`)). With REPORT_ALTERNATE_KEYS the
/// shifted codepoint is appended as `base:alternate`.
fn build_textual(input: &KeyInput) -> Option<(String, Terminator)> {
    let s = match input.modified_key {
        keyboard::Key::Character(s) => s,
        _ => return None,
    };
    if s.chars().count() != 1 {
        return None;
    }
    let shift = input.mods.shift();
    let ch = s.chars().next().unwrap();
    let unshifted = if shift {
        ch.to_lowercase().next().unwrap_or(ch)
    } else {
        ch
    };

    let alternate_code = u32::from(ch);
    let mut unicode_code = u32::from(unshifted);

    // For keys whose glyph only changes with Shift (e.g. `1`→`!`), recover the
    // base codepoint from the layout key so the report carries the base key.
    if shift && alternate_code == unicode_code {
        if let keyboard::Key::Character(base) = input.key {
            if let Some(c) = base.chars().next() {
                unicode_code = u32::from(c);
            }
        }
    }

    let payload =
        if input.mode.contains(TermMode::REPORT_ALTERNATE_KEYS) && alternate_code != unicode_code {
            format!("{unicode_code}:{alternate_code}")
        } else {
            unicode_code.to_string()
        };
    Some((payload, Terminator::Kitty))
}

/// True for a single C0/DEL/C1 control byte — such text is reported via the
/// key code, not as associated text.
fn is_control_text(text: &str) -> bool {
    let b = match text.bytes().next() {
        Some(b) => b,
        None => return false,
    };
    text.len() == 1 && (b < 0x20 || (0x7f..=0x9f).contains(&b))
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
/// this function is the pure encoder and does not inspect mode bits — the
/// caller ([`wheel_dispatch`]) checks the mode.
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
        _ => {
            if pressed {
                b'M'
            } else {
                b'm'
            }
        }
    };

    format!("\x1b[<{};{};{}{}", code, col, row, action as char).into_bytes()
}

/// What a mouse-wheel notch should do, given the terminal's current mode.
///
/// Under sola's tmux config the *outer* emulator never enters the alternate
/// screen (tmux strips `smcup`/`rmcup`), so the classic "alt-screen +
/// ALTERNATE_SCROLL → arrow keys" branch alacritty uses never fires here and is
/// deliberately omitted. Only two outcomes matter: forward the wheel to a
/// mouse-tracking app, or scroll our own scrollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WheelAction {
    /// Forward the wheel to the application as a mouse report (already encoded).
    /// Emitted when the app has enabled mouse tracking and Shift is not held.
    /// Exactly **one** SGR/X10 report per wheel event — xterm-style. Multiplying
    /// by local-scroll line counts flooded mouse-tracking TUIs (Grok, etc.)
    /// into full-redraw storms that locked the UI.
    Report(Vec<u8>),
    /// Scroll the emulator's own scrollback by this many lines (positive = up
    /// into history, matching `alacritty_terminal::grid::Scroll::Delta`). The
    /// default, and the Shift override even under mouse tracking. Uses the full
    /// line-count multiplier so host history still tracks the wheel.
    Scroll(i32),
}

/// Route a mouse-wheel notch count to either a PTY mouse report or a local
/// scrollback scroll, mirroring alacritty's `scroll_terminal` priority ladder
/// (minus the dead alt-screen branch — see [`WheelAction`]).
///
/// `notches` is signed: positive scrolls up (into history / wheel-up). `col`
/// and `row` are the 1-based cell under the pointer, used as the report's
/// position. When the app has mouse tracking on (`TermMode::MOUSE_MODE`) and
/// Shift is not held, **one** report is emitted for the event (SGR when the app
/// negotiated `SGR_MOUSE`, else legacy X10) — not one per local-scroll line.
/// Otherwise (or with Shift held) the wheel scrolls our own buffer by `notches`.
///
/// Callers must skip zero `notches` (no event).
pub fn wheel_dispatch(mode: TermMode, mods: Mods, notches: i32, col: u16, row: u16) -> WheelAction {
    if notches == 0 {
        return WheelAction::Scroll(0);
    }
    if !mods.shift() && mode.intersects(TermMode::MOUSE_MODE) {
        let button = if notches > 0 {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        };
        let bytes = if mode.contains(TermMode::SGR_MOUSE) {
            encode_mouse_sgr(button, col, row, true, mods)
        } else {
            encode_mouse_x10(button, col, row, mods)
        };
        return WheelAction::Report(bytes);
    }
    WheelAction::Scroll(notches)
}

/// Encode a mouse event in the legacy X10 form `ESC [ M Cb Cx Cy`, where each
/// trailing byte is `32 + value` (button code, then 1-based col, then row).
/// Coordinates are clamped to the classic 223 limit. Used for apps that enabled
/// mouse tracking without SGR (no `TermMode::SGR_MOUSE`).
fn encode_mouse_x10(button: MouseButton, col: u16, row: u16, mods: Mods) -> Vec<u8> {
    let base: u8 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    let mut code = base;
    if mods.shift() {
        code += 4;
    }
    if mods.alt() {
        code += 8;
    }
    if mods.ctrl() {
        code += 16;
    }
    let coord = |v: u16| -> u8 { 32u8.saturating_add(v.min(223) as u8) };
    vec![0x1b, b'[', b'M', 32 + code, coord(col), coord(row)]
}

// ── internal helpers ──────────────────────────────────────────────────────

/// Encode Enter, including modified forms.
///
/// Plain Enter is CR.
///
/// **Shift+Enter and Alt+Enter** both emit the portable **ESC+CR** sequence
/// (`\x1b\r`). That is what Claude Code's VS Code `/terminal-setup` installs
/// for Shift+Enter (`sendSequence` of `\u001b\r`), and what Grok documents as
/// the Alt+Enter newline fallback. CSI `13;2u` is nicer when the app has a
/// working kitty keyboard path, but under tmux many apps (Grok included) skip
/// the kitty push — ESC+CR is the sequence they actually honour for newline.
///
/// **Ctrl+Enter** (and other multi-mod combos) still use CSI-u so Grok's
/// interject chord stays distinct.
fn encode_enter(mods: Mods) -> Vec<u8> {
    let shift = mods.shift();
    let alt = mods.alt();
    let ctrl = mods.ctrl();
    let logo = mods.logo();
    if !shift && !alt && !ctrl && !logo {
        return b"\r".to_vec();
    }
    // Shift and/or Alt, without Ctrl/Logo → portable newline (ESC+CR).
    // Matches Claude Code's VS Code Shift+Enter binding and Grok's Alt+Enter.
    if (shift || alt) && !ctrl && !logo {
        return b"\x1b\r".to_vec();
    }
    // Ctrl (and any multi-mod involving Ctrl/Logo) → CSI-u.
    let mut bits = 0u8;
    if shift {
        bits |= 0b0001;
    }
    if alt {
        bits |= 0b0010;
    }
    if ctrl {
        bits |= 0b0100;
    }
    if logo {
        bits |= 0b1000;
    }
    format!("\x1b[13;{}u", bits + 1).into_bytes()
}

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
    use iced::keyboard::{Key, key::Named};

    // Helper: normal mode (no special bits set).
    fn normal() -> TermMode {
        TermMode::empty()
    }

    // Helper: application-cursor mode (DECCKM).
    fn app_cursor() -> TermMode {
        TermMode::APP_CURSOR
    }

    // ── merge_modifiers: Super must not latch ─────────────────────────

    #[test]
    fn merge_keeps_snapshot_shift_when_event_mask_is_empty() {
        // Shift+Enter on Wayland: Enter arrives with an empty mask.
        let merged = merge_modifiers(
            keyboard::Modifiers::empty(),
            keyboard::Modifiers::SHIFT,
            keyboard::Modifiers::empty(),
        );
        assert!(merged.shift());
        assert!(!merged.logo());
    }

    #[test]
    fn merge_keeps_logo_on_the_chord_event() {
        let merged = merge_modifiers(
            keyboard::Modifiers::LOGO,
            keyboard::Modifiers::empty(),
            keyboard::Modifiers::empty(),
        );
        assert!(merged.logo());
    }

    #[test]
    fn merge_drops_stale_snapshot_logo() {
        // Super+T latched LOGO into ModifiersChanged; Super-up never came.
        let merged = merge_modifiers(
            keyboard::Modifiers::empty(),
            keyboard::Modifiers::LOGO | keyboard::Modifiers::SHIFT,
            keyboard::Modifiers::empty(),
        );
        assert!(merged.shift(), "Shift+Enter still needs the snapshot");
        assert!(!merged.logo(), "stale Super must not kill later keys");
    }

    #[test]
    fn merge_keeps_logo_while_super_key_is_held() {
        let merged = merge_modifiers(
            keyboard::Modifiers::empty(),
            keyboard::Modifiers::empty(),
            keyboard::Modifiers::LOGO,
        );
        assert!(merged.logo());
    }

    // ── resolve_bytes: case + priority chain ──────────────────────────

    // Build a legacy-path KeyInput: standard location, no repeat, base key ==
    // modified key (these tests run in non-kitty modes, where `key` is unused).
    fn ki<'a>(
        modified_key: &'a Key,
        mods: Mods,
        mode: TermMode,
        text: Option<&'a str>,
    ) -> KeyInput<'a> {
        KeyInput {
            key: modified_key,
            modified_key,
            mods,
            mode,
            location: keyboard::Location::Standard,
            text,
            repeat: false,
            modify_other_keys: false,
        }
    }

    // Shift+a: iced delivers base key=Character("a"), modified_key=
    // Character("A"). resolve_bytes is sourced from modified_key, so it
    // must emit the uppercase byte — the bug this guards against emitted
    // lowercase because the base key is layout-unshifted.
    #[test]
    fn shift_letter_is_uppercase() {
        assert_eq!(
            resolve_bytes(&ki(
                &Key::Character("A".into()),
                Mods::SHIFT,
                normal(),
                Some("A")
            )),
            Some(b"A".to_vec())
        );
    }

    // Plain lowercase letter still encodes lowercase.
    #[test]
    fn plain_letter_is_lowercase() {
        assert_eq!(
            resolve_bytes(&ki(
                &Key::Character("a".into()),
                Mods::NONE,
                normal(),
                Some("a")
            )),
            Some(b"a".to_vec())
        );
    }

    // Ctrl is excluded from modified_key, so Ctrl+a (modified_key="a")
    // still resolves to the control byte 0x01 rather than a printable.
    #[test]
    fn ctrl_letter_is_control_byte() {
        assert_eq!(
            resolve_bytes(&ki(&Key::Character("a".into()), Mods::CTRL, normal(), None)),
            Some(vec![0x01])
        );
    }

    // Named keys are unaffected by the modified_key wiring.
    #[test]
    fn resolve_named_enter() {
        assert_eq!(
            resolve_bytes(&ki(&Key::Named(Named::Enter), Mods::NONE, normal(), None)),
            Some(b"\r".to_vec())
        );
    }

    // Last-resort fallback: a key the encoders don't model, but the
    // platform produced text for it.
    #[test]
    fn resolve_falls_back_to_text() {
        assert_eq!(
            resolve_bytes(&ki(&Key::Unidentified, Mods::NONE, normal(), Some("é"))),
            Some("é".as_bytes().to_vec())
        );
    }

    // ── kitty keyboard protocol ───────────────────────────────────────

    // Disambiguate level — the mode Claude Code negotiates for Shift+Enter.
    fn kitty() -> TermMode {
        TermMode::DISAMBIGUATE_ESC_CODES
    }

    // KeyInput for kitty tests: standard location, no repeat, base key ==
    // modified key (overridable via the returned struct's pub fields).
    fn kk<'a>(modified_key: &'a Key, mods: Mods, mode: TermMode) -> KeyInput<'a> {
        KeyInput {
            key: modified_key,
            modified_key,
            mods,
            mode,
            location: keyboard::Location::Standard,
            text: None,
            repeat: false,
            modify_other_keys: false,
        }
    }

    // Modified Enter short-circuits before the kitty encoder: Shift+Enter is
    // the portable ESC+CR (same as Alt+Enter / Claude VS Code binding).
    #[test]
    fn shift_enter_is_esc_cr_even_under_kitty() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Enter), Mods::SHIFT, kitty())),
            Some(b"\x1b\r".to_vec())
        );
    }

    // Plain Enter under kitty keeps CR (no modifiers → legacy text form), so
    // pressing Enter still submits.
    #[test]
    fn plain_enter_under_kitty_is_cr() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Enter), Mods::NONE, kitty())),
            Some(b"\r".to_vec())
        );
    }

    // Shift+Enter uses the portable ESC+CR sequence (Claude Code's VS Code
    // Shift+Enter binding, Grok Alt+Enter newline fallback) — not CSI-u.
    #[test]
    fn shift_enter_without_kitty_is_esc_cr() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Enter), Mods::SHIFT, normal())),
            Some(b"\x1b\r".to_vec())
        );
    }

    // Wayland/winit quirk: base key stays Named::Enter under Shift, but
    // modified_key can be Unidentified and text is "\r". Must still get ESC+CR.
    #[test]
    fn shift_enter_with_unidentified_modified_key_is_esc_cr() {
        let base = Key::Named(Named::Enter);
        let modified = Key::Unidentified;
        let k = KeyInput {
            key: &base,
            modified_key: &modified,
            mods: Mods::SHIFT,
            mode: normal(),
            location: keyboard::Location::Standard,
            text: Some("\r"),
            repeat: false,
            modify_other_keys: false,
        };
        assert_eq!(resolve_bytes(&k), Some(b"\x1b\r".to_vec()));
    }

    // Same quirk, no Named identity at all — only text "\r" + Shift.
    #[test]
    fn shift_enter_from_text_cr_only_is_esc_cr() {
        let unidentified = Key::Unidentified;
        let k = KeyInput {
            key: &unidentified,
            modified_key: &unidentified,
            mods: Mods::SHIFT,
            mode: normal(),
            location: keyboard::Location::Standard,
            text: Some("\r"),
            repeat: false,
            modify_other_keys: false,
        };
        assert_eq!(resolve_bytes(&k), Some(b"\x1b\r".to_vec()));
    }

    // Ctrl+Enter (Grok interject) is CSI 13;5u in the legacy path too.
    #[test]
    fn ctrl_enter_without_kitty_is_csi_u() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Enter), Mods::CTRL, normal())),
            Some(b"\x1b[13;5u".to_vec())
        );
    }

    // Alt+Enter is the legacy ESC+CR newline fallback (Grok/xterm.js).
    #[test]
    fn alt_enter_without_kitty_is_esc_cr() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Enter), Mods::ALT, normal())),
            Some(b"\x1b\r".to_vec())
        );
    }

    // Ctrl+Enter under kitty → CSI 13;5u (ctrl bits=4 → modifier 5).
    #[test]
    fn ctrl_enter_is_kitty_csi_u() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Enter), Mods::CTRL, kitty())),
            Some(b"\x1b[13;5u".to_vec())
        );
    }

    // A plain printable key under disambiguate level is NOT escaped — it sends
    // its text, exactly like legacy. (should_build_sequence returns false.)
    #[test]
    fn plain_letter_under_kitty_is_text() {
        let key = Key::Character("a".into());
        let mut k = kk(&key, Mods::NONE, kitty());
        k.text = Some("a");
        assert_eq!(resolve_bytes(&k), Some(b"a".to_vec()));
    }

    // Ctrl+letter under kitty → CSI <codepoint>;5u (here 'a' = 97).
    #[test]
    fn ctrl_letter_under_kitty_is_csi_u() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Character("a".into()), Mods::CTRL, kitty())),
            Some(b"\x1b[97;5u".to_vec())
        );
    }

    // Shift+Tab under kitty → CSI 9;2u (Tab is one of the always-disambiguated
    // keys even with shift-only).
    #[test]
    fn shift_tab_is_kitty_csi_u() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Tab), Mods::SHIFT, kitty())),
            Some(b"\x1b[9;2u".to_vec())
        );
    }

    // Plain arrow under kitty reproduces the exact legacy CSI form (no leading
    // parameter), so unmodified cursor movement is unaffected.
    #[test]
    fn plain_arrow_under_kitty_is_legacy_form() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::ArrowUp), Mods::NONE, kitty())),
            Some(b"\x1b[A".to_vec())
        );
    }

    // Modified arrow gains the 1;<mods> parameter: Shift+Up → CSI 1;2A.
    #[test]
    fn shift_arrow_under_kitty_has_modifier() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::ArrowUp), Mods::SHIFT, kitty())),
            Some(b"\x1b[1;2A".to_vec())
        );
    }

    // Under REPORT_ALL_KEYS_AS_ESC, Shift+'1' reports the *base* key ('1' =
    // 49), not the shifted glyph ('!' = 33): CSI 49;2u. (At plain disambiguate
    // level a printable like this is left as text and not escaped.)
    #[test]
    fn shift_digit_reports_base_key() {
        let one = Key::Character("1".into());
        let bang = Key::Character("!".into());
        let k = KeyInput {
            key: &one,
            modified_key: &bang,
            mods: Mods::SHIFT,
            mode: TermMode::REPORT_ALL_KEYS_AS_ESC,
            location: keyboard::Location::Standard,
            text: Some("!"),
            repeat: false,
            modify_other_keys: false,
        };
        assert_eq!(resolve_bytes(&k), Some(b"\x1b[49;2u".to_vec()));
    }

    // REPORT_ALL_KEYS_AS_ESC escapes even an unmodified printable: 'a' → CSI 97u.
    #[test]
    fn report_all_keys_escapes_plain_letter() {
        let key = Key::Character("a".into());
        let mut k = kk(&key, Mods::NONE, TermMode::REPORT_ALL_KEYS_AS_ESC);
        k.text = Some("a");
        assert_eq!(resolve_bytes(&k), Some(b"\x1b[97u".to_vec()));
    }

    // Event-type reporting: a key *repeat* appends ":2" after the modifier
    // field. Up-arrow repeat with no mods → CSI 1;1:2A.
    #[test]
    fn repeat_appends_event_type() {
        let key = Key::Named(Named::ArrowUp);
        let mut k = kk(
            &key,
            Mods::NONE,
            TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_EVENT_TYPES,
        );
        k.repeat = true;
        assert_eq!(resolve_bytes(&k), Some(b"\x1b[1;1:2A".to_vec()));
    }

    // Numpad keys are disambiguated under kitty: numpad Enter → CSI 57414u.
    #[test]
    fn numpad_enter_is_kitty_code() {
        let key = Key::Named(Named::Enter);
        let mut k = kk(&key, Mods::NONE, kitty());
        k.location = keyboard::Location::Numpad;
        assert_eq!(resolve_bytes(&k), Some(b"\x1b[57414u".to_vec()));
    }

    // Under tmux modifyOtherKeys (synthetic disambiguate), an UNMODIFIED Escape
    // must be a bare ESC — not `CSI 27 u`. Encoding it as CSI-u leaked a literal
    // `27u` into the app. Regression for the modifyOtherKeys/CSI-u work.
    #[test]
    fn escape_unmodified_under_modify_other_keys_is_bare_esc() {
        let key = Key::Named(Named::Escape);
        let mut k = kk(&key, Mods::NONE, kitty());
        k.modify_other_keys = true;
        assert_eq!(resolve_bytes(&k), Some(b"\x1b".to_vec()));
    }

    // A REAL kitty app that negotiated disambiguate DOES want Escape as CSI-u,
    // so that path is preserved (modify_other_keys defaults false in `kk`).
    #[test]
    fn escape_unmodified_under_real_kitty_is_csi_u() {
        assert_eq!(
            resolve_bytes(&kk(&Key::Named(Named::Escape), Mods::NONE, kitty())),
            Some(b"\x1b[27u".to_vec())
        );
    }

    // Shift+Enter under modifyOtherKeys is still ESC+CR (portable newline).
    #[test]
    fn shift_enter_under_modify_other_keys_is_esc_cr() {
        let key = Key::Named(Named::Enter);
        let mut k = kk(&key, Mods::SHIFT, kitty());
        k.modify_other_keys = true;
        assert_eq!(resolve_bytes(&k), Some(b"\x1b\r".to_vec()));
    }

    // Unmodified numpad under modifyOtherKeys stays legacy (numpad Enter → CR),
    // not the kitty numpad CSI-u code.
    #[test]
    fn numpad_unmodified_under_modify_other_keys_is_legacy() {
        let key = Key::Named(Named::Enter);
        let mut k = kk(&key, Mods::NONE, kitty());
        k.location = keyboard::Location::Numpad;
        k.modify_other_keys = true;
        assert_eq!(resolve_bytes(&k), Some(b"\r".to_vec()));
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
            encode_key(&Key::Character("c".into()), Mods::CTRL, normal()),
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

    // ── wheel_dispatch (route a wheel notch by terminal mode) ─────────

    #[test]
    fn wheel_no_mouse_mode_scrolls_local() {
        // Bare shell (no mouse tracking): the wheel scrolls the emulator's own
        // scrollback, unchanged from the pre-fix behaviour.
        let action = wheel_dispatch(TermMode::empty(), Mods::NONE, 5, 10, 3);
        assert_eq!(action, WheelAction::Scroll(5));
    }

    #[test]
    fn wheel_mouse_mode_sgr_forwards_report_up() {
        // App with SGR mouse tracking: one wheel-up notch → one SGR report
        // (button 64) at the cursor cell, not a local scroll.
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let action = wheel_dispatch(mode, Mods::NONE, 1, 7, 2);
        assert_eq!(action, WheelAction::Report(b"\x1b[<64;7;2M".to_vec()));
    }

    #[test]
    fn wheel_mouse_mode_sgr_forwards_report_down() {
        // Negative notches = wheel down → button 65.
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let action = wheel_dispatch(mode, Mods::NONE, -1, 7, 2);
        assert_eq!(action, WheelAction::Report(b"\x1b[<65;7;2M".to_vec()));
    }

    #[test]
    fn wheel_mouse_mode_emits_one_report_per_event() {
        // Local-scroll multiplies by lines-per-notch (e.g. 3), but mouse-mode
        // reports must stay one SGR event — otherwise TUIs redraw once per
        // virtual line and flood the PTY/UI.
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let action = wheel_dispatch(mode, Mods::NONE, 3, 1, 1);
        assert_eq!(action, WheelAction::Report(b"\x1b[<64;1;1M".to_vec()));
    }

    #[test]
    fn wheel_mouse_mode_zero_notches_is_noop_scroll() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let action = wheel_dispatch(mode, Mods::NONE, 0, 1, 1);
        assert_eq!(action, WheelAction::Scroll(0));
    }

    #[test]
    fn wheel_shift_forces_local_scroll_even_in_mouse_mode() {
        // Shift is the escape hatch: scroll the emulator's own history even
        // while an app has mouse tracking on (xterm/alacritty convention).
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let action = wheel_dispatch(mode, Mods::SHIFT, 4, 5, 5);
        assert_eq!(action, WheelAction::Scroll(4));
    }

    #[test]
    fn wheel_mouse_mode_without_sgr_uses_x10() {
        // Mouse tracking without SGR (no DECSET 1006) → legacy X10 report:
        // ESC [ M  (32+button)  (32+col)  (32+row). 32+64=96, 32+1=33, 32+1=33.
        let mode = TermMode::MOUSE_REPORT_CLICK;
        let action = wheel_dispatch(mode, Mods::NONE, 1, 1, 1);
        assert_eq!(
            action,
            WheelAction::Report(vec![0x1b, b'[', b'M', 96, 33, 33])
        );
    }

    #[test]
    fn wheel_motion_only_mouse_mode_still_reports() {
        // MOUSE_MODE covers motion and drag too, not just click; any of them
        // means the app wants the wheel.
        let mode = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        let action = wheel_dispatch(mode, Mods::NONE, -1, 4, 8);
        assert_eq!(action, WheelAction::Report(b"\x1b[<65;4;8M".to_vec()));
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
            encode_key(&Key::Character("c".into()), ctrl_alt(), normal()),
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
