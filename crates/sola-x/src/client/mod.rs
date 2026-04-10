/// Client-side Wayland connection to sola-compositor.
///
/// sola-x connects as a regular Wayland client, creating proxy
/// xdg_toplevel surfaces for each X11 window. Uses a dedicated
/// `ClientApp` type for wayland-client dispatch (separate from
/// the server-side Smithay types on State).
use std::collections::HashMap;

use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

/// Holds the Wayland client connection to sola-compositor.
/// Rebuilt on each reconnection.
pub struct ClientConnection {
    pub conn: Connection,
    pub queue: EventQueue<ClientApp>,
    pub qh: QueueHandle<ClientApp>,
    pub app: ClientApp,
}

/// Client-side state and dispatch target.
pub struct ClientApp {
    // Bound globals from sola-compositor.
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub xdg_wm_base: Option<xdg_wm_base::XdgWmBase>,
    pub seat: Option<wl_seat::WlSeat>,
    pub shm: Option<wl_shm::WlShm>,
    pub dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,

    /// Proxy surfaces in sola-compositor, keyed by X11 window ID.
    pub proxies: HashMap<u32, ProxySurface>,

    /// Reverse map: client-side wl_surface ID → X11 window ID.
    pub surface_to_x11: HashMap<u32, u32>,

    /// Input events queued by Dispatch callbacks for the main loop to inject.
    pub pending_input: Vec<InputEvent>,
}

/// An input event received on a proxy surface, to be injected into XWayland's seat.
pub enum InputEvent {
    PointerEnter { x11_id: u32, x: f64, y: f64 },
    PointerLeave,
    PointerMotion { x: f64, y: f64, time: u32 },
    PointerButton { button: u32, pressed: bool, time: u32 },
    PointerAxis { axis: u32, value: f64, time: u32 },
    Key { key: u32, pressed: bool, time: u32 },
}

/// A proxy surface in sola-compositor representing an X11 window.
pub struct ProxySurface {
    pub surface: wl_surface::WlSurface,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub toplevel: xdg_toplevel::XdgToplevel,
}

impl ClientConnection {
    /// Connect to sola-compositor's Wayland display (wayland-0).
    /// Connects explicitly by path rather than using WAYLAND_DISPLAY,
    /// since sola-x sets that to its own socket (wayland-x0) for XWayland.
    pub fn connect() -> Option<Self> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
        let path = format!("{runtime_dir}/wayland-0");
        let stream = std::os::unix::net::UnixStream::connect(&path).ok()?;
        let conn = Connection::from_socket(stream).ok()?;
        let mut queue = conn.new_event_queue::<ClientApp>();
        let qh = queue.handle();

        conn.display().get_registry(&qh, ());

        let mut app = ClientApp {
            compositor: None,
            xdg_wm_base: None,
            seat: None,
            shm: None,
            dmabuf: None,
            proxies: HashMap::new(),
            surface_to_x11: HashMap::new(),
            pending_input: Vec::new(),
        };

        // Roundtrip to bind globals.
        queue.roundtrip(&mut app).ok()?;

        // Verify required globals are available.
        if app.compositor.is_none() || app.xdg_wm_base.is_none() {
            tracing::warn!("compositor missing required globals");
            return None;
        }

        // Request pointer and keyboard so we receive input on proxy surfaces.
        if let Some(seat) = &app.seat {
            seat.get_pointer(&qh, ());
            seat.get_keyboard(&qh, ());
        }

        tracing::info!("connected to sola-compositor as Wayland client");

        Some(Self { conn, queue, qh, app })
    }

    /// Create a proxy surface in sola-compositor for an X11 window.
    pub fn create_proxy(&mut self, x11_id: u32, title: &str, class: &str) {
        let compositor = match &self.app.compositor {
            Some(c) => c,
            None => return,
        };
        let xdg_wm_base = match &self.app.xdg_wm_base {
            Some(x) => x,
            None => return,
        };

        let surface = compositor.create_surface(&self.qh, ());
        let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &self.qh, ());
        let toplevel = xdg_surface.get_toplevel(&self.qh, ());
        toplevel.set_title(title.to_string());
        toplevel.set_app_id(class.to_string());
        surface.commit();

        let surface_id = surface.id().protocol_id();
        tracing::info!(x11_id, title, class, surface_id, "created proxy surface");

        self.app.surface_to_x11.insert(surface_id, x11_id);
        self.app.proxies.insert(x11_id, ProxySurface {
            surface,
            xdg_surface,
            toplevel,
        });
    }

    /// Destroy a proxy surface.
    pub fn destroy_proxy(&mut self, x11_id: u32) {
        if let Some(proxy) = self.app.proxies.remove(&x11_id) {
            proxy.toplevel.destroy();
            proxy.xdg_surface.destroy();
            proxy.surface.destroy();
            tracing::info!(x11_id, "destroyed proxy surface");
        }
    }

    /// Re-create proxy surfaces for all currently tracked X11 windows.
    /// Called after reconnecting to sola-compositor.
    pub fn recreate_proxies(&mut self, windows: &[(u32, String, String)]) {
        for (x11_id, title, class) in windows {
            self.create_proxy(*x11_id, title, class);
        }
        if !windows.is_empty() {
            // Roundtrip to ensure surfaces are created before we start forwarding.
            let _ = self.queue.roundtrip(&mut self.app);
            tracing::info!(count = windows.len(), "re-created proxy surfaces after reconnect");
        }
    }

    /// Drain queued input events (collected during dispatch).
    pub fn drain_input(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.app.pending_input)
    }

    /// Read new events from the connection and dispatch them.
    /// Returns Err on connection loss.
    pub fn dispatch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Read any new data from the socket into the event queue buffer.
        if let Some(guard) = self.queue.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.queue.dispatch_pending(&mut self.app)?;
        self.conn.flush()?;
        Ok(())
    }
}

// -- Dispatch implementations for ClientApp --

impl Dispatch<wl_registry::WlRegistry, ()> for ClientApp {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(
                        registry.bind::<wl_compositor::WlCompositor, _, _>(
                            name,
                            version.min(6),
                            qh,
                            (),
                        ),
                    );
                }
                "xdg_wm_base" => {
                    state.xdg_wm_base = Some(
                        registry.bind::<xdg_wm_base::XdgWmBase, _, _>(
                            name,
                            version.min(6),
                            qh,
                            (),
                        ),
                    );
                }
                "wl_seat" => {
                    state.seat = Some(
                        registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(9), qh, ()),
                    );
                }
                "wl_shm" => {
                    state.shm = Some(
                        registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ()),
                    );
                }
                "zwp_linux_dmabuf_v1" => {
                    state.dmabuf = Some(
                        registry.bind::<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, _, _>(
                            name,
                            version.min(4),
                            qh,
                            (),
                        ),
                    );
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(ClientApp: ignore wl_compositor::WlCompositor);
delegate_noop!(ClientApp: ignore wl_surface::WlSurface);
delegate_noop!(ClientApp: ignore wl_shm::WlShm);
delegate_noop!(ClientApp: ignore wl_shm_pool::WlShmPool);
delegate_noop!(ClientApp: ignore wl_buffer::WlBuffer);
delegate_noop!(ClientApp: ignore wl_callback::WlCallback);
delegate_noop!(ClientApp: ignore wl_seat::WlSeat);
delegate_noop!(ClientApp: ignore zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1);
delegate_noop!(ClientApp: ignore zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for ClientApp {
    fn event(
        _state: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientApp {
    fn event(
        _state: &mut Self,
        proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            proxy.ack_configure(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for ClientApp {
    fn event(
        _state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        _event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for ClientApp {
    fn event(
        state: &mut Self,
        _proxy: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                let surface_id = surface.id().protocol_id();
                tracing::debug!(surface_id, surface_x, surface_y, "pointer enter on proxy");
                if let Some(&x11_id) = state.surface_to_x11.get(&surface_id) {
                    state.pending_input.push(InputEvent::PointerEnter {
                        x11_id,
                        x: surface_x,
                        y: surface_y,
                    });
                }
            }
            wl_pointer::Event::Leave { .. } => {
                state.pending_input.push(InputEvent::PointerLeave);
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                time,
                ..
            } => {
                tracing::debug!(surface_x, surface_y, "pointer motion on proxy");
                state.pending_input.push(InputEvent::PointerMotion {
                    x: surface_x,
                    y: surface_y,
                    time,
                });
            }
            wl_pointer::Event::Button {
                button,
                state: btn_state,
                time,
                ..
            } => {
                let pressed = matches!(btn_state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                state.pending_input.push(InputEvent::PointerButton {
                    button,
                    pressed,
                    time,
                });
            }
            wl_pointer::Event::Axis {
                axis, value, time, ..
            } => {
                if let WEnum::Value(a) = axis {
                    state.pending_input.push(InputEvent::PointerAxis {
                        axis: a as u32,
                        value,
                        time,
                    });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for ClientApp {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                time,
                ..
            } => {
                let pressed = matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed));
                state
                    .pending_input
                    .push(InputEvent::Key { key, pressed, time });
            }
            _ => {}
        }
    }
}
