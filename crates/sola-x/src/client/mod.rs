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
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
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

    /// Size-change requests from sola-compositor (via xdg_toplevel configure),
    /// queued for the main loop to forward to the matching X11 window.
    pub pending_configures: Vec<PendingConfigure>,
}

/// A resize request for an X11 window, carried from a proxy xdg_toplevel
/// configure event to the main loop where it can be applied via xwm.
pub struct PendingConfigure {
    pub x11_id: u32,
    pub width: u32,
    pub height: u32,
}

/// An input event received on a proxy surface, to be injected into XWayland's seat.
pub enum InputEvent {
    PointerEnter {
        x11_id: u32,
        x: f64,
        y: f64,
    },
    PointerLeave,
    PointerMotion {
        x: f64,
        y: f64,
        time: u32,
    },
    PointerButton {
        button: u32,
        pressed: bool,
        time: u32,
    },
    PointerAxis {
        value: f64,
        time: u32,
    },
    Key {
        key: u32,
        pressed: bool,
        time: u32,
    },
    KeyboardEnter {
        x11_id: u32,
    },
    KeyboardLeave,
    KeyboardModifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
    KeyboardKeymap {
        keymap: String,
    },
}

/// A proxy surface in sola-compositor representing an X11 window.
pub struct ProxySurface {
    pub surface: wl_surface::WlSurface,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub toplevel: xdg_toplevel::XdgToplevel,
}

/// User data stashed on each zwp_linux_buffer_params_v1 we create, so the
/// Dispatch callback for the async `Created` / `Failed` events knows which
/// proxy surface to attach the resulting wl_buffer to.
#[derive(Clone, Debug)]
pub struct ForwardBufferData {
    pub x11_window_id: u32,
    pub width: i32,
    pub height: i32,
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
            pending_configures: Vec::new(),
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

        Some(Self {
            conn,
            queue,
            qh,
            app,
        })
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
        let toplevel = xdg_surface.get_toplevel(&self.qh, x11_id);
        toplevel.set_title(title.to_string());
        toplevel.set_app_id(class.to_string());
        surface.commit();

        let surface_id = surface.id().protocol_id();
        tracing::info!(x11_id, title, class, surface_id, "created proxy surface");

        self.app.surface_to_x11.insert(surface_id, x11_id);
        self.app.proxies.insert(
            x11_id,
            ProxySurface {
                surface,
                xdg_surface,
                toplevel,
            },
        );
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
            tracing::info!(
                count = windows.len(),
                "re-created proxy surfaces after reconnect"
            );
        }
    }

    /// Drain queued input events (collected during dispatch).
    pub fn drain_input(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.app.pending_input)
    }

    /// Drain queued resize requests for X11 windows (from proxy toplevel configures).
    pub fn drain_configures(&mut self) -> Vec<PendingConfigure> {
        std::mem::take(&mut self.app.pending_configures)
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
                    state.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(
                        name,
                        version.min(6),
                        qh,
                        (),
                    ));
                }
                "xdg_wm_base" => {
                    state.xdg_wm_base = Some(registry.bind::<xdg_wm_base::XdgWmBase, _, _>(
                        name,
                        version.min(6),
                        qh,
                        (),
                    ));
                }
                "wl_seat" => {
                    state.seat =
                        Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(9), qh, ()));
                }
                "wl_shm" => {
                    state.shm =
                        Some(registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ()));
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
impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ForwardBufferData>
    for ClientApp
{
    fn event(
        state: &mut Self,
        params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        data: &ForwardBufferData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer } => {
                if let Some(proxy) = state.proxies.get(&data.x11_window_id) {
                    proxy.surface.attach(Some(&buffer), 0, 0);
                    proxy.surface.damage_buffer(0, 0, data.width, data.height);
                    proxy.surface.commit();
                }
                params.destroy();
            }
            zwp_linux_buffer_params_v1::Event::Failed => {
                tracing::debug!(
                    x11_id = data.x11_window_id,
                    "dmabuf import rejected by compositor; frame dropped"
                );
                params.destroy();
            }
            _ => {}
        }
    }
}

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

impl Dispatch<xdg_toplevel::XdgToplevel, u32> for ClientApp {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        data: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Configure carries the compositor's desired size. width/height
        // of 0 means "you pick" — leave the window alone in that case.
        if let xdg_toplevel::Event::Configure { width, height, .. } = event {
            if width > 0 && height > 0 {
                state.pending_configures.push(PendingConfigure {
                    x11_id: *data,
                    width: width as u32,
                    height: height as u32,
                });
            }
        }
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
                if let WEnum::Value(_) = axis {
                    state
                        .pending_input
                        .push(InputEvent::PointerAxis { value, time });
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
            wl_keyboard::Event::Enter { surface, .. } => {
                let surface_id = surface.id().protocol_id();
                if let Some(&x11_id) = state.surface_to_x11.get(&surface_id) {
                    state
                        .pending_input
                        .push(InputEvent::KeyboardEnter { x11_id });
                }
            }
            wl_keyboard::Event::Leave { .. } => {
                state.pending_input.push(InputEvent::KeyboardLeave);
            }
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
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                state.pending_input.push(InputEvent::KeyboardModifiers {
                    depressed: mods_depressed,
                    latched: mods_latched,
                    locked: mods_locked,
                    group,
                });
            }
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if !matches!(format, WEnum::Value(wl_keyboard::KeymapFormat::XkbV1)) {
                    tracing::warn!("unsupported keymap format from compositor");
                    return;
                }
                // Read the keymap text from the shared fd so we can hand
                // it to sola-x's own seat. xkb v1 keymaps are null-terminated.
                use std::io::{Read, Seek, SeekFrom};
                let mut file = std::fs::File::from(fd);
                let _ = file.seek(SeekFrom::Start(0));
                let mut buf = vec![0u8; size as usize];
                if let Err(e) = file.read_exact(&mut buf) {
                    tracing::warn!("failed to read compositor keymap: {e}");
                    return;
                }
                while buf.last() == Some(&0) {
                    buf.pop();
                }
                match String::from_utf8(buf) {
                    Ok(keymap) => {
                        state
                            .pending_input
                            .push(InputEvent::KeyboardKeymap { keymap });
                    }
                    Err(e) => {
                        tracing::warn!("compositor keymap is not valid utf-8: {e}");
                    }
                }
            }
            _ => {}
        }
    }
}
