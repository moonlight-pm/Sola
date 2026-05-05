//! Per-window xdg_toplevel + dma-buf import.
//!
//! `Surface::new` creates a `wl_surface` / `xdg_toplevel` pair, sets the
//! title/app_id/min_size, and commits the initial empty frame so the
//! compositor will send the first configure. `Surface::present_dmabuf` imports
//! a CEF-produced dma-buf via `zwp_linux_dmabuf_v1::create_immed` and
//! attaches + commits the resulting `wl_buffer`.

use std::cell::RefCell;
use std::os::unix::io::{BorrowedFd, RawFd};
use std::rc::Rc;

use smithay_client_toolkit::shell::{
    xdg::window::{Window, WindowDecorations},
    WaylandSurface,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags as DmabufFlags;

use crate::wayland::WaylandClient;
use crate::window::WindowConfig;

/// App-id reported to the compositor for all sola-kit windows.
/// TODO(later): plumb the actual APP_ID through from `SolaApp::APP_ID`.
const APP_ID: &str = "sola.kit";

/// Per-window Wayland state: a `wl_surface` mapped as an `xdg_toplevel`,
/// plus enough context to present CEF-produced dma-bufs as frames.
pub struct Surface {
    /// The underlying Wayland surface.
    pub xdg_window: Window,
    /// Current logical size in pixels.
    pub size: RefCell<(i32, i32)>,
    /// Set to `true` after the first `configure` is acked. CEF rendering must
    /// not start before the compositor has sent an initial configure.
    pub configured: RefCell<bool>,
    /// Shared reference back to the per-process Wayland connection, used to
    /// reach `dmabuf_state` and `qh` in `present_dmabuf`.
    pub client: Rc<RefCell<WaylandClient>>,
}

impl Surface {
    /// Create a new xdg_toplevel surface from `cfg` and commit the initial
    /// empty frame. Returns `Rc<Self>` so it can be shared with closures.
    pub fn new(client: Rc<RefCell<WaylandClient>>, cfg: &WindowConfig) -> Rc<Self> {
        tracing::info!(title = %cfg.title, size = ?cfg.size, "Surface::new entered");
        let xdg_window = {
            let c = client.borrow();

            let wl_surface = c.compositor_state.create_surface(&c.qh);
            let xdg_window = c.xdg_shell.create_window(
                wl_surface,
                WindowDecorations::RequestServer,
                &c.qh,
            );

            xdg_window.set_title(cfg.title.clone());
            xdg_window.set_app_id(APP_ID.to_string());
            xdg_window.set_min_size(Some((400, 300)));

            // Initial empty commit: tells the compositor we exist so it sends
            // the first configure event (which gives us actual dimensions).
            xdg_window.commit();
            tracing::info!("Surface::new initial commit sent (no buffer)");

            xdg_window
        };

        Rc::new(Self {
            xdg_window,
            size: RefCell::new(cfg.size),
            configured: RefCell::new(false),
            client,
        })
    }

    /// Current logical size in pixels.
    pub fn size(&self) -> (i32, i32) {
        *self.size.borrow()
    }

    /// Import a CEF-produced dma-buf as the next frame via
    /// `zwp_linux_dmabuf_v1::create_immed`, then attach + damage + commit.
    ///
    /// `fd` is the dma-buf file descriptor (not owned; CEF retains ownership
    /// until the compositor sends `wl_buffer.release`).
    ///
    /// `format` is the DRM fourcc (e.g. `DRM_FORMAT_ARGB8888 = 0x34325241`).
    ///
    /// `modifier` is the DRM modifier (e.g. `DRM_FORMAT_MOD_LINEAR = 0`).
    ///
    /// `stride` and `offset` are in bytes, for plane index 0 (single-plane).
    ///
    /// `damage_rects` is a list of `(x, y, w, h)` rectangles in buffer
    /// coordinates that have changed. Pass an empty slice to damage the whole
    /// surface.
    pub fn present_dmabuf(
        &self,
        fd: RawFd,
        format: u32,
        modifier: u64,
        stride: u32,
        offset: u32,
        width: i32,
        height: i32,
        damage_rects: &[(i32, i32, i32, i32)],
    ) {
        tracing::info!(fd, format, modifier, stride, offset, width, height, "Surface::present_dmabuf entered");
        let c = self.client.borrow();

        let params = match c.dmabuf_state.create_params(&c.qh) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(?e, "zwp_linux_dmabuf_v1: create_params failed");
                return;
            }
        };

        // SAFETY: `fd` is a valid open dma-buf file descriptor for the
        // duration of this call. The compositor takes a reference via the
        // Wayland protocol; we do not close it here.
        let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
        params.add(borrowed, 0, offset, stride, modifier);

        let (buffer, _params) = params.create_immed(width, height, format, DmabufFlags::empty(), &c.qh);
        drop(c);

        let wl_surface = self.xdg_window.wl_surface();
        wl_surface.attach(Some(&buffer), 0, 0);

        if damage_rects.is_empty() {
            wl_surface.damage_buffer(0, 0, width, height);
        } else {
            for &(x, y, w, h) in damage_rects {
                wl_surface.damage_buffer(x, y, w, h);
            }
        }

        wl_surface.commit();
    }
}
