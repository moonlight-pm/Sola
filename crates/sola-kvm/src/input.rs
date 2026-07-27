//! Input backends that feed [`crate::server::InputEvent`]s into the session.
//!
//! Phase C ships three sources:
//!
//! - **`feed`** — line protocol on stdin (tests / remote drive)
//! - **`demo`** — scripted enter → motion → leave sequence (smoke without HID)
//! - **`evdev`** — optional `/dev/input` read + `EVIOCGRAB` while remote (spike;
//!   needs group/`uaccess` on event nodes; documented operator path)
//!
//! Layer-shell barriers (lan-mouse style) are deferred until `sola-river`
//! exposes `river_layer_shell_v1` on this branch; see design §5.2.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::os::fd::AsRawFd;
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
/// - `key <keycode> <0|1>`
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
            let pressed: u8 = next_parse(&mut parts, "pressed")?;
            Ok(Some(InputEvent::Key {
                keycode,
                pressed: pressed != 0,
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
        },
        InputEvent::Key {
            keycode: 30,
            pressed: false,
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
    pend_scroll_dx: f32,
    pend_scroll_dy: f32,
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
                _ => {}
            },
            EV_KEY => {
                let pressed = ev.value != 0;
                // Mouse buttons vs keyboard
                if (BTN_LEFT..=BTN_MIDDLE).contains(&ev.code) {
                    let button = match ev.code {
                        BTN_LEFT => 0,
                        BTN_RIGHT => 1,
                        BTN_MIDDLE => 2,
                        _ => return,
                    };
                    self.pending.push(InputEvent::Button { button, pressed });
                } else if ev.code < 0x100 {
                    // Keyboard keys are below BTN_*; ignore repeats (value==2)
                    if ev.value == 2 {
                        return;
                    }
                    self.pending.push(InputEvent::Key {
                        keycode: ev.code as u32,
                        pressed,
                    });
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
                if self.pend_scroll_dx != 0.0 || self.pend_scroll_dy != 0.0 {
                    self.pending.push(InputEvent::Scroll {
                        dx: self.pend_scroll_dx,
                        dy: self.pend_scroll_dy,
                    });
                    self.pend_scroll_dx = 0.0;
                    self.pend_scroll_dy = 0.0;
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

/// Multi-device evdev source with grab-while-remote semantics.
pub struct EvdevSource {
    devices: Vec<EvdevDevice>,
    grabbed: bool,
}

impl EvdevSource {
    /// Open pointer-capable `/dev/input/event*` nodes that are readable.
    ///
    /// Prefers `by-id/*-event-mouse` so we do not double-count relative motion
    /// from keyboards / LED controllers (which would race the estimated cursor).
    pub fn open_all() -> io::Result<Self> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        // Prefer stable mouse nodes.
        if let Ok(entries) = std::fs::read_dir("/dev/input/by-id") {
            for ent in entries.flatten() {
                let name = ent.file_name();
                let name = name.to_string_lossy();
                if name.contains("event-mouse") {
                    if let Ok(canon) = std::fs::canonicalize(ent.path()) {
                        candidates.push(canon);
                    }
                }
            }
        }
        if candidates.is_empty() {
            if let Ok(entries) = std::fs::read_dir("/dev/input/by-path") {
                for ent in entries.flatten() {
                    let name = ent.file_name();
                    let name = name.to_string_lossy();
                    if name.contains("event-mouse") {
                        if let Ok(canon) = std::fs::canonicalize(ent.path()) {
                            candidates.push(canon);
                        }
                    }
                }
            }
        }
        // Fall back: event* that looks like a pointer in sysfs (has REL_X).
        if candidates.is_empty() {
            if let Ok(entries) = std::fs::read_dir("/dev/input") {
                for ent in entries.flatten() {
                    let name = ent.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with("event") {
                        continue;
                    }
                    let path = ent.path();
                    if sysfs_has_rel_xy(&name) {
                        candidates.push(path);
                    }
                }
            }
        }

        // De-dupe
        candidates.sort();
        candidates.dedup();

        let mut devices = Vec::new();
        for path in candidates {
            match EvdevDevice::open(&path) {
                Ok(dev) => {
                    set_nonblocking(dev.file.as_raw_fd())?;
                    info!(path = %path.display(), "evdev opened (pointer)");
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
        info!(count = devices.len(), "evdev backend ready");
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

/// Suggested poll interval when using non-blocking evdev.
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
                pressed: true
            })
        ));
        assert!(matches!(
            parse_feed_line("leave").unwrap(),
            Some(InputEvent::ForceLeave)
        ));
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
