//! The view port: paired connections, one viewer at a time. A new viewer
//! takes over from the one before.

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
use crate::inject::{CgInjector, Injector};
use crate::link::{Conn, Keys};
use crate::protocol::{self, Packet};
use crate::view_wire::{Cursor, Down, MAX_CLIPBOARD, PORT_OFFSET, Picture, Size, Stream, Up};

/// How long a connection has to prove it is the paired machine. These run
/// one at a time on the accept thread, so a stranger cannot pile up threads.
const HANDSHAKE: Duration = Duration::from_secs(5);
const OPEN_FRAME: u32 = 16;
/// Input is small; a frame this big is clipboard text.
const UP_FRAME: u32 = MAX_CLIPBOARD as u32 + 16;

/// What a viewer is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Screen,
    /// A moving test card through the real encoder and wire: needs no
    /// Screen Recording grant.
    TestPattern,
}

/// A picture from the encoder, before what the wire says of its way.
pub struct Encoded {
    pub key: bool,
    pub pts_us: u64,
    /// When its frame was captured ([`now_us`]).
    pub captured_us: u64,
    /// The capture it came from ([`Feed::generation`]).
    pub generation: u32,
    pub data: Vec<u8>,
}

pub enum Msg {
    Picture(Encoded),
    /// The viewer decoded picture `seq`; the ack came at `at_us`.
    Ack {
        seq: u32,
        held_us: u32,
        at_us: u64,
    },
    Clipboard(String),
    /// The pointer changed, for the viewer to draw.
    Cursor(Cursor),
    /// The viewer said a new size or Mirror ([`Shared::size`],
    /// [`Shared::mirror`]).
    Resize,
    /// A capture stopped on its own.
    Stopped(u32),
    Closed,
}

/// Where a session's encoded pictures go.
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
    /// Which capture of the session this is: pictures and stops from one
    /// before it are let go.
    pub generation: u32,
}

#[derive(Default)]
struct Shared {
    video: Mutex<Option<TcpStream>>,
    input: Mutex<Option<TcpStream>>,
    force_key: Arc<AtomicBool>,
    /// The video stream's queue, for clipboard answers and acks.
    to_viewer: Mutex<Option<Sender<Msg>>>,
    /// The test card's clipboard: it never touches this Mac's.
    test_clipboard: Mutex<String>,
    /// The size the viewer said last and the video stream has not taken.
    size: Mutex<Option<Size>>,
    /// The viewer would see this Mac's own screen, not one of its own.
    mirror: AtomicBool,
}

impl Shared {
    /// Tell the video stream the viewer's size or Mirror changed.
    fn resize(&self) {
        if let Some(tx) = &*lock(&self.to_viewer) {
            let _ = tx.send(Msg::Resize);
        }
    }

    fn take_over(slot: &Mutex<Option<TcpStream>>, tcp: &TcpStream) -> io::Result<()> {
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = slot.replace(tcp.try_clone()?) {
            let _ = old.shutdown(Shutdown::Both);
        }
        Ok(())
    }
}

/// Listen on the view port (the KVM port plus [`PORT_OFFSET`]).
pub fn spawn(bind: &str, keys: Keys, source: Source) -> io::Result<()> {
    let mut addr: SocketAddr = bind
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{bind}: {e}")))?;
    addr.set_port(addr.port() + PORT_OFFSET);
    let listener = TcpListener::bind(addr)?;
    info!(%addr, ?source, "remote view listening (TCP)");
    thread::Builder::new().name("kvm-view".into()).spawn(move || {
        let shared = Arc::new(Shared::default());
        for tcp in listener.incoming() {
            let opened = tcp.and_then(|mut tcp| {
                let (conn, stream) = open(&mut tcp, &keys)?;
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
                        Stream::Video => video(tcp, conn, shared, source),
                        Stream::Input => input(tcp, conn, shared, source),
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

fn clamp_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn open(tcp: &mut TcpStream, keys: &Keys) -> io::Result<(Conn, Stream)> {
    tcp.set_nodelay(true)?;
    tcp.set_read_timeout(Some(HANDSHAKE))?;
    let mut conn = Conn::accept(tcp, keys)?;
    let stream = Stream::decode(&conn.read_frame(tcp, OPEN_FRAME)?)?;
    tcp.set_read_timeout(None)?;
    Ok((conn, stream))
}

/// What a video stream starts its captures with.
struct Captures {
    source: Source,
    /// The display made for the viewer, which goes with the stream.
    screen: capture::Screen,
    tx: Sender<Msg>,
    skipped: Arc<AtomicU32>,
    force_key: Arc<AtomicBool>,
    generation: u32,
}

impl Captures {
    /// A new capture at `size`, with its own gate.
    fn start(&mut self, size: Option<Size>) -> Result<(Down, Arc<Gate>, capture::Running), String> {
        self.generation += 1;
        let gate = Arc::new(Gate::default());
        let feed = Feed {
            tx: self.tx.clone(),
            gate: Arc::clone(&gate),
            skipped: Arc::clone(&self.skipped),
            force_key: Arc::clone(&self.force_key),
            generation: self.generation,
        };
        let (display, running) = capture::start(self.source, size, &mut self.screen, feed)?;
        Ok((display, gate, running))
    }
}

fn video(mut tcp: TcpStream, mut conn: Conn, shared: Arc<Shared>, source: Source) -> io::Result<()> {
    Shared::take_over(&shared.video, &tcp)?;
    let (tx, rx) = mpsc::channel();
    watch_for_close(&tcp, tx.clone())?;
    *lock(&shared.to_viewer) = Some(tx.clone());
    let skipped = Arc::new(AtomicU32::new(0));
    let mut captures = Captures {
        source,
        screen: capture::Screen::default(),
        tx,
        skipped: Arc::clone(&skipped),
        force_key: Arc::clone(&shared.force_key),
        generation: 0,
    };
    // The viewer says its size and Mirror before it opens this stream. A
    // screen of the window's size unless it mirrors.
    let mut size = lock(&shared.size).take();
    let mut mirror = shared.mirror.load(Ordering::Relaxed);
    let wanted = |size: Option<Size>, mirror: bool| size.filter(|_| !mirror);
    let (screen, mut gate, running) = match captures.start(wanted(size, mirror)) {
        Ok(v) => v,
        Err(why) => {
            warn!(%why, "remote view: no picture to send");
            return conn.write_frame(&mut tcp, &Down::Refused(why).encode());
        }
    };
    info!(?screen, "remote view: sending");
    conn.write_frame(&mut tcp, &screen.encode())?;
    let mut running = Some(running);
    let (mut flow, mut rate, mut seq) = (Flow::default(), Rate::default(), 0u32);
    loop {
        match rx.recv() {
            // From a capture before this one.
            Ok(Msg::Picture(encoded)) if encoded.generation != captures.generation => {}
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
            Ok(Msg::Clipboard(text)) => conn.write_frame(&mut tcp, &Down::Clipboard(text).encode())?,
            Ok(Msg::Cursor(cursor)) => conn.write_frame(&mut tcp, &Down::Cursor(cursor).encode())?,
            Ok(Msg::Resize) => {
                // What came while the last was applied: the newest wins.
                let was = wanted(size, mirror);
                size = lock(&shared.size).take().or(size);
                mirror = shared.mirror.load(Ordering::Relaxed);
                if wanted(size, mirror) == was {
                    continue;
                }
                running = None;
                match captures.start(wanted(size, mirror)) {
                    Ok((screen, new_gate, new_running)) => {
                        info!(?screen, "remote view: sending, at the window's new size");
                        (gate, running) = (new_gate, Some(new_running));
                        conn.write_frame(&mut tcp, &screen.encode())?;
                    }
                    Err(why) => {
                        warn!(%why, "remote view: no picture to send at the new size");
                        conn.write_frame(&mut tcp, &Down::Refused(why).encode())?;
                        break;
                    }
                }
            }
            Ok(Msg::Stopped(generation)) if generation != captures.generation => {}
            Ok(Msg::Stopped(_) | Msg::Closed) | Err(_) => break,
        }
    }
    // The capture first, then the display made for it, with `captures`.
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

fn input(mut tcp: TcpStream, mut conn: Conn, shared: Arc<Shared>, source: Source) -> io::Result<()> {
    Shared::take_over(&shared.input, &tcp)?;
    let mut injector = CgInjector::new();
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
            Up::Size(size) => {
                *lock(&shared.size) = Some(size);
                shared.resize();
            }
            Up::Mirror(mirror) => {
                shared.mirror.store(mirror, Ordering::Relaxed);
                shared.resize();
            }
            Up::ClipboardSet(text) if source == Source::TestPattern => {
                info!(bytes = text.len(), "test card: clipboard set");
                *lock(&shared.test_clipboard) = text;
            }
            Up::ClipboardSet(text) => {
                crate::clip::write_text(&text);
            }
            Up::ClipboardGet => {
                let text = if source == Source::TestPattern {
                    Some(format!("{} (from the test card)", lock(&shared.test_clipboard)))
                } else {
                    crate::clip::read_text()
                };
                // An image on the pasteboard has no text: nothing to send.
                if let (Some(text), Some(tx)) = (text.filter(|t| t.len() <= MAX_CLIPBOARD), &*lock(&shared.to_viewer)) {
                    let _ = tx.send(Msg::Clipboard(text));
                }
            }
            Up::Event(bytes) => match protocol::decode(&bytes) {
                // The test card drives nothing: this Mac's pointer is someone's.
                Ok((_, packet)) if source == Source::TestPattern => info!(?packet, "test card: input"),
                Ok((_, packet)) => injector.handle(&packet),
                Err(e) => warn!(?e, "remote view: a bad input packet"),
            },
        }
    })();
    // Whoever sits down next finds nothing held.
    if source != Source::TestPattern {
        injector.handle(&Packet::Leave);
    }
    result
}
