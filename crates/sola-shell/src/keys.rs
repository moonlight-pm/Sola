//! Chord-event handling for the shell.
//!
//! sola-river registers chords with River's xkb-bindings protocol on our
//! behalf and emits `Topic::Chord` / `Topic::ChordReleased` when they fire.
//! This module:
//!   - Converts a shell-side `KeyChord` into a River `(keysym, modifiers)`
//!     pair (`to_registered`).
//!   - Converts the reverse (`from_chord_event`) so the shell can dispatch
//!     using its existing shortcut-matching logic.
//!
//! The keysyms come from the one key table, `sola_core::keys::KEYS`.
use sola_bus::topics::{ChordEvent, RegisteredChord};
use sola_core::{KeyChord, KeyCode};

// River modifier bits (from `modifiers` enum in river-window-management-v1.xml).
const MOD_SHIFT: u32 = 1;
const MOD_CTRL: u32 = 4;
const MOD_ALT: u32 = 8; // mod1
const MOD_SUPER: u32 = 64; // mod4

pub const KEYSYM_ESCAPE: u32 = 0xFF1B;

/// A shell `KeyChord` as River's `(keysym, modifiers)`. A key outside
/// `sola_core::keys::KEYS` passes its raw keycode, so sola-river can log it.
pub fn to_registered(chord: &KeyChord) -> RegisteredChord {
    RegisteredChord {
        keysym: chord.keycode.keysym().unwrap_or(chord.keycode.raw()),
        modifiers: river_modifiers(chord),
    }
}

/// For keycodes whose keysym changes with NumLock state, return the
/// alternate keysym (NumLock off) so the shell can register both and
/// fire its action regardless of NumLock state.
pub fn to_registered_alt(chord: &KeyChord) -> Option<RegisteredChord> {
    let alt = chord.keycode.keysym_alt()?;
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
    KeyCode::from_keysym(sym)
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
    fn round_trip_letter_h() {
        // Super+H hide — same letter-keysym class as Q (must register as
        // lowercase XK_h, not raw evdev).
        let chord = KeyChord {
            keycode: KeyCode::H,
            meta: true,
            alt: false,
            ctrl: false,
            shift: false,
        };
        let reg = to_registered(&chord);
        assert_eq!(reg.keysym, b'h' as u32, "H must map to XK_h");
        let back = from_chord_event(&ChordEvent {
            keysym: reg.keysym,
            modifiers: reg.modifiers,
        })
        .expect("round-trip must succeed for H");
        assert_eq!(back.keycode, KeyCode::H);
        assert!(back.meta);
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
    fn zoom_keys_register_their_keysyms() {
        // Regression: `=` and `-` were missing, so ⌘= / ⌘− registered as
        // raw keycodes 21 / 20 and zoom never fired.
        for (kc, sym) in [(KeyCode::EQUAL, 0x3Du32), (KeyCode::MINUS, 0x2D)] {
            let reg = to_registered(&kc.meta());
            assert_eq!(reg.keysym, sym);
            let back = from_chord_event(&ChordEvent {
                keysym: reg.keysym,
                modifiers: reg.modifiers,
            })
            .expect("zoom keysym must round-trip");
            assert_eq!(back.keycode, kc);
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
        assert_eq!(alt.keysym, 0xFF97); // XK_KP_Up
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
    fn round_trip_kp_add() {
        // Meta+KP_Add → MiddleRight. Same keysym-mapping class as
        // KP_MULTIPLY: must register as XK_KP_Add (0xFFAB), not raw 86.
        let chord = KeyChord {
            keycode: KeyCode::KP_ADD,
            meta: true,
            alt: false,
            ctrl: false,
            shift: false,
        };
        let reg = to_registered(&chord);
        assert_eq!(reg.keysym, 0xFFAB, "KP_ADD must map to XK_KP_Add");
        let back = from_chord_event(&ChordEvent {
            keysym: reg.keysym,
            modifiers: reg.modifiers,
        })
        .expect("KP_Add keysym must round-trip");
        assert_eq!(back.keycode, KeyCode::KP_ADD);
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
