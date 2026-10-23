//! Chrome-side router: one chrome talks to many headless CEF helpers, one
//! per profile, all live at once (Next slice 5: each workspace picks a
//! profile, so pages of several profiles are on screen together).
//!
//! Each page belongs to the helper that runs it (`owners`). A command that
//! names a page (`Target`, `ShowPage`, `CloseTab`, `SetActiveTab`, an agent
//! call, a dialog answer) goes to that page's helper; the rest go to the
//! helper of the active page (`current`). Frames, tab lists and events come
//! in from every helper; tab lists are merged, each page tagged with its
//! profile. A helper is front (may paint, may show dialogs) while one of its
//! pages is on screen or active; the others rest.

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
    FindResultsHandle, FrameMailbox, FrameReceiver, FrontProfileHandle, HttpAuthHandle, ImeCaret,
    ImeHandle, JsDialogsHandle, NotificationsHandle, PageMenusHandle, PasskeysHandle, TabId,
    TabInfo, TabsHandle,
};
use crate::profiles;

struct Helper {
    to_engine: Sender<ToEngine>,
    #[allow(dead_code)]
    child: Option<Child>,
}

struct HelperSet {
    map: HashMap<String, Helper>,
    pending: HashSet<String>,
}

struct Shared {
    current: FrontProfileHandle,
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
    agent: crate::agent::AgentHandle,
    next_id: Arc<AtomicU64>,
    /// Last chrome content size (physical px) + scale. Helpers must match
    /// this or the shader stretches a 1280×800 park buffer across the window.
    viewport: Mutex<(u32, u32, f64)>,
    /// The profile whose helper runs each page.
    owners: Mutex<HashMap<u64, String>>,
    /// The profile that raised each pending dialog or permission prompt.
    prompts: Mutex<HashMap<(Prompt, u64), String>>,
    /// Each helper's pages as it last reported them, in attach order.
    pages: Mutex<Vec<(String, Vec<TabInfo>)>>,
    /// Pages on screen (`ShowPage`).
    shown: Mutex<HashSet<u64>>,
    /// Profiles whose helper was last told it is front.
    fronts: Mutex<HashSet<String>>,
}

/// What a helper asked chrome about, by kind: their ids come from each
/// helper's own counter, so the kind and id find the asker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Prompt {
    Js,
    Auth,
    Notify,
    Media,
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
    pub agent: crate::agent::AgentHandle,
    pub front_profile: FrontProfileHandle,
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
    let agent: crate::agent::AgentHandle = Arc::new(Mutex::new(Vec::new()));

    let front_profile: FrontProfileHandle = Arc::new(Mutex::new(String::new()));
    let shared = Arc::new(Shared {
        current: front_profile.clone(),
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
        agent: agent.clone(),
        next_id: next_id.clone(),
        viewport: Mutex::new((width, height, 1.0)),
        owners: Mutex::new(HashMap::new()),
        prompts: Mutex::new(HashMap::new()),
        pages: Mutex::new(Vec::new()),
        shown: Mutex::new(HashSet::new()),
        fronts: Mutex::new(HashSet::new()),
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
        agent,
        front_profile,
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
    } else {
        refresh_fronts(&helpers, &shared);
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
            Cmd::InProfile { profile, cmd } => {
                if let Err(e) = attach(&helpers, &shared, &profile, &died_tx) {
                    tracing::error!(error = %e, %profile, "router: attach failed");
                    continue;
                }
                send_to(&helpers, &shared, &profile, *cmd);
                refresh_fronts(&helpers, &shared);
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
                shared.fronts.lock().unwrap().remove(&profile_id);
                shared
                    .owners
                    .lock()
                    .unwrap()
                    .retain(|_, p| *p != profile_id);
                let merged: Vec<TabInfo> = {
                    let mut pages = shared.pages.lock().unwrap();
                    pages.retain(|(p, _)| *p != profile_id);
                    pages.iter().flat_map(|(_, l)| l.iter().cloned()).collect()
                };
                *shared.tabs.lock().unwrap() = merged;
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
                let profile = route(&shared, &other).unwrap_or(current);
                match &other {
                    // The active page's helper takes the commands that
                    // name no page from now on.
                    Cmd::SetActiveTab(id) => {
                        *shared.current.lock().unwrap() = profile.clone();
                        shared.active.store(id.0, Ordering::Relaxed);
                    }
                    Cmd::ShowPage { id, shown } => {
                        let mut set = shared.shown.lock().unwrap();
                        if *shown {
                            set.insert(id.0);
                        } else {
                            set.remove(&id.0);
                        }
                    }
                    _ => {}
                }
                let closed = match &other {
                    Cmd::CloseTab(id) => Some(id.0),
                    _ => None,
                };
                send_to(&helpers, &shared, &profile, other);
                if let Some(id) = closed {
                    shared.owners.lock().unwrap().remove(&id);
                    shared.shown.lock().unwrap().remove(&id);
                }
                refresh_fronts(&helpers, &shared);
            }
        }
    }
}

/// Chrome mints ids below [`crate::cef::host::CHROME_ID_LIMIT`]; keep its
/// counter past any of those a helper reports (a reused helper still runs
/// pages from chrome's last run). Helper-minted ids live in their own
/// ranges and never move it.
fn bump_next(shared: &Shared, id: u64) {
    if id >= crate::cef::host::CHROME_ID_LIMIT {
        return;
    }
    let next = shared.next_id.load(Ordering::Relaxed);
    if id + 1 > next {
        shared.next_id.store(id + 1, Ordering::Relaxed);
    }
}

/// Send `cmd` to `profile`'s helper. A page it opens is that profile's.
fn send_to(helpers: &Arc<Mutex<HelperSet>>, shared: &Shared, profile: &str, cmd: Cmd<CefEngine>) {
    let set = helpers.lock().unwrap();
    let Some(h) = set.map.get(profile) else {
        tracing::warn!(%profile, "router: no helper for cmd (engine dead?)");
        return;
    };
    let Some(wire) = to_wire(cmd) else {
        return;
    };
    if let ToEngine::OpenTab { id, .. } | ToEngine::OpenBoard { id, .. } = &wire {
        bump_next(shared, *id);
        shared
            .owners
            .lock()
            .unwrap()
            .insert(*id, profile.to_string());
    }
    let _ = h.to_engine.send(wire);
}

/// The profile a command belongs to when it names a page or answers a
/// prompt; `None` for the active page's.
fn route(shared: &Shared, cmd: &Cmd<CefEngine>) -> Option<String> {
    let owner = |id: u64| shared.owners.lock().unwrap().get(&id).cloned();
    let asker = |kind: Prompt, id: u64| shared.prompts.lock().unwrap().remove(&(kind, id));
    match cmd {
        Cmd::Target { tab, .. }
        | Cmd::ShowPage { id: tab, .. }
        | Cmd::CloseTab(tab)
        | Cmd::SetActiveTab(tab) => owner(tab.0),
        Cmd::Agent(req) => owner(req.tab),
        Cmd::JsDialog { id, .. } => asker(Prompt::Js, *id),
        Cmd::HttpAuth { id, .. } => asker(Prompt::Auth, *id),
        Cmd::NotifyPermission { prompt_id, .. } => asker(Prompt::Notify, *prompt_id),
        Cmd::MediaPermission { req_id, .. } => asker(Prompt::Media, *req_id),
        _ => None,
    }
}

/// Front: a helper with a page on screen, or the active page's. The rest
/// rest (their active page stops painting). Told only on change.
fn refresh_fronts(helpers: &Arc<Mutex<HelperSet>>, shared: &Shared) {
    let mut want: HashSet<String> = HashSet::new();
    {
        let owners = shared.owners.lock().unwrap();
        for id in shared.shown.lock().unwrap().iter() {
            if let Some(p) = owners.get(id) {
                want.insert(p.clone());
            }
        }
    }
    let current = shared.current.lock().unwrap().clone();
    if !current.is_empty() {
        want.insert(current);
    }
    let set = helpers.lock().unwrap();
    let mut fronts = shared.fronts.lock().unwrap();
    for (id, h) in &set.map {
        let front = want.contains(id);
        if fronts.contains(id) != front {
            let _ = h.to_engine.send(ToEngine::SetFront(front));
            if front {
                fronts.insert(id.clone());
            } else {
                fronts.remove(id);
            }
            tracing::debug!(profile = %id, front, "router: helper front");
        }
    }
}

/// A helper reported its pages: they are its, and chrome sees every
/// helper's pages together.
fn set_pages(shared: &Shared, profile: &str, mut tabs: Vec<TabInfo>) {
    {
        let mut owners = shared.owners.lock().unwrap();
        for t in &mut tabs {
            t.profile = profile.to_string();
            owners.insert(t.id.0, profile.to_string());
        }
    }
    let merged: Vec<TabInfo> = {
        let mut pages = shared.pages.lock().unwrap();
        match pages.iter_mut().find(|(p, _)| p == profile) {
            Some((_, list)) => *list = tabs,
            None => pages.push((profile.to_string(), tabs)),
        }
        pages.iter().flat_map(|(_, list)| list.iter().cloned()).collect()
    };
    *shared.tabs.lock().unwrap() = merged;
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
        // A reader can end on a decode error while the helper still runs.
        // Its CEF holds the profile: a second helper would fail
        // `cef::initialize` and Chromium would relaunch-notify the old one
        // (a stray native window). Kill it before respawning.
        if let Ok(None) = child.try_wait() {
            tracing::warn!(
                profile = %profile_id,
                pid = child.id(),
                "engine helper still running after its IPC ended — killing before respawn"
            );
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    shared.fronts.lock().unwrap().remove(profile_id);
    // Its pages as last reported, less any chrome closed since.
    let reopen: Vec<TabInfo> = {
        let owners = shared.owners.lock().unwrap();
        shared
            .pages
            .lock()
            .unwrap()
            .iter()
            .find(|(p, _)| p == profile_id)
            .map(|(_, list)| {
                list.iter()
                    .filter(|t| owners.get(&t.id.0).is_some_and(|o| o == profile_id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    tracing::error!(
        profile = %profile_id,
        pages = reopen.len(),
        "engine helper died — respawning and reopening its pages"
    );

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
    {
        let set = helpers.lock().unwrap();
        let Some(helper) = set.map.get(profile_id) else {
            return;
        };
        let (vw, vh, vscale) = *shared.viewport.lock().unwrap();
        let _ = helper.to_engine.send(ToEngine::Resize {
            width: vw,
            height: vh,
            scale: vscale,
        });
        for t in &reopen {
            let _ = helper.to_engine.send(ToEngine::OpenTab {
                id: t.id.0,
                url: t.url.clone(),
                title: t.title.clone(),
            });
        }
        let shown: Vec<u64> = shared.shown.lock().unwrap().iter().copied().collect();
        for t in reopen.iter().filter(|t| shown.contains(&t.id.0)) {
            let _ = helper.to_engine.send(ToEngine::ShowPage {
                id: t.id.0,
                shown: true,
            });
        }
    }
    refresh_fronts(helpers, shared);
    tracing::info!(profile = %profile_id, "engine helper restored");
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
                        // Every helper's: page ids are unique across them.
                        shared_f.frames.push(crate::engine::TaggedFrame {
                            tab_id: TabId(meta.tab_id),
                            frame: CefFrame {
                                pixels: Arc::new(pixels),
                                width: meta.width,
                                height: meta.height,
                                dirty: meta.dirty,
                                drag: meta.drag,
                                view: meta.view,
                                board: meta.patch,
                                patch: meta.patch,
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
        child,
    })
}

fn handle_from(
    shared: &Shared,
    profile_id: &str,
    msg: FromEngine,
    ready_tx: &mpsc::SyncSender<Result<(), String>>,
    announced: &mut bool,
) {
    // The active page's helper speaks for "the active tab".
    let is_current = {
        let cur = shared.current.lock().unwrap();
        cur.is_empty() || cur.as_str() == profile_id
    };
    let ask = |kind: Prompt, id: u64| {
        shared
            .prompts
            .lock()
            .unwrap()
            .insert((kind, id), profile_id.to_string());
    };
    match msg {
        FromEngine::Ready { tabs, active } => {
            set_pages(shared, profile_id, tabs);
            if is_current && active != 0 {
                shared.active.store(active, Ordering::Relaxed);
            }
            crate::chrome_wake::wake();
            if !*announced {
                *announced = true;
                let _ = ready_tx.send(Ok(()));
            }
        }
        FromEngine::Tabs(tabs) => {
            if let Some(max) = tabs.iter().map(|t| t.id.0).max() {
                bump_next(shared, max);
            }
            set_pages(shared, profile_id, tabs);
            crate::chrome_wake::wake();
        }
        FromEngine::Active(id) => {
            if is_current {
                shared.active.store(id, Ordering::Relaxed);
                crate::chrome_wake::wake();
            }
        }
        FromEngine::Cursor(c) => {
            shared.cursor.store(c, Ordering::Relaxed);
        }
        FromEngine::Clipboard(text) => {
            *shared.clipboard.lock().unwrap() = Some(text);
            crate::chrome_wake::wake();
        }
        FromEngine::ImeCaret {
            x,
            y,
            w,
            h,
            tab_id,
        } => {
            *shared.ime.lock().unwrap() = ImeCaret {
                x,
                y,
                w,
                h,
                tab_id,
            };
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
            shared.page_menus.lock().unwrap().push(ctx);
            crate::chrome_wake::wake();
        }
        FromEngine::OpenBackgroundTab { url, activate } => {
            if crate::util::href_is_new_tab_target(&url) {
                shared
                    .background_tabs
                    .lock()
                    .unwrap()
                    .push(crate::engine::ChromeTabRequest {
                        url,
                        activate,
                        profile: profile_id.to_string(),
                    });
                crate::chrome_wake::wake();
            }
        }
        FromEngine::Notify(ev) => {
            match &ev {
                crate::notify::Ipc::Perm(p) => ask(Prompt::Notify, p.req_id),
                crate::notify::Ipc::Media(m) => {
                    for id in [m.prompt_id, m.access_id].into_iter().flatten() {
                        ask(Prompt::Media, id);
                    }
                }
                crate::notify::Ipc::Show(_) => {}
            }
            shared.notifications.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::JsDialog(ev) => {
            if let crate::js_dialog::Event::Open(d) = &ev {
                ask(Prompt::Js, d.id);
            }
            shared.js_dialogs.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::HttpAuth(ev) => {
            if let crate::http_auth::Event::Open(d) = &ev {
                ask(Prompt::Auth, d.id);
            }
            shared.http_auth.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::FindResult(ev) => {
            shared.find_results.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::DevTools(ev) => {
            if let crate::engine::DevToolsEvent::Opened { paint_id, .. } = ev {
                bump_next(shared, paint_id.0);
                shared
                    .owners
                    .lock()
                    .unwrap()
                    .insert(paint_id.0, profile_id.to_string());
            }
            shared.devtools.lock().unwrap().push(ev);
            crate::chrome_wake::wake();
        }
        FromEngine::Agent(reply) => {
            shared.agent.lock().unwrap().push(reply);
            crate::chrome_wake::wake();
        }
        FromEngine::Favicon { tab_id, png } => {
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
        Cmd::Agent(req) => Some(ToEngine::Agent(req)),
        Cmd::OpenBoard {
            id,
            url,
            title,
            size,
        } => Some(ToEngine::OpenBoard {
            id: id.0,
            url,
            title,
            size,
        }),
        Cmd::SetBoard { id, size } => Some(ToEngine::SetBoard { id: id.0, size }),
        Cmd::BoardDetail { id, seq, detail } => Some(ToEngine::BoardDetail {
            id: id.0,
            seq,
            detail,
        }),
        Cmd::BoardInput { id, input } => Some(ToEngine::BoardInput { id: id.0, input }),
        Cmd::BoardFocus { id, focused } => Some(ToEngine::BoardFocus { id: id.0, focused }),
        Cmd::BoardRepaint { id } => Some(ToEngine::BoardRepaint { id: id.0 }),
        Cmd::BoardPace { id, pace } => Some(ToEngine::BoardPace { id: id.0, pace }),
        Cmd::Target { tab, cmd } => Some(ToEngine::Target {
            tab: tab.0,
            cmd: Box::new(to_wire(*cmd)?),
        }),
        Cmd::ShowPage { id, shown } => Some(ToEngine::ShowPage { id: id.0, shown }),
        Cmd::InProfile { .. }
        | Cmd::DropParkedProfile { .. }
        | Cmd::CancelDownload { .. }
        | Cmd::HelperDied { .. }
        | Cmd::Quit
        | Cmd::Release { .. }
        | Cmd::FrameDone { .. } => None,
    }
}
