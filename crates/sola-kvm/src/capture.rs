//! Server capture through the compositor (no `/dev/input`): a thin
//! layer-shell strip on the shared edge. The compositor reports the real
//! cursor reaching it; then the strip takes the keyboard (exclusive),
//! locks the pointer (pointer-constraints), reads relative motion
//! (relative-pointer), and holds a shortcuts inhibitor, so every key and
//! button comes here and none goes to the window manager (rivulet keeps
//! only Ctrl+Alt+F1–F12). Leaving releases all of it and puts the cursor
//! back just inside the screen. Ctrl+Alt+Esc comes back from anywhere.
//!
//! Keys arrive without auto-repeat (Wayland leaves repeating to clients),
//! so a held key repeats here at the seat's rate, as the kernel's did.

use std::collections::BTreeSet;
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
    zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1},
};
use wayland_protocols::wp::pointer_constraints::zv1::client::{
    zwp_locked_pointer_v1::{self, ZwpLockedPointerV1},
    zwp_pointer_constraints_v1::{Lifetime, ZwpPointerConstraintsV1},
};
use wayland_protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::layout::Side;
use crate::server::InputEvent;

/// Strip thickness (lan-mouse uses 1; 2 is easier to hit).
const STRIP: i32 = 2;
/// How far inside the screen the cursor comes back on leave, so it is off
/// the strip and does not cross straight back.
const INSET: i32 = 32;
/// How long the compositor may take to hand over the keyboard and the lock.
const GRAB_WAIT: Duration = Duration::from_millis(400);

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const KEY_ESC: u32 = 1;
const KEY_LEFTCTRL: u32 = 29;
const KEY_RIGHTCTRL: u32 = 97;
const KEY_LEFTALT: u32 = 56;
const KEY_RIGHTALT: u32 = 100;
/// Keys that never repeat: modifiers and locks.
const NO_REPEAT: [u32; 10] = [29, 97, 42, 54, 56, 100, 125, 126, 58, 69];

/// What the strip saw.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureEvent {
    /// The real cursor reached the shared edge, `along` it in primary space
    /// (y for left/right, x for top/bottom).
    Edge(i32),
    /// Input while captured.
    Input(InputEvent),
    /// Ctrl+Alt+Esc while captured: come back.
    Escape,
    /// The compositor took the keyboard or the pointer lock away while
    /// captured (a VT switch, the lock screen).
    Lost,
}

pub struct Capture {
    conn: Connection,
    qh: QueueHandle<State>,
    event_queue: EventQueue<State>,
    state: State,
}

struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    seat: Option<wl_seat::WlSeat>,
    constraints: Option<ZwpPointerConstraintsV1>,
    relative_manager: Option<ZwpRelativePointerManagerV1>,
    inhibit_manager: Option<ZwpKeyboardShortcutsInhibitManagerV1>,
    /// First bound output (the layer surface goes on it).
    output: Option<wl_output::WlOutput>,
    output_sizes: Vec<(i32, i32)>,
    primary_w: i32,
    primary_h: i32,
    side: Side,

    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    relative: Option<ZwpRelativePointerV1>,
    configured: bool,
    surface_w: i32,
    surface_h: i32,
    /// The compositor closed the strip (cold-start race); make it again.
    needs_recreate: bool,
    last_recreate: Option<Instant>,

    /// Captured: input goes to the peer.
    captured: bool,
    lock: Option<ZwpLockedPointerV1>,
    locked: bool,
    inhibitor: Option<ZwpKeyboardShortcutsInhibitorV1>,
    /// The strip has the keyboard.
    keyboard_focus: bool,
    /// Ctrl and Alt keys down while captured (the escape chord).
    held: BTreeSet<u32>,
    /// Ctrl and Alt in the keyboard's modifier state (a virtual keyboard
    /// may set them without key events).
    mods_ctrl: bool,
    mods_alt: bool,
    /// The seat's repeat: keys per second (0 = none) and delay.
    repeat_rate: i32,
    repeat_delay: Duration,
    /// The key repeating, and when it next repeats.
    repeating: Option<(u32, Instant)>,
    /// This frame's scroll: wheel detents (value120 / 120 when the
    /// compositor sends it), and whether value120 came.
    scroll: (f32, f32),
    scroll_120: bool,
    events: Vec<CaptureEvent>,
}

impl Capture {
    pub fn connect(side: Side, primary_w: i32, primary_h: i32) -> Result<Self, String> {
        let conn = Connection::connect_to_env()
            .map_err(|e| format!("wayland connect: {e} (WAYLAND_DISPLAY set? compositor up?)"))?;
        let (globals, mut event_queue) =
            registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = event_queue.handle();

        let mut state = State {
            compositor: None,
            shm: None,
            layer_shell: None,
            seat: None,
            constraints: None,
            relative_manager: None,
            inhibit_manager: None,
            output: None,
            output_sizes: Vec::new(),
            primary_w,
            primary_h,
            side,
            surface: None,
            layer_surface: None,
            pointer: None,
            keyboard: None,
            relative: None,
            configured: false,
            surface_w: 0,
            surface_h: 0,
            needs_recreate: false,
            last_recreate: None,
            captured: false,
            lock: None,
            locked: false,
            inhibitor: None,
            keyboard_focus: false,
            held: BTreeSet::new(),
            mods_ctrl: false,
            mods_alt: false,
            repeat_rate: 25,
            repeat_delay: Duration::from_millis(600),
            repeating: None,
            scroll: (0.0, 0.0),
            scroll_120: false,
            events: Vec::new(),
        };

        let need = |what: &str, e: wayland_client::globals::BindError| {
            format!("{what}: {e} (the compositor must offer it to capture input)")
        };
        state.compositor = Some(globals.bind(&qh, 1..=6, ()).map_err(|e| need("wl_compositor", e))?);
        state.shm = Some(globals.bind(&qh, 1..=1, ()).map_err(|e| need("wl_shm", e))?);
        state.layer_shell = Some(globals.bind(&qh, 1..=4, ()).map_err(|e| need("zwlr_layer_shell_v1", e))?);
        state.seat = Some(globals.bind(&qh, 1..=8, ()).map_err(|e| need("wl_seat", e))?);
        state.constraints =
            Some(globals.bind(&qh, 1..=1, ()).map_err(|e| need("zwp_pointer_constraints_v1", e))?);
        state.relative_manager = Some(
            globals
                .bind(&qh, 1..=1, ())
                .map_err(|e| need("zwp_relative_pointer_manager_v1", e))?,
        );
        // rivulet has it; River 0.4.5 does not, and its bindings then still
        // fire while remote.
        state.inhibit_manager = globals.bind(&qh, 1..=1, ()).ok();
        if state.inhibit_manager.is_none() {
            warn!(
                "no zwp_keyboard_shortcuts_inhibit_manager_v1: the window manager's \
                 key bindings still fire while the pointer is on the peer"
            );
        }
        if let Ok(output) = globals.bind::<wl_output::WlOutput, _, _>(&qh, 1..=4, ()) {
            state.output = Some(output);
        }

        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip: {e}"))?;
        apply_best_output_size(&mut state);

        // Cold start: the compositor may close layer surfaces until its
        // window manager is up. Retry briefly, then keep trying in poll.
        const ATTEMPTS: u32 = 20;
        for attempt in 1..=ATTEMPTS {
            create_strip(&mut state, &qh)?;
            event_queue
                .roundtrip(&mut state)
                .map_err(|e| format!("configure: {e}"))?;
            if state.configured {
                break;
            }
            if attempt < ATTEMPTS {
                destroy_strip(&mut state);
                std::thread::sleep(Duration::from_millis(100));
                let _ = event_queue.roundtrip(&mut state);
                apply_best_output_size(&mut state);
            } else {
                warn!("edge strip still unconfigured after retries; will keep trying");
                state.needs_recreate = true;
            }
        }
        info!(
            ?side,
            w = state.primary_w,
            h = state.primary_h,
            "edge strip up: capture through the compositor (no /dev/input)"
        );

        Ok(Self {
            conn,
            qh,
            event_queue,
            state,
        })
    }

    /// The Wayland socket, to wait on with the other inputs.
    pub fn fd(&self) -> RawFd {
        self.conn.as_fd().as_raw_fd()
    }

    pub fn is_captured(&self) -> bool {
        self.state.captured
    }

    /// When a held key next repeats (for the caller's wait).
    pub fn next_repeat(&self) -> Option<Instant> {
        self.state.repeating.map(|(_, at)| at)
    }

    /// Read what the compositor sent, without blocking.
    pub fn poll(&mut self) -> Result<Vec<CaptureEvent>, String> {
        self.maybe_recreate_strip()?;
        self.read(Duration::ZERO)?;
        self.repeat(Instant::now());
        Ok(std::mem::take(&mut self.state.events))
    }

    /// Take the keyboard, lock the pointer and inhibit shortcuts. Fails
    /// (and holds nothing) when the compositor does not hand them over.
    pub fn grab(&mut self) -> Result<(), String> {
        let (Some(layer_surface), Some(surface)) =
            (self.state.layer_surface.clone(), self.state.surface.clone())
        else {
            return Err("no edge strip".into());
        };
        let (Some(constraints), Some(pointer)) =
            (self.state.constraints.clone(), self.state.pointer.clone())
        else {
            return Err("no pointer".into());
        };
        self.state.captured = true;
        self.state.held.clear();
        self.state.repeating = None;
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        if !self.wait_for(|s| s.keyboard_focus)? {
            self.release_all(None);
            return Err("the compositor did not give the strip the keyboard".into());
        }
        // The lock activates on the surface with the keyboard, under the
        // cursor, once a commit applies it.
        self.state.locked = false;
        self.state.lock =
            Some(constraints.lock_pointer(&surface, &pointer, None, Lifetime::Oneshot, &self.qh, ()));
        surface.commit();
        if !self.wait_for(|s| s.locked)? {
            self.release_all(None);
            return Err("the compositor did not lock the pointer (did it leave the edge?)".into());
        }
        if let (Some(mgr), Some(seat)) = (&self.state.inhibit_manager, &self.state.seat) {
            self.state.inhibitor = Some(mgr.inhibit_shortcuts(&surface, seat, &self.qh, ()));
        }
        let _ = self.conn.flush();
        // Anything from before the grab finished is not the peer's.
        self.state.events.retain(|e| !matches!(e, CaptureEvent::Input(_)));
        info!(
            shortcuts_inhibited = self.state.inhibitor.is_some(),
            "captured: keyboard and pointer lock"
        );
        Ok(())
    }

    /// Give everything back, the cursor coming back near `warp_primary`
    /// (primary space, on the shared edge).
    pub fn release(&mut self, warp_primary: (i32, i32)) {
        if !self.state.captured {
            return;
        }
        self.release_all(Some(warp_primary));
        info!("released: keyboard and pointer lock");
    }

    fn release_all(&mut self, warp_primary: Option<(i32, i32)>) {
        let surface = self.state.surface.clone();
        if let Some(lock) = self.state.lock.take() {
            if let (Some(warp), Some(surface)) = (warp_primary, &surface) {
                let (x, y) = self.hint(warp);
                lock.set_cursor_position_hint(x, y);
                surface.commit();
            }
            lock.destroy();
        }
        if let Some(inhibitor) = self.state.inhibitor.take() {
            inhibitor.destroy();
        }
        if let (Some(layer_surface), Some(surface)) = (&self.state.layer_surface, &surface) {
            layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            surface.commit();
        }
        self.state.captured = false;
        self.state.locked = false;
        self.state.held.clear();
        self.state.repeating = None;
        self.state.events.retain(|e| matches!(e, CaptureEvent::Edge(_)));
        let _ = self.conn.flush();
    }

    /// `warp` moved inside the screen, in the strip's coordinates.
    fn hint(&self, (x, y): (i32, i32)) -> (f64, f64) {
        let s = &self.state;
        let (ox, oy, x, y) = match s.side {
            Side::Right => (s.primary_w - s.surface_w, 0, x - INSET, y),
            Side::Left => (0, 0, x + INSET, y),
            Side::Top => (0, 0, x, y + INSET),
            Side::Bottom => (0, s.primary_h - s.surface_h, x, y - INSET),
        };
        ((x - ox) as f64, (y - oy) as f64)
    }

    /// Dispatch until `done` holds or [`GRAB_WAIT`] passes.
    fn wait_for(&mut self, done: impl Fn(&State) -> bool) -> Result<bool, String> {
        let until = Instant::now() + GRAB_WAIT;
        loop {
            self.read(Duration::ZERO)?;
            if done(&self.state) {
                return Ok(true);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(false);
            }
            self.read(left.min(Duration::from_millis(20)))?;
        }
    }

    /// Flush, then read and dispatch what arrives within `timeout`.
    fn read(&mut self, timeout: Duration) -> Result<(), String> {
        let _ = self.conn.flush();
        self.event_queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| format!("dispatch: {e}"))?;
        let Some(guard) = self.conn.prepare_read() else {
            // Events are queued already; dispatch them.
            return self
                .event_queue
                .dispatch_pending(&mut self.state)
                .map(|_| ())
                .map_err(|e| format!("dispatch: {e}"));
        };
        let mut pfd = libc::pollfd {
            fd: self.conn.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll on the Wayland display fd.
        let n = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as i32) };
        if n > 0 {
            guard
                .read()
                .map_err(|e| format!("wayland read: {e}"))?;
        } else {
            drop(guard);
        }
        self.event_queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| format!("dispatch: {e}"))?;
        if !self.state.configured && !self.state.captured {
            self.state.needs_recreate = true;
        }
        Ok(())
    }

    /// Emit due repeats of the held key.
    fn repeat(&mut self, now: Instant) {
        let s = &mut self.state;
        let Some((key, mut at)) = s.repeating else {
            return;
        };
        if !s.captured || s.repeat_rate <= 0 {
            s.repeating = None;
            return;
        }
        let every = Duration::from_secs(1) / s.repeat_rate as u32;
        while at <= now {
            s.events.push(CaptureEvent::Input(InputEvent::Key {
                keycode: key,
                pressed: true,
                repeat: true,
            }));
            at += every;
        }
        s.repeating = Some((key, at));
    }

    fn maybe_recreate_strip(&mut self) -> Result<(), String> {
        if !self.state.needs_recreate || self.state.captured {
            return Ok(());
        }
        let now = Instant::now();
        if self
            .state
            .last_recreate
            .is_some_and(|t| now.duration_since(t) < Duration::from_millis(500))
        {
            return Ok(());
        }
        self.state.last_recreate = Some(now);
        self.state.needs_recreate = false;
        info!("recreating the edge strip");
        create_strip(&mut self.state, &self.qh)?;
        self.event_queue
            .roundtrip(&mut self.state)
            .map_err(|e| format!("recreate: {e}"))?;
        if !self.state.configured {
            self.state.needs_recreate = true;
        }
        Ok(())
    }
}

impl State {
    fn input(&mut self, event: InputEvent) {
        if self.captured && self.locked {
            self.events.push(CaptureEvent::Input(event));
        }
    }

    fn lost(&mut self, why: &str) {
        if self.captured && !self.events.contains(&CaptureEvent::Lost) {
            warn!(why, "capture lost");
            self.events.push(CaptureEvent::Lost);
        }
    }

    fn key(&mut self, key: u32, pressed: bool) {
        if !self.captured || !self.keyboard_focus {
            return;
        }
        if matches!(key, KEY_LEFTCTRL | KEY_RIGHTCTRL | KEY_LEFTALT | KEY_RIGHTALT) {
            if pressed {
                self.held.insert(key);
            } else {
                self.held.remove(&key);
            }
        }
        let ctrl = self.mods_ctrl
            || self.held.contains(&KEY_LEFTCTRL)
            || self.held.contains(&KEY_RIGHTCTRL);
        let alt =
            self.mods_alt || self.held.contains(&KEY_LEFTALT) || self.held.contains(&KEY_RIGHTALT);
        if key == KEY_ESC && pressed && ctrl && alt {
            self.events.push(CaptureEvent::Escape);
            return;
        }
        if pressed && !NO_REPEAT.contains(&key) && self.repeat_rate > 0 {
            self.repeating = Some((key, Instant::now() + self.repeat_delay));
        } else if !pressed && self.repeating.is_some_and(|(k, _)| k == key) {
            self.repeating = None;
        }
        self.events.push(CaptureEvent::Input(InputEvent::Key {
            keycode: key,
            pressed,
            repeat: false,
        }));
    }
}

fn apply_best_output_size(state: &mut State) {
    if let Some((w, h)) = state
        .output_sizes
        .iter()
        .copied()
        .max_by_key(|(w, h)| (*w as i64) * (*h as i64))
    {
        if w > 0 && h > 0 {
            state.primary_w = w.max(state.primary_w);
            state.primary_h = h.max(state.primary_h);
        }
    }
}

fn create_strip(state: &mut State, qh: &QueueHandle<State>) -> Result<(), String> {
    destroy_strip(state);
    let compositor = state.compositor.as_ref().ok_or("no compositor")?;
    let layer_shell = state.layer_shell.as_ref().ok_or("no layer shell")?;
    let surface = compositor.create_surface(qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        state.output.as_ref(),
        Layer::Overlay,
        String::from("sola-kvm-edge"),
        qh,
        (),
    );
    // One edge, the strip × the whole edge; exclusive zone -1 keeps it on
    // the screen's edge whatever else reserves space there.
    let (w, h, anchor) = match state.side {
        Side::Right => (STRIP as u32, state.primary_h.max(1) as u32, Anchor::Right),
        Side::Left => (STRIP as u32, state.primary_h.max(1) as u32, Anchor::Left),
        Side::Top => (state.primary_w.max(1) as u32, STRIP as u32, Anchor::Top),
        Side::Bottom => (state.primary_w.max(1) as u32, STRIP as u32, Anchor::Bottom),
    };
    layer_surface.set_anchor(anchor);
    layer_surface.set_size(w, h);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    surface.set_input_region(None);
    surface.commit();
    state.surface = Some(surface);
    state.layer_surface = Some(layer_surface);
    state.configured = false;
    state.surface_w = w as i32;
    state.surface_h = h as i32;
    Ok(())
}

fn destroy_strip(state: &mut State) {
    if let Some(ls) = state.layer_surface.take() {
        ls.destroy();
    }
    if let Some(s) = state.surface.take() {
        s.destroy();
    }
    state.configured = false;
}

/// A transparent buffer the strip's size.
fn attach_strip_buffer(
    state: &State,
    surface: &wl_surface::WlSurface,
    width: i32,
    height: i32,
    qh: &QueueHandle<State>,
) {
    let Some(shm) = state.shm.as_ref() else {
        return;
    };
    let width = width.max(1);
    let height = height.max(1);
    let stride = width * 4;
    let size = (stride * height) as usize;
    let mut memfd = match rustix::fs::memfd_create(
        "sola-kvm-edge",
        rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
    ) {
        Ok(f) => f,
        Err(e) => {
            warn!(%e, "memfd_create failed");
            return;
        }
    };
    if rustix::io::write(&mut memfd, &vec![0u8; size]).is_err() {
        return;
    }
    let pool = shm.create_pool(memfd.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
    surface.attach(Some(&buffer), 0, 0);
    surface.damage(0, 0, width, height);
    surface.set_input_region(None);
    surface.commit();
    pool.destroy();
}

// --- Dispatch ---------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "wl_output" {
                let output: wl_output::WlOutput = registry.bind(name, version.min(3), qh, ());
                if state.output.is_none() {
                    state.output = Some(output);
                }
            }
        }
    }
}

macro_rules! ignore_events {
    ($($ty:ty),* $(,)?) => {$(
        impl Dispatch<$ty, ()> for State {
            fn event(_: &mut Self, _: &$ty, _: <$ty as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}

ignore_events!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    ZwlrLayerShellV1,
    ZwpPointerConstraintsV1,
    ZwpRelativePointerManagerV1,
    ZwpKeyboardShortcutsInhibitManagerV1,
);

impl Dispatch<wl_buffer::WlBuffer, ()> for State {
    fn event(
        _: &mut Self,
        proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) {
            proxy.destroy();
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            if caps.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                let pointer = seat.get_pointer(qh, ());
                if let Some(mgr) = &state.relative_manager {
                    state.relative = Some(mgr.get_relative_pointer(&pointer, qh, ()));
                }
                state.pointer = Some(pointer);
            }
            if caps.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let ours = |s: &State, surface: &wl_surface::WlSurface| s.surface.as_ref() == Some(surface);
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } if ours(state, &surface) && !state.captured => {
                let along = match state.side {
                    Side::Left | Side::Right => {
                        (surface_y as i32).clamp(0, state.surface_h.max(state.primary_h) - 1)
                    }
                    Side::Top | Side::Bottom => {
                        (surface_x as i32).clamp(0, state.surface_w.max(state.primary_w) - 1)
                    }
                };
                debug!(along, "the cursor reached the shared edge");
                state.events.push(CaptureEvent::Edge(along));
            }
            wl_pointer::Event::Button {
                button,
                state: WEnum::Value(b),
                ..
            } => {
                let button = match button {
                    BTN_LEFT => 0,
                    BTN_RIGHT => 1,
                    BTN_MIDDLE => 2,
                    _ => return,
                };
                state.input(InputEvent::Button {
                    button,
                    pressed: b == wl_pointer::ButtonState::Pressed,
                });
            }
            // On the wire scroll counts wheel detents with the kernel's sign
            // (up and right positive); Wayland's vertical axis is down.
            wl_pointer::Event::AxisValue120 {
                axis: WEnum::Value(axis),
                value120,
            } => {
                if !state.scroll_120 {
                    state.scroll = (0.0, 0.0);
                    state.scroll_120 = true;
                }
                add_scroll(state, axis, value120 as f32 / 120.0);
            }
            wl_pointer::Event::AxisDiscrete {
                axis: WEnum::Value(axis),
                discrete,
            } => {
                if !state.scroll_120 {
                    state.scroll = (0.0, 0.0);
                    state.scroll_120 = true;
                }
                add_scroll(state, axis, discrete as f32);
            }
            wl_pointer::Event::Axis {
                axis: WEnum::Value(axis),
                value,
                ..
            } if !state.scroll_120 => {
                // A touchpad or other continuous source: ~15 px a detent.
                add_scroll(state, axis, value as f32 / 15.0);
            }
            wl_pointer::Event::Frame => {
                let (dx, dy) = std::mem::take(&mut state.scroll);
                state.scroll_120 = false;
                if dx != 0.0 || dy != 0.0 {
                    state.input(InputEvent::Scroll { dx, dy });
                }
            }
            _ => {}
        }
    }
}

fn add_scroll(state: &mut State, axis: wl_pointer::Axis, detents: f32) {
    match axis {
        wl_pointer::Axis::VerticalScroll => state.scroll.1 -= detents,
        wl_pointer::Axis::HorizontalScroll => state.scroll.0 += detents,
        _ => {}
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { surface, .. } => {
                if state.surface.as_ref() == Some(&surface) {
                    state.keyboard_focus = true;
                }
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                if state.surface.as_ref() == Some(&surface) {
                    state.keyboard_focus = false;
                    state.lost("the keyboard went elsewhere");
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(k),
                ..
            } => state.key(key, k == wl_keyboard::KeyState::Pressed),
            // XKB's real modifiers have fixed bits: Control 4, Mod1 (Alt) 8.
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                state.mods_ctrl = mods_depressed & 4 != 0;
                state.mods_alt = mods_depressed & 8 != 0;
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                state.repeat_rate = rate;
                state.repeat_delay = Duration::from_millis(delay.max(0) as u64);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpRelativePointerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion {
            dx_unaccel,
            dy_unaccel,
            ..
        } = event
        {
            // Unaccelerated, as the device reports it: the layout's scale
            // is tuned for these.
            state.input(InputEvent::PointerRel {
                dx: dx_unaccel as f32,
                dy: dy_unaccel as f32,
            });
        }
    }
}

impl Dispatch<ZwpLockedPointerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpLockedPointerV1,
        event: zwp_locked_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_locked_pointer_v1::Event::Locked => state.locked = true,
            zwp_locked_pointer_v1::Event::Unlocked => {
                state.locked = false;
                state.lost("the compositor unlocked the pointer");
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => debug!("shortcuts inhibited"),
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => {
                debug!("shortcuts inhibitor inactive")
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            width,
            height,
            flags,
            ..
        } = event
        {
            let current = match flags {
                WEnum::Value(f) => f.contains(wl_output::Mode::Current),
                _ => true,
            };
            if current && width > 0 && height > 0 {
                state.output_sizes.push((width, height));
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer_surface.ack_configure(serial);
                state.configured = true;
                let w = if width == 0 { state.surface_w.max(STRIP) } else { width as i32 };
                let h = if height == 0 { state.surface_h.max(STRIP) } else { height as i32 };
                state.surface_w = w;
                state.surface_h = h;
                match state.side {
                    Side::Left | Side::Right if h > 0 => state.primary_h = h,
                    Side::Top | Side::Bottom if w > 0 => state.primary_w = w,
                    _ => {}
                }
                if let Some(surface) = state.surface.clone() {
                    attach_strip_buffer(state, &surface, w, h, qh);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                warn!("edge strip closed by the compositor; will make it again");
                state.lost("the strip was closed");
                destroy_strip(state);
                state.needs_recreate = true;
            }
            _ => {}
        }
    }
}
