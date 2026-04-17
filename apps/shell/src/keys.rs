//! Chord-event handling for the shell.
//!
//! sola-river registers chords with River's xkb-bindings protocol on our
//! behalf and emits `Topic::Chord` / `Topic::ChordReleased` when they fire.
//! This module:
//!   - Converts a shell-side `KeyChord` into a River `(keysym, modifiers)`
//!     pair (`to_registered`).
//!   - Converts the reverse (`from_chord_event`) so the shell can dispatch
//!     using its existing shortcut-matching logic.
//!   - Runs the action table (switcher, launcher, zoning, menu shortcuts)
//!     when a chord fires.
//!   - Preserves Meta-release closes switcher by registering Meta+Tab and
//!     acting on its `ChordReleased` event.
use sola_app::SolaApp;
use sola_bus::topics::{
    ChordEvent, FocusTarget, FrameUpdate, RegisteredChord, Topic,
};
use sola_core::{KeyChord, KeyCode};

use crate::app::ShellApp;

// River modifier bits (from `modifiers` enum in river-window-management-v1.xml).
const MOD_SHIFT: u32 = 1;
const MOD_CTRL: u32 = 4;
const MOD_ALT: u32 = 8; // mod1
const MOD_SUPER: u32 = 64; // mod4

// XK_KP_0 .. XK_KP_9 run contiguously starting at 0xFFB0.
const KEYSYM_KP_0: u32 = 0xFFB0;

pub fn to_registered(chord: &KeyChord) -> RegisteredChord {
    RegisteredChord {
        keysym: keycode_to_keysym(chord.keycode),
        modifiers: river_modifiers(chord),
    }
}

fn river_modifiers(c: &KeyChord) -> u32 {
    let mut m = 0u32;
    if c.shift { m |= MOD_SHIFT; }
    if c.ctrl { m |= MOD_CTRL; }
    if c.alt { m |= MOD_ALT; }
    if c.meta { m |= MOD_SUPER; }
    m
}

/// Map the shell's evdev-style KeyCode to the xkbcommon keysym River expects.
fn keycode_to_keysym(k: KeyCode) -> u32 {
    match k {
        KeyCode::TAB => 0xFF09,
        KeyCode::SPACE => 0x20,
        KeyCode::BACKSPACE => 0xFF08,
        KeyCode::LEFT => 0xFF51,
        KeyCode::RIGHT => 0xFF53,
        KeyCode::ENTER => 0xFF0D,
        KeyCode::ESCAPE => 0xFF1B,
        // Zoning uses numpad digits + equal/decimal.
        KeyCode::KP_0 => KEYSYM_KP_0,
        KeyCode::KP_2 => KEYSYM_KP_0 + 2,
        KeyCode::KP_4 => KEYSYM_KP_0 + 4,
        KeyCode::KP_5 => KEYSYM_KP_0 + 5,
        KeyCode::KP_6 => KEYSYM_KP_0 + 6,
        KeyCode::KP_8 => KEYSYM_KP_0 + 8,
        KeyCode::KP_EQUAL => 0xFFBD,   // XK_KP_Equal
        KeyCode::KP_DECIMAL => 0xFFAE, // XK_KP_Decimal
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

const LETTER_KEYCODES: &[(KeyCode, u8)] = &[
    (KeyCode::A, b'A'),
    (KeyCode::B, b'B'),
    (KeyCode::C, b'C'),
    (KeyCode::D, b'D'),
    (KeyCode::E, b'E'),
    (KeyCode::F, b'F'),
    (KeyCode::G, b'G'),
    (KeyCode::H, b'H'),
    (KeyCode::I, b'I'),
    (KeyCode::J, b'J'),
    (KeyCode::K, b'K'),
    (KeyCode::L, b'L'),
    (KeyCode::M, b'M'),
    (KeyCode::N, b'N'),
    (KeyCode::O, b'O'),
    (KeyCode::P, b'P'),
    (KeyCode::Q, b'Q'),
    (KeyCode::R, b'R'),
    (KeyCode::S, b'S'),
    (KeyCode::T, b'T'),
    (KeyCode::U, b'U'),
    (KeyCode::V, b'V'),
    (KeyCode::W, b'W'),
    (KeyCode::X, b'X'),
    (KeyCode::Y, b'Y'),
    (KeyCode::Z, b'Z'),
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
        0xFF08 => Some(KeyCode::BACKSPACE),
        0xFF51 => Some(KeyCode::LEFT),
        0xFF53 => Some(KeyCode::RIGHT),
        0xFF0D => Some(KeyCode::ENTER),
        0xFF1B => Some(KeyCode::ESCAPE),
        KEYSYM_KP_0 => Some(KeyCode::KP_0),
        0xFFB2 => Some(KeyCode::KP_2),
        0xFFB4 => Some(KeyCode::KP_4),
        0xFFB5 => Some(KeyCode::KP_5),
        0xFFB6 => Some(KeyCode::KP_6),
        0xFFB8 => Some(KeyCode::KP_8),
        0xFFBD => Some(KeyCode::KP_EQUAL),
        0xFFAE => Some(KeyCode::KP_DECIMAL),
        0x41..=0x5A => LETTER_KEYCODES
            .iter()
            .find(|(_, c)| *c as u32 == sym)
            .map(|(code, _)| *code),
        _ => None,
    }
}

/// Dispatch a chord event through the shell's action table.
pub fn handle_chord(
    app: &mut ShellApp,
    ctx: &mut sola_app::AppCtx,
    evt: ChordEvent,
) {
    let Some(chord) = from_chord_event(&evt) else {
        tracing::debug!(
            keysym = evt.keysym,
            modifiers = evt.modifiers,
            "unrecognized chord"
        );
        return;
    };

    tracing::debug!(
        keycode = chord.keycode.raw(),
        meta = chord.meta,
        ctrl = chord.ctrl,
        alt = chord.alt,
        shift = chord.shift,
        "chord fired"
    );

    // Switcher active: Meta+Tab (or arrow) cycles; Meta release confirms
    // (handled in `handle_chord_released`).
    if app.switcher.active {
        match chord.keycode {
            code if code == KeyCode::TAB || code == KeyCode::RIGHT => {
                app.switcher.select_next();
                let sel = app.switcher.selected;
                app.windows
                    .switcher
                    .eval_js(&format!("setSelection({sel})"));
                return;
            }
            KeyCode::LEFT => {
                app.switcher.select_prev();
                let sel = app.switcher.selected;
                app.windows
                    .switcher
                    .eval_js(&format!("setSelection({sel})"));
                return;
            }
            _ => {}
        }
    }

    // Shell system shortcuts (e.g. Exit Sola).
    if let Some(action) = app.menus.lookup_shortcut(&chord, ShellApp::APP_ID) {
        tracing::info!(action_id = %action.action_id, "shell shortcut");
        if action.action_id == "exit" {
            ctx.emit(Topic::Shutdown);
        }
        return;
    }

    // Meta+Space: toggle launcher.
    if chord.meta && chord.keycode == KeyCode::SPACE {
        if app.launcher.active {
            app.close_launcher(ctx);
        } else {
            app.open_launcher(ctx);
        }
        return;
    }

    // Meta+Tab: activate switcher.
    if chord.meta && chord.keycode == KeyCode::TAB {
        if app.launcher.active {
            app.close_launcher(ctx);
        }
        tracing::info!("activating switcher");
        app.switcher.apps = app.rebuild_switcher_apps();
        app.switcher.active = true;
        app.switcher.selected = if app.switcher.apps.len() > 1 { 1 } else { 0 };
        let json = app.switcher_apps_json();
        app.windows.switcher.eval_js(&format!(
            "renderSwitcher({}, {})",
            json, app.switcher.selected
        ));

        if let (Some((ow, oh)), Some(wid)) = (
            app.zoning.output_size,
            app.lookup_window_id(ShellApp::APP_ID, "switcher"),
        ) {
            ctx.emit(Topic::Frame(FrameUpdate {
                window_id: wid,
                x: (ow - 800) / 2,
                y: (oh - 400) / 2,
                width: 800,
                height: 400,
            }));
        }
        app.emit_composition(ctx);
        return;
    }

    // Zone snapping (Meta+Numpad).
    if let Some(frame) = app.zoning.handle_key(chord.keycode.raw(), app.focused_window_id) {
        ctx.emit(Topic::Frame(frame));
        return;
    }

    // Focused app menu shortcut lookup.
    if let Some(focused) = app.focused_app_id.clone() {
        if let Some(action) = app.menus.lookup_shortcut(&chord, &focused) {
            tracing::info!(
                app_id = %action.app_id,
                action_id = %action.action_id,
                "menu shortcut matched"
            );
            ctx.emit(Topic::MenuAction(action));
        }
    }
}

/// Entry point invoked on `Topic::ChordReleased`. Mirrors Meta-release
/// behavior from the old GTK path.
pub fn handle_chord_released(
    app: &mut ShellApp,
    ctx: &mut sola_app::AppCtx,
    evt: ChordEvent,
) {
    let Some(chord) = from_chord_event(&evt) else {
        return;
    };
    // Meta+Tab release — only meaningful while the switcher is active.
    if app.switcher.active && chord.keycode == KeyCode::TAB && chord.meta {
        confirm_switcher(app, ctx);
    }
}

fn confirm_switcher(app: &mut ShellApp, ctx: &mut sola_app::AppCtx) {
    let app_id = app.switcher.selected_app_id().map(String::from);
    tracing::info!(app_id = ?app_id, "confirming switcher");
    app.switcher.active = false;
    app.windows.switcher.eval_js("clear()");
    if let Some(ref app_id) = app_id {
        app.set_focus(app_id);
        let wid = app
            .mru_window_by_app
            .get(app_id)
            .copied()
            .or_else(|| app.lookup_any_window_id(app_id));
        if let Some(wid) = wid {
            app.focused_window_id = Some(wid);
            ctx.emit(Topic::Focus(FocusTarget { window_id: wid }));
        }
    }
    app.emit_composition(ctx);
}

