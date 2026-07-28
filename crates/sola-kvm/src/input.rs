//! Input backends that feed [`crate::server::InputEvent`]s into the session.
//!
//! Phase C ships three sources:
//!
//! - **`feed`** — line protocol on stdin (tests / remote drive)
//! - **`demo`** — scripted enter → motion → leave sequence (smoke without HID)
//! - **`evdev`** — `/dev/input` read + `EVIOCGRAB` while remote (needs
//!   group/`uaccess` on event nodes). Opens **pointer** (`*-event-mouse`) and
//!   **keyboard** (`*-event-kbd`) nodes; opening only the mouse was a regression
//!   that left remote typing dead while cursor still moved.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing::{debug, info, warn};

use crate::server::InputEvent;

/// How the server obtains HID events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputBackendKind {
    /// Stdin line protocol (default — works without device perms).
    #[default]
    Feed,
    /// Self-driving smoke sequence then idle.
    Demo,
    /// `/dev/input/event*` relative devices + grab while remote.
    Evdev,
}

impl InputBackendKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "feed" | "stdin" => Some(Self::Feed),
            "demo" => Some(Self::Demo),
            "evdev" | "input" => Some(Self::Evdev),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Feed protocol
// ---------------------------------------------------------------------------

/// Parse one feed line into an event.
///
/// Lines (whitespace-separated):
/// - `rel <dx> <dy>`
/// - `abs <x> <y>`
/// - `btn <button> <0|1>`
/// - `key <keycode> <0|1|2>`  (`2` = auto-repeat, matching Linux `EV_KEY`)
/// - `scroll <dx> <dy>`
/// - `leave`
/// - `#` comments and blank lines → `None`
pub fn parse_feed_line(line: &str) -> Result<Option<InputEvent>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let cmd = parts.next().ok_or_else(|| "empty".to_string())?;
    match cmd {
        "rel" => {
            let dx: f32 = next_parse(&mut parts, "dx")?;
            let dy: f32 = next_parse(&mut parts, "dy")?;
            Ok(Some(InputEvent::PointerRel { dx, dy }))
        }
        "abs" => {
            let x: i32 = next_parse(&mut parts, "x")?;
            let y: i32 = next_parse(&mut parts, "y")?;
            Ok(Some(InputEvent::PointerAbs { x, y }))
        }
        "btn" => {
            let button: u8 = next_parse(&mut parts, "button")?;
            let pressed: u8 = next_parse(&mut parts, "pressed")?;
            Ok(Some(InputEvent::Button {
                button,
                pressed: pressed != 0,
            }))
        }
        "key" => {
            let keycode: u32 = next_parse(&mut parts, "keycode")?;
            let state: u8 = next_parse(&mut parts, "pressed")?;
            Ok(Some(InputEvent::Key {
                keycode,
                pressed: state != 0,
                repeat: state == 2,
            }))
        }
        "scroll" => {
            let dx: f32 = next_parse(&mut parts, "dx")?;
            let dy: f32 = next_parse(&mut parts, "dy")?;
            Ok(Some(InputEvent::Scroll { dx, dy }))
        }
        "leave" => Ok(Some(InputEvent::ForceLeave)),
        other => Err(format!("unknown feed command: {other}")),
    }
}

fn next_parse<T: std::str::FromStr>(
    parts: &mut std::str::SplitWhitespace<'_>,
    name: &str,
) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    let s = parts
        .next()
        .ok_or_else(|| format!("missing feed arg: {name}"))?;
    s.parse()
        .map_err(|e| format!("bad feed arg {name}={s}: {e}"))
}

/// Blocking stdin feed: yields events until EOF.
pub struct FeedSource<R: BufRead> {
    reader: R,
}

impl FeedSource<BufReader<io::Stdin>> {
    pub fn stdin() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
        }
    }
}

impl<R: BufRead> FeedSource<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Next event, or `None` on EOF.
    pub fn next_event(&mut self) -> io::Result<Option<InputEvent>> {
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Ok(None);
            }
            match parse_feed_line(&line) {
                Ok(Some(ev)) => return Ok(Some(ev)),
                Ok(None) => continue,
                Err(e) => {
                    warn!(%e, line = %line.trim(), "feed parse error; skipping");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Demo sequence
// ---------------------------------------------------------------------------

/// Scripted smoke events for `server --input demo`.
pub fn demo_events() -> Vec<InputEvent> {
    vec![
        // Start near right edge (seed abs), then push through enter_push barrier.
        InputEvent::PointerAbs { x: 5119, y: 2000 },
        InputEvent::PointerRel { dx: 50.0, dy: 0.0 }, // enter (enter_push default 48)
        InputEvent::PointerRel { dx: 40.0, dy: 10.0 },
        InputEvent::PointerRel { dx: 20.0, dy: -5.0 },
        InputEvent::Button {
            button: 0,
            pressed: true,
        },
        InputEvent::Button {
            button: 0,
            pressed: false,
        },
        InputEvent::Key {
            keycode: 30, // KEY_A
            pressed: true,
            repeat: false,
        },
        InputEvent::Key {
            keycode: 30,
            pressed: false,
            repeat: false,
        },
        InputEvent::Scroll {
            dx: 0.0,
            dy: -1.0,
        },
        // Leave back toward primary (leftward off Mac left edge).
        InputEvent::PointerRel {
            dx: -200.0,
            dy: 0.0,
        },
    ]
}

// ---------------------------------------------------------------------------
// Evdev spike (optional)
// ---------------------------------------------------------------------------

/// Linux `input_event` layout (matches kernel uapi; 24 bytes on 64-bit with
/// timeval as two longs / two i64 on modern glibc).
///
/// We use the 64-bit layout: `timeval { sec: i64, usec: i64 }` + type/code/value.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawInputEvent {
    sec: i64,
    usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_SYN: u16 = 0x00;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;
const REL_HWHEEL: u16 = 0x06;
/// High-res wheel (1/120 of a detent). Prefer over REL_WHEEL when present.
const REL_WHEEL_HI_RES: u16 = 0x0b;
const REL_HWHEEL_HI_RES: u16 = 0x0c;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
// _IOW('E', 0x90, int) on Linux: dir=WRITE(1), type='E', nr=0x90, size=4
const EVIOCGRAB: u64 = 0x4004_4590;

#[cfg(target_os = "linux")]
mod linux_ioctl {
    use std::os::fd::RawFd;

    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }

    pub fn eviocgrab(fd: RawFd, grab: i32) -> std::io::Result<()> {
        // SAFETY: EVIOCGRAB is the standard evdev grab ioctl; fd is open.
        let rc = unsafe { ioctl(fd, super::EVIOCGRAB, grab) };
        if rc < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// One open event node.
struct EvdevDevice {
    path: PathBuf,
    file: File,
    /// Pending REL deltas between SYN reports.
    pend_dx: f32,
    pend_dy: f32,
    /// Discrete wheel detents (REL_WHEEL / REL_HWHEEL).
    pend_scroll_dx: f32,
    pend_scroll_dy: f32,
    /// High-res wheel in 1/120 detent units (REL_*_HI_RES).
    pend_scroll_hi_dx: f32,
    pend_scroll_hi_dy: f32,
    /// Accumulated events since last SYN.
    pending: Vec<InputEvent>,
}

impl EvdevDevice {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::options().read(true).write(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            pend_dx: 0.0,
            pend_dy: 0.0,
            pend_scroll_dx: 0.0,
            pend_scroll_dy: 0.0,
            pend_scroll_hi_dx: 0.0,
            pend_scroll_hi_dy: 0.0,
            pending: Vec::new(),
        })
    }

    fn set_grab(&self, grab: bool) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            let fd = self.file.as_raw_fd();
            linux_ioctl::eviocgrab(fd, if grab { 1 } else { 0 })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = grab;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "evdev grab only on Linux",
            ))
        }
    }

    /// Non-blocking-ish read: set a short timeout via poll would be better;
    /// for the spike we read one raw event if available (caller sets O_NONBLOCK).
    fn pump(&mut self) -> io::Result<Vec<InputEvent>> {
        let mut out = Vec::new();
        loop {
            let mut buf = [0u8; std::mem::size_of::<RawInputEvent>()];
            match self.file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) if n < buf.len() => {
                    // Short read — treat as no complete event.
                    break;
                }
                Ok(_) => {
                    let ev = raw_from_bytes(&buf);
                    self.handle_raw(ev, &mut out);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    fn handle_raw(&mut self, ev: RawInputEvent, out: &mut Vec<InputEvent>) {
        match ev.type_ {
            EV_REL => match ev.code {
                REL_X => self.pend_dx += ev.value as f32,
                REL_Y => self.pend_dy += ev.value as f32,
                REL_WHEEL => self.pend_scroll_dy += ev.value as f32,
                REL_HWHEEL => self.pend_scroll_dx += ev.value as f32,
                // Do not also add discrete WHEEL when HI_RES is present — both
                // are emitted for the same physical motion on modern mice.
                REL_WHEEL_HI_RES => self.pend_scroll_hi_dy += ev.value as f32,
                REL_HWHEEL_HI_RES => self.pend_scroll_hi_dx += ev.value as f32,
                _ => {}
            },
            EV_KEY => {
                // Mouse buttons vs keyboard
                if (BTN_LEFT..=BTN_MIDDLE).contains(&ev.code) {
                    // Buttons: 0=up, 1=down; ignore value==2 if a device ever
                    // emits it (kernel rarely does for BTN_*).
                    if ev.value == 2 {
                        return;
                    }
                    let pressed = ev.value != 0;
                    let button = match ev.code {
                        BTN_LEFT => 0,
                        BTN_RIGHT => 1,
                        BTN_MIDDLE => 2,
                        _ => return,
                    };
                    self.pending.push(InputEvent::Button { button, pressed });
                } else if ev.code < 0x100 {
                    // Keyboard keys are below BTN_*.
                    // Linux EV_KEY: 0=release, 1=press, 2=auto-repeat.
                    // Forward repeats so Mac inject can set kCGKeyboardEventAutorepeat
                    // (otherwise holding a key only types once).
                    match ev.value {
                        0 => self.pending.push(InputEvent::Key {
                            keycode: ev.code as u32,
                            pressed: false,
                            repeat: false,
                        }),
                        1 => self.pending.push(InputEvent::Key {
                            keycode: ev.code as u32,
                            pressed: true,
                            repeat: false,
                        }),
                        2 => self.pending.push(InputEvent::Key {
                            keycode: ev.code as u32,
                            pressed: true,
                            repeat: true,
                        }),
                        _ => {}
                    }
                }
            }
            EV_SYN => {
                if self.pend_dx != 0.0 || self.pend_dy != 0.0 {
                    self.pending.push(InputEvent::PointerRel {
                        dx: self.pend_dx,
                        dy: self.pend_dy,
                    });
                    self.pend_dx = 0.0;
                    self.pend_dy = 0.0;
                }
                // Prefer HI_RES (÷120 → detent units) when the device sends it.
                let (sdx, sdy) = if self.pend_scroll_hi_dx != 0.0
                    || self.pend_scroll_hi_dy != 0.0
                {
                    (
                        self.pend_scroll_hi_dx / 120.0,
                        self.pend_scroll_hi_dy / 120.0,
                    )
                } else {
                    (self.pend_scroll_dx, self.pend_scroll_dy)
                };
                self.pend_scroll_dx = 0.0;
                self.pend_scroll_dy = 0.0;
                self.pend_scroll_hi_dx = 0.0;
                self.pend_scroll_hi_dy = 0.0;
                if sdx != 0.0 || sdy != 0.0 {
                    self.pending.push(InputEvent::Scroll { dx: sdx, dy: sdy });
                }
                out.append(&mut self.pending);
            }
            _ => {}
        }
    }
}

fn raw_from_bytes(buf: &[u8]) -> RawInputEvent {
    // Little-endian host (Linux x86_64 / aarch64 LE).
    let sec = i64::from_le_bytes(buf[0..8].try_into().unwrap());
    let usec = i64::from_le_bytes(buf[8..16].try_into().unwrap());
    let type_ = u16::from_le_bytes(buf[16..18].try_into().unwrap());
    let code = u16::from_le_bytes(buf[18..20].try_into().unwrap());
    let value = i32::from_le_bytes(buf[20..24].try_into().unwrap());
    RawInputEvent {
        sec,
        usec,
        type_,
        code,
        value,
    }
}

/// Role of an opened evdev node (for logging / selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvdevRole {
    Pointer,
    Keyboard,
}

/// Multi-device evdev source with grab-while-remote semantics.
pub struct EvdevSource {
    devices: Vec<EvdevDevice>,
    grabbed: bool,
}

impl EvdevSource {
    /// Open pointer **and** keyboard `/dev/input/event*` nodes that are readable.
    ///
    /// Pointer selection prefers `by-id/*-event-mouse` so we do not double-count
    /// relative motion from keyboards / LED / consumer-control interfaces (which
    /// would race the estimated cursor).
    ///
    /// Keyboard selection prefers `by-id/*-event-kbd` (and the same under
    /// `by-path`). Without keyboards, remote mode only moves the Mac cursor —
    /// keys never leave the local seat.
    pub fn open_all() -> io::Result<Self> {
        let mut candidates: Vec<(PathBuf, EvdevRole)> = Vec::new();

        collect_by_symlink_role("/dev/input/by-id", &mut candidates);
        // Fill missing roles from by-path (some seats only expose one tree).
        {
            let have_ptr = candidates.iter().any(|(_, r)| *r == EvdevRole::Pointer);
            let have_kbd = candidates.iter().any(|(_, r)| *r == EvdevRole::Keyboard);
            if !have_ptr || !have_kbd {
                collect_by_symlink_role("/dev/input/by-path", &mut candidates);
            }
        }

        // Fall back: sysfs capability probes when symlink names are absent.
        let have_ptr = candidates.iter().any(|(_, r)| *r == EvdevRole::Pointer);
        let have_kbd = candidates.iter().any(|(_, r)| *r == EvdevRole::Keyboard);
        if !have_ptr || !have_kbd {
            if let Ok(entries) = std::fs::read_dir("/dev/input") {
                for ent in entries.flatten() {
                    let name = ent.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with("event") {
                        continue;
                    }
                    let path = ent.path();
                    if !have_ptr && sysfs_has_rel_xy(&name) {
                        candidates.push((path.clone(), EvdevRole::Pointer));
                    }
                    // Real keyboards report KEY_A; skip pure mouse button pads
                    // (REL_X+Y) and consumer/hotkey devices without letter keys.
                    if !have_kbd && sysfs_has_key_a(&name) && !sysfs_has_rel_xy(&name) {
                        candidates.push((path, EvdevRole::Keyboard));
                    }
                }
            }
        }

        // De-dupe by path (prefer first role assigned — mouse vs kbd names differ).
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        candidates.dedup_by(|a, b| a.0 == b.0);

        let mut devices = Vec::new();
        let mut n_ptr = 0usize;
        let mut n_kbd = 0usize;
        for (path, role) in candidates {
            match EvdevDevice::open(&path) {
                Ok(dev) => {
                    set_nonblocking(dev.file.as_raw_fd())?;
                    match role {
                        EvdevRole::Pointer => {
                            n_ptr += 1;
                            info!(path = %path.display(), "evdev opened (pointer)");
                        }
                        EvdevRole::Keyboard => {
                            n_kbd += 1;
                            info!(path = %path.display(), "evdev opened (keyboard)");
                        }
                    }
                    devices.push(dev);
                }
                Err(e) => {
                    debug!(path = %path.display(), %e, "evdev open skipped");
                }
            }
        }
        if devices.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "no readable pointer /dev/input/event* nodes (need input group or uaccess)",
            ));
        }
        if n_kbd == 0 {
            warn!(
                "evdev: no keyboard nodes opened — remote typing will not work \
                 (look for by-id/*-event-kbd or grant access to keyboard event nodes)"
            );
        }
        info!(
            count = devices.len(),
            pointers = n_ptr,
            keyboards = n_kbd,
            "evdev backend ready"
        );
        Ok(Self {
            devices,
            grabbed: false,
        })
    }

    pub fn set_grabbed(&mut self, grab: bool) {
        if grab == self.grabbed {
            return;
        }
        for dev in &self.devices {
            if let Err(e) = dev.set_grab(grab) {
                warn!(path = %dev.path.display(), %e, grab, "EVIOCGRAB failed");
            } else {
                debug!(path = %dev.path.display(), grab, "EVIOCGRAB ok");
            }
        }
        self.grabbed = grab;
        if grab {
            info!("evdev exclusive grab ON (remote)");
        } else {
            info!("evdev exclusive grab OFF (local)");
        }
    }

    /// Poll all devices; return any complete events.
    pub fn poll(&mut self) -> io::Result<Vec<InputEvent>> {
        let mut out = Vec::new();
        for dev in &mut self.devices {
            match dev.pump() {
                Ok(mut v) => out.append(&mut v),
                Err(e) => warn!(path = %dev.path.display(), %e, "evdev read error"),
            }
        }
        Ok(out)
    }

    /// Block until any device (or optional extra fd) is readable, or `timeout`.
    ///
    /// Prefer this over a blind `sleep` so motion/keys wake the loop immediately
    /// instead of adding up to `EVDEV_POLL` of artificial latency.
    pub fn wait_readable(&self, extra_fds: &[RawFd], timeout: Duration) {
        let mut pfds: Vec<libc::pollfd> = self
            .devices
            .iter()
            .map(|d| libc::pollfd {
                fd: d.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        for &fd in extra_fds {
            pfds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        if pfds.is_empty() {
            std::thread::sleep(timeout);
            return;
        }
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: poll on open device/wayland fds; timeout is non-negative.
        let _ = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, ms) };
    }
}

/// Collapse consecutive relative-motion (and scroll) events so one poll batch
/// does not spray N near-identical UDP Motion packets. Key/button order is
/// preserved relative to motion runs.
pub fn coalesce_input_events(events: Vec<InputEvent>) -> Vec<InputEvent> {
    if events.len() <= 1 {
        return events;
    }
    let mut out: Vec<InputEvent> = Vec::with_capacity(events.len());
    let mut acc_dx = 0.0f32;
    let mut acc_dy = 0.0f32;
    let mut has_rel = false;
    let mut acc_sdx = 0.0f32;
    let mut acc_sdy = 0.0f32;
    let mut has_scroll = false;

    let flush_rel =
        |out: &mut Vec<InputEvent>, dx: &mut f32, dy: &mut f32, has: &mut bool| {
            if *has {
                out.push(InputEvent::PointerRel {
                    dx: *dx,
                    dy: *dy,
                });
                *dx = 0.0;
                *dy = 0.0;
                *has = false;
            }
        };
    let flush_scroll =
        |out: &mut Vec<InputEvent>, dx: &mut f32, dy: &mut f32, has: &mut bool| {
            if *has {
                out.push(InputEvent::Scroll {
                    dx: *dx,
                    dy: *dy,
                });
                *dx = 0.0;
                *dy = 0.0;
                *has = false;
            }
        };

    for ev in events {
        match ev {
            InputEvent::PointerRel { dx, dy } => {
                // Keep motion and scroll as separate streams but flush the
                // other when switching, so order stays intuitive.
                flush_scroll(&mut out, &mut acc_sdx, &mut acc_sdy, &mut has_scroll);
                has_rel = true;
                acc_dx += dx;
                acc_dy += dy;
            }
            InputEvent::Scroll { dx, dy } => {
                flush_rel(&mut out, &mut acc_dx, &mut acc_dy, &mut has_rel);
                has_scroll = true;
                acc_sdx += dx;
                acc_sdy += dy;
            }
            other => {
                flush_rel(&mut out, &mut acc_dx, &mut acc_dy, &mut has_rel);
                flush_scroll(&mut out, &mut acc_sdx, &mut acc_sdy, &mut has_scroll);
                out.push(other);
            }
        }
    }
    flush_rel(&mut out, &mut acc_dx, &mut acc_dy, &mut has_rel);
    flush_scroll(&mut out, &mut acc_sdx, &mut acc_sdy, &mut has_scroll);
    out
}

/// Collect `*-event-mouse` / `*-event-kbd` symlinks under `dir` (by-id or by-path).
fn collect_by_symlink_role(dir: &str, out: &mut Vec<(PathBuf, EvdevRole)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        let role = if name.contains("event-mouse") {
            EvdevRole::Pointer
        } else if name.contains("event-kbd") {
            EvdevRole::Keyboard
        } else {
            continue;
        };
        if let Ok(canon) = std::fs::canonicalize(ent.path()) {
            out.push((canon, role));
        }
    }
}

/// True if `/sys/class/input/<eventN>/device/capabilities/rel` has REL_X+REL_Y bits.
fn sysfs_has_rel_xy(event_name: &str) -> bool {
    let path = format!("/sys/class/input/{event_name}/device/capabilities/rel");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    // Bitmask words, least-significant first. REL_X=0, REL_Y=1 → need low bits 0b11.
    let Some(first) = text.split_whitespace().next() else {
        return false;
    };
    let Ok(word) = u64::from_str_radix(first.trim_start_matches("0x"), 16) else {
        return false;
    };
    (word & 0b11) == 0b11
}

/// True if the device reports `KEY_A` (evdev 30) in its key capability bitmask.
///
/// Used to distinguish full keyboards from button-only / power / LED nodes.
fn sysfs_has_key_a(event_name: &str) -> bool {
    let path = format!("/sys/class/input/{event_name}/device/capabilities/key");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    // Kernel prints capability bitmasks as space-separated 64-bit words,
    // least-significant word first. KEY_A = 30 → bit 30 in word 0.
    let Some(first) = text.split_whitespace().next() else {
        return false;
    };
    let Ok(word) = u64::from_str_radix(first.trim_start_matches("0x"), 16) else {
        return false;
    };
    (word & (1u64 << 30)) != 0
}

fn set_nonblocking(fd: i32) -> io::Result<()> {
    // fcntl F_GETFL / F_SETFL O_NONBLOCK without extra crates.
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0o4000; // Linux
    // SAFETY: fcntl on an open fd with GETFL/SETFL is well-defined.
    unsafe {
        let flags = fcntl(fd, F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        let rc = fcntl(fd, F_SETFL, flags | O_NONBLOCK);
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Max idle wait when using non-blocking evdev + `wait_readable`.
///
/// Wakes immediately when a device (or barrier fd) is readable; this is only
/// the upper bound when the desk is idle.
pub const EVDEV_POLL: Duration = Duration::from_millis(2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rel() {
        let ev = parse_feed_line("rel 1.5 -2").unwrap().unwrap();
        assert_eq!(
            ev,
            InputEvent::PointerRel {
                dx: 1.5,
                dy: -2.0
            }
        );
    }

    #[test]
    fn parse_key_and_leave() {
        assert!(matches!(
            parse_feed_line("key 30 1").unwrap(),
            Some(InputEvent::Key {
                keycode: 30,
                pressed: true,
                repeat: false,
            })
        ));
        assert!(matches!(
            parse_feed_line("key 30 2").unwrap(),
            Some(InputEvent::Key {
                keycode: 30,
                pressed: true,
                repeat: true,
            })
        ));
        assert!(matches!(
            parse_feed_line("leave").unwrap(),
            Some(InputEvent::ForceLeave)
        ));
    }

    #[test]
    fn coalesce_sums_rel_and_keeps_keys() {
        let events = vec![
            InputEvent::PointerRel { dx: 1.0, dy: 0.0 },
            InputEvent::PointerRel { dx: 2.0, dy: 3.0 },
            InputEvent::Key {
                keycode: 30,
                pressed: true,
                repeat: false,
            },
            InputEvent::PointerRel { dx: 4.0, dy: 0.0 },
            InputEvent::PointerRel { dx: 1.0, dy: 1.0 },
        ];
        let out = coalesce_input_events(events);
        assert_eq!(
            out,
            vec![
                InputEvent::PointerRel { dx: 3.0, dy: 3.0 },
                InputEvent::Key {
                    keycode: 30,
                    pressed: true,
                    repeat: false,
                },
                InputEvent::PointerRel { dx: 5.0, dy: 1.0 },
            ]
        );
    }

    #[test]
    fn parse_comments() {
        assert!(parse_feed_line("# hi").unwrap().is_none());
        assert!(parse_feed_line("").unwrap().is_none());
    }

    #[test]
    fn demo_enters_and_leaves() {
        // Smoke: demo sequence should be non-empty and contain a force path
        // or enough leftward motion — actual enter tested in server tests.
        assert!(demo_events().len() >= 5);
    }
}
