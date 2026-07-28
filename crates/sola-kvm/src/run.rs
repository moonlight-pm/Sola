//! Server run loop: session + UDP emit + input backend.

use std::thread;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use crate::barrier::EdgeBarrier;
use crate::config::Config;
use crate::input::{
    coalesce_input_events, demo_events, EvdevSource, FeedSource, InputBackendKind, EVDEV_POLL,
};
use crate::server::{InputEvent, Session, SideEffect};
use crate::udp::Sender;

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

    match backend {
        InputBackendKind::Feed => run_feed(&mut session, &mut sender),
        InputBackendKind::Demo => run_demo(&mut session, &mut sender),
        InputBackendKind::Evdev => run_evdev(&mut session, &mut sender),
    }
}

fn apply(
    session: &mut Session,
    sender: &mut Sender,
    event: InputEvent,
    grab: &mut dyn FnMut(bool),
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
    for packet in &step.packets {
        match sender.send(packet) {
            Ok(seq) => debug!(seq, ?packet, "sent"),
            Err(e) => error!(?packet, %e, "udp send failed"),
        }
    }
    if remote_before != session.is_remote() {
        info!(remote = session.is_remote(), "mode change");
    }
}

fn run_feed(session: &mut Session, sender: &mut Sender) -> Result<(), String> {
    info!("input backend: feed (stdin). Commands: rel/abs/btn/key/scroll/leave");
    info!("example: echo -e 'abs 5119 2000\\nrel 3 0\\nrel 50 10\\nleave' | sola-kvm server");
    let mut feed = FeedSource::stdin();
    let mut grab_state = false;
    let mut set_grab = |g: bool| {
        grab_state = g;
        debug!(grab_state, "feed backend has no real grab");
    };
    while let Some(ev) = feed
        .next_event()
        .map_err(|e| format!("stdin: {e}"))?
    {
        apply(session, sender, ev, &mut set_grab);
    }
    if session.is_remote() {
        apply(session, sender, InputEvent::ForceLeave, &mut set_grab);
    }
    info!("feed EOF — server exit");
    Ok(())
}

fn run_demo(session: &mut Session, sender: &mut Sender) -> Result<(), String> {
    info!("input backend: demo — scripted enter/motion/leave");
    let mut grab_state = false;
    let mut set_grab = |g: bool| {
        grab_state = g;
        info!(grab = grab_state, "demo grab side-effect (logical only)");
    };
    for ev in demo_events() {
        apply(session, sender, ev, &mut set_grab);
        thread::sleep(Duration::from_millis(30));
    }
    if session.is_remote() {
        warn!("demo ended still remote; forcing leave");
        apply(session, sender, InputEvent::ForceLeave, &mut set_grab);
    }
    info!("demo complete — idling (Ctrl-C to stop)");
    // Stay up so a user unit / supervisor does not thrash.
    loop {
        thread::sleep(Duration::from_secs(60));
        debug!("demo idle tick");
    }
}

fn run_evdev(session: &mut Session, sender: &mut Sender) -> Result<(), String> {
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

    loop {
        let mut work = false;

        // 1) Physical edge hit while local.
        if !session.is_remote() {
            if let Some(ref mut b) = barrier {
                match b.poll_hit() {
                    Ok(Some(along)) => {
                        work = true;
                        let mut grab_fn = |g: bool| {
                            source.set_grabbed(g);
                            if let Some(ref mut b) = barrier {
                                let _ = b.set_active(!g);
                            }
                        };
                        let step = session.enter_from_physical_edge(along);
                        // Reuse apply's emit path for packets/effects.
                        for effect in &step.effects {
                            match effect {
                                SideEffect::Grab => {
                                    info!(
                                        mac = ?session.mac_pos(),
                                        local_x = session.local_x,
                                        local_y = session.local_y,
                                        "ENTER remote (physical edge barrier)"
                                    );
                                    grab_fn(true);
                                }
                                SideEffect::Release { .. } => {}
                            }
                        }
                        for packet in &step.packets {
                            match sender.send(packet) {
                                Ok(seq) => debug!(seq, ?packet, "sent"),
                                Err(e) => error!(?packet, %e, "udp send failed"),
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => warn!(%e, "barrier poll"),
                }
            }
        }

        // 2) Evdev: only drive the session while remote (or keys/buttons).
        //    While local, ignore relative motion entirely — no estimate creep.
        let events = source.poll().map_err(|e| format!("evdev poll: {e}"))?;
        // Collapse multi-SYN motion runs so a high-rate mouse does not flood
        // ember with one UDP datagram per kernel report.
        let events = coalesce_input_events(events);
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
                source.set_grabbed(g);
                if let Some(ref mut b) = barrier {
                    let _ = b.set_active(!g);
                }
            };
            apply(session, sender, ev, &mut grab_fn);
            if was_remote && !session.is_remote() {
                // Re-arm barrier on leave.
                if let Some(ref mut b) = barrier {
                    let _ = b.set_active(true);
                }
            }
        }

        // Idle only: block on device/wayland fds up to EVDEV_POLL so input
        // wakes us immediately (no fixed 2ms sleep after every busy tick).
        if !work {
            let mut extra = Vec::new();
            if !session.is_remote() {
                if let Some(ref b) = barrier {
                    extra.push(b.wayland_fd());
                }
            }
            source.wait_readable(&extra, EVDEV_POLL);
        }
    }
}

