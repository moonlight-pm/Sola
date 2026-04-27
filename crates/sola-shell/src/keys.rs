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
use sola_bus::topics::{ChordEvent, EditRequest, FocusTarget, FrameUpdate, RegisteredChord, Topic};
use sola_core::{KeyChord, KeyCode};

use crate::app::ShellApp;

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
        KeyCode::BACKSPACE => 0xFF08,
        KeyCode::LEFT => 0xFF51,
        KeyCode::RIGHT => 0xFF53,
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
        0xFF08 => Some(KeyCode::BACKSPACE),
        0xFF51 => Some(KeyCode::LEFT),
        0xFF53 => Some(KeyCode::RIGHT),
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

/// Dispatch a chord event through the shell's action table.
pub fn handle_chord(app: &mut ShellApp, ctx: &mut sola_app::AppCtx, evt: ChordEvent) {
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

    // Escape dismisses whichever shell overlay is up. Only registered
    // while one is active (see `emit_registered_chords`), so we don't
    // steal Escape from terminal apps otherwise.
    let bare = !chord.meta && !chord.ctrl && !chord.alt && !chord.shift;
    if chord.keycode == KeyCode::ESCAPE && bare {
        if app.launcher.active {
            app.close_launcher(ctx);
            return;
        }
        if app.menu_open {
            app.close_menu(ctx);
            return;
        }
        if app.switcher.active {
            tracing::info!("cancelling switcher via Escape");
            app.switcher.active = false;
            app.emit_registered_chords(ctx);
            app.windows.switcher.eval_js("clear()");
            app.emit_composition(ctx);
            return;
        }
    }

    // While the launcher or a dropdown menu is up, eat every other chord.
    // We don't want zoning, menu shortcuts, or app shortcuts firing under
    // a modal overlay. (Switcher has its own navigation branch below.)
    if app.launcher.active || app.menu_open {
        return;
    }

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

    // Meta+C / Meta+V: global clipboard chords. Dispatched to the
    // focused window's owning process via the bus; non-Sola clients
    // aren't subscribers, so Meta+C/V with a foreign focus is a silent
    // no-op (and in practice doesn't fire at all, because the xkb
    // profile rebinds Meta→Ctrl when a non-Sola app is focused).
    if chord.meta
        && !chord.ctrl
        && !chord.alt
        && !chord.shift
        && matches!(chord.keycode, KeyCode::C | KeyCode::V)
    {
        if app.switcher.active {
            return;
        }
        if let Some(window_id) = app.focused_window_id {
            let topic = if chord.keycode == KeyCode::C {
                Topic::Copy(EditRequest { window_id })
            } else {
                Topic::Paste(EditRequest { window_id })
            };
            ctx.emit(topic);
        } else {
            tracing::debug!("clipboard chord with no focused window");
        }
        return;
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
        tracing::info!(
            launcher_active = app.launcher.active,
            "Meta+Space chord — toggling launcher"
        );
        if app.launcher.active {
            app.close_launcher(ctx);
        } else {
            app.open_launcher(ctx);
        }
        return;
    }

    // Meta+Q: close focused app.
    if chord.meta && chord.keycode == KeyCode::Q {
        tracing::info!("Meta+Q — close focused app");
        app.close_focused_app(ctx);
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
        app.emit_registered_chords(ctx);
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
    if let Some(frame) = app
        .zoning
        .handle_key(chord.keycode.raw(), app.focused_window_id)
    {
        ctx.emit(Topic::Frame(frame));
        if let Some(zones) = app.zoning.take_zones_update() {
            ctx.emit(Topic::Zones(zones));
        }
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
/// behavior from the old GTK path: while the switcher is active, the
/// user confirms by letting go of the Super key.
pub fn handle_chord_released(app: &mut ShellApp, ctx: &mut sola_app::AppCtx, evt: ChordEvent) {
    // The bare Super_L binding (keysym=Super_L, modifiers=0) fires its
    // released event exactly when the user lifts the physical Super key.
    // That's when we commit the switcher selection.
    if evt.keysym == KEYSYM_SUPER_L && evt.modifiers == 0 && app.switcher.active {
        confirm_switcher(app, ctx);
    }
}

fn confirm_switcher(app: &mut ShellApp, ctx: &mut sola_app::AppCtx) {
    let app_id = app.switcher.selected_app_id().map(String::from);
    tracing::info!(app_id = ?app_id, "confirming switcher");
    app.switcher.active = false;
    app.emit_registered_chords(ctx);
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
