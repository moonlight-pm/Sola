//! Chrome-side router: one iced window talks to many headless CEF helpers.
//!
//! `Cmd::SwitchProfileWorkspace` swaps which helper receives input / paints
//! frames. Helpers stay alive so switching back is instant and each profile
//! keeps its own cookie root.

use std::collections::{HashMap, HashSet};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::cef::engine::{CefEngine, CefFrame};
use crate::cef::ipc::{self, FromEngine, ToEngine};
use crate::engine::{
    BackgroundTabsHandle, ClipboardHandle, Cmd, DevToolsHandle, DownloadsHandle, FaviconsHandle,
    FindResultsHandle, FrameMailbox, FrameReceiver, HttpAuthHandle, ImeCaret, ImeHandle,
    JsDialogsHandle, NotificationsHandle, PageMenusHandle, PasskeysHandle, TabId, TabInfo,
    TabsHandle,
};
use crate::profiles;

struct Helper {
    to_engine: Sender<ToEngine>,
    tabs: Arc<Mutex<Vec<TabInfo>>>,
    #[allow(dead_code)]
    child: Option<Child>,
}

struct HelperSet {
    map: HashMap<String, Helper>,
    pending: HashSet<String>,
}

struct Shared {
    current: Mutex<String>,
    frames: FrameReceiver<CefFrame>,
    tabs: TabsHandle,
    active: Arc<AtomicU64>,
    cursor: Arc<AtomicU32>,
    clipboard: ClipboardHandle,
    ime: ImeHandle,
    downloads: DownloadsHandle,
    passkeys: PasskeysHandle,
    page_menus: PageMenusHandle,
    background_tabs: BackgroundTabsHandle,
    notifications: NotificationsHandle,
    js_dialogs: JsDialogsHandle,
    http_auth: HttpAuthHandle,
    favicons: FaviconsHandle,
    find_results: FindResultsHandle,
    devtools: DevToolsHandle,
    next_id: Arc<AtomicU64>,
    /// Last chrome content size (physical px) + scale. Helpers must match
    /// this or the shader stretches a 1280×800 park buffer across the window.
    viewport: Mutex<(u32, u32, f64)>,
}

pub struct RouterHandles {
    pub worker: JoinHandle<()>,
    pub cmd_tx: Sender<Cmd<CefEngine>>,
    pub frames: FrameReceiver<CefFrame>,
    pub tabs: TabsHandle,
    pub active: Arc<AtomicU64>,
    pub cursor: Arc<AtomicU32>,
    pub next_id: Arc<AtomicU64>,
    pub clipboard: ClipboardHandle,
    pub ime: ImeHandle,
    pub downloads: DownloadsHandle,
    pub passkeys: PasskeysHandle,
    pub page_menus: PageMenusHandle,
    pub background_tabs: BackgroundTabsHandle,
    pub notifications: NotificationsHandle,
    pub js_dialogs: JsDialogsHandle,
    pub http_auth: HttpAuthHandle,
    pub favicons: FaviconsHandle,
    pub find_results: FindResultsHandle,
    pub devtools: DevToolsHandle,
}

pub fn spawn_router(_app_id: &'static str, width: u32, height: u32) -> RouterHandles {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd<CefEngine>>();
    let frames = FrameMailbox::new();
    let tabs: TabsHandle = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(AtomicU64::new(0));
    let cursor = Arc::new(AtomicU32::new(0));
    let next_id = Arc::new(AtomicU64::new(1));
    let clipboard: ClipboardHandle = Arc::new(Mutex::new(None));
    let ime: ImeHandle = Arc::new(Mutex::new(ImeCaret::default()));
    let downloads: DownloadsHandle = Arc::new(Mutex::new(Vec::new()));
    let passkeys: PasskeysHandle = Arc::new(Mutex::new(Vec::new()));
    let page_menus: PageMenusHandle = Arc::new(Mutex::new(Vec::new()));
    let background_tabs: BackgroundTabsHandle = Arc::new(Mutex::new(Vec::new()));
    let notifications: NotificationsHandle = Arc::new(Mutex::new(Vec::new()));
    let js_dialogs: JsDialogsHandle = Arc::new(Mutex::new(Vec::new()));
    let http_auth: HttpAuthHandle = Arc::new(Mutex::new(Vec::new()));
    let favicons: FaviconsHandle = Arc::new(Mutex::new(Vec::new()));
    let find_results: FindResultsHandle = Arc::new(Mutex::new(Vec::new()));
    let devtools: DevToolsHandle = Arc::new(Mutex::new(Vec::new()));

    let shared = Arc::new(Shared {
        current: Mutex::new(String::new()),
        frames: frames.clone(),
        tabs: tabs.clone(),
        active: active.clone(),
        cursor: cursor.clone(),
        clipboard: clipboard.clone(),
        ime: ime.clone(),
        downloads: downloads.clone(),
        passkeys: passkeys.clone(),
        page_menus: page_menus.clone(),
        background_tabs: background_tabs.clone(),
        notifications: notifications.clone(),
        js_dialogs: js_dialogs.clone(),
        http_auth: http_auth.clone(),
        favicons: favicons.clone(),
        find_results: find_results.clone(),
        devtools: devtools.clone(),
        next_id: next_id.clone(),
        viewport: Mutex::new((width, height, 1.0)),
    });

    let shared_w = shared.clone();
    let died_tx = cmd_tx.clone();
    let worker = thread::Builder::new()
        .name("cef-router".into())
        .spawn(move || router_main(shared_w, cmd_rx, died_tx))
        .expect("spawn cef-router");

    RouterHandles {
        worker,
        cmd_tx,
        frames,
        tabs,
        active,
        cursor,
        next_id,
        clipboard,
        ime,
        downloads,
        passkeys,
        page_menus,
        background_tabs,
        notifications,
        js_dialogs,
        http_auth,
        favicons,
        find_results,
        devtools,
    }
}

fn router_main(
    shared: Arc<Shared>,
    cmd_rx: Receiver<Cmd<CefEngine>>,
    died_tx: Sender<Cmd<CefEngine>>,
) {
    let helpers: Arc<Mutex<HelperSet>> = Arc::new(Mutex::new(HelperSet {
        map: HashMap::new(),
        pending: HashSet::new(),
    }));
    let mut deaths: HashMap<String, Vec<Instant>> = HashMap::new();
    let live = profiles::active().id;
    *shared.current.lock().unwrap() = live.clone();
    if let Err(e) = attach(&helpers, &shared, &live, &died_tx) {
        tracing::error!(error = %e, profile = %live, "router: failed to start active engine");
    } else if let Some(h) = helpers.lock().unwrap().map.get(&live) {
        let _ = h.to_engine.send(ToEngine::SetFront(true));
    }
    // Warm other profiles without blocking the first tab's OpenTab.
    let others: Vec<String> = profiles::list()
        .into_iter()
        .filter(|p| p.id != live)
        .map(|p| p.id)
        .collect();
    {
        let helpers_w = helpers.clone();
        let shared_w = shared.clone();
        let died_w = died_tx.clone();
        thread::Builder::new()
            .name("cef-prewarm".into())
            .spawn(move || {
                for id in others {
                    if let Err(e) = attach(&helpers_w, &shared_w, &id, &died_w) {
                        tracing::warn!(error = %e, profile = %id, "router: prewarm helper failed");
                    }
                }
            })
            .ok();
    }

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::SwitchProfileWorkspace {
                park_as_profile_id: _,
                resume_profile_id,
                cef_cache_path: _,
                create_tabs,
                active,
            } => {
                if let Err(e) = attach(&helpers, &shared, &resume_profile_id, &died_tx) {
                    tracing::error!(
                        error = %e,
                        profile = %resume_profile_id,
                        "router: attach on switch failed"
                    );
                    continue;
                }
                {
                    let mut cur = shared.current.lock().unwrap();
                    let prev = std::mem::replace(&mut *cur, resume_profile_id.clone());
                    if prev != resume_profile_id {
                        if let Some(old) = helpers.lock().unwrap().map.get(&prev) {
                            let _ = old.to_engine.send(ToEngine::SetFront(false));
                        }
                    }
                }
                let set = helpers.lock().unwrap();
                let helper = set.map.get(&resume_profile_id).unwrap();
                let _ = helper.to_engine.send(ToEngine::SetFront(true));
                let helper_tabs = helper.tabs.lock().unwrap().clone();
                if helper_tabs.is_empty() {
                    *shared.tabs.lock().unwrap() = Vec::new();
                    if let Some(tabs) = create_tabs {
                        tracing::info!(
                            profile = %resume_profile_id,
                            tabs = tabs.len(),
                            "router: helper empty — opening parked/session tabs"
                        );
                        for (id, url, title) in tabs {
                            bump_next(&shared, id.0);
                            let _ = helper.to_engine.send(ToEngine::OpenTab {
                                id: id.0,
                                url,
                                title,
                            });
                        }
                    }
                    let _ = helper.to_engine.send(ToEngine::SetActiveTab(active.0));
                    shared.active.store(active.0, Ordering::Relaxed);
                } else {
                    // Helper already has browsers — adopt; never OpenTab
                    // (that would reload parked pages).
                    if let Some(max) = helper_tabs.iter().map(|t| t.id.0).max() {
                        bump_next(&shared, max);
                    }
                    *shared.tabs.lock().unwrap() = helper_tabs.clone();
                    let paint = if helper_tabs.iter().any(|t| t.id == active) {
                        active.0
                    } else {
                        helper_tabs.first().map(|t| t.id.0).unwrap_or(active.0)
                    };
                    tracing::info!(
                        profile = %resume_profile_id,
                        tabs = helper_tabs.len(),
                        active = paint,
                        "router: adopting parked helper tabs"
                    );
                    let _ = helper.to_engine.send(ToEngine::SetActiveTab(paint));
                    shared.active.store(paint, Ordering::Relaxed);
                }
                let (vw, vh, vscale) = *shared.viewport.lock().unwrap();
                let _ = helper.to_engine.send(ToEngine::Resize {
                    width: vw,
                    height: vh,
                    scale: vscale,
                });
                drop(set);
                tracing::info!(
                    profile = %resume_profile_id,
                    width = vw,
                    height = vh,
                    "router: front helper"
                );
            }
            Cmd::CancelDownload { profile_id, id } => {
                let set = helpers.lock().unwrap();
                if let Some(h) = set.map.get(&profile_id) {
                    let _ = h.to_engine.send(ToEngine::CancelDownload { id });
                } else {
                    tracing::warn!(
                        profile = %profile_id,
                        id,
                        "router: no helper for CancelDownload"
                    );
                }
            }
            Cmd::DropParkedProfile { profile_id } => {
                if let Some(h) = helpers.lock().unwrap().map.remove(&profile_id) {
                    let _ = h.to_engine.send(ToEngine::Shutdown);
                    tracing::info!(profile = %profile_id, "router: dropped helper");
                }
            }
            Cmd::HelperDied { profile_id } => {
                restore_dead_helper(&helpers, &shared, &died_tx, &mut deaths, &profile_id);
            }
            Cmd::Quit => {
                let drained: Vec<_> = helpers.lock().unwrap().map.drain().collect();
                for (id, h) in drained {
                    let _ = h.to_engine.send(ToEngine::Shutdown);
                    tracing::info!(profile = %id, "router: shutdown helper");
                }
                break;
            }
            Cmd::Release { .. } | Cmd::FrameDone { .. } => {}
            Cmd::Resize {
                width,
                height,
                scale,
            } => {
                *shared.viewport.lock().unwrap() = (width, height, scale);
                // Every helper must match the widget. Otherwise a parked
                // profile stays at spawn size and the first frame after
                // switch is stretched.
                let set = helpers.lock().unwrap();
                for h in set.map.values() {
                    let _ = h.to_engine.send(ToEngine::Resize {
                        width,
                        height,
                        scale,
                    });
                }
            }
            other => {
                let current = shared.current.lock().unwrap().clone();
                let set = helpers.lock().unwrap();
                if let Some(h) = set.map.get(&current) {
                    if let Some(wire) = to_wire(other) {
                        if let ToEngine::OpenTab { id, .. } = &wire {
                            bump_next(&shared, *id);
                        }
                        let _ = h.to_engine.send(wire);
                    }
                } else {
                    tracing::warn!(profile = %current, "router: no helper for cmd (engine dead?)");
                }
            }
        }
    }
}

fn bump_next(shared: &Shared, id: u64) {
    let next = shared.next_id.load(Ordering::Relaxed);
    if id + 1 > next {
        shared.next_id.store(id + 1, Ordering::Relaxed);
    }
}

fn restore_dead_helper(
    helpers: &Arc<Mutex<HelperSet>>,
    shared: &Arc<Shared>,
    died_tx: &Sender<Cmd<CefEngine>>,
    deaths: &mut HashMap<String, Vec<Instant>>,
    profile_id: &str,
) {
    let removed = helpers.lock().unwrap().map.remove(profile_id);
    let Some(old) = removed else {
        tracing::debug!(profile = %profile_id, "helper died after we already dropped it");
        return;
    };
    if let Some(mut child) = old.child {
        let _ = child.try_wait();
    }
    let was_front = shared.current.lock().unwrap().as_str() == profile_id;
    let active = shared.active.load(Ordering::Relaxed);
    tracing::error!(
        profile = %profile_id,
        was_front,
        "engine helper died — respawning (chrome will reopen its tabs)"
    );
    // Drop the stale snapshot so iced does not merge resurrected
    // closed tabs (old helper ids) into the strip.
    if was_front {
        shared.tabs.lock().unwrap().clear();
        crate::chrome_wake::wake();
    }

    let now = Instant::now();
    let stamps = deaths.entry(profile_id.to_string()).or_default();
    stamps.retain(|t| now.duration_since(*t) < Duration::from_secs(15));
    stamps.push(now);
    if stamps.len() >= 4 {
        tracing::error!(
            profile = %profile_id,
            deaths = stamps.len(),
            "engine helper crash loop — not respawning (quit and relaunch)"
        );
        return;
    }

    thread::sleep(Duration::from_millis(200));
    if let Err(e) = attach(helpers, shared, profile_id, died_tx) {
        tracing::error!(profile = %profile_id, error = %e, "engine helper respawn failed");
        return;
    }
    let set = helpers.lock().unwrap();
    let Some(helper) = set.map.get(profile_id) else {
        return;
    };
    if was_front {
        let _ = helper.to_engine.send(ToEngine::SetFront(true));
        if active != 0 {
            shared.active.store(active, Ordering::Relaxed);
        }
    }
    let (vw, vh, vscale) = *shared.viewport.lock().unwrap();
    let _ = helper.to_engine.send(ToEngine::Resize {
        width: vw,
        height: vh,
        scale: vscale,
    });
    tracing::info!(
        profile = %profile_id,
        "engine helper restored (empty; chrome reopens tabs)"
    );
}

fn attach(
    helpers: &Arc<Mutex<HelperSet>>,
    shared: &Arc<Shared>,
    profile_id: &str,
    died_tx: &Sender<Cmd<CefEngine>>,
) -> Result<(), String> {
    loop {
        let mut set = helpers.lock().unwrap();
        if set.map.contains_key(profile_id) {
            return Ok(());
        }
        if set.pending.contains(profile_id) {
            drop(set);
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        set.pending.insert(profile_id.to_string());
        break;
    }
    let (width, height, scale) = *shared.viewport.lock().unwrap();
    let result = spawn_helper(shared, profile_id, width, height, scale, died_tx);
    let mut set = helpers.lock().unwrap();
    set.pending.remove(profile_id);
    match result {
        Ok(helper) => {
            set.map.insert(profile_id.to_string(), helper);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn spawn_helper(
    shared: &Arc<Shared>,
    profile_id: &str,
    width: u32,
    height: u32,
    scale: f64,
    died_tx: &Sender<Cmd<CefEngine>>,
) -> Result<Helper, String> {
    let sock = profiles::engine_sock_path(profile_id);
    if let Some(parent) = sock.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let child = if UnixStream::connect(&sock).is_err() {
        Some(launch_helper(profile_id)?)
    } else {
        None
    };

    let stream = wait_connect(&sock, Duration::from_secs(25))?;
    let frame_sock = profiles::engine_frame_sock_path(profile_id);
    let mut frame_reader = wait_connect(&frame_sock, Duration::from_secs(25))?;
    let helper_tabs: Arc<Mutex<Vec<TabInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let mut writer = stream
        .try_clone()
        .map_err(|e| format!("clone writer: {e}"))?;
    let mut reader = stream
        .try_clone()
        .map_err(|e| format!("clone reader: {e}"))?;

    let (to_tx, to_rx) = mpsc::channel::<ToEngine>();
    thread::Builder::new()
        .name(format!("eng-write-{:.8}", profile_id))
        .spawn(move || {
            while let Ok(msg) = to_rx.recv() {
                if ipc::write_msg(&mut writer, &msg).is_err() {
                    break;
                }
                if matches!(msg, ToEngine::Shutdown) {
                    break;
                }
            }
        })
        .map_err(|e| format!("spawn writer: {e}"))?;

    let shared_r = Arc::clone(shared);
    let tabs_r = helper_tabs.clone();
    let profile_r = profile_id.to_string();
    let died_r = died_tx.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    thread::Builder::new()
        .name(format!("eng-read-{:.8}", profile_id))
        .spawn(move || {
            let mut announced = false;
            loop {
                match ipc::read_msg::<FromEngine>(&mut reader) {
                    Ok(msg) => {
                        handle_from(
                            &shared_r,
                            &tabs_r,
                            &profile_r,
                            msg,
                            &ready_tx,
                            &mut announced,
                        );
                    }
                    Err(e) => {
                        if !announced {
                            let _ = ready_tx.send(Err(format!("engine ipc: {e}")));
                        }
                        tracing::warn!(profile = %profile_r, error = %e, "helper reader ended");
                        if announced {
                            let _ = died_r.send(Cmd::HelperDied {
                                profile_id: profile_r,
                            });
                        }
                        break;
                    }
                }
            }
        })
        .map_err(|e| format!("spawn reader: {e}"))?;

    let shared_f = Arc::clone(shared);
    let profile_f = profile_id.to_string();
    thread::Builder::new()
        .name(format!("eng-frame-{:.8}", profile_id))
        .spawn(move || {
            loop {
                match ipc::read_frame(&mut frame_reader) {
                    Ok((meta, pixels)) => {
                        let is_front = {
                            let cur = shared_f.current.lock().unwrap();
                            cur.is_empty() || cur.as_str() == profile_f
                        };
                        if !is_front {
                            continue;
                        }
                        shared_f.frames.push(crate::engine::TaggedFrame {
                            tab_id: TabId(meta.tab_id),
                            frame: CefFrame {
                                pixels: Arc::new(pixels),
                                width: meta.width,
                                height: meta.height,
                                dirty: meta.dirty,
                            },
                        });
                    }
                    Err(e) => {
                        tracing::warn!(profile = %profile_f, error = %e, "helper frame reader ended");
                        break;
                    }
                }
            }
        })
        .map_err(|e| format!("spawn frame reader: {e}"))?;

    ready_rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| "engine helper Ready timed out".to_string())??;

    let _ = to_tx.send(ToEngine::Resize {
        width,
        height,
        scale,
    });

    tracing::info!(profile = %profile_id, "router: helper ready");
    Ok(Helper {
        to_engine: to_tx,
        tabs: helper_tabs,
        child,
    })
}

fn handle_from(
    shared: &Shared,
    helper_tabs: &Mutex<Vec<TabInfo>>,
    profile_id: &str,
    msg: FromEngine,
    ready_tx: &mpsc::SyncSender<Result<(), String>>,
    announced: &mut bool,
) {
    let is_front = {
        let cur = shared.current.lock().unwrap();
        cur.is_empty() || cur.as_str() == profile_id
    };
    match msg {
        FromEngine::Ready { tabs, active } => {
            *helper_tabs.lock().unwrap() = tabs.clone();
            if is_front {
                *shared.tabs.lock().unwrap() = tabs;
                shared.active.store(active, Ordering::Relaxed);
                crate::chrome_wake::wake();
            }
            if !*announced {
                *announced = true;
                let _ = ready_tx.send(Ok(()));
            }
        }
        FromEngine::Tabs(tabs) => {
            if let Some(max) = tabs.iter().map(|t| t.id.0).max() {
                bump_next(shared, max);
            }
            *helper_tabs.lock().unwrap() = tabs.clone();
            if is_front {
                *shared.tabs.lock().unwrap() = tabs;
                crate::chrome_wake::wake();
            }
        }
        FromEngine::Active(id) => {
            if is_front {
                shared.active.store(id, Ordering::Relaxed);
                crate::chrome_wake::wake();
            }
        }
        FromEngine::Cursor(c) => {
            if is_front {
                shared.cursor.store(c, Ordering::Relaxed);
            }
        }
        FromEngine::Clipboard(text) => {
            if is_front {
                *shared.clipboard.lock().unwrap() = Some(text);
                crate::chrome_wake::wake();
            }
        }
        FromEngine::ImeCaret { x, y, w, h } => {
            if is_front {
                *shared.ime.lock().unwrap() = ImeCaret { x, y, w, h };
            }
        }
        FromEngine::Download(ev) => {
            // Any helper — parked profiles still finish downloads.
            shared
                .downloads
                .lock()
                .unwrap()
                .push((profile_id.to_string(), ev));
            crate::chrome_wake::wake();
        }
        FromEngine::WebAuthn(ev) => {
            shared.passkeys.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::PageContext(ctx) => {
            if is_front {
                shared.page_menus.lock().unwrap().push(ctx);
                crate::chrome_wake::wake();
            }
        }
        FromEngine::OpenBackgroundTab { url, activate } => {
            if is_front && crate::util::href_is_new_tab_target(&url) {
                shared
                    .background_tabs
                    .lock()
                    .unwrap()
                    .push(crate::engine::ChromeTabRequest { url, activate });
                crate::chrome_wake::wake();
            }
        }
        FromEngine::Notify(ev) => {
            shared.notifications.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::JsDialog(ev) => {
            if is_front {
                shared.js_dialogs.lock().unwrap().push(ev);
                crate::chrome_wake::wake();
            }
        }
        FromEngine::HttpAuth(ev) => {
            shared.http_auth.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::FindResult(ev) => {
            shared.find_results.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::DevTools(ev) => {
            if is_front {
                if let crate::engine::DevToolsEvent::Opened { paint_id, .. } = ev {
                    bump_next(shared, paint_id.0);
                }
                shared.devtools.lock().unwrap().push(ev);
                crate::chrome_wake::wake();
            }
        }
        FromEngine::Favicon { tab_id, png } => {
            if is_front {
                shared
                    .favicons
                    .lock()
                    .unwrap()
                    .push(crate::engine::FaviconIpc {
                        tab_id: TabId(tab_id),
                        png,
                    });
                crate::chrome_wake::wake();
            }
        }
    }
}

fn launch_helper(profile_id: &str) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let child = Command::new(&exe)
        .arg("--engine")
        .arg(format!("--profile={profile_id}"))
        // Earliest possible: OSCrypt must be `basic` before Chromium
        // picks the secret portal (TTY has no keyring → cookies v10
        // that cannot be decrypted after restart).
        .arg("--password-store=basic")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn engine helper: {e}"))?;
    tracing::info!(
        profile = %profile_id,
        pid = child.id(),
        exe = %exe.display(),
        "spawned engine helper"
    );
    Ok(child)
}

fn wait_connect(sock: &Path, timeout: Duration) -> Result<UnixStream, String> {
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < timeout {
        match UnixStream::connect(sock) {
            Ok(s) => {
                return Ok(s);
            }
            Err(e) => {
                last = e.to_string();
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
    Err(format!(
        "connect {} failed after {:?}: {last}",
        sock.display(),
        timeout
    ))
}

fn to_wire(cmd: Cmd<CefEngine>) -> Option<ToEngine> {
    match cmd {
        Cmd::Resize {
            width,
            height,
            scale,
        } => Some(ToEngine::Resize {
            width,
            height,
            scale,
        }),
        Cmd::Input(ev) => Some(ToEngine::Input(ev)),
        Cmd::Focus(f) => Some(ToEngine::Focus(f)),
        Cmd::SetFront(f) => Some(ToEngine::SetFront(f)),
        Cmd::Nav(n) => Some(ToEngine::Nav(n)),
        Cmd::Edit(e) => Some(ToEngine::Edit(e)),
        Cmd::PasteText(s) => Some(ToEngine::PasteText(s)),
        Cmd::PasteImage {
            mime,
            filename,
            bytes,
        } => Some(ToEngine::PasteImage {
            mime,
            filename,
            bytes,
        }),
        Cmd::EvaluateJs(s) => Some(ToEngine::EvaluateJs(s)),
        Cmd::OpenTab { id, url, title } => Some(ToEngine::OpenTab {
            id: id.0,
            url,
            title,
        }),
        Cmd::CloseTab(id) => Some(ToEngine::CloseTab(id.0)),
        Cmd::SetActiveTab(id) => Some(ToEngine::SetActiveTab(id.0)),
        Cmd::NotifyPermission { prompt_id, granted } => {
            Some(ToEngine::NotifyPermission { prompt_id, granted })
        }
        Cmd::MediaPermission { req_id, granted } => {
            Some(ToEngine::MediaPermission { req_id, granted })
        }
        Cmd::JsDialog { id, success, input } => Some(ToEngine::JsDialog { id, success, input }),
        Cmd::HttpAuth {
            id,
            success,
            username,
            password,
        } => Some(ToEngine::HttpAuth {
            id,
            success,
            username,
            password,
        }),
        Cmd::Find {
            text,
            forward,
            next,
        } => Some(ToEngine::Find {
            text,
            forward,
            next,
        }),
        Cmd::StopFind { clear } => Some(ToEngine::StopFind { clear }),
        Cmd::ShowDevTools {
            panel,
            inspect_x,
            inspect_y,
        } => Some(ToEngine::ShowDevTools {
            panel,
            inspect_x,
            inspect_y,
        }),
        Cmd::ResizeDevTools {
            width,
            height,
            scale,
        } => Some(ToEngine::ResizeDevTools {
            width,
            height,
            scale,
        }),
        Cmd::DevToolsInput(ev) => Some(ToEngine::DevToolsInput(ev)),
        Cmd::DevToolsFocus(f) => Some(ToEngine::DevToolsFocus(f)),
        Cmd::CloseDevTools => Some(ToEngine::CloseDevTools),
        Cmd::SwitchProfileWorkspace { .. }
        | Cmd::DropParkedProfile { .. }
        | Cmd::CancelDownload { .. }
        | Cmd::HelperDied { .. }
        | Cmd::Quit
        | Cmd::Release { .. }
        | Cmd::FrameDone { .. } => None,
    }
}
