//! Server run loop: session + UDP emit + input backend.

use std::thread;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::input::{demo_events, EvdevSource, FeedSource, InputBackendKind, EVDEV_POLL};
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

    info!(
        "input backend: evdev — mirror rel motion while local; EVIOCGRAB while remote"
    );
    info!(
        "seed local cursor at primary center ({}, {}); move toward Mac edge to enter",
        session.local_x, session.local_y
    );
    info!(
        "note: local absolute position is estimated from relative deltas (may drift); \
         layer-shell barriers are the planned precise edge path once sola-river enables them"
    );

    loop {
        let events = source.poll().map_err(|e| format!("evdev poll: {e}"))?;
        if events.is_empty() {
            thread::sleep(EVDEV_POLL);
            continue;
        }
        for ev in events {
            let mut grab_fn = |g: bool| source.set_grabbed(g);
            apply(session, sender, ev, &mut grab_fn);
        }
    }
}

