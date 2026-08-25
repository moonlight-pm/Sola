//! Nested Wayland compositor in the CSS hole + `--foreign-client` helper.

use std::os::fd::AsFd;
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use memmap2::Mmap;
use wayland_protocols::xdg::shell::server::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_server::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_region, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_server::{
    backend::{ClientData, ClientId, DisconnectReason},
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, ListeningSocket, New,
    Resource, WEnum,
};

#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub argb: Vec<u32>,
}

pub struct ForeignHole {
    rx: Receiver<Frame>,
    size_tx: Sender<(i32, i32)>,
    child: Option<Child>,
    latest: Option<Frame>,
    socket: String,
}

impl ForeignHole {
    pub fn start(width: i32, height: i32) -> Option<Self> {
        let (frame_tx, frame_rx) = mpsc::channel();
        let (size_tx, size_rx) = mpsc::channel();
        let (name_tx, name_rx) = mpsc::channel();
        let _ = size_tx.send((width.max(1), height.max(1)));
        thread::Builder::new()
            .name("html-spike-nest".into())
            .spawn(move || {
                if let Err(e) = run_compositor(frame_tx, size_rx, name_tx) {
                    tracing::warn!(%e, "nested compositor exited");
                }
            })
            .ok()?;
        let socket = name_rx.recv_timeout(Duration::from_secs(2)).ok()?;
        tracing::info!(%socket, "nested wayland socket");
        let exe = std::env::current_exe().ok()?;
        let child = Command::new(exe)
            .arg("--foreign-client")
            .env("WAYLAND_DISPLAY", &socket)
            .env_remove("WAYLAND_SOCKET")
            .spawn()
            .map_err(|e| tracing::warn!(%e, "spawn --foreign-client"))
            .ok()?;
        tracing::info!(pid = child.id(), "foreign wayland client spawned");
        Some(Self {
            rx: frame_rx,
            size_tx,
            child: Some(child),
            latest: None,
            socket,
        })
    }

    pub fn set_size(&self, w: i32, h: i32) {
        let _ = self.size_tx.send((w.max(1), h.max(1)));
    }

    pub fn poll(&mut self) -> Option<&Frame> {
        loop {
            match self.rx.try_recv() {
                Ok(f) => self.latest = Some(f),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        self.latest.as_ref()
    }

    pub fn socket(&self) -> &str {
        &self.socket
    }
}

impl Drop for ForeignHole {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

struct ClientState;
impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

struct PoolData {
    map: Mmap,
}

struct BufferData {
    pool: wl_shm_pool::WlShmPool,
    offset: i32,
    width: i32,
    height: i32,
    stride: i32,
}

struct SurfaceData {
    buffer: Mutex<Option<wl_buffer::WlBuffer>>,
    callbacks: Mutex<Vec<wl_callback::WlCallback>>,
}

struct Comp {
    frames: Sender<Frame>,
    size: Mutex<(i32, i32)>,
    size_rx: Receiver<(i32, i32)>,
    serial: Mutex<u32>,
    mapped: Mutex<Vec<(xdg_toplevel::XdgToplevel, xdg_surface::XdgSurface)>>,
}

fn run_compositor(
    frames: Sender<Frame>,
    size_rx: Receiver<(i32, i32)>,
    name_tx: Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut display: Display<Comp> = Display::new()?;
    let dh = display.handle();
    dh.create_global::<Comp, wl_compositor::WlCompositor, ()>(4, ());
    dh.create_global::<Comp, wl_shm::WlShm, ()>(1, ());
    dh.create_global::<Comp, xdg_wm_base::XdgWmBase, ()>(3, ());
    let listener = ListeningSocket::bind_auto("wayland-html-hole", 1..32)?;
    let name = listener
        .socket_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or("socket name")?;
    let _ = name_tx.send(name);
    let mut state = Comp {
        frames,
        size: Mutex::new((640, 480)),
        size_rx,
        serial: Mutex::new(1),
        mapped: Mutex::new(Vec::new()),
    };
    loop {
        while let Ok(sz) = state.size_rx.try_recv() {
            *state.size.lock().unwrap() = sz;
            for (tl, xdg) in state.mapped.lock().unwrap().iter() {
                send_configure(&state, xdg, tl);
            }
        }
        if let Ok(Some(stream)) = listener.accept() {
            let _ = display.handle().insert_client(stream, Arc::new(ClientState));
        }
        if let Err(e) = display.dispatch_clients(&mut state) {
            tracing::warn!(%e, "nest dispatch");
        }
        if let Err(e) = display.flush_clients() {
            tracing::warn!(%e, "nest flush");
        }
        thread::sleep(Duration::from_millis(4));
    }
}

fn next_serial(state: &Comp) -> u32 {
    let mut s = state.serial.lock().unwrap();
    *s += 1;
    *s
}

fn send_configure(state: &Comp, xdg: &xdg_surface::XdgSurface, tl: &xdg_toplevel::XdgToplevel) {
    let (w, h) = *state.size.lock().unwrap();
    tl.configure(w, h, vec![]);
    xdg.configure(next_serial(state));
}

impl GlobalDispatch<wl_compositor::WlCompositor, ()> for Comp {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_compositor::WlCompositor>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_compositor::WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                data_init.init(
                    id,
                    SurfaceData {
                        buffer: Mutex::new(None),
                        callbacks: Mutex::new(Vec::new()),
                    },
                );
            }
            wl_compositor::Request::CreateRegion { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_region::WlRegion, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_region::WlRegion,
        _: wl_region::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<wl_shm::WlShm, ()> for Comp {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_shm::WlShm>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let shm = data_init.init(resource, ());
        shm.format(wl_shm::Format::Argb8888);
    }
}

impl Dispatch<wl_shm::WlShm, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_shm::WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, fd, size } = request {
            if size <= 0 {
                return;
            }
            let file = std::fs::File::from(fd);
            let Ok(map) = (unsafe { Mmap::map(&file) }) else {
                return;
            };
            data_init.init(id, PoolData { map });
        }
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, PoolData> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        pool: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        _: &PoolData,
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_shm_pool::Request::CreateBuffer {
                id,
                offset,
                width,
                height,
                stride,
                format: _,
            } => {
                data_init.init(
                    id,
                    BufferData {
                        pool: pool.clone(),
                        offset,
                        width,
                        height,
                        stride,
                    },
                );
            }
            wl_shm_pool::Request::Resize { .. } | wl_shm_pool::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, BufferData> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Request,
        _: &BufferData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_callback::WlCallback,
        _: wl_callback::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, SurfaceData> for Comp {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &wl_surface::WlSurface,
        request: wl_surface::Request,
        data: &SurfaceData,
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_surface::Request::Attach { buffer, .. } => {
                *data.buffer.lock().unwrap() = buffer;
            }
            wl_surface::Request::Frame { callback } => {
                let cb = data_init.init(callback, ());
                data.callbacks.lock().unwrap().push(cb);
            }
            wl_surface::Request::Commit => {
                if let Some(buf) = data.buffer.lock().unwrap().clone() {
                    if let Some(bd) = buf.data::<BufferData>() {
                        if let Some(pool) = bd.pool.data::<PoolData>() {
                            let w = bd.width.max(1) as u32;
                            let h = bd.height.max(1) as u32;
                            let stride = bd.stride.max(4) as usize;
                            let off = bd.offset.max(0) as usize;
                            let mut argb = vec![0u32; (w * h) as usize];
                            for y in 0..h as usize {
                                let row = off + y * stride;
                                for x in 0..w as usize {
                                    let i = row + x * 4;
                                    if i + 3 < pool.map.len() {
                                        let b = pool.map[i] as u32;
                                        let g = pool.map[i + 1] as u32;
                                        let r = pool.map[i + 2] as u32;
                                        argb[y * w as usize + x] = (r << 16) | (g << 8) | b;
                                    }
                                }
                            }
                            let _ = state.frames.send(Frame {
                                width: w,
                                height: h,
                                argb,
                            });
                            buf.release();
                        }
                    }
                }
                let now = 1u32;
                for cb in data.callbacks.lock().unwrap().drain(..) {
                    cb.done(now);
                }
                let _ = resource;
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<xdg_wm_base::XdgWmBase, ()> for Comp {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<xdg_wm_base::XdgWmBase>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        resource: &xdg_wm_base::XdgWmBase,
        request: xdg_wm_base::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::GetXdgSurface { id, surface: _ } => {
                data_init.init(id, ());
            }
            xdg_wm_base::Request::Pong { .. } | xdg_wm_base::Request::Destroy => {}
            xdg_wm_base::Request::CreatePositioner { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
        let _ = resource;
    }
}

impl Dispatch<wayland_protocols::xdg::shell::server::xdg_positioner::XdgPositioner, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wayland_protocols::xdg::shell::server::xdg_positioner::XdgPositioner,
        _: wayland_protocols::xdg::shell::server::xdg_positioner::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for Comp {
    fn request(
        state: &mut Self,
        _: &Client,
        resource: &xdg_surface::XdgSurface,
        request: xdg_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_surface::Request::GetToplevel { id } => {
                let tl = data_init.init(id, ());
                send_configure(state, resource, &tl);
                state
                    .mapped
                    .lock()
                    .unwrap()
                    .push((tl, resource.clone()));
            }
            xdg_surface::Request::AckConfigure { .. }
            | xdg_surface::Request::Destroy
            | xdg_surface::Request::SetWindowGeometry { .. } => {}
            _ => {}
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for Comp {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &xdg_toplevel::XdgToplevel,
        _: xdg_toplevel::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

// --- nested client ----------------------------------------------------------

use wayland_client::protocol::{
    wl_buffer as cbuf, wl_compositor as ccomp, wl_registry, wl_shm as cshm, wl_shm_pool as cpool,
    wl_surface as csurf,
};
use wayland_client::{Connection, Dispatch as CDispatch, EventQueue, QueueHandle, WEnum as CWEnum};
use wayland_protocols::xdg::shell::client::{
    xdg_surface as cxdg_s, xdg_toplevel as cxdg_t, xdg_wm_base as cxdg,
};

struct Slot {
    buffer: cbuf::WlBuffer,
    busy: Arc<std::sync::atomic::AtomicBool>,
}

struct ClientApp {
    compositor: Option<ccomp::WlCompositor>,
    shm: Option<cshm::WlShm>,
    wm: Option<cxdg::XdgWmBase>,
    surface: Option<csurf::WlSurface>,
    xdg_s: Option<cxdg_s::XdgSurface>,
    configured: bool,
    width: i32,
    height: i32,
    file: Option<std::fs::File>,
    mmap: Option<memmap2::MmapMut>,
    pool: Option<cpool::WlShmPool>,
    slots: Vec<Slot>,
    pool_w: i32,
    pool_h: i32,
}

pub fn run_client() {
    tracing::info!("foreign client starting");
    // Die if the parent spike exits without Drop (SIGKILL, crash).
    let _ = rustix::process::set_parent_process_death_signal(Some(
        rustix::process::Signal::TERM,
    ));
    let conn = Connection::connect_to_env().expect("connect nested compositor");
    let mut queue: EventQueue<ClientApp> = conn.new_event_queue();
    let qh = queue.handle();
    let _reg = conn.display().get_registry(&qh, ());
    let mut app = ClientApp {
        compositor: None,
        shm: None,
        wm: None,
        surface: None,
        xdg_s: None,
        configured: false,
        width: 640,
        height: 480,
        file: None,
        mmap: None,
        pool: None,
        slots: Vec::new(),
        pool_w: 0,
        pool_h: 0,
    };
    queue.roundtrip(&mut app).expect("registry");
    let compositor = app.compositor.clone().expect("wl_compositor");
    let shm = app.shm.clone().expect("wl_shm");
    let wm = app.wm.clone().expect("xdg_wm_base");
    let surface = compositor.create_surface(&qh, ());
    let xdg_s = wm.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_s.get_toplevel(&qh, ());
    toplevel.set_title("foreign-hole".into());
    surface.commit();
    app.surface = Some(surface.clone());
    app.xdg_s = Some(xdg_s);
    while !app.configured {
        queue.blocking_dispatch(&mut app).expect("configure");
    }
    let mut t = 0.0f32;
    loop {
        let w = app.width.max(1);
        let h = app.height.max(1);
        if app.pool_w != w || app.pool_h != h || app.slots.is_empty() {
            rebuild_client_pool(&mut app, &shm, &qh, w, h);
        }
        let idx = app
            .slots
            .iter()
            .position(|s| !s.busy.load(std::sync::atomic::Ordering::Acquire));
        if let Some(idx) = idx {
            let stride = (w as u32) * 4;
            let one = stride as usize * h as usize;
            let offset = idx * one;
            if let Some(map) = app.mmap.as_mut() {
                if let Some(dst) = map.get_mut(offset..offset + one) {
                    fill_foreign(dst, w as u32, h as u32, t);
                }
            }
            let slot = &app.slots[idx];
            slot.busy.store(true, std::sync::atomic::Ordering::Release);
            surface.attach(Some(&slot.buffer), 0, 0);
            surface.damage_buffer(0, 0, w, h);
            surface.commit();
            if conn.flush().is_err() {
                tracing::warn!("foreign client: nested compositor gone");
                return;
            }
            t += 0.05;
        }
        // Must read the socket (not only dispatch_pending) or Release never
        // arrives and both buffers stay busy after two frames.
        if queue.blocking_dispatch(&mut app).is_err() {
            tracing::warn!("foreign client: nested compositor gone");
            return;
        }
    }
}

fn rebuild_client_pool(
    app: &mut ClientApp,
    shm: &cshm::WlShm,
    qh: &QueueHandle<ClientApp>,
    w: i32,
    h: i32,
) {
    for slot in app.slots.drain(..) {
        slot.buffer.destroy();
    }
    if let Some(pool) = app.pool.take() {
        pool.destroy();
    }
    let stride = w * 4;
    let one = (stride as usize) * (h as usize);
    let need = one.saturating_mul(2).max(4096);
    let fd = rustix::fs::memfd_create("foreign-client", rustix::fs::MemfdFlags::CLOEXEC)
        .expect("memfd");
    rustix::fs::ftruncate(&fd, need as u64).expect("ftruncate");
    let file = std::fs::File::from(fd);
    let mmap = unsafe { memmap2::MmapMut::map_mut(&file).expect("mmap") };
    let pool = shm.create_pool(file.as_fd(), need as i32, qh, ());
    for i in 0..2 {
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let buffer = pool.create_buffer(
            (i * one) as i32,
            w,
            h,
            stride,
            cshm::Format::Argb8888,
            qh,
            busy.clone(),
        );
        app.slots.push(Slot { buffer, busy });
    }
    app.file = Some(file);
    app.mmap = Some(mmap);
    app.pool = Some(pool);
    app.pool_w = w;
    app.pool_h = h;
}

fn fill_foreign(dst: &mut [u8], w: u32, h: u32, time: f32) {
    let off = (time * 36.0) as i32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let band = ((x - y + off) / 22) & 1 == 0;
            let (r, g, b) = if band {
                (0xE0u8, 0x28, 0x78)
            } else {
                (0x18, 0xC4, 0xC8)
            };
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            dst[i] = b;
            dst[i + 1] = g;
            dst[i + 2] = r;
            dst[i + 3] = 0xFF;
        }
    }
}

impl CDispatch<wl_registry::WlRegistry, ()> for ClientApp {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
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
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                }
                "xdg_wm_base" => {
                    state.wm = Some(registry.bind(name, version.min(3), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl CDispatch<ccomp::WlCompositor, ()> for ClientApp {
    fn event(
        _: &mut Self,
        _: &ccomp::WlCompositor,
        _: ccomp::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl CDispatch<cshm::WlShm, ()> for ClientApp {
    fn event(
        _: &mut Self,
        _: &cshm::WlShm,
        _: cshm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl CDispatch<cpool::WlShmPool, ()> for ClientApp {
    fn event(
        _: &mut Self,
        _: &cpool::WlShmPool,
        _: cpool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl CDispatch<cbuf::WlBuffer, Arc<std::sync::atomic::AtomicBool>> for ClientApp {
    fn event(
        _: &mut Self,
        _: &cbuf::WlBuffer,
        event: cbuf::Event,
        busy: &Arc<std::sync::atomic::AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let cbuf::Event::Release = event {
            busy.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}
impl CDispatch<csurf::WlSurface, ()> for ClientApp {
    fn event(
        _: &mut Self,
        _: &csurf::WlSurface,
        _: csurf::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl CDispatch<cxdg::XdgWmBase, ()> for ClientApp {
    fn event(
        _: &mut Self,
        wm: &cxdg::XdgWmBase,
        event: cxdg::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let cxdg::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}
impl CDispatch<cxdg_s::XdgSurface, ()> for ClientApp {
    fn event(
        state: &mut Self,
        xdg: &cxdg_s::XdgSurface,
        event: cxdg_s::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let cxdg_s::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            state.configured = true;
        }
    }
}
impl CDispatch<cxdg_t::XdgToplevel, ()> for ClientApp {
    fn event(
        state: &mut Self,
        _: &cxdg_t::XdgToplevel,
        event: cxdg_t::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let cxdg_t::Event::Configure { width, height, .. } = event {
            if width > 0 {
                state.width = width;
            }
            if height > 0 {
                state.height = height;
            }
        }
    }
}

// silence unused import if WEnum not needed on server shm format
#[allow(dead_code)]
fn _wenum(_: WEnum<wl_shm::Format>, _: CWEnum<cshm::Format>) {}
