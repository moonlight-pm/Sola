//! The lock: `ext-session-lock-v1` through smithay-client-toolkit, a lock
//! surface on every output, the keyboard into the password, and the check
//! on a thread (Oath answers a wrong password late).

use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use image::RgbaImage;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Event as ChannelEvent};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
    SessionLockSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_output, delegate_registry, delegate_seat,
    delegate_session_lock, delegate_shm, registry_handlers,
};
use tracing::{info, warn};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface};
use wayland_client::backend::ObjectId;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::draw::{Painter, Scene};
use crate::keys::{Edit, Keys};
use crate::picture;

struct Surface {
    lock: SessionLockSurface,
    output: wl_output::WlOutput,
    /// Logical size, from the configure.
    size: Option<(u32, u32)>,
    scale: i32,
    /// The picture cut to this surface's buffer size.
    backdrop: Option<RgbaImage>,
    /// A cut of that size is being made on a thread.
    cutting: Option<(u32, u32)>,
    /// Kept until the next frame replaces it.
    buffer: Option<Buffer>,
}

/// The answer to a check.
type Checked = Result<bool, String>;

/// The picture cut for a surface: never on the event loop (a large photo
/// takes seconds, and keys and the check's answer would wait).
type Cut = (ObjectId, (u32, u32), RgbaImage);

struct Lock {
    conn: Connection,
    registry: RegistryState,
    compositor: CompositorState,
    outputs: OutputState,
    shm: Shm,
    seats: SeatState,
    lock_state: SessionLockState,
    lock: Option<SessionLock>,
    locked: bool,
    surfaces: Vec<Surface>,
    pool: Option<SlotPool>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keys: Keys,
    password: String,
    checking: bool,
    message: String,
    error: bool,
    clock: (String, String),
    picture: Option<Arc<RgbaImage>>,
    /// Cuts of the picture come back here (surface, buffer size, image).
    cuts: channel::Sender<Cut>,
    painter: Painter,
    loop_handle: LoopHandle<'static, Lock>,
    /// Set when the check says right: unlock and leave.
    unlocked: bool,
    failed: Option<String>,
}

pub fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland: {e}"))?;
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh: QueueHandle<Lock> = queue.handle();
    let mut event_loop: EventLoop<Lock> =
        EventLoop::try_new().map_err(|e| format!("event loop: {e}"))?;

    let lock_state = SessionLockState::new(&globals, &qh);
    let (cuts, cut_rx) = channel::channel::<Cut>();
    let mut state = Lock {
        conn: conn.clone(),
        registry: RegistryState::new(&globals),
        compositor: CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?,
        outputs: OutputState::new(&globals, &qh),
        shm: Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?,
        seats: SeatState::new(&globals, &qh),
        lock: None,
        lock_state,
        locked: false,
        surfaces: Vec::new(),
        pool: None,
        keyboard: None,
        keys: Keys::new(),
        password: String::new(),
        checking: false,
        message: String::new(),
        error: false,
        clock: clock(),
        picture: None,
        cuts,
        painter: Painter::new(),
        loop_handle: event_loop.handle(),
        unlocked: false,
        failed: None,
    };
    state.lock = Some(
        state
            .lock_state
            .lock(&qh)
            .map_err(|_| "the compositor has no ext-session-lock-v1".to_string())?,
    );
    WaylandSource::new(conn, queue)
        .insert(event_loop.handle())
        .map_err(|e| format!("wayland source: {e}"))?;
    // The outputs' events, then a lock surface on each at once: rivulet
    // confirms the lock only once every output has one.
    event_loop
        .dispatch(Duration::ZERO, &mut state)
        .map_err(|e| format!("dispatch: {e}"))?;
    let _ = state.conn.roundtrip();
    event_loop
        .dispatch(Duration::ZERO, &mut state)
        .map_err(|e| format!("dispatch: {e}"))?;
    let outputs: Vec<_> = state.outputs.outputs().collect();
    for output in outputs {
        state.add_surface(output, &qh);
    }

    // The picture decodes on a thread; the lock shows the plain ground until then.
    let (pic_tx, pic_rx) = channel::channel::<RgbaImage>();
    std::thread::spawn(move || {
        if let Some(img) = picture::load() {
            let _ = pic_tx.send(img);
        }
    });
    event_loop
        .handle()
        .insert_source(pic_rx, |ev, _, state: &mut Lock| {
            if let ChannelEvent::Msg(img) = ev {
                state.picture = Some(Arc::new(img));
                for s in &mut state.surfaces {
                    s.backdrop = None;
                    s.cutting = None;
                }
                state.redraw();
            }
        })
        .map_err(|e| format!("picture source: {e}"))?;
    event_loop
        .handle()
        .insert_source(cut_rx, |ev, _, state: &mut Lock| {
            let ChannelEvent::Msg((id, size, img)) = ev else { return };
            let Some(i) = state.surfaces.iter().position(|s| s.lock.wl_surface().id() == id) else {
                return;
            };
            let s = &mut state.surfaces[i];
            if s.cutting == Some(size) {
                s.cutting = None;
                s.backdrop = Some(img);
                state.draw(i);
            }
        })
        .map_err(|e| format!("cut source: {e}"))?;

    event_loop
        .handle()
        .insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, state: &mut Lock| {
            let now = clock();
            if now != state.clock {
                state.clock = now;
                state.redraw();
            }
            TimeoutAction::ToDuration(Duration::from_secs(1))
        })
        .map_err(|e| format!("clock: {e}"))?;

    loop {
        event_loop
            .dispatch(Duration::from_millis(250), &mut state)
            .map_err(|e| format!("dispatch: {e}"))?;
        if let Some(why) = state.failed.take() {
            return Err(why);
        }
        if state.unlocked {
            if let Some(lock) = state.lock.take() {
                lock.unlock();
            }
            let _ = state.conn.roundtrip();
            return Ok(());
        }
    }
}

fn clock() -> (String, String) {
    let now = Local::now();
    (now.format("%H:%M").to_string(), now.format("%A, %-d %B").to_string())
}

impl Lock {
    fn add_surface(&mut self, output: wl_output::WlOutput, qh: &QueueHandle<Self>) {
        let Some(lock) = &self.lock else { return };
        if self.surfaces.iter().any(|s| s.output == output) {
            return;
        }
        let scale = self
            .outputs
            .info(&output)
            .map(|i| i.scale_factor.max(1))
            .unwrap_or(1);
        let surface = self.compositor.create_surface(qh);
        let lock = lock.create_lock_surface(surface, &output, qh);
        self.surfaces.push(Surface {
            lock,
            output,
            size: None,
            scale,
            backdrop: None,
            cutting: None,
            buffer: None,
        });
    }

    fn redraw(&mut self) {
        for i in 0..self.surfaces.len() {
            self.draw(i);
        }
    }

    fn draw(&mut self, i: usize) {
        let Some((w, h)) = self.surfaces[i].size else { return };
        let scale = self.surfaces[i].scale;
        let (bw, bh) = (w * scale as u32, h * scale as u32);
        let stride = bw as i32 * 4;
        let pool = match &mut self.pool {
            Some(p) => p,
            None => match SlotPool::new((stride * bh as i32) as usize, &self.shm) {
                Ok(p) => self.pool.insert(p),
                Err(e) => {
                    warn!("shm pool: {e}");
                    return;
                }
            },
        };
        let (buffer, canvas) =
            match pool.create_buffer(bw as i32, bh as i32, stride, wl_shm::Format::Argb8888) {
                Ok(b) => b,
                Err(e) => {
                    warn!("shm buffer: {e}");
                    return;
                }
            };
        let s = &mut self.surfaces[i];
        if s.backdrop.as_ref().is_some_and(|b| b.dimensions() != (bw, bh)) {
            s.backdrop = None;
        }
        if let Some(pic) = &self.picture {
            if s.backdrop.is_none() && s.cutting != Some((bw, bh)) {
                s.cutting = Some((bw, bh));
                let (pic, tx, id) = (pic.clone(), self.cuts.clone(), s.lock.wl_surface().id());
                std::thread::spawn(move || {
                    let _ = tx.send((id, (bw, bh), picture::cover(&pic, bw, bh)));
                });
            }
        }
        let scene = Scene {
            time: &self.clock.0,
            date: &self.clock.1,
            typed: self.password.chars().count(),
            message: &self.message,
            error: self.error,
            main: i == 0,
        };
        self.painter
            .paint(canvas, bw, bh, scale as f32, s.backdrop.as_ref(), &scene);
        let surface = s.lock.wl_surface();
        surface.set_buffer_scale(scale);
        surface.damage_buffer(0, 0, bw as i32, bh as i32);
        if let Err(e) = buffer.attach_to(surface) {
            warn!("attach: {e}");
            return;
        }
        surface.commit();
        s.buffer = Some(buffer);
    }

    fn edit(&mut self, edit: Edit) {
        if self.checking {
            return;
        }
        match edit {
            Edit::Type(t) => self.password.push_str(&t),
            Edit::Erase => {
                self.password.pop();
            }
            Edit::Clear => self.clear(),
            Edit::Submit if self.password.is_empty() => return,
            Edit::Submit => return self.submit(),
        }
        self.message.clear();
        self.error = false;
        self.redraw();
    }

    fn submit(&mut self) {
        self.checking = true;
        self.message = "Checking…".into();
        self.error = false;
        let password = std::mem::take(&mut self.password);
        let (tx, rx) = channel::channel::<Checked>();
        std::thread::spawn(move || {
            let mut password = password;
            let answer = sola_core::login::check(&password).map_err(|e| e.to_string());
            wipe(&mut password);
            let _ = tx.send(answer);
        });
        let inserted = self.loop_handle.insert_source(rx, |ev, _, state: &mut Lock| {
            if let ChannelEvent::Msg(answer) = ev {
                state.checked(answer);
            }
        });
        if let Err(e) = inserted {
            warn!("check source: {e}");
            self.checking = false;
        }
        self.redraw();
    }

    fn checked(&mut self, answer: Checked) {
        self.checking = false;
        match answer {
            Ok(true) => {
                info!("password right: unlocking");
                self.unlocked = true;
                return;
            }
            Ok(false) => {
                info!("wrong password");
                self.message = "Wrong password".into();
            }
            Err(e) => {
                warn!("check failed: {e}");
                self.message = format!("Could not check: {e}");
            }
        }
        self.error = true;
        self.redraw();
    }

    fn clear(&mut self) {
        wipe(&mut self.password);
    }
}

/// Overwrite a password before letting it go.
fn wipe(s: &mut String) {
    let mut bytes = std::mem::take(s).into_bytes();
    bytes.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(&bytes);
}

impl SessionLockHandler for Lock {
    fn locked(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: SessionLock) {
        info!(outputs = self.surfaces.len(), "locked");
        self.locked = true;
        let outputs: Vec<_> = self.outputs.outputs().collect();
        for output in outputs {
            self.add_surface(output, qh);
        }
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        if !self.locked {
            self.failed = Some("the compositor refused the lock (another lock screen?)".into());
        } else {
            warn!("the compositor ended the lock");
            self.unlocked = true;
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        if let Some(i) = self
            .surfaces
            .iter()
            .position(|s| s.lock.wl_surface() == surface.wl_surface())
        {
            let size = configure.new_size;
            if self.surfaces[i].size != Some(size) {
                self.surfaces[i].backdrop = None;
            }
            self.surfaces[i].size = Some(size);
            self.draw(i);
        }
    }
}

impl CompositorHandler for Lock {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(i) = self.surfaces.iter().position(|s| s.lock.wl_surface() == surface) {
            if self.surfaces[i].scale != factor.max(1) {
                self.surfaces[i].scale = factor.max(1);
                self.surfaces[i].backdrop = None;
                self.draw(i);
            }
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Lock {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_surface(output, qh);
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.surfaces.retain(|s| s.output != output);
    }
}

impl SeatHandler for Lock {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Lock {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if format == WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
                    state.keys.keymap(fd, size);
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => state.keys.modifiers(mods_depressed, mods_latched, mods_locked, group),
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                if let Some(edit) = state.keys.press(key) {
                    state.edit(edit);
                }
            }
            _ => {}
        }
    }
}

impl ShmHandler for Lock {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Lock {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Lock);
delegate_output!(Lock);
delegate_shm!(Lock);
delegate_seat!(Lock);
delegate_session_lock!(Lock);
delegate_registry!(Lock);
