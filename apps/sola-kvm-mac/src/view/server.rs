//! The view port: paired connections, one viewer at a time. A new viewer
//! takes over from the one before.

use std::io::{self, Read};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tracing::{info, warn};

use super::capture;
use crate::inject::{CgInjector, Injector};
use crate::link::{Conn, Keys};
use crate::protocol::{self, Packet};
use crate::view_wire::{Down, MAX_CLIPBOARD, PORT_OFFSET, Stream, Up};

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

pub enum Msg {
    Picture(Down),
    Clipboard(String),
    Closed,
}

/// Where a session's encoded pictures go.
pub struct Feed {
    pub tx: Sender<Msg>,
    /// Pictures encoding or queued to send. The capture side skips a frame
    /// when too many are out, so a slow network drops frames before they
    /// are encoded and no later picture loses its reference.
    pub in_flight: Arc<AtomicUsize>,
    pub force_key: Arc<AtomicBool>,
}

#[derive(Default)]
struct Shared {
    video: Mutex<Option<TcpStream>>,
    input: Mutex<Option<TcpStream>>,
    force_key: Arc<AtomicBool>,
    /// The video stream's queue, for clipboard answers.
    to_viewer: Mutex<Option<Sender<Msg>>>,
    /// The test card's clipboard: it never touches this Mac's.
    test_clipboard: Mutex<String>,
}

impl Shared {
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

fn video(mut tcp: TcpStream, mut conn: Conn, shared: Arc<Shared>, source: Source) -> io::Result<()> {
    Shared::take_over(&shared.video, &tcp)?;
    let (tx, rx) = mpsc::channel();
    watch_for_close(&tcp, tx.clone())?;
    *lock(&shared.to_viewer) = Some(tx.clone());
    let in_flight = Arc::new(AtomicUsize::new(0));
    let feed = Feed {
        tx,
        in_flight: Arc::clone(&in_flight),
        force_key: Arc::clone(&shared.force_key),
    };
    let (screen, running) = match capture::start(source, feed) {
        Ok(v) => v,
        Err(why) => {
            warn!(%why, "remote view: no picture to send");
            return conn.write_frame(&mut tcp, &Down::Refused(why).encode());
        }
    };
    info!(?screen, "remote view: sending");
    conn.write_frame(&mut tcp, &screen.encode())?;
    loop {
        match rx.recv() {
            Ok(Msg::Picture(picture)) => {
                let sent = conn.write_frame(&mut tcp, &picture.encode());
                in_flight.fetch_sub(1, Ordering::Relaxed);
                sent?;
            }
            Ok(Msg::Clipboard(text)) => conn.write_frame(&mut tcp, &Down::Clipboard(text).encode())?,
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

fn input(mut tcp: TcpStream, mut conn: Conn, shared: Arc<Shared>, source: Source) -> io::Result<()> {
    Shared::take_over(&shared.input, &tcp)?;
    let mut injector = CgInjector::new();
    let result = (|| loop {
        match Up::decode(&conn.read_frame(&mut tcp, UP_FRAME)?)? {
            Up::Keyframe => shared.force_key.store(true, Ordering::Relaxed),
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
