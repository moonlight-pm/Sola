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
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle, WEnum};
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
}

/// A proxy surface in sola-compositor representing an X11 window.
pub struct ProxySurface {
    pub surface: wl_surface::WlSurface,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub toplevel: xdg_toplevel::XdgToplevel,
}

impl ClientConnection {
    /// Connect to sola-compositor's Wayland display.
    pub fn connect() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
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
        };

        // Roundtrip to bind globals.
        queue.roundtrip(&mut app).ok()?;

        // Verify required globals are available.
        if app.compositor.is_none() || app.xdg_wm_base.is_none() {
            tracing::warn!("compositor missing required globals");
            return None;
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

        tracing::info!(x11_id, title, class, "created proxy surface");

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

    /// Dispatch pending events and flush. Returns Err on connection loss.
    pub fn dispatch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
        _state: &mut Self,
        _proxy: &wl_pointer::WlPointer,
        _event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // TODO: Phase 4 — forward input to server-side XWayland seat.
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for ClientApp {
    fn event(
        _state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        _event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // TODO: Phase 4 — forward input to server-side XWayland seat.
    }
}
