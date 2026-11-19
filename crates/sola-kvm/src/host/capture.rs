//! The picture source: wlr-screencopy of the output into shared memory,
//! with the pointer drawn in, into the encoder. After the first frame each
//! copy waits for the screen to change, so a still screen sends nothing;
//! frames come no faster than [`FPS`]. Two buffers: the desk copies the next
//! frame into one while the last encodes from the other, so a frame costs
//! the longer of the two, not both. Its own thread and Wayland connection;
//! dropping [`Running`] stops it.

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};
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
        buffers: [None, None],
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
            if let Err(why) = capture.run(offer, encoder, &feed) {
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
    /// When the desk had the frame: `CLOCK_MONOTONIC`, nanoseconds.
    ready_ns: Option<u64>,
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
    /// One copying while the other encodes.
    buffers: [Option<Buffer>; 2],
}

/// A frame copied into `buffers[slot]`.
struct Shot {
    slot: usize,
    offer: Offer,
    y_invert: bool,
    /// When the desk had it ([`now_us`]).
    captured_us: u64,
}

/// How a copy ended.
enum Copied {
    Shot(Shot),
    /// The desk failed it.
    Failed,
    Stopped,
}

impl Capture {
    /// Ask for a frame and wait for what buffer it wants; `None` when
    /// stopped. The frame stays in `self.frame` for [`Self::start_copy`].
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

    /// `Ok` when stopped (the viewer went); `Err` says why the capture
    /// could not go on.
    fn run(mut self, mut offer: Offer, encoder: Encoder, feed: &Feed) -> Result<(), String> {
        let mut encoding = Encoding::new(encoder, feed);
        let (mut slot, mut failures) = (0, 0);
        self.start_copy(slot, offer, true)?;
        let mut asked = Instant::now();
        loop {
            let shot = match self.finish_copy(slot, offer)? {
                Copied::Stopped => return Ok(()),
                Copied::Failed => {
                    failures += 1;
                    if failures >= FAILURES {
                        return Err("the desk failed to copy the screen".into());
                    }
                    None
                }
                Copied::Shot(shot) => {
                    failures = 0;
                    Some(shot)
                }
            };
            // No faster than the frame rate: the screen may refresh faster.
            if !self.pause(FRAME.saturating_sub(asked.elapsed())) {
                return Ok(());
            }
            asked = Instant::now();
            let Some(next) = self.offer()? else {
                return Ok(());
            };
            let other = 1 - slot;
            match &shot {
                // The next frame copies into the other buffer while this
                // one encodes.
                Some(shot) if next == offer => {
                    self.start_copy(other, next, false)?;
                    if !encoding.picture(shot, self.bytes(shot.slot)) {
                        return Ok(());
                    }
                }
                // This one first: the next is copied whole, at its size.
                _ => {
                    if let Some(shot) = &shot {
                        if !encoding.picture(shot, self.bytes(shot.slot)) {
                            return Ok(());
                        }
                    }
                    if next != offer && !encoding.change(&offer, &next)? {
                        return Ok(());
                    }
                    self.start_copy(other, next, true)?;
                }
            }
            (slot, offer) = (other, next);
        }
    }

    /// Ask the desk to copy the offered frame into `buffers[slot]`: at
    /// once when `whole`, else once the screen changes.
    fn start_copy(&mut self, slot: usize, offer: Offer, whole: bool) -> Result<(), String> {
        if self.buffers[slot].as_ref().is_none_or(|b| b.offer != offer) {
            self.buffers[slot] = None;
            self.buffers[slot] = Some(Buffer::new(&self.shm, offer, &self.qh)?);
        }
        let buffer = &self.buffers[slot].as_ref().expect("made").buffer;
        let frame = self.frame.as_ref().expect("offered");
        if whole {
            frame.copy(buffer);
        } else {
            frame.copy_with_damage(buffer);
        }
        self.queue.flush().map_err(|e| format!("the desk's connection: {e}"))
    }

    /// Wait for the copy into `buffers[slot]`.
    fn finish_copy(&mut self, slot: usize, offer: Offer) -> Result<Copied, String> {
        let came = self.wait(|s| s.done.is_some());
        if let Some(frame) = self.frame.take() {
            frame.destroy();
        }
        Ok(match (came?, self.state.done) {
            (false, _) => Copied::Stopped,
            (true, Some(true)) => Copied::Shot(Shot {
                slot,
                offer,
                y_invert: self.state.y_invert,
                captured_us: captured_us(self.state.ready_ns),
            }),
            (true, _) => Copied::Failed,
        })
    }

    fn bytes(&self, slot: usize) -> &[u8] {
        self.buffers[slot].as_ref().map_or(&[], Buffer::bytes)
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

/// [`now_us`] when the desk had a frame, from its own timestamp: a frame
/// that was ready while the one before encoded is older than its dispatch.
fn captured_us(ready_ns: Option<u64>) -> u64 {
    let now = now_us();
    let Some(ready_ns) = ready_ns else {
        return now;
    };
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    let mono_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
    let age_us = mono_ns.saturating_sub(ready_ns) / 1_000;
    // Not the clock it should be: the dispatch is the nearest.
    if age_us > 1_000_000 {
        return now;
    }
    now.saturating_sub(age_us)
}

/// The encoder and what it keeps from frame to frame.
struct Encoding<'a> {
    encoder: Encoder,
    feed: &'a Feed,
    /// The bitrate the encoder has.
    kbps: u32,
    /// The next picture is a key one: the first, or after a new encoder.
    key_next: bool,
    key_at: Instant,
    started: Instant,
    second: Second,
}

/// A second of capture, for the debug log.
#[derive(Default)]
struct Second {
    from: Option<Instant>,
    pictures: u32,
    skipped: u32,
    encode: Duration,
    bitrates: u32,
}

impl<'a> Encoding<'a> {
    fn new(encoder: Encoder, feed: &'a Feed) -> Self {
        Self {
            encoder,
            feed,
            kbps: feed.gate.kbps(),
            key_next: true,
            key_at: Instant::now(),
            started: Instant::now(),
            second: Second::default(),
        }
    }

    /// Encode `shot` from `data` and send it, when the gate lets it through.
    /// `false` when the viewer has gone.
    fn picture(&mut self, shot: &Shot, data: &[u8]) -> bool {
        let feed = self.feed;
        if !feed.gate.admit(shot.captured_us, FRAME_US) {
            feed.skipped.fetch_add(1, Ordering::Relaxed);
            self.second.skipped += 1;
            return true;
        }
        let began = Instant::now();
        if feed.gate.kbps() != self.kbps {
            self.kbps = feed.gate.kbps();
            self.second.bitrates += 1;
            if let Err(why) = self.encoder.set_kbps(self.kbps) {
                warn!(%why, kbps = self.kbps, "remote view: the encoder kept its bitrate");
            }
        }
        let key = std::mem::take(&mut self.key_next)
            || feed.force_key.swap(false, Ordering::Relaxed)
            || self.key_at.elapsed() >= KEY_EVERY;
        if key {
            self.key_at = Instant::now();
        }
        let frame = Frame {
            data,
            width: shot.offer.width,
            height: shot.offer.height,
            stride: shot.offer.stride,
            format: shot.offer.format,
            y_invert: shot.y_invert,
        };
        let pts_us = self.started.elapsed().as_micros() as u64;
        let sent = match self.encoder.encode(&frame, key, pts_us) {
            Ok(picture) => {
                let encoded = Encoded {
                    key: picture.key,
                    pts_us,
                    captured_us: shot.captured_us,
                    data: picture.data,
                };
                feed.tx.send(Msg::Picture(encoded)).is_ok()
            }
            Err(why) => {
                warn!(%why, "remote view: the encoder would not take a frame");
                feed.gate.dropped();
                self.key_next = true;
                true
            }
        };
        self.second.pictures += 1;
        self.second.encode += began.elapsed();
        self.log_second();
        sent
    }

    /// The screen changed from `was` to `now`: a new encoder when its size
    /// did, and the viewer told. `false` when the viewer has gone.
    fn change(&mut self, was: &Offer, now: &Offer) -> Result<bool, String> {
        info!(?now, "remote view: the screen changed");
        if !Encoder::supports(now.format) {
            return Err(format!("the desk now captures as format {:#x}", now.format));
        }
        if (now.width, now.height) == (was.width, was.height) {
            return Ok(true);
        }
        self.encoder = Encoder::new(now.width, now.height, FPS, self.kbps)?;
        self.key_next = true;
        Ok(self.feed.tx.send(Msg::Display(display(now))).is_ok())
    }

    fn log_second(&mut self) {
        let second = &mut self.second;
        let from = *second.from.get_or_insert_with(Instant::now);
        if from.elapsed() < Duration::from_secs(1) {
            return;
        }
        let encode_ms = second.encode.as_secs_f64() * 1000.0 / f64::from(second.pictures.max(1));
        debug!(
            pictures = second.pictures,
            skipped = second.skipped,
            encode_ms = format!("{encode_ms:.1}"),
            bitrate_changes = second.bitrates,
            "remote view: a second of capture"
        );
        *second = Second::default();
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
            Event::Ready {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                let secs = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
                state.ready_ns = Some(secs * 1_000_000_000 + u64::from(tv_nsec));
                state.done = Some(true);
            }
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
