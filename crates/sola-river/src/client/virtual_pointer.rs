//! Driver for `wlr-virtual-pointer-unstable-v1`.
//!
//! Owns the manager proxy and a single virtual pointer attached to the
//! seat. Used by `solactl` to script pointer movement, clicks, and
//! scrolling for debugging and end-to-end testing.

use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{info, warn};
use wayland_client::protocol::wl_pointer;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::client::AppData;
use crate::protocol::wlr_virtual_pointer_unstable_v1::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::{self, ZwlrVirtualPointerV1},
};
use sola_bus::topics::{PointerAction, PointerButton};

/// Linux input event codes for mouse buttons.
/// https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Default)]
pub struct VirtualPointerState {
    pub manager: Option<ZwlrVirtualPointerManagerV1>,
    pub pointer: Option<ZwlrVirtualPointerV1>,
}

/// Called once `wl_seat` and `zwlr_virtual_pointer_manager_v1` are both
/// bound. Creates the per-seat virtual pointer. No-op if already
/// created or prerequisites aren't bound yet.
pub fn init_if_ready(state: &mut AppData, qh: &QueueHandle<AppData>) {
    if state.virtual_pointer.pointer.is_some() {
        return;
    }
    let Some(manager) = state.virtual_pointer.manager.as_ref() else {
        return;
    };
    let Some(seat) = state.wl_seat.as_ref() else {
        return;
    };

    let pointer = manager.create_virtual_pointer(Some(seat), qh, ());
    info!("virtual pointer created");
    state.virtual_pointer.pointer = Some(pointer);
}

pub fn dispatch(state: &AppData, action: &PointerAction) -> Result<(), String> {
    let Some(pointer) = state.virtual_pointer.pointer.as_ref() else {
        warn!("SimulatePointer received but virtual pointer not ready");
        return Err("virtual pointer not ready".into());
    };
    let t = now_ms();

    match action {
        PointerAction::Move { x, y } => {
            move_to(state, pointer, t, *x, *y);
            pointer.frame();
        }
        PointerAction::Click { button, x, y } => {
            move_to(state, pointer, t, *x, *y);
            let code = button_code(*button);
            pointer.button(t, code, wl_pointer::ButtonState::Pressed);
            pointer.button(t + 1, code, wl_pointer::ButtonState::Released);
            pointer.frame();
        }
        PointerAction::Press { button } => {
            pointer.button(t, button_code(*button), wl_pointer::ButtonState::Pressed);
            pointer.frame();
        }
        PointerAction::Release { button } => {
            pointer.button(t, button_code(*button), wl_pointer::ButtonState::Released);
            pointer.frame();
        }
        PointerAction::Scroll { dx, dy } => {
            if *dx != 0.0 {
                pointer.axis(t, wl_pointer::Axis::HorizontalScroll, *dx);
            }
            if *dy != 0.0 {
                pointer.axis(t, wl_pointer::Axis::VerticalScroll, *dy);
            }
            pointer.frame();
        }
    }

    if let Some(conn) = state.conn.as_ref() {
        if let Err(e) = conn.flush() {
            warn!(%e, "failed to flush wayland after synthesizing pointer event");
        }
    }
    Ok(())
}

fn move_to(state: &AppData, pointer: &ZwlrVirtualPointerV1, time: u32, x: i32, y: i32) {
    // motion_absolute requires (x, y) in [0, x_extent] / [0, y_extent].
    // We use the primary output's logical size as the extent so callers
    // can pass actual screen coordinates.
    let (ow, oh) = state.output_size.unwrap_or((1920, 1080));
    let cx = x.clamp(0, ow.max(1) - 1) as u32;
    let cy = y.clamp(0, oh.max(1) - 1) as u32;
    pointer.motion_absolute(time, cx, cy, ow.max(1) as u32, oh.max(1) as u32);
}

fn button_code(b: PointerButton) -> u32 {
    match b {
        PointerButton::Left => BTN_LEFT,
        PointerButton::Right => BTN_RIGHT,
        PointerButton::Middle => BTN_MIDDLE,
    }
}

fn now_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 as u32)
        .unwrap_or(0)
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: zwlr_virtual_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
