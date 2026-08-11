//! Wayland subsurface presenter for WPE dma-bufs.
//!
//! Shares iced/winit's `wl_display` via `Backend::from_foreign_display` so a
//! `wl_subsurface` can parent under the iced toplevel.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::sys::client::Backend as SysBackend;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_subcompositor,
    wl_subsurface, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1,
};

use crate::engine::Cmd;
use crate::wpe::engine::{ResourceToken, WpeEngine};

/// Commands into the plane thread.
pub enum ContentPlaneCmd {
    AttachParent { display: usize, surface: usize },
    SetRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Present {
        fd: OwnedFd,
        width: u32,
        height: u32,
        format: u32,
        modifier: u64,
        stride: u32,
        offset: u32,
        extra_planes: Vec<(u32, u32)>,
        token: ResourceToken,
        release_tx: Sender<Cmd<WpeEngine>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub enum PlaneError {
    NotWayland,
    Connect(String),
    MissingGlobal(&'static str),
}

impl std::fmt::Display for PlaneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaneError::NotWayland => write!(f, "window is not Wayland"),
            PlaneError::Connect(s) => write!(f, "connect: {s}"),
            PlaneError::MissingGlobal(g) => write!(f, "missing global {g}"),
        }
    }
}

/// Process-wide plane command sender (set when plane mode boots).
static PLANE_TX: OnceLock<Mutex<Option<Sender<ContentPlaneCmd>>>> = OnceLock::new();

pub fn global_sender() -> Option<Sender<ContentPlaneCmd>> {
    PLANE_TX
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
}

fn install_global(tx: Sender<ContentPlaneCmd>) {
    let slot = PLANE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
}

/// Handle to the content-plane worker.
pub struct ContentPlane {
    tx: Sender<ContentPlaneCmd>,
    _join: JoinHandle<()>,
}

impl ContentPlane {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        install_global(tx.clone());
        let join = std::thread::Builder::new()
            .name("browser-content-plane".into())
            .spawn(move || plane_thread(rx))
            .expect("spawn content plane thread");
        tracing::info!("content plane worker started");
        Self { tx, _join: join }
    }

    pub fn sender(&self) -> Sender<ContentPlaneCmd> {
        self.tx.clone()
    }
}

/// Display + surface pointers from an iced [`Window`] handle.
pub fn parent_ptrs(window: &dyn iced::window::Window) -> Result<(usize, usize), PlaneError> {
    let wh = window
        .window_handle()
        .map_err(|_| PlaneError::NotWayland)?;
    let dh = window
        .display_handle()
        .map_err(|_| PlaneError::NotWayland)?;
    let RawWindowHandle::Wayland(w) = wh.as_raw() else {
        return Err(PlaneError::NotWayland);
    };
    let RawDisplayHandle::Wayland(d) = dh.as_raw() else {
        return Err(PlaneError::NotWayland);
    };
    Ok((d.display.as_ptr() as usize, w.surface.as_ptr() as usize))
}

// --- worker ----------------------------------------------------------------

struct PendingRelease {
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
}

struct PlaneState {
    compositor: Option<wl_compositor::WlCompositor>,
    subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    shm: Option<wl_shm::WlShm>,
    surface: Option<wl_surface::WlSurface>,
    subsurface: Option<wl_subsurface::WlSubsurface>,
    /// Parent surface (for place_above after create).
    parent: Option<wl_surface::WlSurface>,
    live_buffer: Option<wl_buffer::WlBuffer>,
    live_release: Option<PendingRelease>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    formats: HashMap<u32, Vec<u64>>,
    present_ok: u64,
    present_err: u64,
}

fn plane_thread(rx: Receiver<ContentPlaneCmd>) {
    let mut conn: Option<Connection> = None;
    let mut event_queue: Option<EventQueue<PlaneState>> = None;
    let mut state = PlaneState {
        compositor: None,
        subcompositor: None,
        dmabuf: None,
        shm: None,
        surface: None,
        subsurface: None,
        parent: None,
        live_buffer: None,
        live_release: None,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        formats: HashMap::new(),
        present_ok: 0,
        present_err: 0,
    };

    loop {
        let mut got = false;
        loop {
            match rx.try_recv() {
                Ok(ContentPlaneCmd::Shutdown) => {
                    finish_live(&mut state);
                    return;
                }
                Ok(ContentPlaneCmd::AttachParent { display, surface }) => {
                    got = true;
                    match init_from_parent(display, surface, &mut state) {
                        Ok((c, eq)) => {
                            conn = Some(c);
                            event_queue = Some(eq);
                            tracing::info!(
                                "content plane: G1/G2 parent + subsurface ready"
                            );
                        }
                        Err(e) => tracing::error!("content plane attach: {e}"),
                    }
                }
                Ok(ContentPlaneCmd::SetRect {
                    x,
                    y,
                    width,
                    height,
                }) => {
                    got = true;
                    state.x = x;
                    state.y = y;
                    state.width = width.max(1);
                    state.height = height.max(1);
                    if let Some(sub) = &state.subsurface {
                        sub.set_position(x, y);
                    }
                    if let Some(c) = conn.as_ref() {
                        let _ = c.flush();
                    }
                }
                Ok(ContentPlaneCmd::Present {
                    fd,
                    width,
                    height,
                    format,
                    modifier,
                    stride,
                    offset,
                    extra_planes,
                    token,
                    release_tx,
                }) => {
                    got = true;
                    if let Err(e) = present_frame(
                        &mut state,
                        conn.as_ref(),
                        event_queue.as_mut(),
                        fd,
                        width,
                        height,
                        format,
                        modifier,
                        stride,
                        offset,
                        extra_planes,
                        token,
                        release_tx,
                    ) {
                        tracing::warn!("content plane present: {e}");
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finish_live(&mut state);
                    return;
                }
            }
        }

        if let (Some(c), Some(eq)) = (conn.as_ref(), event_queue.as_mut()) {
            let _ = eq.dispatch_pending(&mut state);
            let _ = c.flush();
            if let Some(guard) = eq.prepare_read() {
                let mut pfd = libc::pollfd {
                    fd: c.as_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let pr = unsafe { libc::poll(&mut pfd, 1, 0) };
                if pr > 0 {
                    let _ = guard.read();
                    let _ = eq.dispatch_pending(&mut state);
                } else {
                    drop(guard);
                }
            }
        }

        if !got {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn finish_live(state: &mut PlaneState) {
    if let Some(buf) = state.live_buffer.take() {
        buf.destroy();
    }
    if let Some(pr) = state.live_release.take() {
        let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
    }
}

fn init_from_parent(
    display_ptr: usize,
    parent_surface_ptr: usize,
    state: &mut PlaneState,
) -> Result<(Connection, EventQueue<PlaneState>), PlaneError> {
    let display = display_ptr as *mut wayland_sys::client::wl_display;
    if display.is_null() || parent_surface_ptr == 0 {
        return Err(PlaneError::Connect("null display/surface".into()));
    }

    let backend = unsafe { SysBackend::from_foreign_display(display) };
    let conn = Connection::from_backend(backend);

    let (globals, mut event_queue) = registry_queue_init::<PlaneState>(&conn)
        .map_err(|e| PlaneError::Connect(format!("registry: {e}")))?;
    let qh = event_queue.handle();

    state.compositor = globals
        .bind(&qh, 4..=6, ())
        .map_err(|_| PlaneError::MissingGlobal("wl_compositor"))
        .ok();
    state.subcompositor = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| PlaneError::MissingGlobal("wl_subcompositor"))
        .ok();
    state.dmabuf = globals
        .bind(&qh, 3..=5, ())
        .map_err(|_| PlaneError::MissingGlobal("zwp_linux_dmabuf_v1"))
        .ok();
    state.shm = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| PlaneError::MissingGlobal("wl_shm"))
        .ok();

    let compositor = state
        .compositor
        .clone()
        .ok_or(PlaneError::MissingGlobal("wl_compositor"))?;
    let subcompositor = state
        .subcompositor
        .clone()
        .ok_or(PlaneError::MissingGlobal("wl_subcompositor"))?;
    if state.dmabuf.is_none() {
        return Err(PlaneError::MissingGlobal("zwp_linux_dmabuf_v1"));
    }

    let parent_proxy = parent_surface_ptr as *mut wayland_sys::client::wl_proxy;
    let parent_id = unsafe {
        wayland_backend::sys::client::ObjectId::from_ptr(
            wl_surface::WlSurface::interface(),
            parent_proxy,
        )
        .map_err(|e| PlaneError::Connect(format!("parent id: {e}")))?
    };
    let parent: wl_surface::WlSurface = Proxy::from_id(&conn, parent_id)
        .map_err(|e| PlaneError::Connect(format!("parent proxy: {e}")))?;

    let child = compositor.create_surface(&qh, ());
    let sub = subcompositor.get_subsurface(&child, &parent, &qh, ());
    // Above parent so content is not covered by iced's opaque clear.
    sub.place_above(&parent);
    sub.set_desync();
    sub.set_position(state.x, state.y);

    // G3 probe: solid magenta SHM buffer so we can see the plane without WPE.
    if let Some(shm) = state.shm.clone() {
        if let Err(e) = attach_probe_shm(&child, &shm, &qh, 64, 64) {
            tracing::warn!("content plane SHM probe failed: {e}");
        } else {
            tracing::info!("content plane: G3 SHM magenta probe attached 64x64");
        }
    }

    child.commit();
    let _ = event_queue.flush();
    let _ = conn.flush();

    state.surface = Some(child);
    state.subsurface = Some(sub);
    state.parent = Some(parent);

    let _ = event_queue.roundtrip(state);

    Ok((conn, event_queue))
}

/// Solid-color SHM buffer for visibility probe (G3).
fn attach_probe_shm(
    surface: &wl_surface::WlSurface,
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<PlaneState>,
    w: u32,
    h: u32,
) -> Result<(), String> {
    let stride = w * 4;
    let size = (stride * h) as usize;
    let fd = rustix_memfd(size)?;
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        )
    };
    if map == libc::MAP_FAILED {
        return Err("mmap failed".into());
    }
    // Magenta BGRA
    let pixels = unsafe { std::slice::from_raw_parts_mut(map as *mut u8, size) };
    for px in pixels.chunks_exact_mut(4) {
        px[0] = 0xff; // B
        px[1] = 0x00; // G
        px[2] = 0xff; // R
        px[3] = 0xff; // A
    }
    let pool = shm.create_pool(fd.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        w as i32,
        h as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w as i32, h as i32);
    // Keep buffer alive via state? probe only — leak buffer intentionally for probe.
    std::mem::forget(buffer);
    std::mem::forget(pool);
    unsafe {
        libc::munmap(map, size);
    }
    Ok(())
}

fn rustix_memfd(size: usize) -> Result<OwnedFd, String> {
    // memfd_create + ftruncate
    let name = c"sola-plane-probe";
    let fd = unsafe { libc::syscall(libc::SYS_memfd_create, name.as_ptr(), 0) };
    if fd < 0 {
        return Err(format!(
            "memfd_create: {}",
            std::io::Error::last_os_error()
        ));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    if unsafe { libc::ftruncate(fd.as_raw_fd(), size as i64) } != 0 {
        return Err(format!("ftruncate: {}", std::io::Error::last_os_error()));
    }
    Ok(fd)
}

fn present_frame(
    state: &mut PlaneState,
    conn: Option<&Connection>,
    event_queue: Option<&mut EventQueue<PlaneState>>,
    fd: OwnedFd,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    stride: u32,
    offset: u32,
    extra_planes: Vec<(u32, u32)>,
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
) -> Result<(), String> {
    let conn = conn.ok_or_else(|| "no conn".to_string())?;
    let eq = event_queue.ok_or_else(|| "no queue".to_string())?;
    let dmabuf = state
        .dmabuf
        .as_ref()
        .ok_or_else(|| "no dmabuf".to_string())?;
    let surface = state
        .surface
        .as_ref()
        .ok_or_else(|| "no surface".to_string())?;
    let qh = eq.handle();

    if width == 0 || height == 0 {
        let _ = release_tx.send(Cmd::Release { token });
        return Ok(());
    }

    let params = dmabuf.create_params(&qh, ());
    let raw = fd.into_raw_fd();
    let borrowed = unsafe { BorrowedFd::borrow_raw(raw) };
    params.add(
        borrowed,
        0,
        offset,
        stride,
        (modifier >> 32) as u32,
        (modifier & 0xffff_ffff) as u32,
    );
    for (i, (s, o)) in extra_planes.iter().enumerate() {
        let borrowed = unsafe { BorrowedFd::borrow_raw(raw) };
        params.add(
            borrowed,
            (i + 1) as u32,
            *o,
            *s,
            (modifier >> 32) as u32,
            (modifier & 0xffff_ffff) as u32,
        );
    }

    let buffer = params.create_immed(
        width as i32,
        height as i32,
        format,
        zwp_linux_buffer_params_v1::Flags::empty(),
        &qh,
        (),
    );
    // Protocol took ownership of the fd.
    std::mem::forget(unsafe { OwnedFd::from_raw_fd(raw) });

    let prev_buf = state.live_buffer.take();
    let prev_rel = state.live_release.take();

    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, width as i32, height as i32);
    let _ = surface.frame(&qh, ());
    surface.commit();
    let _ = eq.flush();
    let _ = conn.flush();

    state.live_buffer = Some(buffer);
    state.live_release = Some(PendingRelease { token, release_tx });
    state.width = width;
    state.height = height;
    state.present_ok += 1;
    if state.present_ok == 1 || state.present_ok % 120 == 0 {
        tracing::info!(
            n = state.present_ok,
            width,
            height,
            format = format!("{:#x}", format),
            modifier,
            "content plane present ok"
        );
    }

    if let Some(b) = prev_buf {
        b.destroy();
    }
    if let Some(pr) = prev_rel {
        let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
    }

    Ok(())
}

// --- Dispatch --------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for PlaneState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for PlaneState {
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

impl Dispatch<wl_subcompositor::WlSubcompositor, ()> for PlaneState {
    fn event(
        _: &mut Self,
        _: &wl_subcompositor::WlSubcompositor,
        _: wl_subcompositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for PlaneState {
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

impl Dispatch<wl_subsurface::WlSubsurface, ()> for PlaneState {
    fn event(
        _: &mut Self,
        _: &wl_subsurface::WlSubsurface,
        _: wl_subsurface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for PlaneState {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for PlaneState {
    fn event(
        _: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for PlaneState {
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

impl Dispatch<wl_shm_pool::WlShmPool, ()> for PlaneState {
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

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for PlaneState {
    fn event(
        state: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        event: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_dmabuf_v1::Event::Format { format } => {
                state.formats.entry(format).or_default();
            }
            zwp_linux_dmabuf_v1::Event::Modifier {
                format,
                modifier_hi,
                modifier_lo,
            } => {
                let m = ((modifier_hi as u64) << 32) | modifier_lo as u64;
                state.formats.entry(format).or_default().push(m);
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for PlaneState {
    fn event(
        _: &mut Self,
        _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, zwp_linux_buffer_params_v1::Event::Failed) {
            tracing::error!("linux_dmabuf buffer create failed");
        }
    }
}
