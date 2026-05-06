//! wl_pointer / wl_keyboard / wl_touch / IME → CEF event translation.
//!
//! Currently scope: pointer + keyboard. Touch joins when a CEF
//! `send_touch_event` consumer surfaces (the storybook is mouse + key).

use std::rc::Rc;

use smithay_client_toolkit::{
    delegate_keyboard, delegate_pointer,
    seat::{
        keyboard::{KeyEvent as SctkKeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::WaylandSurface,
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    protocol::{wl_keyboard::WlKeyboard, wl_pointer::WlPointer, wl_surface::WlSurface},
};

#[allow(unused_imports)]
use ::cef::{rc::*, *};

use crate::wayland::{Surface, WaylandClient};

impl PointerHandler for WaylandClient {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            // Resolve the Surface that owns this event's wl_surface. Drop
            // events whose surface isn't ours (e.g. transient cursor surfaces
            // or already-destroyed windows).
            let target_id = event.surface.id();
            let surface: Option<Rc<Surface>> = self
                .surfaces
                .iter()
                .filter_map(|w| w.upgrade())
                .find(|s| s.xdg_window.wl_surface().id() == target_id);
            let Some(surface) = surface else { continue };

            let (x, y) = event.position;
            let mouse_event = ::cef::MouseEvent {
                x: x.round() as i32,
                y: y.round() as i32,
                // TODO(D4c): pull current modifier state from wl_keyboard
                // once keyboard input is wired. CEF treats this as a bitfield
                // of `cef_event_flags_t` values.
                modifiers: 0,
            };

            let Some(browser) = surface.browser.borrow().as_ref().cloned() else {
                continue;
            };
            let Some(host) = browser.host() else { continue };

            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.entered_pointer_surface = Some(surface.clone());
                }
                PointerEventKind::Leave { .. } => {
                    host.send_mouse_move_event(Some(&mouse_event), 1);
                    self.entered_pointer_surface = None;
                }
                PointerEventKind::Motion { .. } => {
                    host.send_mouse_move_event(Some(&mouse_event), 0);
                }
                PointerEventKind::Press { button, .. } => {
                    if let Some(btn) = linux_button_to_cef(button) {
                        host.send_mouse_click_event(Some(&mouse_event), btn, 0, 1);
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if let Some(btn) = linux_button_to_cef(button) {
                        host.send_mouse_click_event(Some(&mouse_event), btn, 1, 1);
                    }
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    // sctk packs both a continuous `absolute` (trackpad) and
                    // a `discrete` (notch wheel) reading. Mouse wheels emit
                    // discrete; touchpads emit absolute. Mix-and-match: use
                    // discrete*120 px/notch when present, else absolute.
                    let dx = if horizontal.discrete != 0 {
                        horizontal.discrete * 120
                    } else {
                        horizontal.absolute.round() as i32
                    };
                    let dy = if vertical.discrete != 0 {
                        vertical.discrete * 120
                    } else {
                        vertical.absolute.round() as i32
                    };
                    if dx != 0 || dy != 0 {
                        // Wayland convention: positive = down/right (motion of
                        // the content, not the wheel). CEF wants "scroll up =
                        // positive deltaY" i.e. wheel-down → negative deltaY,
                        // so flip both axes.
                        host.send_mouse_wheel_event(Some(&mouse_event), -dx, -dy);
                    }
                }
            }
        }
    }
}

fn linux_button_to_cef(button: u32) -> Option<::cef::MouseButtonType> {
    // Linux input event codes — see <linux/input-event-codes.h>.
    const BTN_LEFT: u32 = 0x110;
    const BTN_RIGHT: u32 = 0x111;
    const BTN_MIDDLE: u32 = 0x112;
    match button {
        BTN_LEFT => Some(::cef::MouseButtonType::LEFT),
        BTN_RIGHT => Some(::cef::MouseButtonType::RIGHT),
        BTN_MIDDLE => Some(::cef::MouseButtonType::MIDDLE),
        _ => None,
    }
}

delegate_pointer!(WaylandClient);

// ── Keyboard ────────────────────────────────────────────────────────────────

impl KeyboardHandler for WaylandClient {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        wl_surface: &WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        let id = wl_surface.id();
        self.entered_keyboard_surface = self
            .surfaces
            .iter()
            .filter_map(|w| w.upgrade())
            .find(|s| s.xdg_window.wl_surface().id() == id);
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _wl_surface: &WlSurface,
        _serial: u32,
    ) {
        self.entered_keyboard_surface = None;
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: SctkKeyEvent,
    ) {
        self.dispatch_key(event, true);
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        event: SctkKeyEvent,
    ) {
        self.dispatch_key(event, false);
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _layout: u32,
    ) {
        self.current_modifiers = modifiers;
    }
}

impl WaylandClient {
    fn dispatch_key(&self, event: SctkKeyEvent, pressed: bool) {
        let Some(surface) = self.entered_keyboard_surface.clone() else { return };
        let Some(browser) = surface.browser.borrow().as_ref().cloned() else { return };
        let Some(host) = browser.host() else { return };

        let modifiers_flags = modifiers_to_cef(&self.current_modifiers);
        let windows_key_code = keysym_to_windows_vk(event.keysym);
        // Linux native key code: X11 keycode = Linux input scancode + 8.
        let native_key_code = (event.raw_code as i32) + 8;

        // Primary key event (KEYDOWN/KEYUP). For non-character keys this
        // is the only event the page sees; for character keys we also
        // dispatch a CHAR event below so DOM input fields receive text.
        let key_event = ::cef::KeyEvent {
            size: std::mem::size_of::<::cef::sys::_cef_key_event_t>(),
            type_: if pressed {
                ::cef::KeyEventType::KEYDOWN
            } else {
                ::cef::KeyEventType::KEYUP
            },
            modifiers: modifiers_flags,
            windows_key_code,
            native_key_code,
            is_system_key: 0,
            character: 0,
            unmodified_character: 0,
            focus_on_editable_field: 0,
        };
        host.send_key_event(Some(&key_event));

        if !pressed {
            return;
        }

        // CHAR event: covers the text the user actually typed. Send only
        // for press, and only when xkb produced a UTF-8 string. Use the
        // first UTF-16 code unit; surrogate pairs require sequential CHAR
        // events but for most BMP input the first unit is the glyph.
        if let Some(text) = event.utf8.as_deref() {
            if let Some(ch) = text.chars().next() {
                let mut buf = [0u16; 2];
                let utf16 = ch.encode_utf16(&mut buf);
                let char_event = ::cef::KeyEvent {
                    size: std::mem::size_of::<::cef::sys::_cef_key_event_t>(),
                    type_: ::cef::KeyEventType::CHAR,
                    modifiers: modifiers_flags,
                    windows_key_code: utf16[0] as i32,
                    native_key_code,
                    is_system_key: 0,
                    character: utf16[0],
                    unmodified_character: utf16[0],
                    focus_on_editable_field: 0,
                };
                host.send_key_event(Some(&char_event));
            }
        }
    }
}

fn modifiers_to_cef(m: &Modifiers) -> u32 {
    use ::cef::sys::cef_event_flags_t as F;
    let mut flags = 0u32;
    if m.shift { flags |= F::EVENTFLAG_SHIFT_DOWN.0; }
    if m.ctrl { flags |= F::EVENTFLAG_CONTROL_DOWN.0; }
    if m.alt { flags |= F::EVENTFLAG_ALT_DOWN.0; }
    if m.logo { flags |= F::EVENTFLAG_COMMAND_DOWN.0; }
    if m.caps_lock { flags |= F::EVENTFLAG_CAPS_LOCK_ON.0; }
    if m.num_lock { flags |= F::EVENTFLAG_NUM_LOCK_ON.0; }
    flags
}

/// Translate an X11 keysym to a Windows Virtual-Key code, the form CEF
/// expects in `KeyEvent::windows_key_code`. Coverage is the common-case
/// editing + navigation set; unmapped keysyms return 0 and rely on the
/// CHAR event for text.
fn keysym_to_windows_vk(keysym: Keysym) -> i32 {
    let raw = keysym.raw();
    // ASCII printable (digits + uppercase letters share the VK range).
    match raw {
        0x0030..=0x0039 => return raw as i32,            // '0'..'9' → VK_0..VK_9
        0x0041..=0x005A => return raw as i32,            // 'A'..'Z' → VK_A..VK_Z
        0x0061..=0x007A => return (raw - 0x0020) as i32, // 'a'..'z' → VK_A..VK_Z
        _ => {}
    }
    // Named keys (X11 keysym → Windows VK).
    match raw {
        0xff08 => 0x08, // BackSpace → VK_BACK
        0xff09 => 0x09, // Tab → VK_TAB
        0xff0d => 0x0D, // Return → VK_RETURN
        0xff13 => 0x13, // Pause → VK_PAUSE
        0xff14 => 0x91, // Scroll_Lock → VK_SCROLL
        0xff1b => 0x1B, // Escape → VK_ESCAPE
        0x0020 => 0x20, // space → VK_SPACE
        0xff50 => 0x24, // Home → VK_HOME
        0xff51 => 0x25, // Left → VK_LEFT
        0xff52 => 0x26, // Up → VK_UP
        0xff53 => 0x27, // Right → VK_RIGHT
        0xff54 => 0x28, // Down → VK_DOWN
        0xff55 => 0x21, // Page_Up → VK_PRIOR
        0xff56 => 0x22, // Page_Down → VK_NEXT
        0xff57 => 0x23, // End → VK_END
        0xff63 => 0x2D, // Insert → VK_INSERT
        0xffff => 0x2E, // Delete → VK_DELETE
        0xffe1 | 0xffe2 => 0x10, // Shift_L/R → VK_SHIFT
        0xffe3 | 0xffe4 => 0x11, // Control_L/R → VK_CONTROL
        0xffe9 | 0xffea => 0x12, // Alt_L/R → VK_MENU
        0xffeb | 0xffec => 0x5B, // Super_L/R (logo/Win key) → VK_LWIN
        0xffe5 => 0x14, // Caps_Lock → VK_CAPITAL
        0xff7f => 0x90, // Num_Lock → VK_NUMLOCK
        // F1..F12 (X11 0xffbe..0xffc9 → VK 0x70..0x7B)
        k @ 0xffbe..=0xffc9 => (0x70 + (k - 0xffbe)) as i32,
        _ => 0,
    }
}

delegate_keyboard!(WaylandClient);
