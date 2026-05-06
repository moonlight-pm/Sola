//! Per-process Wayland connection. One global, shared by all Surfaces.
//!
//! `WaylandClient::connect_owned()` connects to the compositor and binds all
//! required globals. Callers wrap the returned value in `Rc<RefCell<>>` to
//! share it across surfaces.

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_dmabuf, delegate_output, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    dmabuf::{DmabufFeedback, DmabufHandler, DmabufState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::xdg::{
        window::{Window, WindowConfigure, WindowHandler},
        XdgShell,
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{
        wl_buffer::WlBuffer,
        wl_output::{Transform, WlOutput},
        wl_seat::WlSeat,
        wl_surface::WlSurface,
    },
    Connection, EventQueue, QueueHandle,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
    zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
};

/// Per-process Wayland connection and bound globals. Single-threaded; callers
/// wrap in `Rc<RefCell<WaylandClient>>` to share across surfaces.
pub struct WaylandClient {
    pub conn: Connection,
    pub registry_state: RegistryState,
    pub compositor_state: CompositorState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub xdg_shell: XdgShell,
    /// sctk-managed zwp_linux_dmabuf_v1 state (binds v3..=5).
    /// Used by `Surface::present_dmabuf` for the GPU-accelerated OSR
    /// transport (`shared_texture_enabled = 1`). Currently unused at
    /// runtime — see `Browser::new` for the active transport choice.
    pub dmabuf_state: DmabufState,
    /// sctk-managed wl_shm state. Used by `Surface::present_paint` for
    /// the CPU OSR transport (`OnPaint` from CEF → BGRA8888 in shared
    /// memory → wl_buffer).
    pub shm: Shm,
    /// The event queue. Owned here; pumped via `dispatch_pending()`.
    /// Stored in an `Option` so `dispatch_pending(&mut self)` can `take()`
    /// the queue, hand `&mut self` to `EventQueue::dispatch_pending`, and
    /// put the queue back — avoiding a "borrow self.queue while &mut self
    /// is also passed" conflict. The queue is always Some between calls.
    pub queue: Option<EventQueue<WaylandClient>>,
    pub qh: QueueHandle<WaylandClient>,
}

impl WaylandClient {
    /// Connect to the Wayland compositor and bind all required globals.
    /// Returns an owned value; callers wrap in `Rc<RefCell<>>` to share.
    ///
    /// Panics if the connection fails or a required protocol is absent.
    pub fn connect_owned() -> Self {
        let conn = Connection::connect_to_env()
            .expect("Wayland: cannot connect to compositor");

        let (globals, event_queue) = registry_queue_init::<Self>(&conn)
            .expect("Wayland: registry init failed");
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let compositor_state = CompositorState::bind(&globals, &qh)
            .expect("Wayland: wl_compositor missing");
        let seat_state = SeatState::new(&globals, &qh);
        let output_state = OutputState::new(&globals, &qh);
        let xdg_shell = XdgShell::bind(&globals, &qh)
            .expect("Wayland: xdg_wm_base missing");

        // sctk wraps zwp_linux_dmabuf_v1 v3..=5 in DmabufState. We require v3+
        // (v4 enables create_immed and per-modifier feedback; v3 has modifier
        // events on the global). DmabufState::new does not fail if absent — we
        // check the version at present_dmabuf time.
        let dmabuf_state = DmabufState::new(&globals, &qh);

        // wl_shm is mandatory in any Wayland compositor (core protocol).
        // sctk's `Shm::bind` exits with `BindError` if missing.
        let shm = Shm::bind(&globals, &qh).expect("Wayland: wl_shm missing");

        Self {
            conn,
            registry_state,
            compositor_state,
            seat_state,
            output_state,
            xdg_shell,
            dmabuf_state,
            shm,
            queue: Some(event_queue),
            qh,
        }
    }

    /// Non-blocking event pump. Call from the CEF main loop on each frame tick.
    ///
    /// `dispatch_pending` alone only drains the in-memory event queue — it
    /// does NOT read from the Wayland socket. So we follow the standard
    /// integrated-event-loop pattern:
    ///   1. `flush()` — send our pending requests to the compositor.
    ///   2. `prepare_read()` + `read()` — if the socket has bytes ready,
    ///      drain them into the in-memory queue. Returns `WouldBlock` when
    ///      the socket is empty, which is fine — we just have nothing new.
    ///   3. `dispatch_pending()` — process whatever's in the queue (newly
    ///      read or otherwise).
    pub fn dispatch_pending(&mut self) {
        if let Some(mut queue) = self.queue.take() {
            let _ = queue.flush();
            if let Some(guard) = queue.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => tracing::warn!(?e, "wayland read failed"),
                }
            }
            let _ = queue.dispatch_pending(self);
            self.queue = Some(queue);
        }
    }
}

// ── sctk delegate macros ─────────────────────────────────────────────────────

delegate_registry!(WaylandClient);
delegate_output!(WaylandClient);
delegate_seat!(WaylandClient);
delegate_compositor!(WaylandClient);
delegate_xdg_shell!(WaylandClient);
delegate_xdg_window!(WaylandClient);
delegate_dmabuf!(WaylandClient);
delegate_shm!(WaylandClient);

impl ShmHandler for WaylandClient {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

// ── ProvidesRegistryState ────────────────────────────────────────────────────

impl ProvidesRegistryState for WaylandClient {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

// ── OutputHandler ────────────────────────────────────────────────────────────

impl OutputHandler for WaylandClient {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}
    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}
    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }
}

// ── SeatHandler ──────────────────────────────────────────────────────────────

impl SeatHandler for WaylandClient {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: WlSeat,
        _capability: Capability,
    ) {
        // TODO(B9 follow-up / D4): on Capability::Keyboard, get_keyboard and store.
        // TODO(D6): on Capability::Pointer, get_pointer and store.
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}
}

// ── CompositorHandler ────────────────────────────────────────────────────────

impl CompositorHandler for WaylandClient {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_transform: Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _time: u32,
    ) {
        // TODO(B11): drive cef external_begin_frame here.
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }
}

// ── WindowHandler ─────────────────────────────────────────────────────────────

impl WindowHandler for WaylandClient {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        // Surfaces close via bus CloseApp, not compositor-initiated close.
        // Proper close handling lands in a later checkpoint.
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        tracing::debug!(new_size = ?configure.new_size, "WindowHandler::configure");
        // sctk's xdg_surface dispatcher already called ack_configure(serial)
        // before invoking us. Marking the Surface configured + dispatching
        // host.was_resized to CEF lands when we have the surface↔window map
        // (planned with surface_to_browser in D4).
    }
}

// ── DmabufHandler ────────────────────────────────────────────────────────────

impl DmabufHandler for WaylandClient {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_feedback(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _proxy: &ZwpLinuxDmabufFeedbackV1,
        _feedback: DmabufFeedback,
    ) {
        // TODO(later): cache supported format/modifier pairs for CEF's
        // on_accelerated_paint dma-buf validation.
    }

    fn created(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _params: &ZwpLinuxBufferParamsV1,
        _buffer: WlBuffer,
    ) {
        // create_immed is used in Surface::present_dmabuf — this callback is
        // only triggered by the async create() path, which we don't use.
    }

    fn failed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _params: &ZwpLinuxBufferParamsV1,
    ) {
        tracing::error!("zwp_linux_buffer_params_v1: create failed");
    }

    fn released(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _buffer: &WlBuffer,
    ) {
        // Compositor released the wl_buffer; CEF will supply a fresh dma-buf
        // on the next on_accelerated_paint callback.
    }
}
