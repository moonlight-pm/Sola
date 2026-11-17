//! The picture source: wlr-screencopy of the output into shared memory,
//! with the pointer drawn in, into the encoder. After the first frame each
//! copy waits for the screen to change, so a still screen sends nothing;
//! frames come no faster than [`FPS`]. Its own thread and Wayland
//! connection; dropping [`Running`] stops it.

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{info, warn};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use super::encode::{Encoder, Frame};
use super::flow::now_us;
use super::rate::CEILING_KBPS;
use super::server::{Encoded, Feed, Msg};
use crate::view_wire::Down;

pub const FPS: u32 = 60;
const FRAME: Duration = Duration::from_micros(1_000_000 / FPS as u64);
const FRAME_US: u32 = 1_000_000 / FPS;
/// A key picture this often, as the Mac sends them.
const KEY_EVERY: Duration = Duration::from_secs(10);
/// Copies the compositor fails in a row before the capture gives up.
const FAILURES: u32 = 3;

/// Why there is no picture: `Lasting` until something here changes (the
/// viewer is told and does not try again), `ForNow` it tries again.
pub enum Failed {
    Lasting(String),
    ForNow(String),
}

/// Stops the capture when dropped.
pub struct Running {
    stop: Arc<Stop>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.set();
    }
}

/// The capture's stop: a flag, and an eventfd that wakes its waits.
struct Stop {
    set: AtomicBool,
    fd: OwnedFd,
}

impl Stop {
    fn new() -> Result<Self, String> {
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(format!("eventfd: {}", std::io::Error::last_os_error()));
        }
        Ok(Self {
            set: AtomicBool::new(false),
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    fn set(&self) {
        self.set.store(true, Ordering::Relaxed);
        let one = 1u64;
        unsafe { libc::write(self.fd.as_raw_fd(), (&raw const one).cast(), 8) };
    }

    fn is_set(&self) -> bool {
        self.set.load(Ordering::Relaxed)
    }
}

/// Start capturing the output into `feed`: the screen it sends, and the
/// capture running.
pub fn start(feed: Feed) -> Result<(Down, Running), Failed> {
    let conn = Connection::connect_to_env().map_err(|e| Failed::ForNow(format!("no Wayland desk to show ({e})")))?;
    let (globals, queue) =
        registry_queue_init::<State>(&conn).map_err(|e| Failed::ForNow(format!("the desk's globals: {e}")))?;
    let qh = queue.handle();
    let lasting = |what: &str, e: &dyn std::fmt::Display| Failed::Lasting(format!("{what}: {e}"));
    let manager: ZwlrScreencopyManagerV1 = globals
        .bind(&qh, 3..=3, ())
        .map_err(|e| lasting("The desk does not offer screen capture (wlr-screencopy 3)", &e))?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(|e| lasting("No shared memory", &e))?;
    let output: wl_output::WlOutput = globals.bind(&qh, 1..=4, ()).map_err(|e| lasting("No screen", &e))?;
    let stop = Arc::new(Stop::new().map_err(Failed::ForNow)?);
    let mut capture = Capture {
        queue,
        qh,
        state: State::default(),
        manager,
        shm,
        output,
        stop: Arc::clone(&stop),
        frame: None,
        buffer: None,
    };
    let offer = capture.offer().map_err(Failed::ForNow)?.ok_or_else(|| Failed::ForNow("stopped".into()))?;
    if !Encoder::supports(offer.format) {
        return Err(Failed::Lasting(format!(
            "The desk captures the screen as format {:#x}, which the encoder does not take",
            offer.format
        )));
    }
    let encoder = Encoder::new(offer.width, offer.height, FPS, CEILING_KBPS.min(feed.gate.kbps()))
        .map_err(|e| Failed::Lasting(format!("No video encoder: {e}")))?;
    let screen = display(&offer);
    info!(?offer, "remote view: capturing the screen");
    thread::Builder::new()
        .name("kvm-view-capture".into())
        .spawn(move || {
            if let Some(why) = capture.run(offer, encoder, &feed) {
                let _ = feed.tx.send(Msg::Stopped(why));
            }
        })
        .map_err(|e| Failed::ForNow(format!("the capture's thread: {e}")))?;
    Ok((screen, Running { stop }))
}

/// The screen as the viewer is told it: its pixels, which input points in
/// too (the injector's extent is the output's mode).
fn display(offer: &Offer) -> Down {
    Down::Display {
        width: offer.width,
        height: offer.height,
        points_w: offer.width,
        points_h: offer.height,
    }
}

/// The buffer the compositor asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Offer {
    format: u32,
    width: u32,
    height: u32,
    stride: u32,
}

/// What a frame said.
#[derive(Default)]
struct State {
    offer: Option<Offer>,
    offered: bool,
    y_invert: bool,
    /// `Some(true)` ready, `Some(false)` failed.
    done: Option<bool>,
}

/// Our shared memory, the size of an offer.
struct Buffer {
    offer: Offer,
    _fd: OwnedFd,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    map: *mut u8,
    len: usize,
}

impl Buffer {
    fn new(shm: &wl_shm::WlShm, offer: Offer, qh: &QueueHandle<State>) -> Result<Self, String> {
        let len = offer.stride as usize * offer.height as usize;
        let fd = unsafe { libc::memfd_create(c"kvm-view".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(format!("memfd: {}", std::io::Error::last_os_error()));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        if unsafe { libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) } != 0 {
            return Err(format!("size the shared memory: {}", std::io::Error::last_os_error()));
        }
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(format!("map the shared memory: {}", std::io::Error::last_os_error()));
        }
        let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let format = wl_shm::Format::try_from(offer.format).map_err(|()| format!("format {:#x}", offer.format))?;
        let buffer = pool.create_buffer(
            0,
            offer.width as i32,
            offer.height as i32,
            offer.stride as i32,
            format,
            qh,
            (),
        );
        Ok(Self {
            offer,
            _fd: fd,
            pool,
            buffer,
            map: map.cast(),
            len,
        })
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.map, self.len) }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
        unsafe { libc::munmap(self.map.cast(), self.len) };
    }
}

// The map is this capture's alone, on its thread.
unsafe impl Send for Buffer {}

struct Capture {
    queue: EventQueue<State>,
    qh: QueueHandle<State>,
    state: State,
    manager: ZwlrScreencopyManagerV1,
    shm: wl_shm::WlShm,
    output: wl_output::WlOutput,
    stop: Arc<Stop>,
    /// The frame asked for and not yet copied.
    frame: Option<ZwlrScreencopyFrameV1>,
    buffer: Option<Buffer>,
}

impl Capture {
    /// Ask for a frame and wait for what buffer it wants; `None` when
    /// stopped. The frame stays in `self.frame` for [`Self::copy`].
    fn offer(&mut self) -> Result<Option<Offer>, String> {
        self.state = State::default();
        let frame = self.manager.capture_output(1, &self.output, &self.qh, ());
        let came = self.wait(|s| s.offered || s.done == Some(false))?;
        if !came || self.state.done == Some(false) {
            frame.destroy();
            return match came {
                true => Err("the desk would not capture the screen".into()),
                false => Ok(None),
            };
        }
        let offer = self.state.offer;
        self.frame = Some(frame);
        offer.map(Some).ok_or_else(|| "the desk offers no shared-memory buffer to capture into".into())
    }

    fn run(mut self, mut offer: Offer, mut encoder: Encoder, feed: &Feed) -> Option<String> {
        let (mut first, mut kbps, mut failures) = (true, feed.gate.kbps(), 0);
        let mut key_at = Instant::now();
        let started = Instant::now();
        loop {
            let asked = Instant::now();
            match self.copy(offer, first) {
                Ok(None) => return None,
                Ok(Some(true)) => failures = 0,
                Ok(Some(false)) => {
                    failures += 1;
                    if failures >= FAILURES {
                        return Some("the desk failed to copy the screen".into());
                    }
                    first = true;
                }
                Err(why) => return Some(why),
            }
            if failures == 0 {
                let captured_us = now_us();
                if feed.gate.admit(captured_us, FRAME_US) {
                    if feed.gate.kbps() != kbps {
                        kbps = feed.gate.kbps();
                        if let Err(why) = encoder.set_kbps(kbps) {
                            warn!(%why, kbps, "remote view: the encoder kept its bitrate");
                        }
                    }
                    let key = first || feed.force_key.swap(false, Ordering::Relaxed) || key_at.elapsed() >= KEY_EVERY;
                    if key {
                        key_at = Instant::now();
                    }
                    let buffer = self.buffer.as_ref().expect("copied into");
                    let frame = Frame {
                        data: buffer.bytes(),
                        width: offer.width,
                        height: offer.height,
                        stride: offer.stride,
                        format: offer.format,
                        y_invert: self.state.y_invert,
                    };
                    let pts_us = started.elapsed().as_micros() as u64;
                    match encoder.encode(&frame, key, pts_us) {
                        Ok(picture) => {
                            let encoded = Encoded {
                                key: picture.key,
                                pts_us,
                                captured_us,
                                data: picture.data,
                            };
                            if feed.tx.send(Msg::Picture(encoded)).is_err() {
                                return None;
                            }
                        }
                        Err(why) => {
                            warn!(%why, "remote view: the encoder would not take a frame");
                            feed.gate.dropped();
                            feed.force_key.store(true, Ordering::Relaxed);
                        }
                    }
                } else {
                    feed.skipped.fetch_add(1, Ordering::Relaxed);
                }
                first = false;
            }
            // No faster than the frame rate: the screen may refresh faster.
            if !self.pause(FRAME.saturating_sub(asked.elapsed())) {
                return None;
            }
            let next = match self.offer() {
                Ok(Some(next)) => next,
                Ok(None) => return None,
                Err(why) => return Some(why),
            };
            if next != offer {
                info!(?next, "remote view: the screen changed");
                if !Encoder::supports(next.format) {
                    return Some(format!("the desk now captures as format {:#x}", next.format));
                }
                if (next.width, next.height) != (offer.width, offer.height) {
                    encoder = match Encoder::new(next.width, next.height, FPS, kbps) {
                        Ok(encoder) => encoder,
                        Err(why) => return Some(why),
                    };
                    if feed.tx.send(Msg::Display(display(&next))).is_err() {
                        return None;
                    }
                    first = true;
                }
                offer = next;
            }
        }
    }

    /// Copy the asked frame into our buffer: once the screen changes, after
    /// the first. `Some(true)` copied, `Some(false)` the desk failed it,
    /// `None` stopped.
    fn copy(&mut self, offer: Offer, first: bool) -> Result<Option<bool>, String> {
        if self.buffer.as_ref().is_none_or(|b| b.offer != offer) {
            self.buffer = None;
            self.buffer = Some(Buffer::new(&self.shm, offer, &self.qh)?);
        }
        let buffer = &self.buffer.as_ref().expect("made").buffer;
        let frame = self.frame.take().expect("offered");
        if first {
            frame.copy(buffer);
        } else {
            frame.copy_with_damage(buffer);
        }
        let came = self.wait(|s| s.done.is_some());
        frame.destroy();
        Ok(match came? {
            true => self.state.done,
            false => None,
        })
    }

    /// Dispatch until `until` holds (`true`), or the stop (`false`).
    fn wait(&mut self, until: impl Fn(&State) -> bool) -> Result<bool, String> {
        let lost = |e: &dyn std::fmt::Display| format!("the desk's connection: {e}");
        loop {
            self.queue.dispatch_pending(&mut self.state).map_err(|e| lost(&e))?;
            if until(&self.state) {
                return Ok(true);
            }
            if self.stop.is_set() {
                return Ok(false);
            }
            self.queue.flush().map_err(|e| lost(&e))?;
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            let ready = poll(&[guard.connection_fd().as_raw_fd(), self.stop.fd.as_raw_fd()], -1);
            if ready.first() == Some(&true) {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(lost(&e)),
                }
            }
        }
    }

    /// Wait `pause`, or less when stopped (`false`).
    fn pause(&self, pause: Duration) -> bool {
        if !pause.is_zero() {
            poll(&[self.stop.fd.as_raw_fd()], pause.as_millis() as i32);
        }
        !self.stop.is_set()
    }
}

/// Which of `fds` are readable, after waiting up to `timeout_ms` (-1:
/// until one is).
fn poll(fds: &[i32], timeout_ms: i32) -> Vec<bool> {
    let mut polled: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    loop {
        let n = unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, timeout_ms) };
        if n >= 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break;
        }
    }
    polled.iter().map(|p| p.revents != 0).collect()
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let format = match format {
                    WEnum::Value(f) => f as u32,
                    WEnum::Unknown(f) => f,
                };
                state.offer = Some(Offer {
                    format,
                    width,
                    height,
                    stride,
                });
            }
            Event::BufferDone => state.offered = true,
            Event::Flags { flags } => {
                state.y_invert = matches!(flags, WEnum::Value(f) if f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
            }
            Event::Ready { .. } => state.done = Some(true),
            Event::Failed => state.done = Some(false),
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(State: ignore ZwlrScreencopyManagerV1);
wayland_client::delegate_noop!(State: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(State: ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(State: ignore wl_output::WlOutput);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
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
