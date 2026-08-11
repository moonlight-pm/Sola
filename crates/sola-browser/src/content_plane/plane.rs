//! Wayland subsurface presenter for WPE dma-bufs.
//!
//! **Main-thread only** Wayland I/O: winit/iced already reads the same
//! `wl_display`. A second thread calling `prepare_read` races the connection
//! and silently drops subsurface commits (G2/G3 invisible).
//!
//! Shares iced's display via `Backend::from_foreign_display`.

use std::collections::VecDeque;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};

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

/// Commands from worker / chrome into the plane (queued; applied on main).
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
    /// Solid color SHM frame (debug / G3).
    ProbeColor { r: u8, g: u8, b: u8 },
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

/// Main-thread content plane (no secondary display reader).
pub struct ContentPlane {
    rx: Receiver<ContentPlaneCmd>,
    tx: Sender<ContentPlaneCmd>,
    inner: Option<PlaneInner>,
    pending: VecDeque<ContentPlaneCmd>,
}

struct PendingRelease {
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
}

struct PlaneInner {
    conn: Connection,
    event_queue: EventQueue<PlaneState>,
    state: PlaneState,
}

struct PlaneState {
    compositor: Option<wl_compositor::WlCompositor>,
    subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    shm: Option<wl_shm::WlShm>,
    surface: Option<wl_surface::WlSurface>,
    subsurface: Option<wl_subsurface::WlSubsurface>,
    parent: Option<wl_surface::WlSurface>,
    live_buffer: Option<wl_buffer::WlBuffer>,
    /// SHM pool backing live_buffer when probe/shm path (keep mapped fd alive).
    _shm_keep: Option<OwnedFd>,
    live_release: Option<PendingRelease>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    present_ok: u64,
    ready: bool,
}

impl ContentPlane {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        install_global(tx.clone());
        tracing::info!("content plane: main-thread mode (no dual display read)");
        Self {
            rx,
            tx,
            inner: None,
            pending: VecDeque::new(),
        }
    }

    pub fn sender(&self) -> Sender<ContentPlaneCmd> {
        self.tx.clone()
    }

    /// Drain commands + dispatch. Call from iced `update` on a timer / each frame.
    pub fn poll_main(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(cmd) => self.pending.push_back(cmd),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        while let Some(cmd) = self.pending.pop_front() {
            self.apply(cmd);
        }
        if let Some(inner) = self.inner.as_mut() {
            let _ = inner
                .event_queue
                .dispatch_pending(&mut inner.state);
            // Do NOT prepare_read / read — iced owns the display read.
            let _ = inner.conn.flush();
        }
    }

    fn apply(&mut self, cmd: ContentPlaneCmd) {
        match cmd {
            ContentPlaneCmd::Shutdown => {
                if let Some(mut inner) = self.inner.take() {
                    finish_live(&mut inner.state);
                }
            }
            ContentPlaneCmd::AttachParent { display, surface } => {
                match init_from_parent(display, surface) {
                    Ok(inner) => {
                        tracing::info!("content plane: G1/G2 subsurface ready (main thread)");
                        self.inner = Some(inner);
                        // Large magenta probe at current rect (or default).
                        self.apply(ContentPlaneCmd::ProbeColor {
                            r: 255,
                            g: 0,
                            b: 255,
                        });
                    }
                    Err(e) => tracing::error!("content plane attach: {e}"),
                }
            }
            ContentPlaneCmd::SetRect {
                x,
                y,
                width,
                height,
            } => {
                if let Some(inner) = self.inner.as_mut() {
                    inner.state.x = x;
                    inner.state.y = y;
                    inner.state.width = width.max(1);
                    inner.state.height = height.max(1);
                    if let Some(sub) = &inner.state.subsurface {
                        sub.set_position(x, y);
                    }
                    // Parent commit needed for position in sync mode; desync
                    // applies immediately but place still benefits from parent
                    // damage on next iced frame.
                    let _ = inner.conn.flush();
                }
            }
            ContentPlaneCmd::ProbeColor { r, g, b } => {
                if let Some(inner) = self.inner.as_mut() {
                    if let Err(e) = present_shm_color(inner, r, g, b) {
                        tracing::warn!("content plane SHM color: {e}");
                    } else {
                        tracing::info!(
                            r,
                            g,
                            b,
                            w = inner.state.width,
                            h = inner.state.height,
                            x = inner.state.x,
                            y = inner.state.y,
                            "content plane: SHM color presented (G3 probe)"
                        );
                    }
                }
            }
            ContentPlaneCmd::Present {
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
            } => {
                if let Some(inner) = self.inner.as_mut() {
                    if let Err(e) = present_dmabuf(
                        inner,
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
                } else {
                    let _ = release_tx.send(Cmd::Release { token });
                }
            }
        }
    }
}

/// Display + surface pointers from an iced window handle.
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

fn finish_live(state: &mut PlaneState) {
    if let Some(buf) = state.live_buffer.take() {
        buf.destroy();
    }
    state._shm_keep = None;
    if let Some(pr) = state.live_release.take() {
        let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
    }
}

fn init_from_parent(
    display_ptr: usize,
    parent_surface_ptr: usize,
) -> Result<PlaneInner, PlaneError> {
    let display = display_ptr as *mut wayland_sys::client::wl_display;
    if display.is_null() || parent_surface_ptr == 0 {
        return Err(PlaneError::Connect("null display/surface".into()));
    }

    let backend = unsafe { SysBackend::from_foreign_display(display) };
    let conn = Connection::from_backend(backend);

    let (globals, mut event_queue) = registry_queue_init::<PlaneState>(&conn)
        .map_err(|e| PlaneError::Connect(format!("registry: {e}")))?;
    let qh = event_queue.handle();

    let mut state = PlaneState {
        compositor: None,
        subcompositor: None,
        dmabuf: None,
        shm: None,
        surface: None,
        subsurface: None,
        parent: None,
        live_buffer: None,
        _shm_keep: None,
        live_release: None,
        x: 200,
        y: 120,
        width: 400,
        height: 400,
        present_ok: 0,
        ready: false,
    };

    state.compositor = globals.bind(&qh, 4..=6, ()).ok();
    state.subcompositor = globals.bind(&qh, 1..=1, ()).ok();
    state.dmabuf = globals.bind(&qh, 3..=5, ()).ok();
    state.shm = globals.bind(&qh, 1..=1, ()).ok();

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
    if state.shm.is_none() {
        return Err(PlaneError::MissingGlobal("wl_shm"));
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
    sub.place_above(&parent);
    // Sync: apply with parent commits from iced (more reliable under winit).
    sub.set_sync();
    sub.set_position(state.x, state.y);

    child.commit();
    // Nudge parent so sync subsurface maps (safe request on same object).
    parent.commit();

    let _ = event_queue.flush();
    let _ = conn.flush();
    // One dispatch of *our* queue only (no display read).
    let _ = event_queue.dispatch_pending(&mut state);

    state.surface = Some(child);
    state.subsurface = Some(sub);
    state.parent = Some(parent);
    state.ready = true;

    Ok(PlaneInner {
        conn,
        event_queue,
        state,
    })
}

fn present_shm_color(inner: &mut PlaneInner, r: u8, g: u8, b: u8) -> Result<(), String> {
    let shm = inner
        .state
        .shm
        .as_ref()
        .ok_or_else(|| "no shm".to_string())?;
    let surface = inner
        .state
        .surface
        .as_ref()
        .ok_or_else(|| "no surface".to_string())?;
    let qh = inner.event_queue.handle();
    let w = inner.state.width.max(1);
    let h = inner.state.height.max(1);
    let stride = w * 4;
    let size = (stride * h) as usize;

    let fd = memfd(size)?;
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
    let pixels = unsafe { std::slice::from_raw_parts_mut(map as *mut u8, size) };
    for px in pixels.chunks_exact_mut(4) {
        // wl_shm Argb8888 is little-endian: B,G,R,A in memory on LE.
        px[0] = b;
        px[1] = g;
        px[2] = r;
        px[3] = 0xff;
    }
    let pool = shm.create_pool(fd.as_fd(), size as i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        w as i32,
        h as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        &qh,
        (),
    );
    unsafe {
        libc::munmap(map, size);
    }

    let prev_buf = inner.state.live_buffer.take();
    let prev_rel = inner.state.live_release.take();
    let prev_shm = inner.state._shm_keep.take();

    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w as i32, h as i32);
    surface.commit();
    if let Some(parent) = &inner.state.parent {
        parent.commit();
    }
    let _ = inner.event_queue.flush();
    let _ = inner.conn.flush();

    inner.state.live_buffer = Some(buffer);
    inner.state._shm_keep = Some(fd);
    std::mem::forget(pool); // buffer holds pool ref via protocol

    if let Some(b) = prev_buf {
        b.destroy();
    }
    drop(prev_shm);
    if let Some(pr) = prev_rel {
        let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
    }
    Ok(())
}

fn present_dmabuf(
    inner: &mut PlaneInner,
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
    let dmabuf = inner
        .state
        .dmabuf
        .as_ref()
        .ok_or_else(|| "no dmabuf".to_string())?;
    let surface = inner
        .state
        .surface
        .as_ref()
        .ok_or_else(|| "no surface".to_string())?;
    let qh = inner.event_queue.handle();

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
    std::mem::forget(unsafe { OwnedFd::from_raw_fd(raw) });

    let prev_buf = inner.state.live_buffer.take();
    let prev_rel = inner.state.live_release.take();
    let prev_shm = inner.state._shm_keep.take();

    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, width as i32, height as i32);
    let _ = surface.frame(&qh, ());
    surface.commit();
    if let Some(parent) = &inner.state.parent {
        parent.commit();
    }
    let _ = inner.event_queue.flush();
    let _ = inner.conn.flush();

    inner.state.live_buffer = Some(buffer);
    inner.state.live_release = Some(PendingRelease { token, release_tx });
    inner.state.present_ok += 1;
    if inner.state.present_ok == 1 || inner.state.present_ok % 60 == 0 {
        tracing::info!(
            n = inner.state.present_ok,
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
    drop(prev_shm);
    if let Some(pr) = prev_rel {
        let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
    }
    Ok(())
}

fn memfd(size: usize) -> Result<OwnedFd, String> {
    let name = c"sola-plane";
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
        _: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        _: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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
