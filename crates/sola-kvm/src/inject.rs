//! Linux client inject: Wayland virtual pointer + virtual keyboard.
//!
//! `sola-kvm listen` on a River/Sola seat (canto, or any Linux peer) binds
//! `zwlr_virtual_pointer_v1` and `zwp_virtual_keyboard_v1` and turns KVM1
//! UDP packets into compositor input. Mac inject stays in `apps/sola-kvm-mac`.

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rustix::fs::{MemfdFlags, memfd_create};
use tracing::{debug, info, warn};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_output,
    wl_pointer::{Axis, AxisSource, ButtonState},
    wl_registry, wl_seat,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::{self, ZwlrVirtualPointerV1},
};
use xkbcommon::xkb::{self, KeyDirection, Keycode};

use crate::clip::{self, ClipConfig, ClipHandle};
use crate::protocol::Packet;
use crate::udp::{Listener, UdpError};

/// Linux `BTN_LEFT` / `BTN_RIGHT` / `BTN_MIDDLE`.
pub fn linux_button(button: u8) -> u32 {
    match button {
        1 => 0x111,
        2 => 0x112,
        _ => 0x110,
    }
}

/// XKB keycode is evdev + 8.
fn xkb_keycode(evdev: u32) -> Keycode {
    Keycode::new(evdev.saturating_add(8))
}

fn now_msec() -> u32 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u32
}

fn ensure_xkb_root() {
    if std::env::var_os("XKB_CONFIG_ROOT").is_some() {
        return;
    }
    for p in ["/oath/store/pkg/river/share/X11/xkb", "/usr/share/X11/xkb"] {
        if Path::new(p).is_dir() {
            unsafe {
                std::env::set_var("XKB_CONFIG_ROOT", p);
            }
            return;
        }
    }
}

struct InjectState {
    seat: Option<wl_seat::WlSeat>,
    output: Option<wl_output::WlOutput>,
    output_w: i32,
    output_h: i32,
    pointer_mgr: Option<ZwlrVirtualPointerManagerV1>,
    keyboard_mgr: Option<ZwpVirtualKeyboardManagerV1>,
}

pub struct WaylandInjector {
    conn: Connection,
    qh: QueueHandle<InjectState>,
    event_queue: EventQueue<InjectState>,
    state: InjectState,
    pointer: ZwlrVirtualPointerV1,
    keyboard: ZwpVirtualKeyboardV1,
    /// Keep the keymap memfd alive for the compositor mmap.
    _keymap_fd: File,
    xkb: xkb::State,
    extent_w: u32,
    extent_h: u32,
    pressed_keys: Vec<u32>,
    pressed_buttons: Vec<u8>,
}

impl WaylandInjector {
    pub fn connect(fallback_w: i32, fallback_h: i32) -> Result<Self, String> {
        sola_core::env::activate_wayland_session(30_000);
        ensure_xkb_root();

        let conn = Connection::connect_to_env()
            .map_err(|e| format!("wayland connect: {e} (WAYLAND_DISPLAY set? River up?)"))?;
        let (globals, mut event_queue) =
            registry_queue_init::<InjectState>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = event_queue.handle();

        let mut state = InjectState {
            seat: None,
            output: None,
            output_w: 0,
            output_h: 0,
            pointer_mgr: None,
            keyboard_mgr: None,
        };

        state.seat = Some(
            globals
                .bind(&qh, 1..=8, ())
                .map_err(|e| format!("wl_seat: {e}"))?,
        );
        if let Ok(output) = globals.bind::<wl_output::WlOutput, _, _>(&qh, 1..=4, ()) {
            state.output = Some(output);
        }
        state.pointer_mgr = Some(globals.bind(&qh, 1..=2, ()).map_err(|e| {
            format!("zwlr_virtual_pointer_manager_v1: {e} (River must advertise virtual pointer)")
        })?);
        state.keyboard_mgr = Some(globals.bind(&qh, 1..=1, ()).map_err(|e| {
            format!("zwp_virtual_keyboard_manager_v1: {e} (River must advertise virtual keyboard)")
        })?);

        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip: {e}"))?;

        let Some(seat) = state.seat.clone() else {
            return Err("no wl_seat".into());
        };
        let Some(pointer_mgr) = state.pointer_mgr.clone() else {
            return Err("no virtual pointer manager".into());
        };
        let Some(keyboard_mgr) = state.keyboard_mgr.clone() else {
            return Err("no virtual keyboard manager".into());
        };

        let pointer = match (pointer_mgr.version() >= 2, state.output.as_ref()) {
            (true, Some(output)) => {
                pointer_mgr.create_virtual_pointer_with_output(Some(&seat), Some(output), &qh, ())
            }
            _ => pointer_mgr.create_virtual_pointer(Some(&seat), &qh, ()),
        };
        let keyboard = keyboard_mgr.create_virtual_keyboard(&seat, &qh, ());

        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap =
            xkb::Keymap::new_from_names(&ctx, "", "", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)
                .ok_or_else(|| {
                    "xkb keymap compile failed (XKB_CONFIG_ROOT / xkeyboard-config missing?)"
                        .to_string()
                })?;
        let text = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        if text.is_empty() {
            return Err("xkb keymap string empty".into());
        }
        let keymap_fd = write_memfd("kvm-keymap", text.as_bytes())?;
        keyboard.keymap(1, keymap_fd.as_fd(), text.len() as u32);

        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("keymap roundtrip: {e}"))?;

        let extent_w = if state.output_w > 0 {
            state.output_w as u32
        } else {
            fallback_w.max(1) as u32
        };
        let extent_h = if state.output_h > 0 {
            state.output_h as u32
        } else {
            fallback_h.max(1) as u32
        };

        info!(
            extent_w,
            extent_h, "linux kvm client: virtual pointer + keyboard ready"
        );

        Ok(Self {
            conn,
            qh,
            event_queue,
            state,
            pointer,
            keyboard,
            _keymap_fd: keymap_fd,
            xkb: xkb::State::new(&keymap),
            extent_w,
            extent_h,
            pressed_keys: Vec::new(),
            pressed_buttons: Vec::new(),
        })
    }

    pub fn dispatch(&mut self) {
        let _ = self.event_queue.dispatch_pending(&mut self.state);
        let _ = self.conn.flush();
    }

    pub fn handle(&mut self, packet: &Packet) {
        match packet {
            Packet::Enter { x, y, edge } => {
                info!(x, y, ?edge, "enter → warp");
                self.release_all();
                self.warp(*x, *y);
            }
            Packet::Leave => {
                debug!("leave");
                self.release_all();
            }
            Packet::Motion { x, y } => self.warp(*x, *y),
            Packet::Button { button, pressed } => self.button(*button, *pressed != 0),
            Packet::Key { keycode, pressed } => match *pressed {
                0 => self.key(*keycode, false),
                _ => self.key(*keycode, true),
            },
            Packet::Scroll { dx, dy } => self.scroll(*dx, *dy),
            Packet::Modifiers { mask } => {
                debug!(mask, "modifiers packet (keys carry state)");
            }
        }
        self.dispatch();
    }

    fn warp(&self, x: i32, y: i32) {
        let x = x.clamp(0, self.extent_w.saturating_sub(1) as i32) as u32;
        let y = y.clamp(0, self.extent_h.saturating_sub(1) as i32) as u32;
        self.pointer
            .motion_absolute(now_msec(), x, y, self.extent_w, self.extent_h);
        self.pointer.frame();
    }

    fn button(&mut self, button: u8, pressed: bool) {
        let code = linux_button(button);
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        if pressed {
            if !self.pressed_buttons.contains(&button) {
                self.pressed_buttons.push(button);
            }
        } else {
            self.pressed_buttons.retain(|b| *b != button);
        }
        self.pointer.button(now_msec(), code, state);
        self.pointer.frame();
    }

    fn key(&mut self, evdev: u32, pressed: bool) {
        if pressed {
            if !self.pressed_keys.contains(&evdev) {
                self.pressed_keys.push(evdev);
            }
        } else {
            self.pressed_keys.retain(|k| *k != evdev);
        }
        let dir = if pressed {
            KeyDirection::Down
        } else {
            KeyDirection::Up
        };
        self.xkb.update_key(xkb_keycode(evdev), dir);
        let depressed = self.xkb.serialize_mods(xkb::STATE_MODS_DEPRESSED);
        let latched = self.xkb.serialize_mods(xkb::STATE_MODS_LATCHED);
        let locked = self.xkb.serialize_mods(xkb::STATE_MODS_LOCKED);
        let group = self.xkb.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
        let key_state: u32 = if pressed { 1 } else { 0 };
        // Physical libinput keyboards emit the key *before* xkb updates
        // modifiers (`wlr_keyboard_notify_key`). River matches registered
        // Super_L (modifiers=0) on that press; Super-up then sends
        // ChordReleased, which confirms the app switcher. Sending
        // modifiers() first leaves Super already depressed, so Super_L
        // never matches and the switcher stays up after Super+Tab.
        self.keyboard.key(now_msec(), evdev, key_state);
        self.keyboard.modifiers(depressed, latched, locked, group);
    }

    fn scroll(&self, dx: f32, dy: f32) {
        let time = now_msec();
        self.pointer.axis_source(AxisSource::Wheel);
        if dy.abs() > f32::EPSILON {
            let steps = dy.round() as i32;
            let value = (dy * 10.0) as f64;
            if steps != 0 {
                self.pointer
                    .axis_discrete(time, Axis::VerticalScroll, value, steps);
            } else {
                self.pointer.axis(time, Axis::VerticalScroll, value);
            }
        }
        if dx.abs() > f32::EPSILON {
            let steps = dx.round() as i32;
            let value = (dx * 10.0) as f64;
            if steps != 0 {
                self.pointer
                    .axis_discrete(time, Axis::HorizontalScroll, value, steps);
            } else {
                self.pointer.axis(time, Axis::HorizontalScroll, value);
            }
        }
        self.pointer.frame();
    }

    fn release_all(&mut self) {
        let keys: Vec<u32> = self.pressed_keys.clone();
        for k in keys {
            self.key(k, false);
        }
        let buttons: Vec<u8> = self.pressed_buttons.clone();
        for b in buttons {
            self.button(b, false);
        }
    }
}

fn write_memfd(name: &str, bytes: &[u8]) -> Result<File, String> {
    let fd = memfd_create(name, MemfdFlags::CLOEXEC).map_err(|e| format!("memfd: {e}"))?;
    let mut file = File::from(fd);
    file.write_all(bytes)
        .map_err(|e| format!("keymap write: {e}"))?;
    file.flush().map_err(|e| format!("keymap flush: {e}"))?;
    Ok(file)
}

/// Bind UDP and inject until the process is killed.
pub fn run_listen(
    bind: &str,
    fallback_w: i32,
    fallback_h: i32,
    clip_cfg: Option<ClipConfig>,
) -> Result<(), String> {
    let listener = Listener::bind(bind).map_err(|e| format!("bind {bind}: {e}"))?;
    listener
        .set_read_timeout(Some(Duration::from_millis(16)))
        .map_err(|e| format!("udp timeout: {e}"))?;
    let local = listener.local_addr().ok();
    info!(?local, "listening for sola-kvm UDP (linux inject)");

    let mut inj = WaylandInjector::connect(fallback_w, fallback_h)?;
    let clip: Option<ClipHandle> = match clip_cfg {
        Some(cfg) => match clip::spawn_listen(bind, cfg) {
            Ok(h) => {
                info!("clipboard TCP listening (sync on Leave → novus; Enter applies inbound)");
                Some(h)
            }
            Err(e) => {
                warn!(%e, "clipboard TCP listen failed — clip sync disabled");
                None
            }
        },
        None => None,
    };
    loop {
        match listener.recv() {
            Ok((src, seq, packet)) => {
                debug!(%src, seq, ?packet, "recv");
                let leave = matches!(packet, Packet::Leave);
                inj.handle(&packet);
                if leave {
                    if let Some(c) = &clip {
                        c.push_to_peer();
                    }
                }
            }
            Err(UdpError::Io(e))
                if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock =>
            {
                inj.dispatch();
            }
            Err(e) => {
                warn!("recv: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Dump packets to the log (no inject). Debug stand-in.
pub fn run_dump(bind: &str) -> Result<(), String> {
    let listener = Listener::bind(bind).map_err(|e| format!("bind {bind}: {e}"))?;
    let local = listener.local_addr().ok();
    info!(?local, "listening for sola-kvm UDP packets (dump)");
    loop {
        match listener.recv() {
            Ok((src, seq, packet)) => info!(%src, seq, ?packet, "recv"),
            Err(e) => {
                warn!("recv: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for InjectState {
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
            if interface == "wl_output" && state.output.is_none() {
                state.output = Some(registry.bind(name, version.min(4), qh, ()));
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for InjectState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for InjectState {
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
                state.output_w = width;
                state.output_h = height;
            }
        }
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for InjectState {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for InjectState {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: zwlr_virtual_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for InjectState {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardManagerV1,
        _: wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for InjectState {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardV1,
        _: wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::linux_button;

    #[test]
    fn buttons_are_linux_evdev() {
        assert_eq!(linux_button(0), 0x110);
        assert_eq!(linux_button(1), 0x111);
        assert_eq!(linux_button(2), 0x112);
        assert_eq!(linux_button(9), 0x110);
    }
}
