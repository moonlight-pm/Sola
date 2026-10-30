//! sola-lock — the lock screen.
//!
//! Locks the session with `ext-session-lock-v1` (rivulet): while locked the
//! compositor shows and sends input to nothing else, and stays locked if
//! this process dies until another lock takes over. The password is checked
//! by Oath's login service (`sola_core::login`). Started by the shell (Super+L,
//! the idle fade, the desk's start); see `docs/specs/2026-10-29-secure-access-design.md`.
//!
//! Exit: 0 unlocked (or another sola-lock already holds the lock); 2 no
//! password set, nothing to lock with; 3 no login service; 1 the lock
//! failed.

mod app;
mod draw;
mod keys;
mod picture;

use std::os::fd::OwnedFd;

use tracing::{error, info, warn};

fn main() {
    sola_core::log::init("sola-lock");
    let why = std::env::args().nth(1).unwrap_or_else(|| "asked".into());
    if why == "--preview" {
        // The main output's look as a PNG (design review); never locks.
        let out = std::env::args().nth(2).unwrap_or_else(|| "sola-lock.png".into());
        if let Err(e) = preview(&out) {
            eprintln!("sola-lock: preview: {e}");
            std::process::exit(1);
        }
        return;
    }

    let _instance = match only_instance() {
        Instance::Taken => {
            info!(%why, "already locked by another sola-lock");
            return;
        }
        held => held,
    };
    match sola_core::login::status() {
        Ok(s) if s.password => {}
        Ok(_) => {
            warn!(%why, "no password set: not locking");
            eprintln!("sola-lock: no password set (Settings → Login)");
            std::process::exit(2);
        }
        Err(e) => {
            warn!(%why, "not locking: {e}");
            eprintln!("sola-lock: {e}");
            std::process::exit(3);
        }
    }
    info!(%why, "locking");
    match app::run() {
        Ok(()) => info!("unlocked"),
        Err(e) => {
            error!("lock failed: {e}");
            std::process::exit(1);
        }
    }
}

fn preview(out: &str) -> Result<(), String> {
    let (w, h) = (1920u32, 1080u32);
    let backdrop = picture::load().map(|p| picture::cover(&p, w, h));
    let now = chrono::Local::now();
    let (time, date) = (now.format("%H:%M").to_string(), now.format("%A, %-d %B").to_string());
    let scene = draw::Scene {
        time: &time,
        date: &date,
        typed: 6,
        message: "Wrong password",
        error: true,
        main: true,
    };
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    draw::Painter::new().paint(&mut canvas, w, h, 1.0, backdrop.as_ref(), &scene);
    for px in canvas.chunks_exact_mut(4) {
        px.swap(0, 2); // ARGB8888 bytes (BGRA) → RGBA
    }
    image::RgbaImage::from_raw(w, h, canvas)
        .ok_or("canvas size")?
        .save(out)
        .map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}

/// One sola-lock at a time.
enum Instance {
    /// Holding `$XDG_RUNTIME_DIR/sola-lock.lock` for the process's life.
    #[allow(dead_code)]
    Mine(OwnedFd),
    /// The lock file could not be opened: lock anyway, unguarded.
    Unguarded,
    /// Another sola-lock holds it (Super+L while locked, idle).
    Taken,
}

fn only_instance() -> Instance {
    use rustix::fs::{FlockOperation, Mode, OFlags, flock, open};
    let path = sola_core::env::runtime_dir().join("sola-lock.lock");
    let fd = match open(
        &path,
        OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    ) {
        Ok(fd) => fd,
        Err(e) => {
            warn!(path = %path.display(), "lock file: {e}; locking unguarded");
            return Instance::Unguarded;
        }
    };
    match flock(&fd, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Instance::Mine(fd),
        Err(_) => Instance::Taken,
    }
}
