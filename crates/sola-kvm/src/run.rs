//! Server run loop: session + UDP emit + input backend.

use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};

use crate::barrier::EdgeBarrier;
use crate::clip::{self, ClipHandle};
use crate::config::Config;
use crate::input::{
    coalesce_input_events, demo_events, EvdevSource, FeedSource, InputBackendKind, EVDEV_POLL,
};
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

/// Run the Phase C server until input EOF (feed) or forever (evdev/demo tail).
pub fn run_server(cfg: &Config, backend: InputBackendKind) -> Result<(), String> {
    let layout = cfg.layout();
    let peer = cfg.peer_addr();
    let mut sender = Sender::connect(&peer).map_err(|e| format!("udp connect {peer}: {e}"))?;

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
        })
    } else {
        info!("clipboard sync disabled in config");
        clip::disabled_handle()
    };

    match backend {
        InputBackendKind::Feed => run_feed(&mut session, &mut sender, &clip),
        InputBackendKind::Demo => run_demo(&mut session, &mut sender, &clip),
        InputBackendKind::Evdev => run_evdev(&mut session, &mut sender, &clip),
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
    match sender.send(packet) {
        Ok(seq) => {
            debug!(seq, ?packet, "sent");
            metrics.on_send(packet, t0.elapsed(), false);
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

fn apply(
    session: &mut Session,
    sender: &mut Sender,
    pacer: &mut MotionPacer,
    metrics: &mut Metrics,
    event: InputEvent,
    grab: &mut dyn FnMut(bool),
    clip: &ClipHandle,
) {
    let remote_before = session.is_remote();
    let step = session.handle(event);
    for effect in &step.effects {
        match effect {
            SideEffect::Grab => {
                info!(
                    mac = ?session.mac_pos(),
                    local_x = session.local_x,
                    local_y = session.local_y,
                    "ENTER remote — grab + chord suppress (if available)"
                );
                grab(true);
            }
            SideEffect::Release { warp_primary } => {
                info!(
                    warp_x = warp_primary.0,
                    warp_y = warp_primary.1,
                    "LEAVE remote — release grab + restore chords"
                );
                grab(false);
                // Wayland warp is not wired in this spike; log the desired point.
                debug!(
                    x = warp_primary.0,
                    y = warp_primary.1,
                    "desired local pointer warp (compositor warp not yet implemented)"
                );
            }
        }
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
    let mut grab_state = false;
    let mut set_grab = |g: bool| {
        grab_state = g;
        debug!(grab_state, "feed backend has no real grab");
    };
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
    let mut grab_state = false;
    let mut set_grab = |g: bool| {
        grab_state = g;
        info!(grab = grab_state, "demo grab side-effect (logical only)");
    };
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

fn run_evdev(
    session: &mut Session,
    sender: &mut Sender,
    clip: &ClipHandle,
) -> Result<(), String> {
    let mut source = EvdevSource::open_all().map_err(|e| {
        format!(
            "evdev open failed: {e}. \
             Need read/write on /dev/input/event* (input group or seat uaccess). \
             Fall back: sola-kvm server --input feed|demo"
        )
    })?;

    // Physical edge only: layer-shell strip. Relative estimate is NEVER used
    // to enter remote (that was the "hit zone creeping left" bug).
    let mut barrier = match EdgeBarrier::connect(
        session.layout.side,
        session.layout.primary_w,
        session.layout.primary_h,
    ) {
        Ok(b) => {
            let (w, h) = b.primary_size();
            // Prefer compositor-reported size if larger than config.
            if w > 0 && h > 0 {
                info!(w, h, "barrier reports output size");
            }
            Some(b)
        }
        Err(e) => {
            warn!(
                %e,
                "layer-shell barrier unavailable — edge enter disabled (use feed/demo, or fix WAYLAND_DISPLAY)"
            );
            None
        }
    };

    info!(
        "input backend: evdev + layer-shell barrier — enter ONLY on physical shared edge"
    );
    info!(
        "while remote: relative motion + EVIOCGRAB; leave toward primary releases"
    );
    info!(
        "lag metrics enabled — grep kvm_metrics / LAG in /opt/sola/log/sola.log"
    );

    let mut pacer = MotionPacer::new(MOTION_MIN_INTERVAL);
    let mut metrics = Metrics::new();

    // If a previous sola-kvm died mid-remote, ember still has keyboard associate.
    // Always clear that on boot before any edge hit can re-enter.
    info!(
        spray = LEAVE_SPRAY,
        "startup Leave — clear any stale remote associate on peer"
    );
    for _ in 0..LEAVE_SPRAY {
        send_packet(sender, &mut metrics, &Packet::Leave);
    }

    let mut last_rescan = Instant::now();
    let mut last_empty_warn = Instant::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap_or_else(Instant::now);
    /// How often to look for newly plugged (and newly-readable) devices.
    const RESCAN: Duration = Duration::from_secs(1);
    /// Don't spam the log every second while waiting for ACL after boot.
    const EMPTY_WARN: Duration = Duration::from_secs(30);

    loop {
        let mut work = false;

        // Hotplug: drop dead nodes; try to open new ones. If a pointer vanishes
        // while remote, force Leave immediately — never keep keyboard grab alone.
        let lost_ptr = source.prune_dead();
        if last_rescan.elapsed() >= RESCAN {
            let had_ptr = source.has_pointer();
            let gained = source.rescan_new();
            if gained && !had_ptr {
                info!(
                    "pointer /dev/input became readable — remote enter re-enabled"
                );
            } else if !source.has_pointer() && last_empty_warn.elapsed() >= EMPTY_WARN {
                warn!(
                    "still no readable pointer — edge enter disabled. \
                     sudo /opt/sola/bin/sola-kvm-grant-input-acl  (then wait ~1s, no restart needed)"
                );
                last_empty_warn = Instant::now();
            }
            last_rescan = Instant::now();
        }
        if session.is_remote() && (lost_ptr || !source.has_pointer()) {
            error!(
                "pointer lost while remote — forcing leave (prevents split seat: \
                 local mouse + grabbed keyboard)"
            );
            let mut grab_fn = |g: bool| {
                let _ = source.set_grabbed(g);
                if let Some(ref mut b) = barrier {
                    let _ = b.set_active(!g);
                }
            };
            apply(
                session,
                sender,
                &mut pacer,
                &mut metrics,
                InputEvent::ForceLeave,
                &mut grab_fn,
                clip,
            );
            if let Some(ref mut b) = barrier {
                let _ = b.set_active(true);
            }
            work = true;
        }

        // 1) Physical edge hit while local.
        // Poll the barrier without holding the borrow across grab/session work.
        let edge_hit = if !session.is_remote() {
            if let Some(ref mut b) = barrier {
                match b.poll_hit() {
                    Ok(hit) => hit,
                    Err(e) => {
                        warn!(%e, "barrier poll");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(along) = edge_hit {
            // Keyboard-only open + remote = split seat (local mouse,
            // grabbed keys to Mac). Refuse enter until a pointer is open.
            if !source.has_pointer() {
                // Keep trying to open a mouse (ACL may have been fixed).
                let _ = source.rescan_new();
                if !source.has_pointer() {
                    warn!(
                        "edge hit ignored — no pointer /dev/input open. \
                         Fix: sudo /opt/sola/bin/sola-kvm-grant-input-acl \
                         && killall sola-kvm \
                         (permanent: services.sola udev uaccess rule)"
                    );
                    // Fall through to rest of loop (rescan / poll).
                }
            }
            if source.has_pointer() {
                // Grab pointer(+kbd) *before* mutating session / sending Enter.
                // If grab fails, stay fully local — never keyboard-only remote.
                if !source.set_grabbed(true) {
                    error!(
                        "ENTER aborted — EVIOCGRAB failed for pointer. \
                         Staying local (no Enter UDP). Fix mouse ACL if re-plugged."
                    );
                    let _ = source.set_grabbed(false);
                } else {
                    work = true;
                    let step = session.enter_from_physical_edge(along);
                    info!(
                        mac = ?session.mac_pos(),
                        local_x = session.local_x,
                        local_y = session.local_y,
                        "ENTER remote (physical edge barrier)"
                    );
                    if let Some(ref mut b) = barrier {
                        let _ = b.set_active(false);
                    }
                    // Drop kernel-buffered events from the edge push / grab
                    // transition so they don't spray as a laggy burst on the
                    // first remote frames.
                    match source.poll() {
                        Ok(stale) if !stale.is_empty() => {
                            debug!(
                                n = stale.len(),
                                "discarded stale evdev events after grab"
                            );
                        }
                        Ok(_) => {}
                        Err(e) => warn!(%e, "post-grab evdev drain"),
                    }
                    // Pointer may have died during drain — never emit Enter
                    // without a live grabbed pointer.
                    if !source.has_pointer() {
                        error!(
                            "ENTER aborted mid-grab — pointer died. Force leave, no Enter."
                        );
                        let _ = source.set_grabbed(false);
                        let undo = session.handle(InputEvent::ForceLeave);
                        emit_packets(sender, &mut pacer, &mut metrics, &undo.packets);
                        if let Some(ref mut b) = barrier {
                            let _ = b.set_active(true);
                        }
                    } else {
                        emit_packets(sender, &mut pacer, &mut metrics, &step.packets);
                        metrics.set_remote(true);
                        info!(remote = true, "mode change");
                        info!("clip: enqueue PushToMac after physical enter");
                        clip.push_to_mac();
                    }
                }
            }
        }

        // 2) Evdev: only drive the session while remote (or keys/buttons).
        //    While local, ignore relative motion entirely — no estimate creep.
        let events = source.poll().map_err(|e| format!("evdev poll: {e}"))?;
        // If poll dropped the last pointer while remote, leave before forwarding
        // any keys from this batch (keys may still be in `events` from keyboard).
        if session.is_remote() && !source.has_pointer() {
            error!(
                "pointer died during poll — forcing leave before key forward \
                 (prevents split seat)"
            );
            let mut grab_fn = |g: bool| {
                let _ = source.set_grabbed(g);
                if let Some(ref mut b) = barrier {
                    let _ = b.set_active(!g);
                }
            };
            apply(
                session,
                sender,
                &mut pacer,
                &mut metrics,
                InputEvent::ForceLeave,
                &mut grab_fn,
                clip,
            );
            if let Some(ref mut b) = barrier {
                let _ = b.set_active(true);
            }
            // Drop this batch — it was captured under a dying seat.
            continue;
        }
        // Collapse multi-SYN motion runs so a high-rate mouse does not flood
        // ember with one UDP datagram per kernel report.
        let events = coalesce_input_events(events);
        metrics.on_input_events(events.len());
        if !events.is_empty() {
            work = true;
        }
        for ev in events {
            if !session.is_remote() {
                // Local: drop pointer rel/abs from evdev — barrier owns enter.
                match &ev {
                    InputEvent::PointerRel { .. } | InputEvent::PointerAbs { .. } => continue,
                    _ => continue, // also drop keys while local for now
                }
            }
            let was_remote = session.is_remote();
            let mut grab_fn = |g: bool| {
                let _ = source.set_grabbed(g);
                if let Some(ref mut b) = barrier {
                    let _ = b.set_active(!g);
                }
            };
            apply(
                session,
                sender,
                &mut pacer,
                &mut metrics,
                ev,
                &mut grab_fn,
                clip,
            );
            if was_remote && !session.is_remote() {
                // Re-arm barrier on leave.
                if let Some(ref mut b) = barrier {
                    let _ = b.set_active(true);
                }
            }
        }

        // Flush paced motion if the interval has elapsed (even mid-burst).
        pacer.try_flush(sender, &mut metrics);

        // Idle only: block on device/wayland fds up to EVDEV_POLL so input
        // wakes us immediately (no fixed 2ms sleep after every busy tick).
        // If we still hold a paced Motion, wait at most until it is due.
        if !work {
            let wait = if pacer.has_pending() {
                match pacer.last_sent {
                    Some(t) => pacer.min_interval.saturating_sub(t.elapsed()).min(EVDEV_POLL),
                    None => Duration::ZERO,
                }
            } else {
                EVDEV_POLL
            };
            if wait.is_zero() && pacer.has_pending() {
                pacer.flush(sender, &mut metrics);
                continue;
            }
            let mut extra = Vec::new();
            if !session.is_remote() {
                if let Some(ref b) = barrier {
                    extra.push(b.wayland_fd());
                }
            }
            source.wait_readable(&extra, wait);
            // After a short wait for pacing, send any held motion.
            pacer.try_flush(sender, &mut metrics);
            metrics.on_idle_tick();
        }
    }
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
