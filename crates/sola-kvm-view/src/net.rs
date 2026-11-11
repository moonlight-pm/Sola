//! The two streams to the host, on their own threads. The video thread
//! connects, decodes as pictures arrive, and connects again when the host
//! goes away; the writer thread lives across connections.

use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sola_kvm::clip;
use sola_kvm::link::{self, Conn, Keys};
use sola_kvm::protocol::{self, Packet};
use sola_kvm::view_wire::{Down, MAX_CLIPBOARD, MAX_FRAME, PORT_OFFSET, Stream, Up};
use tracing::{debug, info, warn};

use crate::decode::Decoder;
use crate::pictures::Pictures;
use crate::stats::{Sample, micros};

const CONNECT: Duration = Duration::from_secs(3);
const HANDSHAKE: Duration = Duration::from_secs(5);
const RECONNECT: Duration = Duration::from_secs(1);
/// A host that vanishes (asleep, unplugged) is noticed in about this:
/// idle seconds, then probes at an interval, this many times.
const KEEPALIVE: (libc::c_int, libc::c_int, libc::c_int) = (5, 2, 3);

/// The screen being shown, in the host's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    pub points: (u32, u32),
}

/// Where [`Event`]s go: the window, or `--measure`.
pub type Tell = Arc<dyn Fn(Event) + Send + Sync>;

/// What the window hears.
#[derive(Debug, Clone)]
pub enum Event {
    /// Where to send input from now on.
    Ready(Sender<Outgoing>),
    Connected(Display),
    /// A new picture waits in [`Pictures`].
    Picture,
    /// The host's pointer changed; it waits in [`Pictures`].
    Cursor,
    Lost(String),
    /// The host will not send a picture; trying again will not help.
    Refused(String),
}

#[derive(Debug, Clone)]
pub enum Outgoing {
    Packet(Packet),
    Keyframe,
    /// Picture `seq` is decoded.
    Ack { seq: u32, held_us: u32 },
    /// Give the host this machine's clipboard.
    PushClipboard,
    /// Ask for the host's clipboard.
    PullClipboard,
    /// This machine's clipboard text, read and on its way.
    Clipboard(String),
}

/// Clipboard work, off the input and video threads: reading or writing the
/// desk's clipboard can take a moment.
enum ClipJob {
    Push,
    Apply(String),
}

type Link = Arc<Mutex<Option<(TcpStream, Conn)>>>;

/// `--measure --link <mbps>`: pictures read no faster than a link this fast
/// carries them, so the host meets a slower way than the real one. It
/// stands in for the link; nothing else waits on it.
pub struct Throttle {
    kbps: u64,
    free_at: Instant,
}

impl Throttle {
    pub fn new(mbps: f64) -> Self {
        Self {
            kbps: ((mbps * 1000.0) as u64).max(1),
            free_at: Instant::now(),
        }
    }

    /// Wait until `bytes`, read just now, would have come over the link.
    fn carry(&mut self, bytes: usize) {
        let now = Instant::now();
        self.free_at = self.free_at.max(now) + Duration::from_micros(bytes as u64 * 8 * 1000 / self.kbps);
        thread::sleep(self.free_at.saturating_duration_since(now));
    }
}

pub fn start(host: String, kvm_port: u16, pictures: Arc<Pictures>, events: Tell, mut throttle: Option<Throttle>) {
    let (tx, rx) = mpsc::channel();
    events(Event::Ready(tx.clone()));
    let link: Link = Arc::default();
    let writer = Arc::clone(&link);
    let (clip_tx, clip_rx) = mpsc::channel();
    let (to_writer, to_video) = (clip_tx.clone(), clip_tx);
    let outgoing = tx.clone();
    let spawned = thread::Builder::new()
        .name("kvm-view-input".into())
        .spawn(move || write_loop(rx, writer, to_writer))
        .and_then(|_| {
            thread::Builder::new()
                .name("kvm-view-clip".into())
                .spawn(move || clip_loop(clip_rx, outgoing))
        })
        .and_then(|_| {
            thread::Builder::new()
                .name("kvm-view-video".into())
                .spawn(move || video_loop(&host, kvm_port, &pictures, &events, &link, &tx, &to_video, &mut throttle))
        });
    if let Err(e) = spawned {
        warn!(%e, "remote view: could not start its threads");
    }
}

fn video_loop(
    host: &str,
    kvm_port: u16,
    pictures: &Arc<Pictures>,
    events: &Tell,
    link: &Link,
    outgoing: &Sender<Outgoing>,
    clip: &Sender<ClipJob>,
    throttle: &mut Option<Throttle>,
) {
    let keys = match link::load_view(&link::dir(), host) {
        Ok(Some(keys)) => keys,
        Ok(None) => {
            events(Event::Refused(format!(
                "This machine is not paired to view {host}. Run: sola-kvm pair {host} --view"
            )));
            return;
        }
        Err(e) => {
            events(Event::Refused(format!("The keys for {host} cannot be read: {e}")));
            return;
        }
    };
    loop {
        match session(host, kvm_port, &keys, pictures, events, link, outgoing, clip, throttle) {
            Ok(Stop::Refused) => return,
            Err(e) => {
                info!(host, error = %e, "remote view: connection lost");
                events(Event::Lost(e.to_string()));
            }
        }
        lock(link).take();
        thread::sleep(RECONNECT);
    }
}

enum Stop {
    Refused,
}

fn session(
    host: &str,
    kvm_port: u16,
    keys: &Keys,
    pictures: &Arc<Pictures>,
    events: &Tell,
    link: &Link,
    outgoing: &Sender<Outgoing>,
    clip: &Sender<ClipJob>,
    throttle: &mut Option<Throttle>,
) -> io::Result<Stop> {
    let addr = resolve(host, kvm_port + PORT_OFFSET)?;
    let (mut tcp, mut conn) = connect(addr, keys, Stream::Video)?;
    *lock(link) = Some(connect(addr, keys, Stream::Input)?);
    let mut decoder = None;
    info!(%addr, "remote view: connected");
    loop {
        let frame = conn.read_frame(&mut tcp, MAX_FRAME)?;
        if let Some(throttle) = throttle {
            throttle.carry(frame.len());
        }
        let Some(down) = Down::decode(&frame)? else {
            continue;
        };
        match down {
            Down::Display { points_w, points_h, .. } => {
                // A host that sends no pointer keeps it in the picture.
                pictures.set_cursor(None);
                events(Event::Connected(Display {
                    points: (points_w, points_h),
                }));
            }
            Down::Refused(why) => {
                events(Event::Refused(why));
                return Ok(Stop::Refused);
            }
            Down::Clipboard(text) => {
                let _ = clip.send(ClipJob::Apply(text));
            }
            Down::Cursor(cursor) => {
                debug!(
                    width = cursor.width,
                    height = cursor.height,
                    hot_x = cursor.hot_x,
                    hot_y = cursor.hot_y,
                    "remote view: the host's pointer"
                );
                pictures.set_cursor(Some(cursor));
                events(Event::Cursor);
            }
            Down::Picture(picture) => {
                let arrived = Instant::now();
                let decoder = match &mut decoder {
                    Some(decoder) => decoder,
                    None => decoder.insert(Decoder::new(Arc::clone(pictures)).map_err(invalid)?),
                };
                let before_us = picture.age_us.saturating_add(picture.net_us);
                let pushed = decoder.push(&picture.data, picture.pts_us, before_us, arrived);
                let held_us = micros(arrived.elapsed());
                let _ = outgoing.send(Outgoing::Ack {
                    seq: picture.seq,
                    held_us,
                });
                let sample = Sample {
                    bytes: picture.data.len(),
                    host_us: picture.age_us,
                    net_us: picture.net_us,
                    decode_us: held_us,
                    kbps: picture.kbps,
                    skipped: picture.skipped,
                };
                if let Some(second) = pictures.stats.picture(&sample) {
                    debug!(stats = %second.line(), "remote view: a second of pictures");
                }
                match pushed {
                    Ok(true) => events(Event::Picture),
                    Ok(false) => {}
                    Err(e) => {
                        warn!(error = %e, "remote view: a picture did not decode; asking for a key picture");
                        let _ = outgoing.send(Outgoing::Keyframe);
                    }
                }
            }
        }
    }
}

fn resolve(host: &str, port: u16) -> io::Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| invalid(format!("{host} has no address")))
}

fn connect(addr: SocketAddr, keys: &Keys, stream: Stream) -> io::Result<(TcpStream, Conn)> {
    let mut tcp = TcpStream::connect_timeout(&addr, CONNECT)?;
    tcp.set_nodelay(true)?;
    keepalive(&tcp);
    tcp.set_read_timeout(Some(HANDSHAKE))?;
    let mut conn = Conn::connect(&mut tcp, keys)?;
    conn.write_frame(&mut tcp, &stream.encode())?;
    tcp.set_read_timeout(None)?;
    Ok((tcp, conn))
}

fn keepalive(tcp: &TcpStream) {
    let fd = tcp.as_raw_fd();
    let (idle, interval, count) = KEEPALIVE;
    for (level, option, value) in [
        (libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1),
        (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, idle),
        (libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, interval),
        (libc::IPPROTO_TCP, libc::TCP_KEEPCNT, count),
    ] {
        let status = unsafe {
            libc::setsockopt(fd, level, option, (&raw const value).cast(), size_of::<libc::c_int>() as _)
        };
        if status != 0 {
            warn!(option, error = %io::Error::last_os_error(), "remote view: a keepalive option was refused");
        }
    }
}

/// The text both machines last agreed on, so a sync that changes nothing
/// does nothing: leaving the window must not overwrite what was copied here
/// meanwhile with the host's unchanged clipboard.
fn clip_loop(jobs: Receiver<ClipJob>, outgoing: Sender<Outgoing>) {
    let mut agreed = None;
    while let Ok(job) = jobs.recv() {
        match job {
            ClipJob::Push => {
                let Some(text) = clip::read_text().filter(|t| t.len() <= MAX_CLIPBOARD) else {
                    continue;
                };
                let hash = clip::hash_text(&text);
                if agreed != Some(hash) {
                    agreed = Some(hash);
                    let _ = outgoing.send(Outgoing::Clipboard(text));
                }
            }
            ClipJob::Apply(text) => {
                let hash = clip::hash_text(&text);
                if agreed != Some(hash) && clip::write_text(&text) {
                    agreed = Some(hash);
                }
            }
        }
    }
}

fn write_loop(rx: Receiver<Outgoing>, link: Link, clip: Sender<ClipJob>) {
    let mut seq = 0u32;
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        for out in coalesce(batch) {
            let up = match out {
                Outgoing::Keyframe => Up::Keyframe,
                Outgoing::Ack { seq, held_us } => Up::Ack { seq, held_us },
                Outgoing::PushClipboard => {
                    let _ = clip.send(ClipJob::Push);
                    continue;
                }
                Outgoing::PullClipboard => Up::ClipboardGet,
                Outgoing::Clipboard(text) => Up::ClipboardSet(text),
                Outgoing::Packet(packet) => {
                    seq = seq.wrapping_add(1);
                    match protocol::encode(seq, &packet) {
                        Ok(bytes) => Up::Event(bytes),
                        Err(e) => {
                            warn!(?e, "remote view: an input packet would not encode");
                            continue;
                        }
                    }
                }
            };
            let mut guard = lock(&link);
            if let Some((tcp, conn)) = guard.as_mut() {
                if let Err(e) = conn.write_frame(tcp, &up.encode()) {
                    info!(%e, "remote view: the input stream ended");
                    *guard = None;
                }
            }
        }
    }
}

/// Runs of pointer motion collapse to where the pointer ended up.
fn coalesce(batch: Vec<Outgoing>) -> Vec<Outgoing> {
    let mut out: Vec<Outgoing> = Vec::with_capacity(batch.len());
    for item in batch {
        if let (Some(Outgoing::Packet(Packet::Motion { .. })), Outgoing::Packet(Packet::Motion { .. })) =
            (out.last(), &item)
        {
            out.pop();
        }
        out.push(item);
    }
    out
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion(x: i32) -> Outgoing {
        Outgoing::Packet(Packet::Motion { x, y: 0 })
    }

    fn xs(batch: &[Outgoing]) -> Vec<i32> {
        batch
            .iter()
            .map(|o| match o {
                Outgoing::Packet(Packet::Motion { x, .. }) => *x,
                _ => -1,
            })
            .collect()
    }

    #[test]
    fn motion_runs_collapse_but_keep_their_place_around_clicks() {
        let click = Outgoing::Packet(Packet::Button { button: 0, pressed: 1 });
        let batch = vec![motion(1), motion(2), click, motion(3), motion(4), Outgoing::Keyframe];
        assert_eq!(xs(&coalesce(batch)), [2, -1, 4, -1]);
    }
}
