//! Headless CEF helper process (`sola-browser --engine --profile=<id>`).
//!
//! No iced, no kit window, no bus menus. CEF `root_cache_path` is this
//! profile's `…/cef/` so cookies persist. Chrome talks to us over a Unix
//! socket — the user never sees this process in the app switcher.

use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    sola_core::log::init(app_id);
    sola_core::env::activate_gpu_env();

    // Wrappers call [`profiles::bind_external`] before `try_run` so this
    // helper does not look up sola-browser's `profiles.json`.
    let already = profiles::active_if_bound().is_some_and(|p| p.id == profile_id);
    if !already {
        if let Err(e) = profiles::bind_process_only(profile_id) {
            tracing::error!(error = %e, %profile_id, "engine helper: bind profile failed");
            return ExitCode::FAILURE;
        }
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
        ToEngine::PasteImage {
            mime,
            filename,
            bytes,
        } => Cmd::PasteImage {
            mime,
            filename,
            bytes,
        },
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
        ToEngine::ShowDevTools {
            panel,
            inspect_x,
            inspect_y,
        } => Cmd::ShowDevTools {
            panel,
            inspect_x,
            inspect_y,
        },
        ToEngine::ResizeDevTools {
            width,
            height,
            scale,
        } => Cmd::ResizeDevTools {
            width,
            height,
            scale,
        },
        ToEngine::DevToolsInput(ev) => Cmd::DevToolsInput(ev),
        ToEngine::DevToolsFocus(f) => Cmd::DevToolsFocus(f),
        ToEngine::CloseDevTools => Cmd::CloseDevTools,
        ToEngine::NotifyPermission { prompt_id, granted } => {
            Cmd::NotifyPermission { prompt_id, granted }
        }
        ToEngine::MediaPermission { req_id, granted } => Cmd::MediaPermission { req_id, granted },
        ToEngine::JsDialog { id, success, input } => Cmd::JsDialog { id, success, input },
        ToEngine::HttpAuth {
            id,
            success,
            username,
            password,
        } => Cmd::HttpAuth {
            id,
            success,
            username,
            password,
        },
        ToEngine::Find {
            text,
            forward,
            next,
        } => Cmd::Find {
            text,
            forward,
            next,
        },
        ToEngine::StopFind { clear } => Cmd::StopFind { clear },
        ToEngine::Shutdown => return None,
    })
}

/// Ask every profile helper to Quit (flush cookies) before chrome starts.
///
/// `exec_self` after `cargo make install` leaves `--engine` children with
/// ppid == us. SIGTERM skips CEF's cookie flush — GitHub/Google look
/// signed-out on the next launch. Send `Shutdown` on the control socket
/// first and wait for the pid file to die.
pub fn stop_all_profile_engines() {
    for p in profiles::list() {
        stop_profile_engine(&p.id);
    }
}

fn stop_profile_engine(profile_id: &str) {
    let sock = profiles::engine_sock_path(profile_id);
    let pid_path = profiles::engine_pid_path(profile_id);
    let pid = fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if let Ok(mut stream) = UnixStream::connect(&sock) {
        match ipc::write_msg(&mut stream, &ToEngine::Shutdown) {
            Ok(()) => {
                tracing::info!(%profile_id, "asked engine helper to shutdown (flush cookies)")
            }
            Err(e) => tracing::warn!(%profile_id, error = %e, "engine shutdown write failed"),
        }
    }
    if let Some(pid) = pid {
        if wait_pid_gone(pid, Duration::from_secs(6)) {
            tracing::info!(%profile_id, pid, "engine helper exited");
        } else {
            tracing::warn!(%profile_id, pid, "engine helper still up — SIGTERM");
            let _ = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status();
            let _ = wait_pid_gone(pid, Duration::from_secs(2));
        }
    }
    let _ = fs::remove_file(&sock);
    let _ = fs::remove_file(profiles::engine_frame_sock_path(profile_id));
    let _ = fs::remove_file(&pid_path);
}

fn wait_pid_gone(pid: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(40));
    }
    !Path::new(&format!("/proc/{pid}")).exists()
}

/// Kill leftover iced fleet windows (`--parked`) and **orphan** helpers.
///
/// Never kill an `--engine` whose parent is a *different* live chrome —
/// a second sola-browser used to do that and leave the first window on a
/// parked last-frame with a dead CEF (reload painted nothing).
///
/// After `exec_self` our own pre-restart helpers still have ppid == us;
/// those *are* stale (old binary) and must go.
pub fn reap_stale_browser_procs() {
    let me = std::process::id();
    let Ok(dir) = fs::read_dir("/proc") else {
        return;
    };
    let mut killed = 0u32;
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
        if !cmdline_is_sola_browser(&text) {
            continue;
        }
        let ppid = proc_ppid(pid);
        let ppid_live_chrome = ppid.filter(|&p| p != me).is_some_and(pid_is_live_chrome);
        match decide_reap(me, pid, &text, ppid, ppid_live_chrome) {
            ReapDecision::Keep { why } => {
                tracing::info!(
                    pid,
                    ppid,
                    why,
                    "leaving sola-browser process (live helper or chrome)"
                );
            }
            ReapDecision::Kill {
                why,
                parked,
                engine,
            } => {
                tracing::info!(
                    pid,
                    ppid,
                    parked,
                    engine,
                    why,
                    "reaping leftover sola-browser process"
                );
                let _ = std::process::Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status();
                killed += 1;
            }
        }
    }
    let root = profiles::browser_data_root();
    let _ = fs::remove_file(root.join("fleet-focus.json"));
    // Only unlink engine sockets whose helper pid is dead. Wiping live
    // socks used to race a just-started helper.
    if let Ok(dir) = fs::read_dir(root.join("profiles")) {
        for ent in dir.flatten() {
            let p = ent.path();
            let pid_file = p.join("engine.pid");
            let helper_live = fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists());
            if helper_live {
                continue;
            }
            let _ = fs::remove_file(p.join("engine.sock"));
            let _ = fs::remove_file(p.join("engine.frame.sock"));
            let _ = fs::remove_file(p.join("instance.pid"));
        }
    }
    if killed > 0 {
        thread::sleep(Duration::from_millis(150));
    }
    tracing::info!(killed, me, "helper reap finished");
}

#[derive(Debug, PartialEq, Eq)]
enum ReapDecision {
    Keep {
        why: &'static str,
    },
    Kill {
        why: &'static str,
        parked: bool,
        engine: bool,
    },
}

fn cmdline_is_sola_browser(text: &str) -> bool {
    text.split('\0')
        .any(|a| a.ends_with("sola-browser") || a == "sola-browser")
        || text.contains("sola-browser")
}

fn decide_reap(
    me: u32,
    pid: u32,
    cmdline: &str,
    ppid: Option<u32>,
    ppid_live_chrome: bool,
) -> ReapDecision {
    let args: Vec<&str> = cmdline.split('\0').filter(|s| !s.is_empty()).collect();
    if args.iter().any(|a| a.starts_with("--type=")) {
        return ReapDecision::Keep {
            why: "cef subprocess",
        };
    }
    let parked = args.iter().any(|a| *a == "--parked");
    let engine = args.iter().any(|a| *a == "--engine");
    if parked {
        return ReapDecision::Kill {
            why: "legacy --parked fleet window",
            parked,
            engine,
        };
    }
    if engine {
        if ppid == Some(me) {
            return ReapDecision::Kill {
                why: "our child from before exec_self / leftover",
                parked,
                engine,
            };
        }
        if ppid_live_chrome {
            return ReapDecision::Keep {
                why: "engine owned by another live chrome",
            };
        }
        return ReapDecision::Kill {
            why: "orphan engine (parent gone or not chrome)",
            parked,
            engine,
        };
    }
    // Another iced chrome — singleton should have prevented this; do not
    // SIGTERM it (that's how we used to murder a window the user still sees).
    if pid != me {
        return ReapDecision::Keep {
            why: "other chrome window",
        };
    }
    ReapDecision::Keep { why: "self" }
}

fn proc_ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat_ppid(&stat)
}

fn parse_stat_ppid(stat: &str) -> Option<u32> {
    let rparen = stat.rfind(')')?;
    let mut rest = stat[rparen + 1..].split_whitespace();
    let _state = rest.next()?;
    rest.next()?.parse().ok()
}

fn pid_is_live_chrome(pid: u32) -> bool {
    if !Path::new(&format!("/proc/{pid}")).exists() {
        return false;
    }
    let Ok(raw) = fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let text = String::from_utf8_lossy(&raw);
    if !cmdline_is_sola_browser(&text) {
        return false;
    }
    let args: Vec<&str> = text.split('\0').filter(|s| !s.is_empty()).collect();
    !args
        .iter()
        .any(|a| *a == "--engine" || a.starts_with("--type="))
}

#[cfg(test)]
mod reap_tests {
    use super::*;

    #[test]
    fn keeps_foreign_live_helper() {
        let cmd = "/opt/sola/bin/sola-browser\0--engine\0--profile=abc\0";
        assert!(matches!(
            decide_reap(10, 99, cmd, Some(20), true),
            ReapDecision::Keep { why } if why.contains("another live chrome")
        ));
    }

    #[test]
    fn kills_orphan_helper() {
        let cmd = "/opt/sola/bin/sola-browser\0--engine\0--profile=abc\0";
        assert!(matches!(
            decide_reap(10, 99, cmd, Some(1), false),
            ReapDecision::Kill { .. }
        ));
    }

    #[test]
    fn kills_our_pre_exec_helper() {
        let cmd = "/opt/sola/bin/sola-browser\0--engine\0--profile=abc\0";
        assert!(matches!(
            decide_reap(10, 99, cmd, Some(10), false),
            ReapDecision::Kill { why, .. } if why.contains("exec_self")
        ));
    }

    #[test]
    fn never_kills_other_chrome() {
        let cmd = "/opt/sola/bin/sola-browser\0";
        assert!(matches!(
            decide_reap(10, 50, cmd, Some(1), false),
            ReapDecision::Keep { why } if why.contains("other chrome")
        ));
    }

    #[test]
    fn parse_ppid_with_spaces_in_comm() {
        let stat = "123 (sola-browser) S 456 456 456 0 -1";
        assert_eq!(parse_stat_ppid(stat), Some(456));
    }
}
