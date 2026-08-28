//! sctk + calloop River client. No winit.
//!
//! Identity is [`crate::APP_ID`] / [`crate::WINDOW_TITLE`]. Client-side
//! decorations only — River must not draw SSD.

use std::ptr::NonNull;
use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, Proxy, QueueHandle,
};

use crate::app::{App, Click};
use crate::gpu::Present;
use crate::{APP_ID, WINDOW_TITLE};

const BTN_LEFT: u32 = 0x110;
const DEFAULT_W: u32 = 960;
const DEFAULT_H: u32 = 680;
/// Match sola-kit `titlebar::floating_frame` hit bands.
const EDGE_GRIP: f32 = 12.0;
const CORNER_GRIP: f32 = 18.0;

pub fn run() {
    boot();

    let conn = Connection::connect_to_env().expect("wayland connect");
    let (globals, event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<Host> = EventLoop::try_new().expect("calloop");
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .expect("wayland source");

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg_wm_base");
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::ClientOnly, &qh);
    window.set_app_id(APP_ID);
    window.set_title(WINDOW_TITLE);
    window.set_min_size(Some((480, 360)));
    window.commit();
    let _ = conn.flush();

    let mut host = Host {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        conn,
        window,
        present: None,
        app: App::new(DEFAULT_W as f32, DEFAULT_H as f32, 1.0),
        width: DEFAULT_W,
        height: DEFAULT_H,
        scale: 1,
        seat: None,
        keyboard: None,
        pointer: None,
        cursor: (0.0, 0.0),
        exit: false,
        dirty: false,
        configured: false,
    };

    tracing::info!(app_id = APP_ID, title = WINDOW_TITLE, "sctk window created");
    let mut last = std::time::Instant::now();

    loop {
        let wait = if host.app.needs_frame() {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(200)
        };
        event_loop.dispatch(wait, &mut host).expect("dispatch");
        if host.exit {
            break;
        }
        if !host.configured {
            continue;
        }
        let now = std::time::Instant::now();
        let dt = now.saturating_duration_since(last).as_secs_f32();
        last = now;
        host.app.tick(dt);
        if host.app.reload_if_changed() || host.app.needs_frame() {
            host.dirty = true;
        }
        if host.dirty {
            host.draw();
        }
    }
}

fn boot() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!("{}=info,sola_kit_spike=info", APP_ID.replace('-', "_")).into()
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("{APP_ID} starting");
    let socket = activate_wayland_session(20_000);
    tracing::info!(socket = %socket, "wayland socket resolved");
    if wait_for_wayland_socket(&socket, 10_000) {
        tracing::info!(socket = %socket, "wayland socket ready");
    } else {
        tracing::warn!(socket = %socket, "wayland socket not present after 10s");
    }
}

fn activate_wayland_session(timeout_ms: u64) -> String {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let name_file = runtime.join("sola-wayland");
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Ok(s) = std::fs::read_to_string(&name_file) {
            let name = s.trim();
            if !name.is_empty() {
                unsafe { std::env::set_var("WAYLAND_DISPLAY", name) };
                return name.to_string();
            }
        }
        if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
            if !v.is_empty() {
                return v;
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into())
}

fn wait_for_wayland_socket(display: &str, timeout_ms: u64) -> bool {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let path = runtime.join(display);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if path.exists() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct Host {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    conn: Connection,
    window: Window,
    present: Option<Present>,
    app: App,
    width: u32,
    height: u32,
    scale: i32,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor: (f32, f32),
    exit: bool,
    dirty: bool,
    /// First `xdg_surface` configure has been acked and flushed.
    /// wgpu must not attach a buffer before this.
    configured: bool,
}

impl Host {
    fn physical(&self) -> (u32, u32) {
        let s = self.scale.max(1) as u32;
        (self.width.max(1) * s, self.height.max(1) * s)
    }

    fn ensure_present(&mut self) {
        if self.present.is_some() {
            return;
        }
        let display = self.conn.backend().display_ptr() as *mut _;
        let surface = self.window.wl_surface().id().as_ptr() as *mut _;
        if NonNull::new(display).is_none() || NonNull::new(surface).is_none() {
            tracing::error!("missing wayland display/surface pointers");
            return;
        }
        let (w, h) = self.physical();
        match Present::new(display, surface, w, h) {
            Some(p) => self.present = Some(p),
            None => tracing::error!("wgpu present failed"),
        }
    }

    fn draw(&mut self) {
        if !self.configured {
            return;
        }
        // sctk acks configure in-handler; wgpu's Vulkan WSI flushes the
        // same wl_display when attaching. Send the ack before that attach
        // or River raises xdg_surface error 3 (unconfigured_buffer).
        let _ = self.conn.flush();
        self.ensure_present();
        let Some(present) = self.present.as_mut() else {
            return;
        };
        let scale = self.scale.max(1) as f32;
        self.app.scale = scale;
        self.app.css_w = self.width as f32;
        self.app.css_h = self.height as f32;
        let (quads, glyphs) = self.app.live_layers();
        let (w, h) = self.app.buffer_size();
        present.frame(&quads, &glyphs, w, h, None, self.app.time());
        self.dirty = false;
    }
}

fn resize_edge(x: f32, y: f32, w: f32, h: f32) -> Option<ResizeEdge> {
    let w = w.max(1.0);
    let h = h.max(1.0);
    let left_c = x <= CORNER_GRIP;
    let right_c = x >= w - CORNER_GRIP;
    let top_c = y <= CORNER_GRIP;
    let bottom_c = y >= h - CORNER_GRIP;
    if left_c && top_c {
        return Some(ResizeEdge::TopLeft);
    }
    if right_c && top_c {
        return Some(ResizeEdge::TopRight);
    }
    if left_c && bottom_c {
        return Some(ResizeEdge::BottomLeft);
    }
    if right_c && bottom_c {
        return Some(ResizeEdge::BottomRight);
    }
    if y <= EDGE_GRIP {
        return Some(ResizeEdge::Top);
    }
    if y >= h - EDGE_GRIP {
        return Some(ResizeEdge::Bottom);
    }
    if x <= EDGE_GRIP {
        return Some(ResizeEdge::Left);
    }
    if x >= w - EDGE_GRIP {
        return Some(ResizeEdge::Right);
    }
    None
}

impl CompositorHandler for Host {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if surface != self.window.wl_surface() {
            return;
        }
        self.scale = new_factor.max(1);
        self.window.wl_surface().set_buffer_scale(self.scale);
        self.dirty = true;
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Host {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for Host {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        self.width = configure.new_size.0.map(|v| v.get()).unwrap_or(self.width);
        self.height = configure.new_size.1.map(|v| v.get()).unwrap_or(self.height);
        self.configured = true;
        self.dirty = true;
        // Do not present here. wgpu attaches on this wl_display; the ack
        // is still in wayland-client's write buffer until dispatch ends.
    }
}

impl SeatHandler for Host {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.seat = Some(seat.clone());
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(kbd) => self.keyboard = Some(kbd),
                Err(e) => tracing::warn!(?e, "keyboard"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(ptr) => self.pointer = Some(ptr),
                Err(e) => tracing::warn!(?e, "pointer"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        self.seat = None;
    }
}

impl KeyboardHandler for Host {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape {
            if self.app.has_focus() {
                self.app.blur();
                self.dirty = true;
            } else {
                self.exit = true;
            }
            return;
        }
        if event.keysym == Keysym::BackSpace {
            self.app.backspace();
            self.dirty = true;
            return;
        }
        if let Some(text) = event.utf8.as_deref() {
            if !text.is_empty() && !text.chars().any(|c| c.is_control()) {
                self.app.type_text(text);
                self.dirty = true;
            }
        }
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for Host {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            let x = event.position.0 as f32;
            let y = event.position.1 as f32;
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.cursor = (x, y);
                    if self.app.mouse_move(x, y) {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.app.mouse_move(-1.0, -1.0) {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button, serial, .. } if button == BTN_LEFT => {
                    self.cursor = (x, y);
                    let click = self.app.click(x, y);
                    if click == Click::Close {
                        self.exit = true;
                        continue;
                    }
                    let css_w = self.width as f32;
                    let css_h = self.height as f32;
                    if let (Some(edge), Some(seat)) =
                        (resize_edge(x, y, css_w, css_h), self.seat.as_ref())
                    {
                        self.window.resize(seat, serial, edge);
                        continue;
                    }
                    match click {
                        Click::Drag => {
                            if let Some(seat) = &self.seat {
                                self.window.move_(seat, serial);
                            }
                        }
                        Click::Select => self.dirty = true,
                        Click::Close | Click::None => {}
                    }
                }
                PointerEventKind::Release { .. } => {
                    self.app.mouse_up();
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let _ = vertical;
                }
                _ => {}
            }
        }
    }
}

delegate_compositor!(Host);
delegate_output!(Host);
delegate_seat!(Host);
delegate_keyboard!(Host);
delegate_pointer!(Host);
delegate_xdg_shell!(Host);
delegate_xdg_window!(Host);
delegate_registry!(Host);

impl ProvidesRegistryState for Host {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
