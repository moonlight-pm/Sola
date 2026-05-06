//! Per-window xdg_toplevel + two CEF OSR transports.
//!
//! `Surface::new` creates a `wl_surface` / `xdg_toplevel` pair, sets the
//! title/app_id/min_size, and commits the initial empty frame so the
//! compositor will send the first configure.
//!
//! There are two ways CEF delivers the rendered frame; we support both:
//!
//!   1. **`present_dmabuf`** — for `shared_texture_enabled = 1`. CEF's GPU
//!      process produces a dma-buf; we import via `zwp_linux_dmabuf_v1::
//!      create_immed` for zero-copy presentation. Requires Mesa-style EGL
//!      (NVIDIA proprietary doesn't expose the `*MESA` extensions ANGLE
//!      needs at link time — see Distribution.md → "Known incompatibility").
//!   2. **`present_paint`** — for `shared_texture_enabled = 0`. CEF's GPU
//!      process reads pixels back to CPU memory and hands us a BGRA8888
//!      buffer pointer via `OnPaint`. We memcpy into a sctk `SlotPool`
//!      (wl_shm) and attach the resulting `wl_buffer`. Works on any GPU
//!      stack including SwiftShader.

use std::cell::RefCell;
use std::os::unix::io::{BorrowedFd, RawFd};
use std::rc::Rc;

use smithay_client_toolkit::shell::{
    xdg::window::{Window, WindowDecorations},
    WaylandSurface,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use wayland_client::protocol::wl_shm;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags as DmabufFlags;

use crate::wayland::WaylandClient;
use crate::window::WindowConfig;

#[allow(unused_imports)]
use ::cef::{rc::*, *};

/// App-id reported to the compositor for all sola-kit windows.
/// TODO(later): plumb the actual APP_ID through from `SolaApp::APP_ID`.
const APP_ID: &str = "sola.kit";

/// Per-window Wayland state: a `wl_surface` mapped as an `xdg_toplevel`,
/// plus enough context to present CEF-produced frames via either dma-buf
/// (zero-copy GPU) or wl_shm (CPU readback via OnPaint).
pub struct Surface {
    /// The underlying Wayland surface.
    pub xdg_window: Window,
    /// Current logical size in pixels.
    pub size: RefCell<(i32, i32)>,
    /// Set to `true` after the first `configure` is acked. CEF rendering must
    /// not start before the compositor has sent an initial configure.
    pub configured: RefCell<bool>,
    /// Shared reference back to the per-process Wayland connection, used to
    /// reach `dmabuf_state`, `shm`, and `qh`.
    pub client: Rc<RefCell<WaylandClient>>,
    /// Lazy-initialised shm pool for `present_paint`. Created on the first
    /// call sized to fit the current surface; grows automatically inside
    /// sctk if a later frame is bigger.
    paint_pool: RefCell<Option<SlotPool>>,
    /// The CEF browser bound to this surface. Set by `Surface::bind_browser`
    /// after `browser_host_create_browser_sync` returns; consumed by
    /// `Surface::on_configure` to drive `BrowserHost::was_resized` when the
    /// compositor changes our size.
    pub browser: RefCell<Option<::cef::Browser>>,
}

impl Surface {
    /// Create a new xdg_toplevel surface from `cfg` and commit the initial
    /// empty frame. Returns `Rc<Self>` so it can be shared with closures.
    pub fn new(client: Rc<RefCell<WaylandClient>>, cfg: &WindowConfig) -> Rc<Self> {
        tracing::debug!(title = %cfg.title, size = ?cfg.size, "Surface::new");
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

            xdg_window
        };

        let surface = Rc::new(Self {
            xdg_window,
            size: RefCell::new(cfg.size),
            configured: RefCell::new(false),
            client: client.clone(),
            paint_pool: RefCell::new(None),
            browser: RefCell::new(None),
        });
        client.borrow_mut().register_surface(&surface);
        surface
    }

    /// Current logical size in pixels.
    pub fn size(&self) -> (i32, i32) {
        *self.size.borrow()
    }

    /// Bind the CEF browser created against this surface. Called from
    /// `ctx::add_window` immediately after `Browser::new`. The configure
    /// handler uses this to invoke `BrowserHost::was_resized` so CEF
    /// re-queries `RenderHandler::view_rect` and rasterises at the new size.
    pub fn bind_browser(&self, browser: ::cef::Browser) {
        *self.browser.borrow_mut() = Some(browser);
    }

    /// Apply a compositor configure: update our cached size and notify CEF.
    /// Compositors send 0/0 for "you choose"; ignore that and keep our last
    /// known size.
    pub fn on_configure(&self, new_size: Option<(u32, u32)>) {
        if let Some((w, h)) = new_size {
            if w > 0 && h > 0 {
                *self.size.borrow_mut() = (w as i32, h as i32);
            }
        }
        *self.configured.borrow_mut() = true;

        if let Some(browser) = self.browser.borrow().as_ref() {
            if let Some(host) = browser.host() {
                host.was_resized();
            }
        }
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

    /// CPU OSR transport. Called from `KitRenderHandler::on_paint` when CEF
    /// runs with `shared_texture_enabled = 0`.
    ///
    /// `pixels` is a `BGRA8888` (memory order: B, G, R, A) buffer of length
    /// at least `(width * height * 4)` bytes, valid only for the duration
    /// of this call (CEF reuses it). We memcpy into a `wl_shm` slot, attach
    /// the resulting `wl_buffer`, damage the rects, and commit.
    ///
    /// `damage_rects` are buffer-coordinate (x, y, w, h). Empty = full
    /// surface. `wl_surface.damage_buffer` accepts multiple — the
    /// compositor unions them.
    pub fn present_paint(
        &self,
        pixels: *const u8,
        width: i32,
        height: i32,
        damage_rects: &[(i32, i32, i32, i32)],
    ) {
        if width <= 0 || height <= 0 {
            return;
        }
        let stride = width * 4;
        let frame_bytes = (stride as usize) * (height as usize);

        let mut pool_slot = self.paint_pool.borrow_mut();
        let pool = match pool_slot.as_mut() {
            Some(p) => p,
            None => {
                let c = self.client.borrow();
                match SlotPool::new(frame_bytes, &c.shm) {
                    Ok(p) => {
                        *pool_slot = Some(p);
                        pool_slot.as_mut().unwrap()
                    }
                    Err(e) => {
                        tracing::error!(?e, "wl_shm: SlotPool::new failed");
                        return;
                    }
                }
            }
        };

        // sctk grows the pool internally if `frame_bytes` exceeds the
        // current capacity, so this works across resize events without
        // an explicit pool recreation.
        let (buffer, canvas) = match pool.create_buffer(
            width,
            height,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(e) => {
                tracing::error!(?e, "wl_shm: create_buffer failed");
                return;
            }
        };

        // SAFETY: CEF guarantees `pixels` is valid for `frame_bytes` bytes
        // for the duration of `OnPaint`. `canvas` is a fresh writable mmap
        // region of the same size we just requested. Non-overlapping by
        // construction (different allocations).
        unsafe {
            std::ptr::copy_nonoverlapping(pixels, canvas.as_mut_ptr(), frame_bytes);
        }

        let wl_surface = self.xdg_window.wl_surface();
        if let Err(e) = buffer.attach_to(wl_surface) {
            tracing::error!(?e, "wl_shm: buffer.attach_to failed");
            return;
        }

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
