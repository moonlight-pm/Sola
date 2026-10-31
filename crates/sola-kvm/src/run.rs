//! Server run loop: session + UDP emit + input backend.

use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};

use crate::capture::{Capture, CaptureEvent};
use crate::clip::{self, ClipHandle};
use crate::config::Config;
use crate::input::{coalesce_input_events, demo_events, FeedSource, InputBackendKind};
use crate::link::Keys;
use crate::metrics::Metrics;
use crate::protocol::Packet;
use crate::server::{InputEvent, Session, SideEffect};
use crate::udp::Sender;

/// Cap absolute Motion spray while remote.
///
/// ~120 Hz is enough for smooth cursor feel and leaves headroom on the Mac
/// inject path (CGWarp + CGEvent per motion). Keys/buttons always flush
/// pending motion first.
const MOTION_MIN_INTERVAL: Duration = Duration::from_millis(8);

/// How long the server waits on its inputs before pumping the link again.
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// Run the server until input EOF (feed) or forever (wayland, demo tail).
/// Input and clipboard go only to the paired peer (`keys`).
pub fn run_server(cfg: &Config, backend: InputBackendKind, keys: Keys) -> Result<(), String> {
    let layout = cfg.layout();
    let peer = cfg.peer_addr();
    let mut sender =
        Sender::connect(&peer, keys.clone()).map_err(|e| format!("udp connect {peer}: {e}"))?;

    info!(
        peer = %peer,
        origin_x = layout.origin_x,
        origin_y = layout.origin_y,
        mac_w = layout.mac_w,
        mac_h = layout.mac_h,
        scale = layout.scale,
        primary_w = layout.primary_w,
        primary_h = layout.primary_h,
        ?backend,
        "sola-kvm server starting (Phase C)"
    );
    info!(
        "layout: Mac origin ({}, {}); bottoms at y={}; side={:?}",
        layout.origin_x,
        layout.origin_y,
        layout.mac_bottom(),
        layout.side
    );

    let mut session = Session::new(layout);

    let clip = if cfg.clipboard.enable {
        clip::spawn(clip::ClipConfig {
            peer_host: cfg.peer.host.clone(),
            peer_port: cfg.peer.port,
            max_bytes: cfg.clipboard.max_bytes,
            sync_on_enter: cfg.clipboard.sync_on_enter,
            sync_on_leave: cfg.clipboard.sync_on_leave,
            keys,
        })
    } else {
        info!("clipboard sync disabled in config");
        clip::disabled_handle()
    };

    match backend {
        InputBackendKind::Wayland => run_wayland(&mut session, &mut sender, &clip),
        InputBackendKind::Feed => {
            wait_link(&mut sender);
            run_feed(&mut session, &mut sender, &clip)
        }
        InputBackendKind::Demo => {
            wait_link(&mut sender);
            run_demo(&mut session, &mut sender, &clip)
        }
    }
}

/// The one-shot backends: give the handshake a moment before sending.
fn wait_link(sender: &mut Sender) {
    if !sender.wait_up(Duration::from_secs(3)) {
        warn!(peer = %sender.peer(), "the paired peer is not answering; packets are dropped until it does");
    }
}

/// Holds the latest absolute Motion and emits at most once per min interval.
struct MotionPacer {
    pending: Option<(i32, i32)>,
    last_sent: Option<Instant>,
    min_interval: Duration,
}

impl MotionPacer {
    fn new(min_interval: Duration) -> Self {
        Self {
            pending: None,
            last_sent: None,
            min_interval,
        }
    }

    fn hold(&mut self, x: i32, y: i32) {
        self.pending = Some((x, y));
    }

    fn clear(&mut self) {
        self.pending = None;
        self.last_sent = None;
    }

    /// Force-send any held motion (before keys/buttons/leave, or on idle flush).
    fn flush(&mut self, sender: &mut Sender, metrics: &mut Metrics) {
        if let Some((x, y)) = self.pending.take() {
            send_packet(sender, metrics, &Packet::Motion { x, y });
            self.last_sent = Some(Instant::now());
        }
    }

    /// Send held motion only if the min interval has elapsed (or never sent).
    /// Returns true if a send happened; false if still holding (paced).
    fn try_flush(&mut self, sender: &mut Sender, metrics: &mut Metrics) -> bool {
        let Some((x, y)) = self.pending else {
            return false;
        };
        let ready = match self.last_sent {
            None => true,
            Some(t) => t.elapsed() >= self.min_interval,
        };
        if ready {
            self.pending = None;
            send_packet(sender, metrics, &Packet::Motion { x, y });
            self.last_sent = Some(Instant::now());
            true
        } else {
            false
        }
    }

    fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

fn send_packet(sender: &mut Sender, metrics: &mut Metrics, packet: &Packet) {
    let t0 = Instant::now();
    sender.pump();
    match sender.send(packet) {
        Ok(seq) => {
            debug!(seq, ?packet, "sent");
            metrics.on_send(packet, t0.elapsed(), false);
        }
        Err(crate::udp::UdpError::LinkDown) => {
            debug!(?packet, "not sent: the link is down");
            metrics.on_send(packet, t0.elapsed(), true);
        }
        Err(e) => {
            error!(?packet, %e, "udp send failed");
            metrics.on_send(packet, t0.elapsed(), true);
        }
    }
}

/// How many identical Leave datagrams to spray. Leave is idempotent on ember
/// (re-associate + stuck-key clear). UDP is fire-and-forget — a single drop
/// leaves the Mac pointer dissociated and modifiers stuck.
const LEAVE_SPRAY: usize = 3;

/// Emit packets with Motion pacing. Non-motion always flushes pending first.
fn emit_packets(
    sender: &mut Sender,
    pacer: &mut MotionPacer,
    metrics: &mut Metrics,
    packets: &[Packet],
) {
    for packet in packets {
        match packet {
            Packet::Motion { x, y } => {
                pacer.hold(*x, *y);
                if !pacer.try_flush(sender, metrics) {
                    metrics.on_paced_motion();
                }
            }
            Packet::Enter { .. } => {
                // Enter establishes position; clear any stale hold and send immediately.
                pacer.clear();
                send_packet(sender, metrics, packet);
            }
            Packet::Leave => {
                pacer.flush(sender, metrics);
                pacer.clear();
                // Spray: lost Leave is a stuck-seat failure mode (Claude note).
                // Each send advances seq so ember sees distinct datagrams.
                for _ in 0..LEAVE_SPRAY {
                    send_packet(sender, metrics, packet);
                }
            }
            _ => {
                pacer.flush(sender, metrics);
                send_packet(sender, metrics, packet);
            }
        }
    }
}

/// Apply one event to the session; `effect` carries out its grab and
/// release.
fn apply(
    session: &mut Session,
    sender: &mut Sender,
    pacer: &mut MotionPacer,
    metrics: &mut Metrics,
    event: InputEvent,
    effect: &mut dyn FnMut(&SideEffect),
    clip: &ClipHandle,
) {
    let remote_before = session.is_remote();
    let step = session.handle(event);
    for e in &step.effects {
        match e {
            SideEffect::Grab => info!(
                mac = ?session.mac_pos(),
                local_x = session.local_x,
                local_y = session.local_y,
                "ENTER remote"
            ),
            SideEffect::Release { warp_primary } => info!(
                warp_x = warp_primary.0,
                warp_y = warp_primary.1,
                "LEAVE remote"
            ),
        }
        effect(e);
    }
    emit_packets(sender, pacer, metrics, &step.packets);
    if remote_before != session.is_remote() {
        metrics.set_remote(session.is_remote());
        info!(remote = session.is_remote(), "mode change");
        if session.is_remote() {
            // Enter: push Linux clipboard → Mac (worker thread).
            info!("clip: enqueue PushToMac after session enter");
            clip.push_to_mac();
        }
        // Leave: ember pushes its pasteboard over TCP; we only receive.
    }
}

fn run_feed(
    session: &mut Session,
    sender: &mut Sender,
    clip: &ClipHandle,
) -> Result<(), String> {
    info!("input backend: feed (stdin). Commands: rel/abs/btn/key/scroll/leave");
    info!("example: echo -e 'abs 5119 2000\\nrel 3 0\\nrel 50 10\\nleave' | sola-kvm server");
    let mut feed = FeedSource::stdin();
    let mut set_grab = |e: &SideEffect| debug!(?e, "feed backend has no real grab");
    let mut pacer = MotionPacer::new(MOTION_MIN_INTERVAL);
    let mut metrics = Metrics::new();
    while let Some(ev) = feed
        .next_event()
        .map_err(|e| format!("stdin: {e}"))?
    {
        apply(
            session,
            sender,
            &mut pacer,
            &mut metrics,
            ev,
            &mut set_grab,
            clip,
        );
    }
    pacer.flush(sender, &mut metrics);
    if session.is_remote() {
        apply(
            session,
            sender,
            &mut pacer,
            &mut metrics,
            InputEvent::ForceLeave,
            &mut set_grab,
            clip,
        );
    }
    info!("feed EOF — server exit");
    Ok(())
}

fn run_demo(
    session: &mut Session,
    sender: &mut Sender,
    clip: &ClipHandle,
) -> Result<(), String> {
    info!("input backend: demo — scripted enter/motion/leave");
    let mut set_grab = |e: &SideEffect| info!(?e, "demo grab side-effect (logical only)");
    let mut pacer = MotionPacer::new(Duration::ZERO); // no pacing in demo
    let mut metrics = Metrics::new();
    for ev in demo_events() {
        apply(
            session,
            sender,
            &mut pacer,
            &mut metrics,
            ev,
            &mut set_grab,
            clip,
        );
        thread::sleep(Duration::from_millis(30));
    }
    pacer.flush(sender, &mut metrics);
    if session.is_remote() {
        warn!("demo ended still remote; forcing leave");
        apply(
            session,
            sender,
            &mut pacer,
            &mut metrics,
            InputEvent::ForceLeave,
            &mut set_grab,
            clip,
        );
    }
    info!("demo complete — idling (Ctrl-C to stop)");
    // Stay up so a user unit / supervisor does not thrash.
    loop {
        thread::sleep(Duration::from_secs(60));
        debug!("demo idle tick");
    }
}

fn run_wayland(
    session: &mut Session,
    sender: &mut Sender,
    clip: &ClipHandle,
) -> Result<(), String> {
    let mut capture = Capture::connect(
        session.layout.side,
        session.layout.primary_w,
        session.layout.primary_h,
    )?;
    info!(
        "input: through the compositor — edge strip, keyboard, pointer lock, \
         shortcuts inhibited while remote; Ctrl+Alt+Esc comes back"
    );
    info!(
        log = %sola_core::paths::log_path("sola.log").display(),
        "lag metrics enabled — grep kvm_metrics / LAG in the log"
    );

    let mut pacer = MotionPacer::new(MOTION_MIN_INTERVAL);
    let mut metrics = Metrics::new();
    // A server that died remote may have left the peer holding keys: clear
    // that once the link is up, before anything can enter.
    let mut cleared = false;
    let mut refused_logged: Option<Instant> = None;

    loop {
        sender.pump();
        if !cleared && sender.is_up() {
            info!(spray = LEAVE_SPRAY, "startup Leave — clear any stale remote on the peer");
            for _ in 0..LEAVE_SPRAY {
                send_packet(sender, &mut metrics, &Packet::Leave);
            }
            cleared = true;
        }

        let mut inputs = Vec::new();
        if session.is_remote() && !sender.is_up() {
            warn!("the paired peer stopped answering while remote; coming back");
            inputs.push(InputEvent::ForceLeave);
        }
        for event in capture.poll()? {
            match event {
                CaptureEvent::Edge(along) if !session.is_remote() => {
                    if !sender.is_up() {
                        if refused_logged.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
                            warn!("edge reached, but the paired peer is not answering; staying here");
                            refused_logged = Some(Instant::now());
                        }
                        continue;
                    }
                    if let Err(e) = capture.grab() {
                        warn!(%e, "ENTER aborted: input not captured, staying here");
                        continue;
                    }
                    let step = session.enter_from_physical_edge(along);
                    info!(mac = ?session.mac_pos(), along, "ENTER remote (edge)");
                    emit_packets(sender, &mut pacer, &mut metrics, &step.packets);
                    metrics.set_remote(true);
                    clip.push_to_mac();
                }
                CaptureEvent::Input(ev) if session.is_remote() => inputs.push(ev),
                CaptureEvent::Escape | CaptureEvent::Lost if session.is_remote() => {
                    inputs.push(InputEvent::ForceLeave)
                }
                _ => {}
            }
        }
        let inputs = coalesce_input_events(inputs);
        metrics.on_input_events(inputs.len());
        let worked = !inputs.is_empty();
        for ev in inputs {
            let mut release = |e: &SideEffect| {
                if let SideEffect::Release { warp_primary } = e {
                    capture.release(*warp_primary);
                }
            };
            apply(session, sender, &mut pacer, &mut metrics, ev, &mut release, clip);
        }
        if !session.is_remote() && capture.is_captured() {
            // Never hold the keyboard while local.
            capture.release((session.local_x, session.local_y));
        }

        pacer.try_flush(sender, &mut metrics);
        if worked {
            continue;
        }
        let mut wait = IDLE_WAIT;
        if pacer.has_pending() {
            wait = match pacer.last_sent {
                Some(t) => pacer.min_interval.saturating_sub(t.elapsed()).min(wait),
                None => Duration::ZERO,
            };
        }
        if let Some(at) = capture.next_repeat() {
            wait = wait.min(at.saturating_duration_since(Instant::now()));
        }
        wait_readable(&[capture.fd(), sender.fd()], wait);
        pacer.try_flush(sender, &mut metrics);
        metrics.on_idle_tick();
    }
}

/// Block until one of `fds` is readable or `timeout` passes.
fn wait_readable(fds: &[std::os::fd::RawFd], timeout: Duration) {
    let mut pfds: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: poll on open fds; the timeout is non-negative.
    let _ = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, ms) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Align, Layout, LayoutSpec, Side};
    use crate::protocol::{Edge, Packet};
    use crate::server::Session;

    fn desk() -> Layout {
        Layout::compute(&LayoutSpec {
            primary_w: 5120,
            primary_h: 2160,
            mac_w: 2560,
            mac_h: 2880,
            side: Side::Right,
            align: Align::Bottom,
            scale: 1.0,
            offset_x: None,
            offset_y: None,
            edge_band: 64,
            enter_push: 48.0,
        })
    }

    #[test]
    fn motion_pacer_holds_until_interval() {
        let mut pacer = MotionPacer::new(Duration::from_millis(50));
        pacer.hold(1, 2);
        assert!(pacer.has_pending());
        // Immediate try_flush with no last_sent should send.
        // We can't easily mock Sender without a socket; just test hold/clear.
        pacer.clear();
        assert!(!pacer.has_pending());
        pacer.hold(3, 4);
        assert_eq!(pacer.pending, Some((3, 4)));
        pacer.hold(5, 6); // latest wins
        assert_eq!(pacer.pending, Some((5, 6)));
    }

    #[test]
    fn enter_from_edge_emits_enter_and_motion() {
        let mut s = Session::new(desk());
        let step = s.enter_from_physical_edge(2000);
        assert!(matches!(
            step.packets[0],
            Packet::Enter {
                edge: Edge::Right,
                ..
            }
        ));
        assert!(matches!(step.packets[1], Packet::Motion { .. }));
    }
}
