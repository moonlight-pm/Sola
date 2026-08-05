//! UDP receive + inject (two threads, lock-free motion, **no busy-spin**).
//!
//! Metrics after lock-free+timer still showed multi-hundred-ms gaps. Likely
//! causes on macOS:
//! 1. `poll(0)` busy-spin → scheduler demotes the process
//! 2. `thread::sleep(8ms)` timer coalescing stretches waits under load
//!
//! Fix: `poll(1)` when hot (always yield), inject uses short timed parks
//! watching atomics (wake early on new motion).
//!
//! **Hard warp policy:** CGWarp only on Enter and before discrete events that
//! carry a wire-ordered cursor stamp (click resync). Continuous paint is
//! CGEvent soft-move only — alternating hard-warps under thrash collapsed
//! the inject loop (Claude second-opinion note).
//!
//! **Motion / click order:** pure motion stays lock-free (latest-wins atomics).
//! Buttons/keys/scroll go through a stamped discrete queue with the cursor
//! position as of that event in wire order, so a later motion cannot pull the
//! click to a newer point.

use std::io;
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};

use crate::clip::{self, ClipHandle};
use crate::inject::{CgInjector, Injector};
use crate::metrics::Metrics;
use crate::protocol::{self, Packet, MAX_PACKET_LEN};

/// Max time between cursor paints while remote traffic is flowing.
const INJECT_MAX_PERIOD: Duration = Duration::from_millis(8);

/// Discrete wire event plus the absolute cursor that applied *before* it in
/// stream order (last Motion). Used so click inject cannot race a newer
/// motion that arrived after the button on the atomics path.
#[derive(Debug, Clone)]
struct StampedDiscrete {
    packet: Packet,
    /// Cursor for this event's place in wire order. `None` for Enter/Leave
    /// (those carry their own semantics) or if no motion has been seen yet.
    at: Option<(i32, i32)>,
}

pub fn run(bind: &str) -> Result<(), AgentError> {
    let sock = UdpSocket::bind(bind).map_err(AgentError::Io)?;
    let local = sock.local_addr().ok();
    info!(?local, "sola-kvm-mac listening (UDP)");

    sock.set_nonblocking(true).map_err(AgentError::Io)?;
    boost_recv_buf(&sock);

    crate::inject::check_accessibility_at_startup();

    // TCP clipboard on the same bind address/port as UDP (different protocol).
    let clip = match clip::spawn(bind) {
        Ok(h) => {
            info!("clipboard worker up (TCP, sync on Leave → novus)");
            Some(h)
        }
        Err(e) => {
            warn!(%e, "clipboard TCP listen failed — clip sync disabled");
            None
        }
    };

    let motion = Arc::new(MotionAtomics::new());
    let (discrete_tx, discrete_rx) = mpsc::channel::<StampedDiscrete>();
    let running = Arc::new(AtomicBool::new(true));

    info!(
        max_period_ms = INJECT_MAX_PERIOD.as_millis() as u64,
        "lag metrics — lock-free motion, hard-warp enter/click only; grep kvm-metrics|LAG"
    );

    let motion_i = Arc::clone(&motion);
    let running_i = Arc::clone(&running);
    let inject_handle = thread::Builder::new()
        .name("kvm-inject".into())
        .spawn(move || {
            crate::priority::boost_process();
            inject_loop(motion_i, discrete_rx, running_i);
        })
        .map_err(AgentError::Io)?;

    let recv_result = recv_loop(
        sock,
        Arc::clone(&motion),
        discrete_tx,
        Arc::clone(&running),
        clip,
    );

    running.store(false, Ordering::SeqCst);
    let _ = inject_handle.join();
    recv_result
}

struct MotionAtomics {
    x: AtomicI32,
    y: AtomicI32,
    gen: AtomicU64,
}

impl MotionAtomics {
    fn new() -> Self {
        Self {
            x: AtomicI32::new(0),
            y: AtomicI32::new(0),
            gen: AtomicU64::new(0),
        }
    }

    fn store(&self, x: i32, y: i32) {
        self.x.store(x, Ordering::Relaxed);
        self.y.store(y, Ordering::Relaxed);
        self.gen.fetch_add(1, Ordering::Release);
    }

    fn load(&self) -> (i32, i32, u64) {
        let gen = self.gen.load(Ordering::Acquire);
        (
            self.x.load(Ordering::Relaxed),
            self.y.load(Ordering::Relaxed),
            gen,
        )
    }
}

fn recv_loop(
    sock: UdpSocket,
    motion: Arc<MotionAtomics>,
    discrete_tx: Sender<StampedDiscrete>,
    running: Arc<AtomicBool>,
    clip: Option<ClipHandle>,
) -> Result<(), AgentError> {
    let mut buf = [0u8; 256];
    let mut last_seq: Option<u32> = None;
    let mut ok_count: u64 = 0;
    let mut err_count: u64 = 0;
    let mut metrics = Metrics::new();
    let fd = sock.as_raw_fd();
    let mut hot_until = Instant::now();
    // Last Motion position in *stream* order (across batches). Stamps discrete.
    let mut stream_cursor: Option<(i32, i32)> = None;

    while running.load(Ordering::Relaxed) {
        // Never poll(0): busy-spin burns the quantum and macOS demotes us,
        // which showed up as 150–700ms gaps. 1ms still wakes promptly.
        let timeout_ms = if Instant::now() < hot_until { 1 } else { 1000 };
        match wait_readable(fd, timeout_ms) {
            Ok(false) => {
                if timeout_ms >= 1000 {
                    metrics.on_idle_tick();
                }
                continue;
            }
            Ok(true) => {}
            Err(e) => {
                error!(error = %e, "poll error");
                thread::sleep(Duration::from_millis(5));
                continue;
            }
        }

        let mut raw: Vec<Packet> = Vec::with_capacity(64);
        let mut src_for_log: Option<std::net::SocketAddr> = None;
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, src)) => {
                    src_for_log.get_or_insert(src);
                    if let Some((_seq, packet)) =
                        take_decoded(&buf[..n], src, &mut last_seq, &mut ok_count, &mut err_count)
                    {
                        raw.push(packet);
                    }
                }
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => {
                    error!(error = %e, "recv error");
                    break;
                }
            }
        }

        if raw.is_empty() {
            continue;
        }

        hot_until = Instant::now() + Duration::from_millis(250);

        let before = raw.len();
        let batch = coalesce_recv_batch(
            raw.into_iter()
                .enumerate()
                .map(|(i, p)| (i as u32, p))
                .collect(),
        );
        let collapsed = before.saturating_sub(batch.len());
        let src = src_for_log.expect("non-empty");
        let t0 = Instant::now();
        let mut handed = Vec::with_capacity(batch.len());

        for (seq, packet) in batch {
            match &packet {
                Packet::Enter { .. } | Packet::Leave => {
                    info!(%src, seq, ?packet, ok_count, "recv")
                }
                Packet::Motion { .. } => debug!(%src, seq, ?packet, ok_count, "recv"),
                _ => debug!(%src, seq, ?packet, ok_count, "recv"),
            }

            match packet {
                Packet::Motion { x, y } => {
                    motion.store(x, y);
                    stream_cursor = Some((x, y));
                    handed.push((seq, Packet::Motion { x, y }));
                }
                other => {
                    // Leave: enqueue clipboard push before inject (worker is
                    // async — must not block the UDP path).
                    if matches!(other, Packet::Leave) {
                        if let Some(ref c) = clip {
                            c.push_to_novus();
                        }
                    }
                    // Enter/Leave: no stamp (Enter has its own coords; Leave
                    // is seat teardown). Everything else: wire-ordered cursor.
                    let at = match &other {
                        Packet::Enter { x, y, .. } => {
                            stream_cursor = Some((*x, *y));
                            Some((*x, *y))
                        }
                        Packet::Leave => None,
                        _ => stream_cursor,
                    };
                    let stamped = StampedDiscrete {
                        packet: other.clone(),
                        at,
                    };
                    if discrete_tx.send(stamped).is_err() {
                        running.store(false, Ordering::SeqCst);
                        break;
                    }
                    handed.push((seq, other));
                }
            }
        }

        metrics.on_batch(before, collapsed, &handed, t0.elapsed());
    }

    Ok(())
}

fn inject_loop(
    motion: Arc<MotionAtomics>,
    discrete_rx: Receiver<StampedDiscrete>,
    running: Arc<AtomicBool>,
) {
    let mut injector = CgInjector::new();
    let mut metrics = Metrics::new();
    let mut last_gen = 0u64;

    while running.load(Ordering::Relaxed) {
        let t0 = Instant::now();
        let mut batch_for_metrics: Vec<(u32, Packet)> = Vec::new();
        let mut discrete_n = 0usize;

        // 1) Discrete events immediately — apply *stamped* cursor, never the
        //    latest motion gen (that race put clicks at a newer point).
        loop {
            match discrete_rx.try_recv() {
                Ok(StampedDiscrete { packet, at }) => {
                    discrete_n += 1;
                    if let Some((x, y)) = at {
                        injector.hard_warp(x, y);
                        batch_for_metrics.push((0, Packet::Motion { x, y }));
                        // If atomics still match this stamp, mark gen consumed so
                        // we don't soft-paint the same point again. If motion has
                        // already moved on, leave last_gen so paint follows after.
                        let (cx, cy, gen) = motion.load();
                        if (cx, cy) == (x, y) {
                            last_gen = gen;
                        }
                    }
                    injector.handle(&packet);
                    batch_for_metrics.push((0, packet));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }

        // 2) Paint latest motion if it advanced — soft CGEvent only.
        let (x, y, gen) = motion.load();
        if gen != last_gen {
            injector.warp(x, y);
            last_gen = gen;
            batch_for_metrics.push((0, Packet::Motion { x, y }));
        }

        if !batch_for_metrics.is_empty() {
            metrics.on_batch(
                batch_for_metrics.len().max(discrete_n).max(1),
                0,
                &batch_for_metrics,
                t0.elapsed(),
            );
        } else {
            metrics.on_idle_tick();
        }

        // 3) Wait for next motion gen or period end.
        //
        // Critical: never `sleep` for sub-millisecond durations on macOS — the
        // kernel coalesces them into multi-ms (sometimes 10ms+) waits, which is
        // exactly how we collapsed to ~30–40 Hz paint despite an "8 ms" design.
        // Use 1 ms sleeps while far from the deadline, then spin/yield.
        // Discrete events are picked up on the next loop (≤8 ms).
        wait_for_motion_gen(&motion, last_gen, INJECT_MAX_PERIOD, &running);
    }
}

/// Wait until `motion.gen != last_gen`, `timeout`, or stop.
fn wait_for_motion_gen(
    motion: &MotionAtomics,
    last_gen: u64,
    timeout: Duration,
    running: &AtomicBool,
) {
    let deadline = Instant::now() + timeout;
    while running.load(Ordering::Relaxed) {
        if motion.load().2 != last_gen {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let left = deadline - now;
        if left > Duration::from_millis(2) {
            // Coarse wait — 1 ms is the practical floor on macOS.
            thread::sleep(Duration::from_millis(1));
        } else {
            // Final ≤2 ms: spin + yield (no short sleeps).
            while Instant::now() < deadline {
                if motion.load().2 != last_gen {
                    return;
                }
                std::hint::spin_loop();
                thread::yield_now();
            }
            return;
        }
    }
}

fn take_decoded(
    slice: &[u8],
    src: std::net::SocketAddr,
    last_seq: &mut Option<u32>,
    ok_count: &mut u64,
    err_count: &mut u64,
) -> Option<(u32, Packet)> {
    if slice.len() > MAX_PACKET_LEN {
        warn!(%src, n = slice.len(), "oversized datagram; decoding first bytes only");
    }
    let slice = &slice[..slice.len().min(MAX_PACKET_LEN)];
    match protocol::decode(slice) {
        Ok((seq, packet)) => {
            if let Some(prev) = *last_seq {
                let expected = prev.wrapping_add(1);
                if seq != expected {
                    debug!(seq, expected, prev, %src, "seq gap");
                }
            }
            *last_seq = Some(seq);
            *ok_count = ok_count.saturating_add(1);
            Some((seq, packet))
        }
        Err(e) => {
            *err_count = err_count.saturating_add(1);
            warn!(%src, err_count, error = %e, "decode failed");
            None
        }
    }
}

pub fn coalesce_recv_batch(packets: Vec<(u32, Packet)>) -> Vec<(u32, Packet)> {
    if packets.len() <= 1 {
        return packets;
    }

    let mut out: Vec<(u32, Packet)> = Vec::with_capacity(packets.len());
    let mut pending_motion: Option<(u32, Packet)> = None;
    let mut pend_scroll: Option<(u32, f32, f32)> = None;

    let flush_motion = |out: &mut Vec<(u32, Packet)>, m: &mut Option<(u32, Packet)>| {
        if let Some(item) = m.take() {
            out.push(item);
        }
    };
    let flush_scroll = |out: &mut Vec<(u32, Packet)>, s: &mut Option<(u32, f32, f32)>| {
        if let Some((seq, dx, dy)) = s.take() {
            out.push((seq, Packet::Scroll { dx, dy }));
        }
    };

    for (seq, packet) in packets {
        match packet {
            Packet::Motion { x, y } => {
                flush_scroll(&mut out, &mut pend_scroll);
                pending_motion = Some((seq, Packet::Motion { x, y }));
            }
            Packet::Scroll { dx, dy } => {
                flush_motion(&mut out, &mut pending_motion);
                match &mut pend_scroll {
                    Some((_, adx, ady)) => {
                        *adx += dx;
                        *ady += dy;
                    }
                    None => pend_scroll = Some((seq, dx, dy)),
                }
            }
            other => {
                flush_motion(&mut out, &mut pending_motion);
                flush_scroll(&mut out, &mut pend_scroll);
                out.push((seq, other));
            }
        }
    }
    flush_motion(&mut out, &mut pending_motion);
    flush_scroll(&mut out, &mut pend_scroll);
    out
}

fn wait_readable(fd: i32, timeout_ms: i32) -> io::Result<bool> {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    const POLLIN: i16 = 0x1;
    #[link(name = "c")]
    extern "C" {
        fn poll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
    }
    let mut pfd = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    let rc = unsafe { poll(&mut pfd, 1, timeout_ms) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(rc > 0 && (pfd.revents & POLLIN) != 0)
    }
}

fn boost_recv_buf(sock: &UdpSocket) {
    let fd = sock.as_raw_fd();
    let bytes: i32 = 1024 * 1024;
    #[link(name = "c")]
    extern "C" {
        fn setsockopt(
            sockfd: i32,
            level: i32,
            optname: i32,
            optval: *const std::ffi::c_void,
            optlen: u32,
        ) -> i32;
    }
    #[cfg(target_os = "macos")]
    const SOL_SOCKET: i32 = 0xffff;
    #[cfg(target_os = "macos")]
    const SO_RCVBUF: i32 = 0x1002;
    #[cfg(target_os = "linux")]
    const SOL_SOCKET: i32 = 1;
    #[cfg(target_os = "linux")]
    const SO_RCVBUF: i32 = 8;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (fd, bytes);
        return;
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let rc = unsafe {
            setsockopt(
                fd,
                SOL_SOCKET,
                SO_RCVBUF,
                &bytes as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&bytes) as u32,
            )
        };
        if rc != 0 {
            debug!(error = %io::Error::last_os_error(), "SO_RCVBUF failed");
        } else {
            debug!(bytes, "UDP SO_RCVBUF raised");
        }
    }
}

#[derive(Debug)]
pub enum AgentError {
    Io(std::io::Error),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for AgentError {}

#[cfg(test)]
pub fn parse_bind(s: &str) -> Result<std::net::SocketAddr, String> {
    s.parse().map_err(|e| format!("{s}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{encode, Edge, Packet};
    use std::net::UdpSocket;

    #[test]
    fn parse_default_bind() {
        assert_eq!(parse_bind("0.0.0.0:4242").unwrap().port(), 4242);
    }

    #[test]
    fn localhost_udp_decode_path() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let packet = Packet::Enter {
            edge: Edge::Right,
            x: 100,
            y: 200,
        };
        client
            .send_to(&encode::encode(7, &packet), addr)
            .unwrap();
        let mut buf = [0u8; 64];
        let (n, _) = server.recv_from(&mut buf).unwrap();
        let (seq, got) = protocol::decode(&buf[..n]).unwrap();
        assert_eq!(seq, 7);
        assert_eq!(got, packet);
    }

    #[test]
    fn coalesce_keeps_latest_motion_before_click() {
        let batch = vec![
            (1, Packet::Motion { x: 10, y: 10 }),
            (2, Packet::Motion { x: 20, y: 15 }),
            (3, Packet::Motion { x: 30, y: 20 }),
            (
                4,
                Packet::Button {
                    button: 0,
                    pressed: 1,
                },
            ),
            (5, Packet::Motion { x: 40, y: 25 }),
            (6, Packet::Motion { x: 50, y: 30 }),
        ];
        assert_eq!(
            coalesce_recv_batch(batch),
            vec![
                (3, Packet::Motion { x: 30, y: 20 }),
                (
                    4,
                    Packet::Button {
                        button: 0,
                        pressed: 1,
                    }
                ),
                (6, Packet::Motion { x: 50, y: 30 }),
            ]
        );
    }

    #[test]
    fn coalesce_pure_motion_run_keeps_only_latest() {
        let batch: Vec<_> = (0..100)
            .map(|i| (i as u32, Packet::Motion { x: i, y: i * 2 }))
            .collect();
        assert_eq!(
            coalesce_recv_batch(batch),
            vec![(99, Packet::Motion { x: 99, y: 198 })]
        );
    }

    #[test]
    fn motion_atomics_latest_wins() {
        let m = MotionAtomics::new();
        m.store(1, 2);
        m.store(3, 4);
        let (x, y, gen) = m.load();
        assert_eq!((x, y), (3, 4));
        assert!(gen >= 2);
    }

    /// Simulates recv stamping: after coalesce, a click must carry the motion
    /// that immediately preceded it in stream order — not a later motion.
    #[test]
    fn discrete_stamp_uses_stream_cursor_not_latest_motion() {
        let batch = coalesce_recv_batch(vec![
            (1, Packet::Motion { x: 10, y: 10 }),
            (2, Packet::Motion { x: 30, y: 20 }),
            (
                3,
                Packet::Button {
                    button: 0,
                    pressed: 1,
                },
            ),
            (4, Packet::Motion { x: 99, y: 99 }),
        ]);
        let mut stream_cursor: Option<(i32, i32)> = None;
        let mut stamped: Vec<StampedDiscrete> = Vec::new();
        for (_seq, packet) in batch {
            match packet {
                Packet::Motion { x, y } => {
                    stream_cursor = Some((x, y));
                }
                other => {
                    let at = match &other {
                        Packet::Enter { x, y, .. } => Some((*x, *y)),
                        Packet::Leave => None,
                        _ => stream_cursor,
                    };
                    stamped.push(StampedDiscrete { packet: other, at });
                }
            }
        }
        assert_eq!(stamped.len(), 1);
        assert_eq!(stamped[0].at, Some((30, 20)));
        assert!(matches!(
            stamped[0].packet,
            Packet::Button {
                button: 0,
                pressed: 1
            }
        ));
        // Stream cursor after full batch is the post-click motion.
        assert_eq!(stream_cursor, Some((99, 99)));
    }
}
