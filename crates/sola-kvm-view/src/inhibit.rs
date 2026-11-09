//! Keep the desk's own shortcuts out of the way, so Super reaches the other
//! machine. The inhibitor is made on the window's own surface, reached
//! through winit's handles, and the compositor applies it only while that
//! surface has the keyboard.

use std::fmt;
use std::ptr::NonNull;

use iced::window::Window;
use iced::window::raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use tracing::warn;
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
    zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1,
};

struct State;

/// Shortcuts stay inhibited until this is dropped.
pub struct Inhibitor {
    connection: Connection,
    _queue: EventQueue<State>,
    inhibitor: ZwpKeyboardShortcutsInhibitorV1,
}

impl fmt::Debug for Inhibitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Inhibitor")
    }
}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        self.inhibitor.destroy();
        if let Err(e) = self.connection.flush() {
            warn!(%e, "remote view: could not give the shortcuts back");
        }
    }
}

/// Inhibit the desk's shortcuts for `window`. `None` (and a log line) when
/// the window is not Wayland or the compositor does not offer it.
pub fn acquire(window: &dyn Window) -> Option<Inhibitor> {
    match try_acquire(window) {
        Ok(inhibitor) => Some(inhibitor),
        Err(e) => {
            warn!(error = %e, "remote view: the desk's shortcuts stay on");
            None
        }
    }
}

fn try_acquire(window: &dyn Window) -> Result<Inhibitor, String> {
    let display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
    let surface = window.window_handle().map_err(|e| e.to_string())?.as_raw();
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(surface)) = (display, surface) else {
        return Err("not a Wayland window".into());
    };
    // Borrowing winit's connection and surface, for as long as the window.
    let (connection, surface) = unsafe { borrow(display.display, surface.surface) }?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("wl_seat: {e}"))?;
    let manager: ZwpKeyboardShortcutsInhibitManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("zwp_keyboard_shortcuts_inhibit_manager_v1: {e}"))?;
    let inhibitor = manager.inhibit_shortcuts(&surface, &seat, &qh, ());
    queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
    Ok(Inhibitor {
        connection,
        _queue: queue,
        inhibitor,
    })
}

unsafe fn borrow(display: NonNull<std::ffi::c_void>, surface: NonNull<std::ffi::c_void>) -> Result<(Connection, WlSurface), String> {
    let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
    let connection = Connection::from_backend(backend);
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }.map_err(|e| e.to_string())?;
    let surface = WlSurface::from_id(&connection, id).map_err(|e| e.to_string())?;
    Ok((connection, surface))
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwpKeyboardShortcutsInhibitManagerV1);
delegate_noop!(State: ignore ZwpKeyboardShortcutsInhibitorV1);
