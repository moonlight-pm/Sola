//! sctk + calloop River client. No winit.
//!
//! Identity is [`crate::APP_ID`] / [`crate::WINDOW_TITLE`]. Client-side
//! decorations only — River must not draw SSD.

use std::ptr::NonNull;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge;
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

use crate::app::Click;
use crate::gpu::{Present, Quad};

/// Per-window UI driven by the sctk host. Storybook and lab twins
/// each implement this; the host is kit-agnostic.
pub trait Surface: Send {
    fn set_view(&mut self, w: f32, h: f32, scale: f32);
    fn tick(&mut self, dt: f32);
    fn time(&self) -> f32;
    fn needs_frame(&self) -> bool;
    fn has_overlay(&self) -> bool;
    fn has_focus(&self) -> bool;
    fn blur(&mut self);
    fn dismiss_overlays(&mut self) -> bool;
    fn type_text(&mut self, s: &str);
    fn backspace(&mut self);
    fn tab(&mut self, back: bool);
    fn arrow(&mut self, up: bool);
    fn arrow_horizontal(&mut self, left: bool) {
        let _ = left;
    }
    fn caret_line(&mut self, end: bool) {
        let _ = end;
    }
    fn select_all(&mut self) {}
    fn delete_forward(&mut self) {}
    fn kill_to_end(&mut self) {}
    fn arrow_word(&mut self, left: bool) {
        let _ = left;
    }
    fn kill_word_back(&mut self) {}
    fn enter(&mut self);
    fn mouse_up(&mut self);
    fn buffer_size(&self) -> (u32, u32);
    fn reload_if_changed(&mut self) -> bool;
    /// Glyph overlay is `Some` when it must be uploaded. `None` keeps the
    /// previous texture (hover-only frames).
    fn live_layers(&mut self) -> (Vec<Quad>, Option<Vec<u32>>);
    fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool;
    fn mouse_move(&mut self, x: f32, y: f32) -> bool;
    fn right_click(&mut self, x: f32, y: f32) -> bool;
    fn click(&mut self, x: f32, y: f32) -> Click;
    fn poll(&mut self) -> bool;
    /// Draw kit CSD (titlebar + edge grips). Iced `wrap_if_floating`:
    /// zoned windows are chrome-less; floats get the titlebar.
    fn floating_chrome(&self) -> bool {
        true
    }
    fn cursor_at(&self, _x: f32, _y: f32) -> CursorKind {
        CursorKind::Default
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Default,
    Text,
    Pointer,
    NsResize,
    EwResize,
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const DEFAULT_W: u32 = 960;
const DEFAULT_H: u32 = 680;
/// Match sola-kit `titlebar::floating_frame` hit bands.
const EDGE_GRIP: f32 = 12.0;
const CORNER_GRIP: f32 = 18.0;

pub fn run() {
    run_with(
        crate::APP_ID,
        crate::WINDOW_TITLE,
        Box::new(crate::app::App::new(
            DEFAULT_W as f32,
            DEFAULT_H as f32,
            1.0,
        )),
    );
}

pub fn run_with(app_id: &'static str, title: &'static str, app: Box<dyn Surface>) {
    boot(app_id);

    let conn = Connection::connect_to_env().expect("wayland connect");
    let (globals, event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<Host> = EventLoop::try_new().expect("calloop");
    let loop_handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .expect("wayland source");

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg_wm_base");
    let cursor_shapes = CursorShapeManager::bind(&globals, &qh).ok();
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::ClientOnly, &qh);
    window.set_app_id(app_id);
    window.set_title(title);
    window.set_min_size(Some((480, 360)));
    window.commit();
    let _ = conn.flush();

    let mut host = Host {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        conn,
        loop_handle,
        window,
        present: None,
        app,
        width: DEFAULT_W,
        height: DEFAULT_H,
        scale: 1,
        seat: None,
        keyboard: None,
        pointer: None,
        cursor_shapes,
        shape_device: None,
        pointer_serial: 0,
        cursor_kind: CursorKind::Default,
        cursor: (0.0, 0.0),
        exit: false,
        dirty: false,
        configured: false,
        shift: false,
        ctrl: false,
        alt: false,
        last_frame: Instant::now(),
    };

    tracing::info!(app_id, title, "sctk window created");
    let mut last = Instant::now();

    loop {
        // Always pump Wayland on a short cadence. A long wait + a heavy
        // draw (live bus inspector) lets the socket fill until River
        // drops the client (Broken pipe).
        match event_loop.dispatch(Duration::from_millis(8), &mut host) {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(%e, "wayland dispatch ended");
                break;
            }
        }
        if host.exit {
            break;
        }
        if !host.configured {
            continue;
        }
        let now = Instant::now();
        let dt = now.saturating_duration_since(last).as_secs_f32();
        last = now;
        host.app.tick(dt);
        if host.app.poll() {
            host.dirty = true;
        }
        if host.app.reload_if_changed() || host.app.needs_frame() {
            host.dirty = true;
        }
        if host.dirty && host.last_frame.elapsed() >= Duration::from_millis(32) {
            host.draw();
            host.last_frame = Instant::now();
        }
    }
}

fn boot(app_id: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!("{}=info,sola_kit_spike=info", app_id.replace('-', "_")).into()
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("{app_id} starting");
    let socket = activate_wayland_session(20_000);
    tracing::info!(socket = %socket, "wayland socket resolved");
    if wait_for_wayland_socket(&socket, 10_000) {
        tracing::info!(socket = %socket, "wayland socket ready");
    } else {
        tracing::warn!(socket = %socket, "wayland socket not present after 10s");
    }
    // Same contract as iced kit `startup`: re-exec when this binary
    // changes on disk. Lab twins are not installed, so this watches
    // `current_exe()` (`target/release/sola-settings-lab` /
    // `sola-monitor-lab` / `sola-mail-lab`, not `/opt/sola/bin`). Skip when the process
    // manager already supervises.
    if std::env::var_os("SOLA_NO_SELF_WATCH").is_none() {
        sola_core::watcher::watch_own_binary();
    } else {
        tracing::debug!("SOLA_NO_SELF_WATCH set, skipping self-watch");
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
    loop_handle: LoopHandle<'static, Host>,
    window: Window,
    present: Option<Present>,
    app: Box<dyn Surface>,
    width: u32,
    height: u32,
    scale: i32,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shapes: Option<CursorShapeManager>,
    shape_device: Option<WpCursorShapeDeviceV1>,
    pointer_serial: u32,
    cursor_kind: CursorKind,
    cursor: (f32, f32),
    exit: bool,
    dirty: bool,
    /// First `xdg_surface` configure has been acked and flushed.
    /// wgpu must not attach a buffer before this.
    configured: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
    last_frame: Instant,
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
        self.app
            .set_view(self.width as f32, self.height as f32, scale);
        let (quads, glyphs) = self.app.live_layers();
        let (w, h) = self.app.buffer_size();
        let radius = if self.app.floating_chrome() {
            14.0 * self.scale.max(1) as f32
        } else {
            0.0
        };
        present.frame(
            &quads,
            glyphs.as_deref(),
            w,
            h,
            None,
            self.app.time(),
            radius,
        );
        self.dirty = false;
    }

    fn apply_cursor(&mut self, x: f32, y: f32) {
        let kind = self.app.cursor_at(x, y);
        if kind == self.cursor_kind {
            return;
        }
        self.cursor_kind = kind;
        let Some(dev) = self.shape_device.as_ref() else {
            return;
        };
        if self.pointer_serial == 0 {
            return;
        }
        let shape = match kind {
            CursorKind::Text => Shape::Text,
            CursorKind::Pointer => Shape::Pointer,
            CursorKind::Default => Shape::Default,
            CursorKind::NsResize => Shape::NsResize,
            CursorKind::EwResize => Shape::EwResize,
        };
        dev.set_shape(self.pointer_serial, shape);
    }

    fn handle_key(&mut self, event: KeyEvent) {
        if event.keysym == Keysym::Escape {
            if self.app.dismiss_overlays() {
                self.dirty = true;
            } else if self.app.has_focus() {
                self.app.blur();
                self.dirty = true;
            } else {
                self.exit = true;
            }
            return;
        }
        if event.keysym == Keysym::Tab {
            self.app.tab(self.shift);
            self.dirty = true;
            return;
        }
        if self.ctrl {
            match event.keysym {
                Keysym::a | Keysym::A => self.app.caret_line(false),
                Keysym::e | Keysym::E => self.app.caret_line(true),
                Keysym::f | Keysym::F => self.app.arrow_horizontal(false),
                Keysym::b | Keysym::B => self.app.arrow_horizontal(true),
                Keysym::d | Keysym::D => self.app.delete_forward(),
                Keysym::h | Keysym::H => self.app.backspace(),
                Keysym::k | Keysym::K => self.app.kill_to_end(),
                Keysym::w | Keysym::W => self.app.kill_word_back(),
                Keysym::Left => self.app.arrow_word(true),
                Keysym::Right => self.app.arrow_word(false),
                _ => {}
            }
            self.dirty = true;
            return;
        }
        if self.alt {
            match event.keysym {
                Keysym::f | Keysym::F | Keysym::Right => self.app.arrow_word(false),
                Keysym::b | Keysym::B | Keysym::Left => self.app.arrow_word(true),
                Keysym::BackSpace => self.app.kill_word_back(),
                _ => {}
            }
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Up {
            self.app.arrow(true);
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Down {
            self.app.arrow(false);
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Left {
            self.app.arrow_horizontal(true);
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Right {
            self.app.arrow_horizontal(false);
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Home {
            self.app.caret_line(false);
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::End {
            self.app.caret_line(true);
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Return || event.keysym == Keysym::KP_Enter {
            self.app.enter();
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::BackSpace {
            self.app.backspace();
            self.dirty = true;
            return;
        }
        if event.keysym == Keysym::Delete {
            self.app.delete_forward();
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
            match self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|state, _, event| {
                    state.handle_key(event);
                }),
            ) {
                Ok(kbd) => self.keyboard = Some(kbd),
                Err(e) => tracing::warn!(?e, "keyboard"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(ptr) => {
                    if let Some(mgr) = &self.cursor_shapes {
                        self.shape_device = Some(mgr.get_shape_device(&ptr, qh));
                    }
                    self.pointer = Some(ptr);
                }
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
            self.shape_device = None;
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
        self.handle_key(event);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.handle_key(event);
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
        modifiers: Modifiers,
        _raw: RawModifiers,
        _layout: u32,
    ) {
        self.shift = modifiers.shift;
        self.ctrl = modifiers.ctrl;
        self.alt = modifiers.alt;
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
                PointerEventKind::Enter { serial } => {
                    self.pointer_serial = serial;
                    self.cursor = (x, y);
                    self.cursor_kind = CursorKind::Text;
                    self.apply_cursor(x, y);
                    if self.app.mouse_move(x, y) {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.cursor = (x, y);
                    self.apply_cursor(x, y);
                    if self.app.mouse_move(x, y) {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.cursor_kind = CursorKind::Text;
                    self.apply_cursor(-1.0, -1.0);
                    if self.app.mouse_move(-1.0, -1.0) {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button, serial, .. } if button == BTN_LEFT => {
                    self.pointer_serial = serial;
                    self.cursor = (x, y);
                    let click = self.app.click(x, y);
                    if click == Click::Close {
                        self.exit = true;
                        continue;
                    }
                    let css_w = self.width as f32;
                    let css_h = self.height as f32;
                    if click == Click::None && self.app.floating_chrome() {
                        if let (Some(edge), Some(seat)) =
                            (resize_edge(x, y, css_w, css_h), self.seat.as_ref())
                        {
                            self.window.resize(seat, serial, edge);
                            continue;
                        }
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
                PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                    self.cursor = (x, y);
                    if self.app.right_click(x, y) {
                        self.dirty = true;
                    }
                }
                PointerEventKind::Release { .. } => {
                    self.app.mouse_up();
                }
                PointerEventKind::Axis { vertical, .. } => {
                    if vertical.is_none() {
                        continue;
                    }
                    // wl_pointer v8+ sends value120 (120 = one detent) and
                    // dropped axis_discrete. sctk 0.19 discarded that event.
                    let dy = if vertical.value120 != 0 {
                        vertical.value120 as f32 / 120.0 * 28.0 * 5.0
                    } else if vertical.discrete != 0 {
                        vertical.discrete as f32 * 28.0 * 5.0
                    } else {
                        vertical.absolute as f32 * 3.0
                    };
                    if dy.abs() < 0.01 {
                        continue;
                    }
                    let (x, y) = self.cursor;
                    if self.app.wheel(x, y, dy) {
                        self.dirty = true;
                    }
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
