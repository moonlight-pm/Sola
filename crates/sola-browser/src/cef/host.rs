//! Headless CEF helper process (`sola-browser --engine --profile=<id>`).
//!
//! No iced, no kit window, no bus menus. CEF `root_cache_path` is this
//! profile's `…/cef/` so cookies persist. Chrome talks to us over a Unix
//! socket — the user never sees this process in the app switcher.

use std::fs;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::cef::engine::CefEngine;
use crate::cef::ipc::{self, FromEngine, ToEngine};
use crate::engine::{ClipboardHandle, Cmd, FrameMailbox, TabId, TabInfo};
use crate::profiles;

/// True when this process is a CEF engine helper (not iced chrome).
pub fn is_engine_process() -> bool {
    std::env::args().any(|a| a == "--engine")
}

pub fn profile_flag() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if let Some(id) = a.strip_prefix("--profile=") {
            return Some(id.to_string());
        }
        if a == "--profile" {
            return args.next();
        }
    }
    None
}

/// Run the helper and never return to iced. `None` if this process is chrome.
pub fn try_run(app_id: &'static str) -> Option<ExitCode> {
    if !is_engine_process() {
        return None;
    }
    let Some(id) = profile_flag() else {
        eprintln!("sola-browser --engine requires --profile=<id>");
        return Some(ExitCode::FAILURE);
    };
    Some(run_helper(app_id, &id))
}

fn run_helper(app_id: &'static str, profile_id: &str) -> ExitCode {
    sola_core::log::init("sola-browser");
    sola_core::env::activate_gpu_env();

    if let Err(e) = profiles::bind_process_only(profile_id) {
        tracing::error!(error = %e, %profile_id, "engine helper: bind profile failed");
        return ExitCode::FAILURE;
    }

    let sock = profiles::engine_sock_path(profile_id);
    let frame_sock = profiles::engine_frame_sock_path(profile_id);
    let pid_path = profiles::engine_pid_path(profile_id);
    if let Some(parent) = sock.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(&frame_sock);

    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, path = %sock.display(), "engine helper: bind socket");
            return ExitCode::FAILURE;
        }
    };
    let frame_listener = match UnixListener::bind(&frame_sock) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, path = %frame_sock.display(), "engine helper: bind frame socket");
            return ExitCode::FAILURE;
        }
    };
    let _ = fs::write(&pid_path, std::process::id().to_string());
    tracing::info!(
        profile = %profile_id,
        path = %sock.display(),
        "engine helper listening (no window)"
    );

    let (stream, _) = match listener.accept() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "engine helper: accept failed");
            let _ = fs::remove_file(&sock);
            let _ = fs::remove_file(&frame_sock);
            let _ = fs::remove_file(&pid_path);
            return ExitCode::FAILURE;
        }
    };
    let (frame_stream, _) = match frame_listener.accept() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "engine helper: accept frame failed");
            let _ = fs::remove_file(&sock);
            let _ = fs::remove_file(&frame_sock);
            let _ = fs::remove_file(&pid_path);
            return ExitCode::FAILURE;
        }
    };
    // Only one chrome client; drop the listeners so a stale socket is obvious.
    drop(listener);
    drop(frame_listener);

    let mut reader = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "engine helper: clone stream");
            return ExitCode::FAILURE;
        }
    };
    let writer = Arc::new(Mutex::new(stream));
    let mut frame_writer = frame_stream;

    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd<CefEngine>>();
    let frames = FrameMailbox::<crate::cef::engine::CefFrame>::new();
    let cursor = Arc::new(AtomicU32::new(0));
    let tabs_snapshot = Arc::new(Mutex::new(Vec::<TabInfo>::new()));
    let active_atomic = Arc::new(AtomicU64::new(0));
    let next_id = Arc::new(AtomicU64::new(1));
    let clipboard_out: ClipboardHandle = Arc::new(Mutex::new(None));
    let (event_tx, event_rx) = mpsc::channel::<FromEngine>();

    let cmd_tx_r = cmd_tx.clone();
    thread::Builder::new()
        .name("engine-ipc-read".into())
        .spawn(move || {
            loop {
                match ipc::read_msg::<ToEngine>(&mut reader) {
                    Ok(msg) => {
                        if matches!(msg, ToEngine::Shutdown) {
                            let _ = cmd_tx_r.send(Cmd::Quit);
                            break;
                        }
                        if let Some(cmd) = to_cmd(msg) {
                            if cmd_tx_r.send(cmd).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::info!(error = %e, "engine helper: chrome disconnected");
                        let _ = cmd_tx_r.send(Cmd::Quit);
                        break;
                    }
                }
            }
        })
        .expect("spawn engine-ipc-read");

    let frames_w = frames.clone();
    thread::Builder::new()
        .name("engine-ipc-frames".into())
        .spawn(move || {
            loop {
                match frames_w.recv() {
                    Ok(tagged) => {
                        let meta = ipc::FrameMeta {
                            tab_id: tagged.tab_id.0,
                            width: tagged.frame.width,
                            height: tagged.frame.height,
                            dirty: tagged.frame.dirty.clone(),
                        };
                        // Write the Arc slice directly — no extra 8 MiB clone.
                        if ipc::write_frame(
                            &mut frame_writer,
                            &meta,
                            tagged.frame.pixels.as_slice(),
                        )
                        .is_err()
                        {
                            break;
                        }
                    }
                    Err(()) => break,
                }
            }
        })
        .expect("spawn engine-ipc-frames");

    let writer_e = writer;
    thread::Builder::new()
        .name("engine-ipc-events".into())
        .spawn(move || {
            while let Ok(msg) = event_rx.recv() {
                let mut g = writer_e.lock().unwrap();
                if ipc::write_msg(&mut g, &msg).is_err() {
                    break;
                }
            }
        })
        .expect("spawn engine-ipc-events");

    // Poll cursor / clipboard into the event channel (CEF writes atomics).
    let cursor_p = cursor.clone();
    let clip_p = clipboard_out.clone();
    let ev_p = event_tx.clone();
    thread::Builder::new()
        .name("engine-ipc-poll".into())
        .spawn(move || {
            let mut last_cursor = 0u32;
            loop {
                thread::sleep(Duration::from_millis(16));
                let c = cursor_p.load(std::sync::atomic::Ordering::Relaxed);
                if c != last_cursor {
                    last_cursor = c;
                    if ev_p.send(FromEngine::Cursor(c)).is_err() {
                        break;
                    }
                }
                if let Ok(mut g) = clip_p.lock() {
                    if let Some(text) = g.take() {
                        if ev_p.send(FromEngine::Clipboard(text)).is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .expect("spawn engine-ipc-poll");

    tracing::info!(profile = %profile_id, "engine helper starting CEF");
    super::engine::run_worker(
        app_id,
        1280,
        800,
        frames,
        cmd_rx,
        cursor,
        tabs_snapshot,
        active_atomic,
        next_id,
        Some(event_tx),
    );

    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(&frame_sock);
    if let Ok(s) = fs::read_to_string(&pid_path) {
        if s.trim() == std::process::id().to_string() {
            let _ = fs::remove_file(&pid_path);
        }
    }
    ExitCode::SUCCESS
}

fn to_cmd(msg: ToEngine) -> Option<Cmd<CefEngine>> {
    Some(match msg {
        ToEngine::Resize {
            width,
            height,
            scale,
        } => Cmd::Resize {
            width,
            height,
            scale,
        },
        ToEngine::Input(ev) => Cmd::Input(ev),
        ToEngine::Focus(f) => Cmd::Focus(f),
        ToEngine::SetFront(f) => Cmd::SetFront(f),
        ToEngine::Nav(n) => Cmd::Nav(n),
        ToEngine::Edit(e) => Cmd::Edit(e),
        ToEngine::PasteText(s) => Cmd::PasteText(s),
        ToEngine::EvaluateJs(s) => Cmd::EvaluateJs(s),
        ToEngine::OpenTab { id, url, title } => Cmd::OpenTab {
            id: TabId(id),
            url,
            title,
        },
        ToEngine::CloseTab(id) => Cmd::CloseTab(TabId(id)),
        ToEngine::SetActiveTab(id) => Cmd::SetActiveTab(TabId(id)),
        ToEngine::CancelDownload { id } => Cmd::CancelDownload {
            profile_id: String::new(),
            id,
        },
        ToEngine::Shutdown => return None,
    })
}

/// Kill leftover iced fleet windows (`--parked` / `--profile=` without `--engine`)
/// and stale helpers from a previous chrome process.
pub fn reap_stale_browser_procs() {
    let me = std::process::id();
    let Ok(dir) = fs::read_dir("/proc") else {
        return;
    };
    for ent in dir.flatten() {
        let Ok(pid) = ent.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(raw) = fs::read(ent.path().join("cmdline")) else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw);
        if !text.contains("sola-browser") {
            continue;
        }
        // Chromium/CEF workers of a helper we are about to replace.
        if text.split('\0').any(|a| a.starts_with("--type=")) {
            continue;
        }
        let parked = text.split('\0').any(|a| a == "--parked");
        let profile_flag = text.split('\0').any(|a| a == "--profile" || a.starts_with("--profile="));
        let engine = text.split('\0').any(|a| a == "--engine");
        // Old two-window fleet, plus helpers from the last chrome (new binary).
        if parked || engine || profile_flag {
            tracing::info!(pid, parked, engine, "reaping leftover sola-browser process");
            let _ = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status();
        }
    }
    // Stale sockets / fleet focus file.
    let root = profiles::browser_data_root();
    let _ = fs::remove_file(root.join("fleet-focus.json"));
    if let Ok(dir) = fs::read_dir(root.join("profiles")) {
        for ent in dir.flatten() {
            let p = ent.path();
            let _ = fs::remove_file(p.join("engine.sock"));
            let _ = fs::remove_file(p.join("engine.frame.sock"));
            let _ = fs::remove_file(p.join("instance.pid"));
        }
    }
    // Give TERM a beat so binds succeed.
    thread::sleep(Duration::from_millis(150));
}
