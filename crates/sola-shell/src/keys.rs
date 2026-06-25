//! Chord-event handling for the shell.
//!
//! sola-river registers chords with River's xkb-bindings protocol on our
//! behalf and emits `Topic::Chord` / `Topic::ChordReleased` when they fire.
//! This module:
//!   - Converts a shell-side `KeyChord` into a River `(keysym, modifiers)`
//!     pair (`to_registered`).
//!   - Converts the reverse (`from_chord_event`) so the shell can dispatch
//!     using its existing shortcut-matching logic.
//!   - Provides the keycode/keysym translation tables used by zoning chord
//!     registration.
//!
//! Dispatch logic (handle_chord / handle_chord_released) references Shell
//! state; that wiring lands in Task 10 (chord dispatch into Shell::update).
use sola_bus::topics::{ChordEvent, RegisteredChord};
use sola_core::{KeyChord, KeyCode};

// River modifier bits (from `modifiers` enum in river-window-management-v1.xml).
const MOD_SHIFT: u32 = 1;
const MOD_CTRL: u32 = 4;
const MOD_ALT: u32 = 8; // mod1
const MOD_SUPER: u32 = 64; // mod4

// XK_KP_0 .. XK_KP_9 run contiguously starting at 0xFFB0 (NumLock on).
const KEYSYM_KP_0: u32 = 0xFFB0;
// XK_Super_L — the left Super/Meta key on a stock xkb layout.
pub const KEYSYM_SUPER_L: u32 = 0xFFEB;
pub const KEYSYM_ESCAPE: u32 = 0xFF1B;

// Numpad navigation keysyms (what the keys produce when NumLock is off).
// Registering these alongside the KP_0..KP_9 digits lets zoning chords
// fire regardless of NumLock state.
const KEYSYM_KP_UP: u32 = 0xFF97;
const KEYSYM_KP_LEFT: u32 = 0xFF96;
const KEYSYM_KP_RIGHT: u32 = 0xFF98;
const KEYSYM_KP_DOWN: u32 = 0xFF99;
const KEYSYM_KP_BEGIN: u32 = 0xFF9D;
const KEYSYM_KP_INSERT: u32 = 0xFF9E;
const KEYSYM_KP_DELETE: u32 = 0xFF9F;

pub fn to_registered(chord: &KeyChord) -> RegisteredChord {
    RegisteredChord {
        keysym: keycode_to_keysym(chord.keycode),
        modifiers: river_modifiers(chord),
    }
}

/// For keycodes whose keysym changes with NumLock state, return the
/// alternate keysym (NumLock off) so the shell can register both and
/// fire its action regardless of NumLock state.
pub fn to_registered_alt(chord: &KeyChord) -> Option<RegisteredChord> {
    let alt = match chord.keycode {
        KeyCode::KP_8 => Some(KEYSYM_KP_UP),
        KeyCode::KP_4 => Some(KEYSYM_KP_LEFT),
        KeyCode::KP_5 => Some(KEYSYM_KP_BEGIN),
        KeyCode::KP_6 => Some(KEYSYM_KP_RIGHT),
        KeyCode::KP_2 => Some(KEYSYM_KP_DOWN),
        KeyCode::KP_0 => Some(KEYSYM_KP_INSERT),
        KeyCode::KP_DECIMAL => Some(KEYSYM_KP_DELETE),
        _ => None,
    }?;
    Some(RegisteredChord {
        keysym: alt,
        modifiers: river_modifiers(chord),
    })
}

/// Global media keysyms (xkbcommon `XF86Audio*`, the `0x1008FFxx` range)
/// paired with their `solactl media` action. Registered as bare,
/// no-modifier global chords (see `emit_registered_chords`) so the
/// keyboard's media keys control playback/volume regardless of focus —
/// River does not deliver bound keys to any window. Two keysyms map to
/// `play-pause` because some keyboards emit `XF86AudioPause` for the
/// combined play/pause key.
///
/// Apple Magic Keyboards can emit slightly different keysyms; confirm with
/// `nix-shell -p libinput --run 'libinput debug-events'` (press each media
/// key, note the resulting `XF86Audio*` keysym) and adjust this table.
pub const MEDIA_KEYS: &[(u32, &str)] = &[
    (0x1008FF14, "play-pause"), // XF86AudioPlay
    (0x1008FF31, "play-pause"), // XF86AudioPause
    (0x1008FF17, "next"),       // XF86AudioNext
    (0x1008FF16, "prev"),       // XF86AudioPrev
    (0x1008FF12, "mute"),       // XF86AudioMute
    (0x1008FF13, "vol-up"),     // XF86AudioRaiseVolume
    (0x1008FF11, "vol-down"),   // XF86AudioLowerVolume
];

/// The `solactl media` action for a media keysym, if it is one. The shell
/// dispatches these out-of-process rather than through `from_chord_event`
/// (they map to no shell `KeyChord`).
pub fn media_action(keysym: u32) -> Option<&'static str> {
    MEDIA_KEYS
        .iter()
        .find(|(sym, _)| *sym == keysym)
        .map(|(_, action)| *action)
}

fn river_modifiers(c: &KeyChord) -> u32 {
    let mut m = 0u32;
    if c.shift {
        m |= MOD_SHIFT;
    }
    if c.ctrl {
        m |= MOD_CTRL;
    }
    if c.alt {
        m |= MOD_ALT;
    }
    if c.meta {
        m |= MOD_SUPER;
    }
    m
}

/// Map the shell's evdev-style KeyCode to the xkbcommon keysym River expects.
fn keycode_to_keysym(k: KeyCode) -> u32 {
    match k {
        KeyCode::TAB => 0xFF09,
        KeyCode::SPACE => 0x20,
        KeyCode::GRAVE => 0x60,
        KeyCode::BACKSPACE => 0xFF08,
        KeyCode::LEFT => 0xFF51,
        KeyCode::UP => 0xFF52,
        KeyCode::RIGHT => 0xFF53,
        KeyCode::DOWN => 0xFF54,
        KeyCode::ENTER => 0xFF0D,
        KeyCode::ESCAPE => 0xFF1B,
        // Top-row digit keys — ASCII '0'..'9'.
        KeyCode::KEY_0 => b'0' as u32,
        KeyCode::KEY_1 => b'1' as u32,
        KeyCode::KEY_2 => b'2' as u32,
        KeyCode::KEY_3 => b'3' as u32,
        KeyCode::KEY_4 => b'4' as u32,
        KeyCode::KEY_5 => b'5' as u32,
        KeyCode::KEY_6 => b'6' as u32,
        KeyCode::KEY_7 => b'7' as u32,
        KeyCode::KEY_8 => b'8' as u32,
        KeyCode::KEY_9 => b'9' as u32,
        // Zoning uses numpad digits + equal/decimal.
        KeyCode::KP_0 => KEYSYM_KP_0,
        KeyCode::KP_2 => KEYSYM_KP_0 + 2,
        KeyCode::KP_4 => KEYSYM_KP_0 + 4,
        KeyCode::KP_5 => KEYSYM_KP_0 + 5,
        KeyCode::KP_6 => KEYSYM_KP_0 + 6,
        KeyCode::KP_8 => KEYSYM_KP_0 + 8,
        KeyCode::KP_EQUAL => 0xFFBD,   // XK_KP_Equal
        KeyCode::KP_DECIMAL => 0xFFAE, // XK_KP_Decimal
        KeyCode::KP_ENTER => 0xFF8D,    // XK_KP_Enter
        KeyCode::KP_MULTIPLY => 0xFFAA, // XK_KP_Multiply — floats the window
        KeyCode::F12 => 0xFFC9,         // XK_F12
        _ => {
            if let Some(sym) = letter_keysym(k) {
                sym
            } else {
                // Unknown — pass through so at least sola-river can log.
                k.raw()
            }
        }
    }
}

// XKB letter keysyms are lowercase (XK_a = 0x61 .. XK_z = 0x7A). River
// emits those when Shift isn't held, so the chord registration must
// match on lowercase — matching on uppercase meant Meta+<letter>
// bindings never fired.
const LETTER_KEYCODES: &[(KeyCode, u8)] = &[
    (KeyCode::A, b'a'),
    (KeyCode::B, b'b'),
    (KeyCode::C, b'c'),
    (KeyCode::D, b'd'),
    (KeyCode::E, b'e'),
    (KeyCode::F, b'f'),
    (KeyCode::G, b'g'),
    (KeyCode::H, b'h'),
    (KeyCode::I, b'i'),
    (KeyCode::J, b'j'),
    (KeyCode::K, b'k'),
    (KeyCode::L, b'l'),
    (KeyCode::M, b'm'),
    (KeyCode::N, b'n'),
    (KeyCode::O, b'o'),
    (KeyCode::P, b'p'),
    (KeyCode::Q, b'q'),
    (KeyCode::R, b'r'),
    (KeyCode::S, b's'),
    (KeyCode::T, b't'),
    (KeyCode::U, b'u'),
    (KeyCode::V, b'v'),
    (KeyCode::W, b'w'),
    (KeyCode::X, b'x'),
    (KeyCode::Y, b'y'),
    (KeyCode::Z, b'z'),
];

fn letter_keysym(k: KeyCode) -> Option<u32> {
    LETTER_KEYCODES
        .iter()
        .find(|(code, _)| *code == k)
        .map(|(_, c)| *c as u32)
}

/// Inverse of `to_registered`. Returns `None` for keysyms the shell never
/// registers.
pub fn from_chord_event(evt: &ChordEvent) -> Option<KeyChord> {
    let keycode = keysym_to_keycode(evt.keysym)?;
    Some(KeyChord {
        keycode,
        meta: evt.modifiers & MOD_SUPER != 0,
        alt: evt.modifiers & MOD_ALT != 0,
        ctrl: evt.modifiers & MOD_CTRL != 0,
        shift: evt.modifiers & MOD_SHIFT != 0,
    })
}

fn keysym_to_keycode(sym: u32) -> Option<KeyCode> {
    match sym {
        0xFF09 => Some(KeyCode::TAB),
        0x20 => Some(KeyCode::SPACE),
        0x60 => Some(KeyCode::GRAVE),
        0xFF08 => Some(KeyCode::BACKSPACE),
        0xFF51 => Some(KeyCode::LEFT),
        0xFF52 => Some(KeyCode::UP),
        0xFF53 => Some(KeyCode::RIGHT),
        0xFF54 => Some(KeyCode::DOWN),
        0xFF0D => Some(KeyCode::ENTER),
        0xFF1B => Some(KeyCode::ESCAPE),
        // Top-row digits.
        0x30 => Some(KeyCode::KEY_0),
        0x31 => Some(KeyCode::KEY_1),
        0x32 => Some(KeyCode::KEY_2),
        0x33 => Some(KeyCode::KEY_3),
        0x34 => Some(KeyCode::KEY_4),
        0x35 => Some(KeyCode::KEY_5),
        0x36 => Some(KeyCode::KEY_6),
        0x37 => Some(KeyCode::KEY_7),
        0x38 => Some(KeyCode::KEY_8),
        0x39 => Some(KeyCode::KEY_9),
        KEYSYM_KP_0 => Some(KeyCode::KP_0),
        0xFFB2 => Some(KeyCode::KP_2),
        0xFFB4 => Some(KeyCode::KP_4),
        0xFFB5 => Some(KeyCode::KP_5),
        0xFFB6 => Some(KeyCode::KP_6),
        0xFFB8 => Some(KeyCode::KP_8),
        0xFFBD => Some(KeyCode::KP_EQUAL),
        0xFFAE => Some(KeyCode::KP_DECIMAL),
        0xFF8D => Some(KeyCode::KP_ENTER),
        0xFFAA => Some(KeyCode::KP_MULTIPLY),
        0xFFC9 => Some(KeyCode::F12),
        // NumLock-off variants of the zoning numpad keys.
        KEYSYM_KP_UP => Some(KeyCode::KP_8),
        KEYSYM_KP_LEFT => Some(KeyCode::KP_4),
        KEYSYM_KP_BEGIN => Some(KeyCode::KP_5),
        KEYSYM_KP_RIGHT => Some(KeyCode::KP_6),
        KEYSYM_KP_DOWN => Some(KeyCode::KP_2),
        KEYSYM_KP_INSERT => Some(KeyCode::KP_0),
        KEYSYM_KP_DELETE => Some(KeyCode::KP_DECIMAL),
        0x61..=0x7A => LETTER_KEYCODES
            .iter()
            .find(|(_, c)| *c as u32 == sym)
            .map(|(code, _)| *code),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_core::KeyCode;

    #[test]
    fn round_trip_tab() {
        let chord = KeyChord {
            keycode: KeyCode::TAB,
            meta: true,
            alt: false,
            ctrl: false,
            shift: false,
        };
        let reg = to_registered(&chord);
        let back = from_chord_event(&ChordEvent {
            keysym: reg.keysym,
            modifiers: reg.modifiers,
        })
        .expect("round-trip must succeed for TAB");
        assert_eq!(back.keycode, KeyCode::TAB);
        assert!(back.meta);
        assert!(!back.alt);
    }

    #[test]
    fn round_trip_letter_q() {
        let chord = KeyChord {
            keycode: KeyCode::Q,
            meta: true,
            alt: false,
            ctrl: false,
            shift: false,
        };
        let reg = to_registered(&chord);
        let back = from_chord_event(&ChordEvent {
            keysym: reg.keysym,
            modifiers: reg.modifiers,
        })
        .expect("round-trip must succeed for Q");
        assert_eq!(back.keycode, KeyCode::Q);
        assert!(back.meta);
    }

    #[test]
    fn round_trip_arrow_keys() {
        // Regression: DOWN/UP were missing from keycode_to_keysym, so
        // ⌘⇧↓ (Split Horizontal) registered as keysym 116 (= 't') and
        // never fired. All four arrows must map to their xkb keysym and
        // round-trip back to the same KeyCode.
        for (kc, sym) in [
            (KeyCode::LEFT, 0xFF51u32),
            (KeyCode::UP, 0xFF52),
            (KeyCode::RIGHT, 0xFF53),
            (KeyCode::DOWN, 0xFF54),
        ] {
            let chord = KeyChord {
                keycode: kc,
                meta: true,
                alt: false,
                ctrl: false,
                shift: true,
            };
            let reg = to_registered(&chord);
            assert_eq!(reg.keysym, sym, "{kc:?} must map to {sym:#x}");
            let back = from_chord_event(&ChordEvent {
                keysym: reg.keysym,
                modifiers: reg.modifiers,
            })
            .expect("arrow keysym must round-trip");
            assert_eq!(back.keycode, kc);
            assert!(back.meta && back.shift);
        }
    }

    #[test]
    fn numpad_alt_registration() {
        let chord = KeyChord {
            keycode: KeyCode::KP_8,
            meta: true,
            alt: false,
            ctrl: false,
            shift: false,
        };
        let alt = to_registered_alt(&chord).expect("KP_8 must have an alt keysym");
        assert_eq!(alt.keysym, KEYSYM_KP_UP);
        // Alt keysym must also round-trip.
        let back = from_chord_event(&ChordEvent {
            keysym: alt.keysym,
            modifiers: alt.modifiers,
        })
        .expect("KP_UP keysym must round-trip to KP_8");
        assert_eq!(back.keycode, KeyCode::KP_8);
    }

    #[test]
    fn round_trip_kp_multiply() {
        // Regression: KP_MULTIPLY was missing from keycode_to_keysym, so
        // ⌘+numpad-* (Float) registered as keysym 63 (= '?') and never
        // fired — the same class of bug as round_trip_arrow_keys. It must
        // map to XK_KP_Multiply (0xFFAA) and round-trip back to KP_MULTIPLY.
        let chord = KeyChord {
            keycode: KeyCode::KP_MULTIPLY,
            meta: true,
            alt: false,
            ctrl: false,
            shift: false,
        };
        let reg = to_registered(&chord);
        assert_eq!(reg.keysym, 0xFFAA, "KP_MULTIPLY must map to XK_KP_Multiply");
        let back = from_chord_event(&ChordEvent {
            keysym: reg.keysym,
            modifiers: reg.modifiers,
        })
        .expect("KP_Multiply keysym must round-trip");
        assert_eq!(back.keycode, KeyCode::KP_MULTIPLY);
        assert!(back.meta);
    }

    #[test]
    fn modifier_encoding() {
        let chord = KeyChord {
            keycode: KeyCode::SPACE,
            meta: true,
            alt: false,
            ctrl: true,
            shift: true,
        };
        let mods = river_modifiers(&chord);
        assert!(mods & MOD_SUPER != 0);
        assert!(mods & MOD_CTRL != 0);
        assert!(mods & MOD_SHIFT != 0);
        assert!(mods & MOD_ALT == 0);
    }

    // Every MEDIA_KEYS action string must be a value the `solactl media`
    // subcommand accepts — the two agree by convention only, so a typo here
    // would silently no-op at runtime. Keep this list in sync with
    // solactl's `MediaAction` ValueEnum (kebab-case).
    #[test]
    fn media_action_strings_are_valid_solactl_values() {
        const VALID: &[&str] = &["play-pause", "next", "prev", "mute", "vol-up", "vol-down"];
        for (keysym, action) in MEDIA_KEYS {
            assert!(
                VALID.contains(action),
                "media keysym {keysym:#x} maps to unknown solactl action {action:?}"
            );
        }
    }

    #[test]
    fn media_action_lookup() {
        assert_eq!(media_action(0x1008FF12), Some("mute")); // XF86AudioMute
        assert_eq!(media_action(0x1008FF14), Some("play-pause")); // XF86AudioPlay
        // A normal shell chord keysym is not a media key.
        assert_eq!(media_action(0x20), None); // space
    }
}
