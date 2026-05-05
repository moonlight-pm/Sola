//! Per-window xdg_toplevel + dma-buf import + frame callback handling.

use std::rc::Rc;
use crate::wayland::WaylandClient;
use crate::window::WindowConfig;

pub struct Surface {
    // TODO(taskB9): wl_surface, xdg_toplevel, frame callback state,
    // dma-buf params builder, current size, configure ack state.
}

impl Surface {
    pub fn new(_client: &Rc<WaylandClient>, _cfg: &WindowConfig) -> Rc<Self> {
        // TODO(taskB9): create wl_surface, xdg_toplevel, set title/app_id/size.
        Rc::new(Self {})
    }

    /// Present a CEF-produced dma-buf as the next frame.
    pub fn present_dmabuf(
        &self,
        _fd: std::os::unix::io::RawFd,
        _format: u32,         // DRM fourcc, e.g. DRM_FORMAT_ARGB8888
        _modifier: u64,       // DRM modifier
        _stride: u32,
        _offset: u32,
        _width: i32,
        _height: i32,
        _damage_rects: &[(i32, i32, i32, i32)],
    ) {
        // TODO(taskB9 — final wiring in B11)
    }

    /// Width / height (px).
    pub fn size(&self) -> (i32, i32) {
        // TODO(taskB9)
        (1100, 720)
    }
}
