//! The view port: connections from paired viewers, one viewer at a time. A
//! new viewer takes over from the one before. Viewers' keys are read as
//! each connects, so one paired since the start gets in.

use std::io::{self, Read};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tracing::{debug, info, warn};

use super::capture;
use super::flow::{Flow, Gate, now_us};
use super::rate::Rate;
use crate::inject::WaylandInjector;
use crate::link::{self, Conn};
use crate::protocol::{self, Packet};
use crate::view_wire::{Down, MAX_CLIPBOARD, PORT_OFFSET, Picture, Stream, Up};

/// How long a connection has to prove it is a paired viewer. These run one
/// at a time on the accept thread, so a stranger cannot pile up threads.
const HANDSHAKE: Duration = Duration::from_secs(5);
const OPEN_FRAME: u32 = 16;
/// Input is small; a frame this big is clipboard text.
const UP_FRAME: u32 = MAX_CLIPBOARD as u32 + 16;

/// A picture from the encoder, before what the wire says of its way.
pub struct Encoded {
    pub key: bool,
    pub pts_us: u64,
    /// When its frame was captured ([`now_us`]).
    pub captured_us: u64,
    pub data: Vec<u8>,
}

pub enum Msg {
    Picture(Encoded),
    /// The screen changed size: a new encoder, its first picture a key one.
    Display(Down),
    /// The viewer decoded picture `seq`; the ack came at `at_us`.
    Ack {
        seq: u32,
        held_us: u32,
        at_us: u64,
    },
    Clipboard(String),
    /// The capture stopped on its own, and why.
    Stopped(String),
    Closed,
}

/// Where a capture's encoded pictures go.
pub struct Feed {
    pub tx: Sender<Msg>,
    /// Pictures out, from the encoder to the viewer's ack, and the bitrate:
    /// capture skips a frame when too many are out, so a slow network drops
    /// frames before they are encoded and no later picture loses its
    /// reference.
    pub gate: Arc<Gate>,
    /// Frames skipped since the last picture went.
    pub skipped: Arc<AtomicU32>,
    pub force_key: Arc<AtomicBool>,
}

#[derive(Default)]
struct Shared {
    video: Mutex<Option<TcpStream>>,
    input: Mutex<Option<TcpStream>>,
    force_key: Arc<AtomicBool>,
    /// The video stream's queue, for clipboard answers and acks.
    to_viewer: Mutex<Option<Sender<Msg>>>,
}

impl Shared {
    fn take_over(slot: &Mutex<Option<TcpStream>>, tcp: &TcpStream) -> io::Result<()> {
        if let Some(old) = lock(slot).replace(tcp.try_clone()?) {
            let _ = old.shutdown(Shutdown::Both);
        }
        Ok(())
    }
}

/// Listen on the view port (the KVM port plus [`PORT_OFFSET`]).
pub fn spawn(kvm_port: u16) -> io::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], kvm_port + PORT_OFFSET));
    let listener = TcpListener::bind(addr)?;
    info!(%addr, "remote view listening (TCP)");
    thread::Builder::new().name("kvm-view".into()).spawn(move || {
        let shared = Arc::new(Shared::default());
        for tcp in listener.incoming() {
            let opened = tcp.and_then(|mut tcp| {
                let (conn, stream) = open(&mut tcp)?;
                Ok((tcp, conn, stream))
            });
            let (tcp, conn, stream) = match opened {
                Ok(v) => v,
                Err(e) => {
                    warn!(%e, "remote view: a connection was refused");
                    continue;
                }
            };
            let shared = Arc::clone(&shared);
            let spawned = thread::Builder::new()
                .name(format!("kvm-view-{stream:?}"))
                .spawn(move || {
                    let result = match stream {
                        Stream::Video => video(tcp, conn, &shared),
                        Stream::Input => input(tcp, conn, &shared),
                    };
                    match result {
                        Ok(()) => info!(?stream, "remote view: stream ended"),
                        Err(e) => info!(?stream, %e, "remote view: stream ended"),
                    }
                });
            if let Err(e) = spawned {
                warn!(%e, "remote view: could not start a stream's thread");
            }
        }
    })?;
    Ok(())
}

/// The handshake, from any paired viewer, and which stream it opens.
fn open(tcp: &mut TcpStream) -> io::Result<(Conn, Stream)> {
    let dir = link::dir();
    let (private, _) = link::own(&dir)?.ok_or_else(|| io::Error::other("this machine has no key yet"))?;
    let viewers = link::viewer_keys(&dir)?;
    tcp.set_nodelay(true)?;
    tcp.set_read_timeout(Some(HANDSHAKE))?;
    let mut conn = Conn::accept_from(tcp, &private, &viewers)?;
    let stream = Stream::decode(&conn.read_frame(tcp, OPEN_FRAME)?)?;
    tcp.set_read_timeout(None)?;
    Ok((conn, stream))
}

fn video(mut tcp: TcpStream, mut conn: Conn, shared: &Shared) -> io::Result<()> {
    Shared::take_over(&shared.video, &tcp)?;
    let (tx, rx) = mpsc::channel();
    watch_for_close(&tcp, tx.clone())?;
    *lock(&shared.to_viewer) = Some(tx.clone());
    let skipped = Arc::new(AtomicU32::new(0));
    let gate = Arc::new(Gate::default());
    let feed = Feed {
        tx,
        gate: Arc::clone(&gate),
        skipped: Arc::clone(&skipped),
        force_key: Arc::clone(&shared.force_key),
    };
    conn.write_frame(&mut tcp, &Down::CanExtend(false).encode())?;
    let (screen, running) = match capture::start(feed) {
        Ok(v) => v,
        Err(capture::Failed::Lasting(why)) => {
            warn!(%why, "remote view: no picture to send");
            return conn.write_frame(&mut tcp, &Down::Refused(why).encode());
        }
        Err(capture::Failed::ForNow(why)) => {
            warn!(%why, "remote view: no picture to send yet; the viewer comes back");
            return Ok(());
        }
    };
    info!(?screen, "remote view: sending");
    conn.write_frame(&mut tcp, &screen.encode())?;
    conn.write_frame(&mut tcp, &Down::CanMirror(true).encode())?;
    conn.write_frame(&mut tcp, &Down::Mirroring(true).encode())?;
    let (mut flow, mut rate, mut seq) = (Flow::default(), Rate::default(), 0u32);
    loop {
        match rx.recv() {
            Ok(Msg::Picture(encoded)) => {
                seq = seq.wrapping_add(1).max(1);
                let now = now_us();
                let picture = Picture {
                    key: encoded.key,
                    seq,
                    pts_us: encoded.pts_us,
                    age_us: clamp_u32(now.saturating_sub(encoded.captured_us)),
                    net_us: flow.net_us(),
                    kbps: rate.kbps(),
                    skipped: u16::try_from(skipped.swap(0, Ordering::Relaxed)).unwrap_or(u16::MAX),
                    data: encoded.data,
                };
                flow.sent(seq, now, picture.data.len(), picture.key);
                gate.follow(&flow, 0, rate.kbps());
                conn.write_frame(&mut tcp, &Down::Picture(picture).encode())?;
            }
            Ok(Msg::Ack { seq, held_us, at_us }) => {
                let acked = flow.acked(seq, held_us, at_us);
                let (was, reading) = (rate.kbps(), flow.reading(at_us));
                rate.follow(&reading);
                if rate.kbps() != was {
                    debug!(kbps = rate.kbps(), ?reading, "remote view: bitrate");
                }
                gate.follow(&flow, acked, rate.kbps());
            }
            Ok(Msg::Display(screen)) => {
                info!(?screen, "remote view: the screen changed size");
                conn.write_frame(&mut tcp, &screen.encode())?;
                conn.write_frame(&mut tcp, &Down::Mirroring(true).encode())?;
            }
            Ok(Msg::Clipboard(text)) => conn.write_frame(&mut tcp, &Down::Clipboard(text).encode())?,
            Ok(Msg::Stopped(why)) => {
                warn!(%why, "remote view: the capture stopped");
                break;
            }
            Ok(Msg::Closed) | Err(_) => break,
        }
    }
    drop(running);
    Ok(())
}

/// The viewer sends nothing on this stream, so a read ends only when it has
/// gone. A still screen writes nothing, and this is how that session ends.
fn watch_for_close(tcp: &TcpStream, tx: Sender<Msg>) -> io::Result<()> {
    let mut probe = tcp.try_clone()?;
    thread::Builder::new().name("kvm-view-close".into()).spawn(move || {
        let mut buf = [0u8; 64];
        while !matches!(probe.read(&mut buf), Ok(0) | Err(_)) {}
        let _ = tx.send(Msg::Closed);
    })?;
    Ok(())
}

fn input(mut tcp: TcpStream, mut conn: Conn, shared: &Shared) -> io::Result<()> {
    Shared::take_over(&shared.input, &tcp)?;
    // The output's own size: the picture's, which the viewer points in.
    let mut injector = match WaylandInjector::connect(0, 0) {
        Ok(injector) => Some(injector),
        Err(why) => {
            warn!(%why, "remote view: no keyboard or pointer to drive; showing only");
            None
        }
    };
    let result = (|| loop {
        let Some(up) = Up::decode(&conn.read_frame(&mut tcp, UP_FRAME)?)? else {
            continue;
        };
        match up {
            Up::Ack { seq, held_us } => {
                let at_us = now_us();
                if let Some(tx) = &*lock(&shared.to_viewer) {
                    let _ = tx.send(Msg::Ack { seq, held_us, at_us });
                }
            }
            Up::Keyframe => shared.force_key.store(true, Ordering::Relaxed),
            // The screen is this machine's own, at its own size.
            Up::Size(_) | Up::Mirror(_) => {}
            Up::ClipboardSet(text) => {
                crate::clip::write_text(&text);
            }
            Up::ClipboardGet => {
                let text = crate::clip::read_text().filter(|t| t.len() <= MAX_CLIPBOARD);
                if let (Some(text), Some(tx)) = (text, &*lock(&shared.to_viewer)) {
                    let _ = tx.send(Msg::Clipboard(text));
                }
            }
            Up::Event(bytes) => match (protocol::decode(&bytes), injector.as_mut()) {
                (Ok((_, packet)), Some(injector)) => injector.handle(&packet),
                (Ok(_), None) => {}
                (Err(e), _) => warn!(?e, "remote view: a bad input packet"),
            },
        }
    })();
    // Whoever sits down next finds nothing held.
    if let Some(injector) = injector.as_mut() {
        injector.handle(&Packet::Leave);
    }
    result
}

fn clamp_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
