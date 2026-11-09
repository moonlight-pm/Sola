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

use iced::futures::channel::mpsc::UnboundedSender;
use sola_kvm::link::{self, Conn, Keys};
use sola_kvm::protocol::{self, Packet};
use sola_kvm::view_wire::{Down, MAX_FRAME, PORT_OFFSET, Stream, Up};
use tracing::{debug, info, warn};

use crate::decode::Decoder;
use crate::pictures::Pictures;

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

/// What the window hears.
#[derive(Debug, Clone)]
pub enum Event {
    /// Where to send input from now on.
    Ready(Sender<Outgoing>),
    Connected(Display),
    /// A new picture waits in [`Pictures`].
    Picture,
    Lost(String),
    /// The host will not send a picture; trying again will not help.
    Refused(String),
}

#[derive(Debug, Clone)]
pub enum Outgoing {
    Packet(Packet),
    Keyframe,
}

type Link = Arc<Mutex<Option<(TcpStream, Conn)>>>;

pub fn start(host: String, kvm_port: u16, pictures: Arc<Pictures>, events: UnboundedSender<Event>) {
    let (tx, rx) = mpsc::channel();
    let _ = events.unbounded_send(Event::Ready(tx.clone()));
    let link: Link = Arc::default();
    let writer = Arc::clone(&link);
    let spawned = thread::Builder::new()
        .name("kvm-view-input".into())
        .spawn(move || write_loop(rx, writer))
        .and_then(|_| {
            thread::Builder::new()
                .name("kvm-view-video".into())
                .spawn(move || video_loop(&host, kvm_port, &pictures, &events, &link, &tx))
        });
    if let Err(e) = spawned {
        warn!(%e, "remote view: could not start its threads");
    }
}

fn video_loop(
    host: &str,
    kvm_port: u16,
    pictures: &Arc<Pictures>,
    events: &UnboundedSender<Event>,
    link: &Link,
    outgoing: &Sender<Outgoing>,
) {
    let keys = match link::load_view(&link::dir(), host) {
        Ok(Some(keys)) => keys,
        Ok(None) => {
            let _ = events.unbounded_send(Event::Refused(format!(
                "This machine is not paired to view {host}. Run: sola-kvm pair {host} --view"
            )));
            return;
        }
        Err(e) => {
            let _ = events.unbounded_send(Event::Refused(format!("The keys for {host} cannot be read: {e}")));
            return;
        }
    };
    loop {
        match session(host, kvm_port, &keys, pictures, events, link, outgoing) {
            Ok(Stop::Refused) => return,
            Err(e) => {
                info!(host, error = %e, "remote view: connection lost");
                let _ = events.unbounded_send(Event::Lost(e.to_string()));
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
    events: &UnboundedSender<Event>,
    link: &Link,
    outgoing: &Sender<Outgoing>,
) -> io::Result<Stop> {
    let addr = resolve(host, kvm_port + PORT_OFFSET)?;
    let (mut tcp, mut conn) = connect(addr, keys, Stream::Video)?;
    *lock(link) = Some(connect(addr, keys, Stream::Input)?);
    let mut decoder = None;
    let mut rate = Rate::new();
    info!(%addr, "remote view: connected");
    loop {
        match Down::decode(&conn.read_frame(&mut tcp, MAX_FRAME)?)? {
            Down::Display { points_w, points_h, .. } => {
                let _ = events.unbounded_send(Event::Connected(Display {
                    points: (points_w, points_h),
                }));
            }
            Down::Refused(why) => {
                let _ = events.unbounded_send(Event::Refused(why));
                return Ok(Stop::Refused);
            }
            Down::Picture { pts_us, data, .. } => {
                let decoder = match &mut decoder {
                    Some(decoder) => decoder,
                    None => decoder.insert(Decoder::new(Arc::clone(pictures)).map_err(invalid)?),
                };
                let started = Instant::now();
                let pushed = decoder.push(&data, pts_us);
                rate.count(data.len(), started.elapsed());
                match pushed {
                    Ok(true) => {
                        let _ = events.unbounded_send(Event::Picture);
                    }
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

/// Pictures and bits per second, in the log at debug level.
struct Rate {
    since: Instant,
    pictures: u32,
    bytes: usize,
    decoding: Duration,
}

impl Rate {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            pictures: 0,
            bytes: 0,
            decoding: Duration::ZERO,
        }
    }

    fn count(&mut self, bytes: usize, decoding: Duration) {
        self.pictures += 1;
        self.bytes += bytes;
        self.decoding += decoding;
        let elapsed = self.since.elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            debug!(
                fps = self.pictures as f64 / elapsed,
                mbps = self.bytes as f64 * 8.0 / elapsed / 1e6,
                decode_ms = self.decoding.as_secs_f64() * 1e3 / f64::from(self.pictures),
                "remote view: rate"
            );
            *self = Self::new();
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

fn write_loop(rx: Receiver<Outgoing>, link: Link) {
    let mut seq = 0u32;
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        for out in coalesce(batch) {
            let up = match out {
                Outgoing::Keyframe => Up::Keyframe,
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
