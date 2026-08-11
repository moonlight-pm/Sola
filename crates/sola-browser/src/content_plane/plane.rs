//! Wayland subsurface presenter for WPE dma-bufs.
//!
//! **Main-thread only** Wayland I/O: winit/iced already reads the same
//! `wl_display`. A second thread calling `prepare_read` races the connection
//! and silently drops subsurface commits (G2/G3 invisible).
//!
//! Shares iced's display via `Backend::from_foreign_display`.

use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::sys::client::Backend as SysBackend;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool,
    wl_subcompositor, wl_subsurface, wl_surface,
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
        /// WebKit device scale → `wl_surface.set_buffer_scale`.
        buffer_scale: i32,
    },
    Present {
        /// WPEBuffer pointer — cache key (stock WPEViewWayland reuses wl_buffer).
        buffer_key: usize,
        width: u32,
        height: u32,
        format: u32,
        modifier: u64,
        /// All planes: (fd, stride, offset). FDs are dups owned by this cmd.
        planes: Vec<(OwnedFd, u32, u32)>,
        /// WebKit render fence; wait before attach when present.
        render_fence: Option<OwnedFd>,
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

/// Per-`wl_buffer` data: release WPE only after compositor `Release`.
///
/// Matches stock WPEViewWayland: the protocol object lives across many
/// present cycles; we only return the WPE loan on Release, we do **not**
/// destroy the wl_buffer.
struct BufferData {
    release: Mutex<Option<PendingRelease>>,
    /// Client-side plane FDs (must stay open for wl_buffer lifetime).
    keep_fds: Mutex<Vec<OwnedFd>>,
    buffer_key: usize,
}

/// User data for `wl_surface.frame` — FrameDone after compositor accepts frame.
struct FrameCbData {
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
}

/// Cached Wayland buffer for one WPE pool slot (create once, attach many).
struct CachedWl {
    buffer: wl_buffer::WlBuffer,
    data: Arc<BufferData>,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
}

/// A dma-buf frame waiting for the previous commit's `wl_surface.frame`.
struct QueuedPresent {
    buffer_key: usize,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    planes: Vec<(OwnedFd, u32, u32)>,
    render_fence: Option<OwnedFd>,
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
}

impl QueuedPresent {
    fn release(self) {
        let _ = self.release_tx.send(Cmd::Release { token: self.token });
    }
}

/// Soft cap: if compositor never sends `Release`, stop attaching new buffers
/// (return loan to WPE) rather than destroying buffers still on screen.
const MAX_INFLIGHT: usize = 6;

/// If `wl_callback.Done` is late/missing, unlock attach so scroll does not
/// freeze on the first committed frame (foreign-display dispatch races).
const FRAME_CB_TIMEOUT: Duration = Duration::from_millis(32);

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
    /// Buffers awaiting compositor `Release` this cycle (WPE loan not returned yet).
    inflight_keys: Vec<usize>,
    /// Stock WPEViewWayland: one wl_buffer per WPEBuffer pool slot, reused.
    buffer_cache: HashMap<usize, CachedWl>,
    /// After attach+commit until `wl_callback.Done` — paces to display rate.
    awaiting_frame: bool,
    /// When `awaiting_frame` was set (timeout unlock if callback is late).
    awaiting_since: Option<Instant>,
    /// Token for the commit waiting on frame cb (FrameDone on Done or timeout).
    awaiting_framedone: Option<(ResourceToken, Sender<Cmd<WpeEngine>>)>,
    /// Latest unattached present (latest-wins while awaiting frame).
    queued: Option<QueuedPresent>,
    /// wl_surface buffer_scale (matches WebKit device scale when >1).
    buffer_scale: i32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    present_ok: u64,
    frame_done: u64,
    drop_queued: u64,
    drop_inflight_cap: u64,
    cache_hit: u64,
    cache_create: u64,
    fence_wait: u64,
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
        // Coalesce Present: only latest is shown; drop intermediates to WPE.
        {
            let mut out: VecDeque<ContentPlaneCmd> = VecDeque::new();
            let mut last_present: Option<ContentPlaneCmd> = None;
            while let Some(cmd) = self.pending.pop_front() {
                if matches!(cmd, ContentPlaneCmd::Present { .. }) {
                    if let Some(old) = last_present.replace(cmd) {
                        if let ContentPlaneCmd::Present {
                            token, release_tx, ..
                        } = old
                        {
                            let _ = release_tx.send(Cmd::Release { token });
                        }
                    }
                } else {
                    out.push_back(cmd);
                }
            }
            if let Some(p) = last_present {
                out.push_back(p);
            }
            self.pending = out;
        }

        // Dispatch compositor events first so frame-done / Release clear
        // `awaiting_frame` before we try to attach the next buffer.
        if let Some(inner) = self.inner.as_mut() {
            let _ = inner.event_queue.dispatch_pending(&mut inner.state);
            unlock_stale_frame_gate(&mut inner.state);
            flush_queued_after_frame(inner);
        }

        while let Some(cmd) = self.pending.pop_front() {
            self.apply(cmd);
        }
        if let Some(inner) = self.inner.as_mut() {
            // Second pass: frame-done may have arrived mid-apply; present queue.
            let _ = inner.event_queue.dispatch_pending(&mut inner.state);
            unlock_stale_frame_gate(&mut inner.state);
            flush_queued_after_frame(inner);
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
                        tracing::info!(
                            "content plane: G1/G2 subsurface ready (main thread, empty input region)"
                        );
                        self.inner = Some(inner);
                        // Magenta SHM probe only when debugging visibility.
                        if std::env::var_os("SOLA_BROWSER_PLANE_PROBE").is_some() {
                            self.apply(ContentPlaneCmd::ProbeColor {
                                r: 255,
                                g: 0,
                                b: 255,
                            });
                        }
                    }
                    Err(e) => tracing::error!("content plane attach: {e}"),
                }
            }
            ContentPlaneCmd::SetRect {
                x,
                y,
                width,
                height,
                buffer_scale,
            } => {
                if let Some(inner) = self.inner.as_mut() {
                    inner.state.x = x;
                    inner.state.y = y;
                    inner.state.width = width.max(1);
                    inner.state.height = height.max(1);
                    let scale = buffer_scale.clamp(1, 2);
                    if scale != inner.state.buffer_scale {
                        inner.state.buffer_scale = scale;
                        if let Some(surf) = &inner.state.surface {
                            surf.set_buffer_scale(scale);
                        }
                    }
                    if let Some(sub) = &inner.state.subsurface {
                        sub.set_position(x, y);
                    }
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
                buffer_key,
                width,
                height,
                format,
                modifier,
                planes,
                render_fence,
                token,
                release_tx,
            } => {
                if let Some(inner) = self.inner.as_mut() {
                    if let Err(e) = present_dmabuf(
                        inner,
                        buffer_key,
                        width,
                        height,
                        format,
                        modifier,
                        planes,
                        render_fence,
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
    if let Some(q) = state.queued.take() {
        q.release();
    }
    if let Some((token, tx)) = state.awaiting_framedone.take() {
        let _ = tx.send(Cmd::FrameDone { token });
    }
    state.awaiting_frame = false;
    state.awaiting_since = None;
    state.inflight_keys.clear();
    // Shutdown: return WPE loans and destroy cached protocol objects.
    for (_, cached) in state.buffer_cache.drain() {
        if let Some(pr) = cached.data.release.lock().unwrap().take() {
            let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
        }
        *cached.data.keep_fds.lock().unwrap() = Vec::new();
        cached.buffer.destroy();
    }
}

fn unlock_stale_frame_gate(state: &mut PlaneState) {
    if !state.awaiting_frame {
        return;
    }
    let Some(since) = state.awaiting_since else {
        state.awaiting_frame = false;
        return;
    };
    if since.elapsed() >= FRAME_CB_TIMEOUT {
        // Missing frame cb must still FrameDone or WebKit stalls forever.
        if let Some((token, tx)) = state.awaiting_framedone.take() {
            let _ = tx.send(Cmd::FrameDone { token });
        }
        state.awaiting_frame = false;
        state.awaiting_since = None;
    }
}

/// After `wl_callback.Done` (or timeout unlock), attach the latest queued present.
fn flush_queued_after_frame(inner: &mut PlaneInner) {
    if inner.state.awaiting_frame {
        return;
    }
    let Some(q) = inner.state.queued.take() else {
        return;
    };
    if let Err(e) = attach_dmabuf(
        inner,
        q.buffer_key,
        q.width,
        q.height,
        q.format,
        q.modifier,
        q.planes,
        q.render_fence,
        q.token,
        q.release_tx,
    ) {
        tracing::warn!("content plane queued present: {e}");
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
        inflight_keys: Vec::new(),
        buffer_cache: HashMap::new(),
        awaiting_frame: false,
        awaiting_since: None,
        awaiting_framedone: None,
        queued: None,
        buffer_scale: 1,
        x: 200,
        y: 120,
        width: 400,
        height: 400,
        present_ok: 0,
        frame_done: 0,
        drop_queued: 0,
        drop_inflight_cap: 0,
        cache_hit: 0,
        cache_create: 0,
        fence_wait: 0,
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
    // Empty input region: pointer/scroll hit the iced parent (shader → WPE).
    // Without this, place_above steals input and page scroll/click die.
    {
        let region = compositor.create_region(&qh, ());
        child.set_input_region(Some(&region));
        region.destroy();
    }
    let sub = subcompositor.get_subsurface(&child, &parent, &qh, ());
    sub.place_above(&parent);
    // Desync: content commits without waiting for iced parent commit.
    sub.set_desync();
    sub.set_position(state.x, state.y);

    child.commit();
    parent.commit();

    let _ = event_queue.flush();
    let _ = conn.flush();
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
    let data = Arc::new(BufferData {
        release: Mutex::new(None),
        keep_fds: Mutex::new(vec![fd]),
        buffer_key: 0,
    });
    let pool = {
        let keep = data.keep_fds.lock().unwrap();
        shm.create_pool(keep[0].as_fd(), size as i32, &qh, ())
    };
    let buffer = pool.create_buffer(
        0,
        w as i32,
        h as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        &qh,
        data,
    );
    unsafe {
        libc::munmap(map, size);
    }
    std::mem::forget(pool);

    surface.set_buffer_scale(inner.state.buffer_scale);
    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w as i32, h as i32);
    surface.commit();
    let _ = inner.event_queue.flush();
    let _ = inner.conn.flush();
    // Probe path: keep buffer alive via forget (one-shot debug only).
    std::mem::forget(buffer);
    Ok(())
}

fn present_dmabuf(
    inner: &mut PlaneInner,
    buffer_key: usize,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    planes: Vec<(OwnedFd, u32, u32)>,
    render_fence: Option<OwnedFd>,
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
) -> Result<(), String> {
    if width == 0 || height == 0 || planes.is_empty() {
        let _ = release_tx.send(Cmd::Release { token });
        return Ok(());
    }

    // Stock: only one commit until frame callback (RELEASE_ASSERT no second).
    if inner.state.awaiting_frame {
        if let Some(old) = inner.state.queued.replace(QueuedPresent {
            buffer_key,
            width,
            height,
            format,
            modifier,
            planes,
            render_fence,
            token,
            release_tx,
        }) {
            old.release();
            inner.state.drop_queued += 1;
        }
        return Ok(());
    }

    attach_dmabuf(
        inner,
        buffer_key,
        width,
        height,
        format,
        modifier,
        planes,
        render_fence,
        token,
        release_tx,
    )
}

/// Wait for WebKit's render fence (stock UI path before paint / acquire).
fn wait_fence(fence: OwnedFd, timeout_ms: i32) -> bool {
    let raw = fence.as_raw_fd();
    if raw < 0 {
        return true;
    }
    let mut pfd = libc::pollfd {
        fd: raw,
        events: libc::POLLIN,
        revents: 0,
    };
    let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    r > 0
}

/// Attach + commit a dma-buf. Caller must ensure `!awaiting_frame`.
///
/// Mirrors `WPEViewWayland::render_buffer`: cache wl_buffer per pool slot,
/// wait fence, attach, full damage, one frame cb, FrameDone on Done, Release
/// returns WPE loan **without destroying** the protocol object.
fn attach_dmabuf(
    inner: &mut PlaneInner,
    buffer_key: usize,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    planes: Vec<(OwnedFd, u32, u32)>,
    render_fence: Option<OwnedFd>,
    token: ResourceToken,
    release_tx: Sender<Cmd<WpeEngine>>,
) -> Result<(), String> {
    if width == 0 || height == 0 || planes.is_empty() || buffer_key == 0 {
        let _ = release_tx.send(Cmd::Release { token });
        return Ok(());
    }

    if inner.state.inflight_keys.len() >= MAX_INFLIGHT {
        inner.state.drop_inflight_cap += 1;
        if inner.state.drop_inflight_cap == 1 || inner.state.drop_inflight_cap % 30 == 0 {
            tracing::warn!(
                inflight = inner.state.inflight_keys.len(),
                drop_cap = inner.state.drop_inflight_cap,
                "content plane: inflight cap — drop new frame (keep displayed)"
            );
        }
        let _ = release_tx.send(Cmd::Release { token });
        return Ok(());
    }

    if let Some(fence) = render_fence {
        let _ = wait_fence(fence, 50);
        inner.state.fence_wait += 1;
    }

    let can_reuse = inner.state.buffer_cache.get(&buffer_key).is_some_and(|c| {
        c.width == width && c.height == height && c.format == format && c.modifier == modifier
    });

    if can_reuse {
        inner.state.cache_hit += 1;
        // Drop plane dups — cached wl_buffer already owns the mapping FDs.
        drop(planes);
        let cached = inner.state.buffer_cache.get_mut(&buffer_key).unwrap();
        *cached.data.release.lock().unwrap() = Some(PendingRelease {
            token,
            release_tx: release_tx.clone(),
        });
    } else {
        // Stale geometry/format for this key — drop old protocol object.
        if let Some(old) = inner.state.buffer_cache.remove(&buffer_key) {
            if let Some(pr) = old.data.release.lock().unwrap().take() {
                let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
            }
            *old.data.keep_fds.lock().unwrap() = Vec::new();
            old.buffer.destroy();
        }

        let dmabuf = inner
            .state
            .dmabuf
            .as_ref()
            .ok_or_else(|| "no dmabuf".to_string())?
            .clone();
        let qh = inner.event_queue.handle();

        let layouts: Vec<(u32, u32)> = planes.iter().map(|(_, s, o)| (*s, *o)).collect();
        let keep_fds: Vec<OwnedFd> = planes.into_iter().map(|(fd, _, _)| fd).collect();

        let data = Arc::new(BufferData {
            release: Mutex::new(Some(PendingRelease {
                token,
                release_tx: release_tx.clone(),
            })),
            keep_fds: Mutex::new(keep_fds),
            buffer_key,
        });

        let params = dmabuf.create_params(&qh, ());
        {
            let keep = data.keep_fds.lock().unwrap();
            for (i, ((stride, offset), fd)) in layouts.iter().zip(keep.iter()).enumerate() {
                let borrowed = unsafe { BorrowedFd::borrow_raw(fd.as_raw_fd()) };
                params.add(
                    borrowed,
                    i as u32,
                    *offset,
                    *stride,
                    (modifier >> 32) as u32,
                    (modifier & 0xffff_ffff) as u32,
                );
            }
        }

        let buffer = params.create_immed(
            width as i32,
            height as i32,
            format,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &qh,
            Arc::clone(&data),
        );

        inner.state.buffer_cache.insert(
            buffer_key,
            CachedWl {
                buffer,
                data,
                width,
                height,
                format,
                modifier,
            },
        );
        inner.state.cache_create += 1;
    }

    // Attach cached (or just-created) buffer — same as stock re-attach.
    {
        let cached = inner
            .state
            .buffer_cache
            .get(&buffer_key)
            .ok_or_else(|| "cache insert failed".to_string())?;
        let surface = inner
            .state
            .surface
            .as_ref()
            .ok_or_else(|| "no surface".to_string())?;
        let qh = inner.event_queue.handle();

        surface.set_buffer_scale(inner.state.buffer_scale.max(1));
        surface.attach(Some(&cached.buffer), 0, 0);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        let _ = surface.frame(
            &qh,
            FrameCbData {
                token,
                release_tx: release_tx.clone(),
            },
        );
        surface.commit();
    }

    let _ = inner.event_queue.flush();
    let _ = inner.conn.flush();

    inner.state.awaiting_frame = true;
    inner.state.awaiting_since = Some(Instant::now());
    inner.state.awaiting_framedone = Some((token, release_tx));
    if !inner.state.inflight_keys.contains(&buffer_key) {
        inner.state.inflight_keys.push(buffer_key);
    }
    inner.state.present_ok += 1;
    if inner.state.present_ok == 1 || inner.state.present_ok % 60 == 0 {
        tracing::info!(
            n = inner.state.present_ok,
            frame_done = inner.state.frame_done,
            drop_queued = inner.state.drop_queued,
            drop_inflight_cap = inner.state.drop_inflight_cap,
            cache_hit = inner.state.cache_hit,
            cache_create = inner.state.cache_create,
            fence_wait = inner.state.fence_wait,
            width,
            height,
            buffer_scale = inner.state.buffer_scale,
            inflight = inner.state.inflight_keys.len(),
            cached = inner.state.buffer_cache.len(),
            format = format!("{:#x}", format),
            modifier,
            "content plane present ok"
        );
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

impl Dispatch<wl_buffer::WlBuffer, Arc<BufferData>> for PlaneState {
    fn event(
        state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        data: &Arc<BufferData>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            // Stock WPEViewWayland: return WPE loan, KEEP wl_buffer for reuse.
            if let Some(pr) = data.release.lock().unwrap().take() {
                let _ = pr.release_tx.send(Cmd::Release { token: pr.token });
            }
            state.inflight_keys.retain(|k| *k != data.buffer_key);
            // Do NOT destroy proxy or drop keep_fds — cache owns them.
        }
    }
}

/// Unused unit-udata path for any leftover creates.
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
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data: _ } = event {
            // SHM probe / unit-data callbacks — unlock attach gate only.
            state.awaiting_frame = false;
            state.awaiting_since = None;
            state.frame_done += 1;
        }
    }
}

impl Dispatch<wl_callback::WlCallback, FrameCbData> for PlaneState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        data: &FrameCbData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data: _ } = event {
            // Compositor accepted this commit — FrameDone so WebKit can
            // produce the next frame; buffer stays loaned until Release.
            let _ = data.release_tx.send(Cmd::FrameDone {
                token: data.token,
            });
            // Drop timeout copy if it still points at this commit.
            if state
                .awaiting_framedone
                .as_ref()
                .is_some_and(|(t, _)| t.buffer == data.token.buffer)
            {
                state.awaiting_framedone = None;
            }
            state.awaiting_frame = false;
            state.awaiting_since = None;
            state.frame_done += 1;
        }
    }
}

impl Dispatch<wl_region::WlRegion, ()> for PlaneState {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: wl_region::Event,
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
