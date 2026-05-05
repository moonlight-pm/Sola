//! Per-process Wayland connection. One global, shared by all Surfaces.

use std::rc::Rc;

pub struct WaylandClient {
    // TODO(taskB9): connection, registry, globals (xdg_wm_base,
    // zwp_linux_dmabuf_v1, wl_seat, …).
}

impl WaylandClient {
    /// Connect to the Wayland compositor and bind globals. Panics if
    /// the connection fails or required protocols are missing.
    pub fn connect() -> Rc<Self> {
        // TODO(taskB9)
        Rc::new(Self {})
    }

    /// Drive the dispatch loop one iteration (for our main loop's
    /// integration with CEF).
    pub fn dispatch_pending(&self) {
        // TODO(taskB9)
    }
}
