//! Ember clipboard worker: TCP listen (same port as UDP), duplex with novus.
//!
//! Resilience (Mac→Linux was dying after one bad Leave):
//! - `pending_push` if Leave fires with no peer — flush on next accept
//! - Ack read timeout is soft (do not drop peer); only real socket errors drop
//! - pasteboard CLI is hard-capped in `platform` so we always reach Ack path

use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use super::platform;
use super::proto::{
    AckStatus, MIME_TEXT_UTF8, Message, Role, hash_text, read_message, write_message,
};

const MAX_BYTES: u32 = 8 * 1024 * 1024;
const ACK_WAIT: Duration = Duration::from_secs(3);
const POLL_READ: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy)]
pub enum ClipJob {
    /// Leave remote: push Mac pasteboard → novus.
    PushToNovus,
}

#[derive(Clone)]
pub struct ClipHandle {
    tx: Sender<ClipJob>,
}

impl ClipHandle {
    pub fn push_to_novus(&self) {
        let _ = self.tx.send(ClipJob::PushToNovus);
    }
}

/// Spawn TCP listener on `bind` (e.g. `0.0.0.0:4242`) and clip worker.
pub fn spawn(bind: &str) -> std::io::Result<ClipHandle> {
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    let local = listener.local_addr().ok();
    info!(?local, "clipboard TCP listening (same port as UDP)");

    let (tx, rx) = mpsc::channel::<ClipJob>();
    thread::Builder::new()
        .name("kvm-clip".into())
        .spawn(move || worker_main(listener, rx))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(ClipHandle { tx })
}

struct Cache {
    last_sent: Option<u32>,
    last_recv: Option<u32>,
}

impl Cache {
    fn new() -> Self {
        Self {
            last_sent: None,
            last_recv: None,
        }
    }

    fn should_skip_send(&self, hash: u32) -> bool {
        self.last_sent == Some(hash) || self.last_recv == Some(hash)
    }
}

fn arm_poll_read(s: &TcpStream) {
    s.set_read_timeout(Some(POLL_READ)).ok();
}

fn arm_ack_wait(s: &TcpStream) {
    s.set_read_timeout(Some(ACK_WAIT)).ok();
    // Keep socket blocking for writes — short read timeouts on Darwin can
    // surface as EAGAIN on subsequent write_all if we leave weird state.
    s.set_nonblocking(false).ok();
}

fn is_soft_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    ) || e.raw_os_error() == Some(35) // EAGAIN on Darwin
}

fn worker_main(listener: TcpListener, jobs: Receiver<ClipJob>) {
    let mut cache = Cache::new();
    let mut peer: Option<TcpStream> = None;
    let mut out_seq: u32 = 1;
    // Leave sprays 3× UDP Leaves → 3 jobs; only push once per burst.
    let mut last_push = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);
    // If Leave happens while disconnected, push once peer returns.
    let mut pending_push = false;

    loop {
        // Accept
        match listener.accept() {
            Ok((mut s, addr)) => {
                info!(%addr, "clipboard TCP accept");
                s.set_nodelay(true).ok();
                s.set_nonblocking(false).ok();
                s.set_read_timeout(Some(Duration::from_millis(500))).ok();
                // Expect Hello from novus, reply.
                match read_message(&mut s, MAX_BYTES) {
                    Ok((_, Message::Hello { role })) => {
                        info!(?role, "clip hello from novus");
                    }
                    Ok(other) => info!(?other, "clip first msg (not hello)"),
                    Err(e) => warn!(%e, "clip hello wait failed"),
                }
                let seq = out_seq;
                out_seq = out_seq.wrapping_add(1);
                if let Err(e) = write_message(&mut s, seq, &Message::Hello { role: Role::Ember }) {
                    warn!(%e, "clip hello reply failed");
                } else {
                    arm_poll_read(&s);
                    peer = Some(s);
                    info!("clip peer ready (duplex)");
                    if pending_push {
                        info!("clip flushing pending PushToNovus after reconnect");
                        pending_push = false;
                        last_push = Instant::now();
                        if let Some(ref mut s) = peer {
                            if let Err(e) = push_local(s, &mut cache, &mut out_seq) {
                                warn!(%e, "clip pending push failed");
                                // Soft failures keep peer; hard drop below.
                                if !is_soft_timeout(&e) {
                                    peer = None;
                                    pending_push = true;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => {
                warn!(%e, "clip accept error");
                thread::sleep(Duration::from_millis(200));
            }
        }

        // Jobs
        match jobs.recv_timeout(Duration::from_millis(100)) {
            Ok(ClipJob::PushToNovus) => {
                if last_push.elapsed() < Duration::from_millis(400) {
                    info!("clip PushToNovus debounced (Leave spray)");
                } else if let Some(ref mut s) = peer {
                    info!("clip job PushToNovus (leave)");
                    last_push = Instant::now();
                    match push_local(s, &mut cache, &mut out_seq) {
                        Ok(()) => {
                            pending_push = false;
                        }
                        Err(e) if is_soft_timeout(&e) => {
                            // Offer may already be on the wire / applied; keep peer.
                            warn!(%e, "clip push Ack wait soft-timeout — keeping peer");
                            arm_poll_read(s);
                        }
                        Err(e) => {
                            warn!(%e, "clip push to novus failed — drop peer, pending");
                            peer = None;
                            pending_push = true;
                        }
                    }
                } else {
                    pending_push = true;
                    warn!("clip PushToNovus deferred — no TCP peer (will flush on connect)");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("clipboard worker stopping");
                return;
            }
        }
        loop {
            match jobs.try_recv() {
                Ok(ClipJob::PushToNovus) => {
                    info!("clip PushToNovus drained (debounced)");
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // Inbound from novus (Enter push)
        if let Some(ref mut s) = peer {
            arm_poll_read(s);
            match read_message(s, MAX_BYTES) {
                Ok((seq, Message::Offer { mime, hash, body })) => {
                    info!(
                        seq,
                        hash,
                        mime,
                        bytes = body.len(),
                        "clip inbound Offer from novus"
                    );
                    let ok = apply_inbound(mime, hash, &body, &mut cache);
                    let aseq = out_seq;
                    out_seq = out_seq.wrapping_add(1);
                    if let Err(e) = write_message(
                        s,
                        aseq,
                        &Message::Ack {
                            of_seq: seq,
                            status: if ok { AckStatus::Ok } else { AckStatus::Reject },
                        },
                    ) {
                        warn!(%e, "clip Ack write failed");
                        peer = None;
                    }
                }
                Ok((seq, Message::Empty)) => {
                    platform::clear();
                    cache.last_recv = Some(hash_text(""));
                    cache.last_sent = Some(hash_text(""));
                    info!("clip cleared from novus Empty");
                    let aseq = out_seq;
                    out_seq = out_seq.wrapping_add(1);
                    if let Err(e) = write_message(
                        s,
                        aseq,
                        &Message::Ack {
                            of_seq: seq,
                            status: AckStatus::Ok,
                        },
                    ) {
                        warn!(%e, "clip Empty Ack write failed");
                        peer = None;
                    }
                }
                Ok((_, Message::Hello { role })) => {
                    debug!(?role, "clip hello");
                }
                Ok((_, Message::Ack { .. })) => {}
                Err(e) if is_soft_timeout(&e) => {}
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                    info!("clip peer closed");
                    peer = None;
                }
                Err(e) => {
                    warn!(%e, "clip read error");
                    peer = None;
                }
            }
        }
    }
}

fn push_local(s: &mut TcpStream, cache: &mut Cache, out_seq: &mut u32) -> std::io::Result<()> {
    s.set_nonblocking(false).ok();
    let text = platform::read_text().unwrap_or_default();
    if text.len() as u32 > MAX_BYTES {
        warn!(bytes = text.len(), "clip local too large; skip");
        return Ok(());
    }
    if text.is_empty() {
        if cache.last_sent.is_none() && cache.last_recv.is_none() {
            debug!("clip skip empty push");
            return Ok(());
        }
        let seq = *out_seq;
        *out_seq = out_seq.wrapping_add(1);
        write_message(s, seq, &Message::Empty)?;
        arm_ack_wait(s);
        match read_message(s, MAX_BYTES) {
            Ok(_) => info!("clip Empty → novus"),
            Err(e) if is_soft_timeout(&e) => {
                warn!(%e, "clip Empty Ack soft-timeout");
            }
            Err(e) => {
                arm_poll_read(s);
                return Err(e);
            }
        }
        cache.last_sent = Some(hash_text(""));
        arm_poll_read(s);
        return Ok(());
    }
    let hash = hash_text(&text);
    if cache.should_skip_send(hash) {
        info!(hash, "clip skip unchanged → novus");
        return Ok(());
    }
    let seq = *out_seq;
    *out_seq = out_seq.wrapping_add(1);
    info!(seq, hash, bytes = text.len(), "clip sending Offer → novus");
    write_message(
        s,
        seq,
        &Message::Offer {
            mime: MIME_TEXT_UTF8,
            hash,
            body: text.as_bytes().to_vec(),
        },
    )?;
    arm_ack_wait(s);
    let deadline = Instant::now() + ACK_WAIT;
    loop {
        match read_message(s, MAX_BYTES) {
            Ok((
                _,
                Message::Ack {
                    status: AckStatus::Ok,
                    ..
                },
            )) => {
                cache.last_sent = Some(hash);
                info!(hash, bytes = text.len(), "clip Offer → novus ok");
                break;
            }
            Ok((_, Message::Ack { status, .. })) => {
                warn!(?status, "clip Offer ack not ok");
                break;
            }
            Ok((
                iseq,
                Message::Offer {
                    mime: imime,
                    hash: ih,
                    body,
                },
            )) => {
                // Novus pushed on Enter while we pushed on Leave — apply and
                // keep waiting for our Ack.
                info!(
                    iseq,
                    hash = ih,
                    mime = imime,
                    bytes = body.len(),
                    "clip inbound Offer while awaiting Ack — apply now"
                );
                let ok = apply_inbound(imime, ih, &body, cache);
                let aseq = *out_seq;
                *out_seq = out_seq.wrapping_add(1);
                let _ = write_message(
                    s,
                    aseq,
                    &Message::Ack {
                        of_seq: iseq,
                        status: if ok { AckStatus::Ok } else { AckStatus::Reject },
                    },
                );
                if Instant::now() >= deadline {
                    warn!("clip Offer Ack wait expired after handling peer Offer");
                    break;
                }
            }
            Ok(other) => {
                debug!(?other, "clip non-Ack while awaiting Ack");
                if Instant::now() >= deadline {
                    break;
                }
            }
            Err(e) if is_soft_timeout(&e) => {
                warn!(
                    %e,
                    hash,
                    "clip Offer Ack soft-timeout — Offer was sent; keeping peer"
                );
                arm_poll_read(s);
                return Err(e);
            }
            Err(e) => {
                arm_poll_read(s);
                return Err(e);
            }
        }
    }
    arm_poll_read(s);
    Ok(())
}

fn apply_inbound(mime: u8, hash: u32, body: &[u8], cache: &mut Cache) -> bool {
    if cache.last_recv == Some(hash) {
        info!(hash, "clip skip apply (already have)");
        return true;
    }
    if mime != MIME_TEXT_UTF8 {
        warn!(mime, bytes = body.len(), "clip Mac rejects non-text mime");
        return false;
    }
    let text = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(e) => {
            warn!(%e, "clip inbound text not utf-8");
            return false;
        }
    };
    if platform::write_text(text) {
        cache.last_recv = Some(hash);
        cache.last_sent = Some(hash);
        info!(hash, bytes = text.len(), "clip applied from novus → Mac");
        true
    } else {
        warn!(
            hash,
            bytes = text.len(),
            "clip apply from novus FAILED (pbcopy)"
        );
        false
    }
}
