//! Novus clipboard worker: TCP client to ember, jobs from the input thread.

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use super::platform;
use super::platform::LocalClip;
use super::proto::{
    AckStatus, MIME_PNG, MIME_TEXT_UTF8, Message, Role, hash_text, read_message, write_message,
};

/// Work enqueued from the input / enter-leave path (never blocks long).
#[derive(Debug, Clone, Copy)]
pub enum ClipJob {
    /// Enter remote: push novus clipboard → ember.
    PushToMac,
}

/// Handle held by the server loop.
#[derive(Clone)]
pub struct ClipHandle {
    tx: Sender<ClipJob>,
}

impl ClipHandle {
    pub fn notify(&self, job: ClipJob) {
        if self.tx.send(job).is_err() {
            debug!("clip worker gone; drop job");
        }
    }

    pub fn push_to_mac(&self) {
        self.notify(ClipJob::PushToMac);
    }

    /// Linux client Leave: push local clipboard to the server.
    pub fn push_to_peer(&self) {
        self.notify(ClipJob::PushToMac);
    }
}

/// No-op handle when clipboard is disabled.
pub fn disabled_handle() -> ClipHandle {
    let (tx, rx) = mpsc::channel();
    // Drain forever in a tiny thread so send never blocks on a full buffer.
    thread::Builder::new()
        .name("kvm-clip-disabled".into())
        .spawn(move || while rx.recv().is_ok() {})
        .ok();
    ClipHandle { tx }
}

pub struct ClipConfig {
    pub peer_host: String,
    pub peer_port: u16,
    pub max_bytes: u32,
    pub sync_on_enter: bool,
    pub sync_on_leave: bool,
}

/// Spawn the clip worker. Returns a handle for Enter notifications.
/// Leave-side offers arrive on the TCP stream (ember pushes).
pub fn spawn(cfg: ClipConfig) -> ClipHandle {
    let (tx, rx) = mpsc::channel::<ClipJob>();
    thread::Builder::new()
        .name("kvm-clip".into())
        .spawn(move || worker_main(cfg, rx))
        .expect("spawn kvm-clip");
    ClipHandle { tx }
}

/// TCP listen on the same bind as UDP (Linux client / ember role).
pub fn spawn_listen(bind: &str, cfg: ClipConfig) -> std::io::Result<ClipHandle> {
    use std::net::TcpListener;
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    let local = listener.local_addr().ok();
    info!(?local, "clipboard TCP listening (same port as UDP)");
    let (tx, rx) = mpsc::channel::<ClipJob>();
    thread::Builder::new()
        .name("kvm-clip-listen".into())
        .spawn(move || listen_main(listener, cfg, rx))
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

fn is_soft_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

fn worker_main(cfg: ClipConfig, jobs: Receiver<ClipJob>) {
    info!(
        peer = %format!("{}:{}", cfg.peer_host, cfg.peer_port),
        max_bytes = cfg.max_bytes,
        sync_on_enter = cfg.sync_on_enter,
        sync_on_leave = cfg.sync_on_leave,
        "clipboard worker starting (TCP client, same port as UDP)"
    );
    platform::probe_and_log();

    let mut cache = Cache::new();
    let mut stream: Option<TcpStream> = None;
    let mut out_seq: u32 = 1;
    let mut last_connect_try = Instant::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap_or_else(Instant::now);

    loop {
        // 1) Jobs (non-blocking poll with short timeout via recv_timeout).
        match jobs.recv_timeout(Duration::from_millis(100)) {
            Ok(ClipJob::PushToMac) if cfg.sync_on_enter => {
                info!("clip job PushToMac (enter)");
                // Always allow an immediate connect attempt for Enter jobs.
                last_connect_try = Instant::now()
                    .checked_sub(Duration::from_secs(60))
                    .unwrap_or_else(Instant::now);
                ensure_connected(&cfg, &mut stream, &mut last_connect_try, &mut out_seq);
                if let Some(ref mut s) = stream {
                    if let Err(e) = push_local_to_peer(s, &cfg, &mut cache, &mut out_seq) {
                        if is_soft_timeout(&e) {
                            warn!(%e, "clip push to mac soft-timeout — keeping stream");
                        } else {
                            warn!(%e, "clip push to mac failed");
                            stream = None;
                        }
                    }
                } else {
                    warn!("clip PushToMac skipped — not connected to ember TCP");
                }
            }
            Ok(ClipJob::PushToMac) => {
                info!("clip job PushToMac ignored (sync_on_enter=false)");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("clipboard worker stopping");
                return;
            }
        }

        // Drain any burst of jobs quickly.
        loop {
            match jobs.try_recv() {
                Ok(ClipJob::PushToMac) if cfg.sync_on_enter => {
                    info!("clip job PushToMac (enter, drained)");
                    ensure_connected(&cfg, &mut stream, &mut last_connect_try, &mut out_seq);
                    if let Some(ref mut s) = stream {
                        if let Err(e) = push_local_to_peer(s, &cfg, &mut cache, &mut out_seq) {
                            if is_soft_timeout(&e) {
                                warn!(%e, "clip push drained soft-timeout — keeping stream");
                            } else {
                                warn!(%e, "clip push to mac failed");
                                stream = None;
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        // 2) Read inbound Offers from ember (Leave path).
        //    Apply is hard-capped in platform so we always reach the Ack write
        //    (a hung wl-copy previously froze this worker forever).
        if let Some(ref mut s) = stream {
            s.set_nonblocking(false).ok();
            s.set_read_timeout(Some(Duration::from_millis(1))).ok();
            match read_message(s, cfg.max_bytes) {
                Ok((seq, Message::Offer { mime, hash, body })) => {
                    info!(
                        seq,
                        hash,
                        mime,
                        bytes = body.len(),
                        "clip inbound Offer from peer"
                    );
                    let ok = apply_inbound(mime, hash, &body, &mut cache);
                    if let Err(e) = write_message(
                        s,
                        out_seq,
                        &Message::Ack {
                            of_seq: seq,
                            status: if ok { AckStatus::Ok } else { AckStatus::Reject },
                        },
                    ) {
                        warn!(%e, "clip Ack write to ember failed");
                        stream = None;
                    }
                    out_seq = out_seq.wrapping_add(1);
                }
                Ok((seq, Message::Empty)) => {
                    info!(seq, "clip inbound Empty from ember");
                    let ok = platform::clear();
                    if ok {
                        cache.last_recv = Some(hash_text(""));
                        info!("clip cleared from ember Empty");
                    } else {
                        warn!("clip clear from Empty failed");
                    }
                    if let Err(e) = write_message(
                        s,
                        out_seq,
                        &Message::Ack {
                            of_seq: seq,
                            status: if ok { AckStatus::Ok } else { AckStatus::Reject },
                        },
                    ) {
                        warn!(%e, "clip Empty Ack write failed");
                        stream = None;
                    }
                    out_seq = out_seq.wrapping_add(1);
                }
                Ok((_seq, Message::Hello { role })) => {
                    info!(?role, "clip hello from peer");
                }
                Ok((_seq, Message::Ack { of_seq, status })) => {
                    info!(of_seq, ?status, "clip ack (async)");
                }
                Err(e) if is_soft_timeout(&e) => {}
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                    warn!("clip peer closed TCP");
                    stream = None;
                    // Reconnect ASAP so the next Leave has a peer.
                    last_connect_try = Instant::now()
                        .checked_sub(Duration::from_secs(60))
                        .unwrap_or_else(Instant::now);
                }
                Err(e) => {
                    warn!(%e, "clip read error");
                    stream = None;
                    last_connect_try = Instant::now()
                        .checked_sub(Duration::from_secs(60))
                        .unwrap_or_else(Instant::now);
                }
            }
        } else if last_connect_try.elapsed() > Duration::from_secs(2) {
            // Opportunistic connect so Leave offers can arrive once linked.
            ensure_connected(&cfg, &mut stream, &mut last_connect_try, &mut out_seq);
        }
    }
}

fn ensure_connected(
    cfg: &ClipConfig,
    stream: &mut Option<TcpStream>,
    last_try: &mut Instant,
    out_seq: &mut u32,
) {
    if stream.is_some() {
        return;
    }
    if last_try.elapsed() < Duration::from_secs(1) {
        return;
    }
    *last_try = Instant::now();
    let addr_str = format!("{}:{}", cfg.peer_host, cfg.peer_port);
    let sock_addr: SocketAddr = match addr_str.to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => {
                warn!(%addr_str, "clip peer resolve empty");
                return;
            }
        },
        Err(e) => {
            warn!(%e, %addr_str, "clip peer resolve failed");
            return;
        }
    };
    match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(2)) {
        Ok(mut s) => {
            s.set_nodelay(true).ok();
            s.set_nonblocking(false).ok();
            if let Err(e) = write_message(&mut s, *out_seq, &Message::Hello { role: Role::Novus }) {
                warn!(%e, "clip hello send failed");
                return;
            }
            *out_seq = out_seq.wrapping_add(1);
            // Optional peer hello.
            s.set_read_timeout(Some(Duration::from_millis(500))).ok();
            match read_message(&mut s, cfg.max_bytes) {
                Ok((_, Message::Hello { role })) => {
                    info!(?role, %addr_str, "clipboard TCP connected");
                }
                Ok(other) => {
                    debug!(?other, "clip first message not hello");
                    info!(%addr_str, "clipboard TCP connected");
                }
                Err(e) => {
                    debug!(%e, "clip no hello reply yet");
                    info!(%addr_str, "clipboard TCP connected");
                }
            }
            s.set_read_timeout(Some(Duration::from_millis(1))).ok();
            *stream = Some(s);
        }
        Err(e) => {
            // Visible at info once in a while so "no peer" is diagnosable.
            debug!(%e, %addr_str, "clip connect failed");
        }
    }
}

fn push_local_to_peer(
    s: &mut TcpStream,
    cfg: &ClipConfig,
    cache: &mut Cache,
    out_seq: &mut u32,
) -> std::io::Result<()> {
    let local = platform::read_local();
    info!(
        bytes = local.len(),
        empty = local.is_empty(),
        mime = local.mime(),
        "clip push read local"
    );
    if local.len() as u32 > cfg.max_bytes {
        warn!(
            bytes = local.len(),
            max = cfg.max_bytes,
            "clip local too large; skip push"
        );
        return Ok(());
    }
    if local.is_empty() {
        if cache.last_sent.is_none() && cache.last_recv.is_none() {
            info!("clip skip empty → peer (nothing ever sent)");
            return Ok(());
        }
        let seq = *out_seq;
        *out_seq = out_seq.wrapping_add(1);
        write_message(s, seq, &Message::Empty)?;
        // Wait ack briefly.
        s.set_read_timeout(Some(Duration::from_secs(2))).ok();
        let _ = read_message(s, cfg.max_bytes);
        s.set_read_timeout(Some(Duration::from_millis(1))).ok();
        cache.last_sent = Some(hash_text(""));
        info!("clip Empty → peer");
        return Ok(());
    }
    let hash = local.hash();
    if cache.should_skip_send(hash) {
        info!(
            hash,
            last_sent = ?cache.last_sent,
            last_recv = ?cache.last_recv,
            "clip skip unchanged → peer"
        );
        return Ok(());
    }
    let seq = *out_seq;
    *out_seq = out_seq.wrapping_add(1);
    let nbytes = local.len();
    let mime = local.mime();
    info!(seq, hash, mime, bytes = nbytes, "clip sending Offer → peer");
    write_message(
        s,
        seq,
        &Message::Offer {
            mime: local.mime(),
            hash,
            body: local.body().to_vec(),
        },
    )?;
    s.set_nonblocking(false).ok();
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    // Peer may push an Offer at the same moment (Leave/Enter race). Drain
    // any inbound Offers while waiting for our Ack.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match read_message(s, cfg.max_bytes) {
            Ok((
                _,
                Message::Ack {
                    status: AckStatus::Ok,
                    ..
                },
            )) => {
                cache.last_sent = Some(hash);
                info!(hash, bytes = nbytes, "clip Offer → peer ok");
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
                info!(
                    iseq,
                    hash = ih,
                    mime = imime,
                    bytes = body.len(),
                    "clip inbound Offer while awaiting Ack — apply now"
                );
                let ok = apply_inbound(imime, ih, &body, cache);
                let _ = write_message(
                    s,
                    *out_seq,
                    &Message::Ack {
                        of_seq: iseq,
                        status: if ok { AckStatus::Ok } else { AckStatus::Reject },
                    },
                );
                *out_seq = out_seq.wrapping_add(1);
                // Keep waiting for our original Ack.
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
                s.set_read_timeout(Some(Duration::from_millis(1))).ok();
                warn!(%e, "clip Offer Ack soft-timeout — Offer sent; keeping stream");
                return Err(e);
            }
            Err(e) => {
                s.set_read_timeout(Some(Duration::from_millis(1))).ok();
                return Err(e);
            }
        }
    }
    s.set_read_timeout(Some(Duration::from_millis(1))).ok();
    Ok(())
}

fn apply_inbound(mime: u8, hash: u32, body: &[u8], cache: &mut Cache) -> bool {
    if cache.last_recv == Some(hash) {
        info!(hash, "clip skip apply (already have)");
        return true;
    }
    let clip = match mime {
        MIME_PNG => LocalClip::Png(body.to_vec()),
        MIME_TEXT_UTF8 => match std::str::from_utf8(body) {
            Ok(s) => LocalClip::Text(s.to_string()),
            Err(e) => {
                warn!(%e, "clip inbound text not utf-8");
                return false;
            }
        },
        other => {
            warn!(mime = other, "clip inbound unsupported mime — reject");
            return false;
        }
    };
    if platform::write_local(&clip) {
        cache.last_recv = Some(hash);
        cache.last_sent = Some(hash);
        info!(hash, mime, bytes = body.len(), "clip applied from peer");
        true
    } else {
        warn!(
            hash,
            mime,
            bytes = body.len(),
            "clip apply from peer FAILED"
        );
        false
    }
}

fn listen_main(listener: std::net::TcpListener, cfg: ClipConfig, jobs: Receiver<ClipJob>) {
    let mut cache = Cache::new();
    let mut peer: Option<TcpStream> = None;
    let mut out_seq: u32 = 1;
    let mut last_push = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);
    let mut pending_push = false;

    loop {
        match listener.accept() {
            Ok((mut s, addr)) => {
                info!(%addr, "clipboard TCP accept");
                s.set_nodelay(true).ok();
                s.set_nonblocking(false).ok();
                s.set_read_timeout(Some(Duration::from_millis(500))).ok();
                match read_message(&mut s, cfg.max_bytes) {
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
                    s.set_read_timeout(Some(Duration::from_millis(1))).ok();
                    peer = Some(s);
                    info!("clip peer ready (duplex)");
                    if pending_push {
                        pending_push = false;
                        last_push = Instant::now();
                        if let Some(ref mut s) = peer {
                            if let Err(e) = push_local_to_peer(s, &cfg, &mut cache, &mut out_seq) {
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

        match jobs.recv_timeout(Duration::from_millis(100)) {
            Ok(ClipJob::PushToMac) => {
                if last_push.elapsed() < Duration::from_millis(400) {
                    debug!("clip Leave push debounced");
                } else if let Some(ref mut s) = peer {
                    info!("clip job push (leave)");
                    last_push = Instant::now();
                    match push_local_to_peer(s, &cfg, &mut cache, &mut out_seq) {
                        Ok(()) => pending_push = false,
                        Err(e) if is_soft_timeout(&e) => {
                            warn!(%e, "clip leave push Ack soft-timeout — keeping peer");
                        }
                        Err(e) => {
                            warn!(%e, "clip leave push failed");
                            peer = None;
                            pending_push = true;
                        }
                    }
                } else {
                    pending_push = true;
                    warn!("clip leave push deferred — no TCP peer");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        while matches!(jobs.try_recv(), Ok(_)) {}

        if let Some(ref mut s) = peer {
            s.set_read_timeout(Some(Duration::from_millis(1))).ok();
            match read_message(s, cfg.max_bytes) {
                Ok((seq, Message::Offer { mime, hash, body })) => {
                    info!(seq, hash, mime, bytes = body.len(), "clip inbound Offer");
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
                    let _ = platform::clear();
                    cache.last_recv = Some(hash_text(""));
                    cache.last_sent = Some(hash_text(""));
                    info!(seq, "clip cleared from Empty");
                    let aseq = out_seq;
                    out_seq = out_seq.wrapping_add(1);
                    if write_message(
                        s,
                        aseq,
                        &Message::Ack {
                            of_seq: seq,
                            status: AckStatus::Ok,
                        },
                    )
                    .is_err()
                    {
                        peer = None;
                    }
                }
                Ok((_, Message::Hello { .. })) | Ok((_, Message::Ack { .. })) => {}
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
