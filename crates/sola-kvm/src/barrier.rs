//! Physical edge capture via `zwlr_layer_shell_v1` (thin strip on the shared edge).
//!
//! The compositor only delivers pointer enter when the **real** cursor hits
//! that edge — relative-evdev estimates cannot fake it.

use std::os::fd::AsFd;

use tracing::{debug, info, warn};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::layout::Side;

/// Strip thickness (lan-mouse uses 1; we use 2 for easier hit-testing).
const STRIP: i32 = 2;

/// Wayland edge barrier. Poll from the server loop while local.
pub struct EdgeBarrier {
    conn: Connection,
    qh: QueueHandle<BarrierState>,
    event_queue: EventQueue<BarrierState>,
    state: BarrierState,
}

struct BarrierState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    seat: Option<wl_seat::WlSeat>,
    /// First bound output (prefer binding the layer surface to it).
    output: Option<wl_output::WlOutput>,
    /// Output geometries discovered so far.
    output_sizes: Vec<(i32, i32)>,
    primary_w: i32,
    primary_h: i32,
    side: Side,

    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    pointer: Option<wl_pointer::WlPointer>,
    configured: bool,
    /// Size assigned by the compositor (after configure).
    surface_w: i32,
    surface_h: i32,
    active: bool,
    /// Coordinate along the edge when the real pointer entered the strip
    /// (y for left/right, x for top/bottom), in surface-local coords.
    pending_hit_along: Option<i32>,
}

impl EdgeBarrier {
    pub fn connect(side: Side, primary_w: i32, primary_h: i32) -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| {
            format!("wayland connect: {e} (WAYLAND_DISPLAY set? sola-river up?)")
        })?;
        let (globals, mut event_queue) =
            registry_queue_init::<BarrierState>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = event_queue.handle();

        let mut state = BarrierState {
            compositor: None,
            shm: None,
            layer_shell: None,
            seat: None,
            output: None,
            output_sizes: Vec::new(),
            primary_w,
            primary_h,
            side,
            surface: None,
            layer_surface: None,
            pointer: None,
            configured: false,
            surface_w: 0,
            surface_h: 0,
            active: true,
            pending_hit_along: None,
        };

        state.compositor = Some(
            globals
                .bind(&qh, 1..=6, ())
                .map_err(|e| format!("wl_compositor: {e}"))?,
        );
        state.shm = Some(
            globals
                .bind(&qh, 1..=1, ())
                .map_err(|e| format!("wl_shm: {e}"))?,
        );
        state.layer_shell = Some(
            globals
                .bind(&qh, 1..=4, ())
                .map_err(|e| {
                    format!("zwlr_layer_shell_v1: {e} (sola-river must enable layer-shell)")
                })?,
        );
        // Multi-instance: bind first seat only for pointer enter.
        state.seat = Some(
            globals
                .bind(&qh, 1..=8, ())
                .map_err(|e| format!("wl_seat: {e}"))?,
        );
        // Multi-instance outputs: bind the first advertised one for geometry + layer target.
        // (Further outputs can arrive via registry Global events.)
        if let Ok(output) = globals.bind::<wl_output::WlOutput, _, _>(&qh, 1..=4, ()) {
            state.output = Some(output);
        }

        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip: {e}"))?;

        if let Some((w, h)) = state
            .output_sizes
            .iter()
            .copied()
            .max_by_key(|(w, h)| (*w as i64) * (*h as i64))
        {
            if w > 0 && h > 0 {
                info!(w, h, "barrier: using compositor output size");
                state.primary_w = w;
                state.primary_h = h;
            }
        }

        info!(
            ?side,
            w = state.primary_w,
            h = state.primary_h,
            "layer-shell edge barrier mapping strip (physical edge only)"
        );
        create_strip(&mut state, &qh)?;
        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("configure: {e}"))?;

        Ok(Self {
            conn,
            qh,
            event_queue,
            state,
        })
    }

    /// Non-blocking pump; `Some(y)` when the real cursor enters the strip.
    pub fn poll_hit(&mut self) -> Result<Option<i32>, String> {
        let _ = self.conn.flush();
        self.event_queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| format!("dispatch: {e}"))?;

        // Non-blocking socket read.
        let mut pfd = libc::pollfd {
            fd: self.conn.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll on wayland display fd, timeout 0.
        let n = unsafe { libc::poll(&mut pfd, 1, 0) };
        if n > 0 {
            if let Some(guard) = self.conn.prepare_read() {
                let _ = guard.read();
            }
            self.event_queue
                .dispatch_pending(&mut self.state)
                .map_err(|e| format!("dispatch2: {e}"))?;
        }
        Ok(self.state.pending_hit_along.take())
    }

    pub fn set_active(&mut self, active: bool) -> Result<(), String> {
        if self.state.active == active {
            return Ok(());
        }
        self.state.active = active;
        if active {
            create_strip(&mut self.state, &self.qh)?;
        } else {
            destroy_strip(&mut self.state);
        }
        self.event_queue
            .roundtrip(&mut self.state)
            .map_err(|e| format!("set_active: {e}"))?;
        Ok(())
    }

    pub fn primary_size(&self) -> (i32, i32) {
        (self.state.primary_w, self.state.primary_h)
    }
}

fn create_strip(state: &mut BarrierState, qh: &QueueHandle<BarrierState>) -> Result<(), String> {
    destroy_strip(state);

    let compositor = state.compositor.as_ref().ok_or("no compositor")?;
    let layer_shell = state.layer_shell.as_ref().ok_or("no layer_shell")?;
    let surface = compositor.create_surface(qh, ());

    // Bind to a concrete output when we have one (matches lan-mouse).
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        state.output.as_ref(),
        Layer::Overlay,
        String::from("sola-kvm-edge"),
        qh,
        (),
    );

    // lan-mouse pattern: anchor to ONE edge only, size = strip × full edge length,
    // exclusive_zone = -1 (extend to edges; don't get pushed by other exclusives).
    let (w, h, anchor) = match state.side {
        Side::Right => (STRIP as u32, state.primary_h.max(1) as u32, Anchor::Right),
        Side::Left => (STRIP as u32, state.primary_h.max(1) as u32, Anchor::Left),
        Side::Top => (state.primary_w.max(1) as u32, STRIP as u32, Anchor::Top),
        Side::Bottom => (state.primary_w.max(1) as u32, STRIP as u32, Anchor::Bottom),
    };

    layer_surface.set_anchor(anchor);
    layer_surface.set_size(w, h);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    // Entire surface is input-sensitive (don't inherit a tiny buffer region).
    surface.set_input_region(None);
    surface.commit();

    state.surface = Some(surface);
    state.layer_surface = Some(layer_surface);
    state.configured = false;
    state.surface_w = w as i32;
    state.surface_h = h as i32;
    state.pending_hit_along = None;
    info!(w, h, ?state.side, "barrier strip requested (full edge length)");
    Ok(())
}

fn destroy_strip(state: &mut BarrierState) {
    if let Some(ls) = state.layer_surface.take() {
        ls.destroy();
    }
    if let Some(s) = state.surface.take() {
        s.destroy();
    }
    state.configured = false;
}

/// Transparent ARGB buffer matching the strip pixel size (lan-mouse style).
fn attach_strip_buffer(
    state: &BarrierState,
    surface: &wl_surface::WlSurface,
    width: i32,
    height: i32,
    qh: &QueueHandle<BarrierState>,
) {
    let Some(shm) = state.shm.as_ref() else {
        return;
    };
    let width = width.max(1);
    let height = height.max(1);
    let stride = width * 4;
    let size = (stride * height) as usize;
    let mut memfd = match rustix::fs::memfd_create(
        "sola-kvm-barrier",
        rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
    ) {
        Ok(f) => f,
        Err(e) => {
            warn!(%e, "memfd_create failed");
            return;
        }
    };
    // Transparent black ARGB8888.
    let zeros = vec![0u8; size];
    if rustix::io::write(&mut memfd, &zeros).is_err() {
        return;
    }
    let pool = shm.create_pool(memfd.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width,
        height,
        stride,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.damage(0, 0, width, height);
    surface.set_input_region(None);
    surface.commit();
    pool.destroy();
    let _ = buffer;
}

// --- Dispatch ---------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for BarrierState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "wl_output" {
                let output: wl_output::WlOutput =
                    registry.bind(name, version.min(3), qh, ());
                if state.output.is_none() {
                    state.output = Some(output);
                }
                debug!(name, "barrier: bound wl_output (hotplug)");
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for BarrierState {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for BarrierState {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for BarrierState {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for BarrierState {
    fn event(
        _: &mut Self,
        proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) {
            proxy.destroy();
        }
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for BarrierState {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for BarrierState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            if let WEnum::Value(caps) = capabilities {
                if caps.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                    state.pointer = Some(seat.get_pointer(qh, ()));
                    debug!("barrier: wl_pointer bound");
                }
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for BarrierState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if !state.active {
            return;
        }
        if let wl_pointer::Event::Enter {
            surface,
            surface_x,
            surface_y,
            ..
        } = event
        {
            let ours = state.surface.as_ref().is_some_and(|s| s == &surface);
            if !ours {
                return;
            }
            // Along-edge coordinate: y for left/right strips, x for top/bottom.
            let along = match state.side {
                Side::Left | Side::Right => {
                    let max = state.surface_h.max(state.primary_h).saturating_sub(1);
                    (surface_y as i32).clamp(0, max)
                }
                Side::Top | Side::Bottom => {
                    let max = state.surface_w.max(state.primary_w).saturating_sub(1);
                    (surface_x as i32).clamp(0, max)
                }
            };
            info!(
                along,
                surface_x,
                surface_y,
                surface_w = state.surface_w,
                surface_h = state.surface_h,
                "PHYSICAL edge hit (layer-shell barrier)"
            );
            state.pending_hit_along = Some(along);
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for BarrierState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            width,
            height,
            flags,
            ..
        } = event
        {
            let current = match flags {
                WEnum::Value(f) => f.contains(wl_output::Mode::Current),
                _ => true,
            };
            if current && width > 0 && height > 0 {
                state.output_sizes.push((width, height));
            }
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for BarrierState {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for BarrierState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer_surface.ack_configure(serial);
                state.configured = true;
                // Compositor may rewrite size; use what it assigned for the buffer.
                let w = if width == 0 {
                    state.surface_w.max(STRIP)
                } else {
                    width as i32
                };
                let h = if height == 0 {
                    state.surface_h.max(STRIP)
                } else {
                    height as i32
                };
                state.surface_w = w;
                state.surface_h = h;
                // Keep primary edge length in sync with what the compositor gave us.
                match state.side {
                    Side::Left | Side::Right if h > 0 => state.primary_h = h,
                    Side::Top | Side::Bottom if w > 0 => state.primary_w = w,
                    _ => {}
                }
                info!(w, h, "barrier layer configured (full-edge buffer)");
                if let Some(surface) = state.surface.clone() {
                    attach_strip_buffer(state, &surface, w, h, qh);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                warn!("barrier layer surface closed");
                state.configured = false;
            }
            _ => {}
        }
    }
}

use std::os::fd::AsRawFd;
