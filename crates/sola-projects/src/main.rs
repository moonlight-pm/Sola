//! sola-projects (Projects) — project / tab rail + agent-aware PTYs.
//!
//! Persist + spawn: catalog on disk, siblings under `.worktrees/`.
//! Grok + Codex hooks, OSC 9999, process-tree. Calls on sola-call owner `projects`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{canvas, container, mouse_area, stack};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard, window};

use sola_bus::Message;
use sola_bus::topics::{
    AgentIslandAction, AppNotification, IslandAction, IslandState, MenuActionPayload, Topic,
    TopicKind,
};
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
};
use sola_core::window_label::WindowLabel;
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::input::{self, Mods};
use sola_terminal::pty::PtyBackend;
use sola_terminal::state::{self as term_state, PaneRuntime};
use sola_terminal::term_view::{self, CellMetrics, Palette};
use sola_terminal::{extkeys, links, remote as rtmux, tmux};

mod calls;
mod cards;
mod catalog;
mod cli;
mod hooks;
mod instance;
mod islands;
mod link;
mod machine;
mod menu;
mod paths;
mod presence;
mod remote;
mod sessions;
mod spawn;
mod status;
mod windows;

use machine::Machine;

const APP_ID: &str = "sola-projects";
const WINDOW_TITLE: &str = "Projects";

/// An island's screen read: first a moment after the ask (the prompt
/// draws after its hook), then again while no choices show, a few times.
const ISLAND_SCAN_MS: u64 = 400;
const ISLAND_RESCAN_MS: u64 = 700;
const ISLAND_SCAN_TRIES: u8 = 4;
/// Lines of the screen's end that stand for a body the hook did not give.
const ISLAND_TAIL_LINES: usize = 14;
/// Between picking a question's "Type something" and typing the answer.
const ISLAND_TYPE_SETTLE: Duration = Duration::from_millis(150);

fn island_scan_later(pane: String, ms: u64) -> Task<Msg> {
    Task::perform(tokio::time::sleep(Duration::from_millis(ms)), move |_| {
        Msg::IslandScan(pane.clone())
    })
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

const TMUX_SOCKET: &str = "sola-ws";
const TMUX_UNIT: &str = "sola-ws-tmux.service";
const TMUX_PREFIX: &str = "sws-";
/// Started with no project open, Projects waits this long for one, then
/// quits.
const IDLE_QUIT_AFTER: Duration = Duration::from_secs(20);
/// A remote pane's ssh that ran less than this and ended with its session
/// alive is not reattached on its own (it would only end again).
const REMOTE_REATTACH_AFTER: Duration = Duration::from_secs(5);

const DEFAULT_COLS: u16 = 80;
/// A window's size before it reports one.
const DEFAULT_WINDOW: iced::Size = iced::Size::new(1100.0, 720.0);
const DEFAULT_ROWS: u16 = 24;

fn main() -> iced::Result {
    startup(APP_ID);

    // One Projects: a second launch asks the running one to show its
    // project. Before tmux: the helpers below act on the running one's
    // server.
    if !instance::claim() {
        BusSetup::new(APP_ID).install();
        if let Ok(mut client) = bus().lock() {
            if let Err(e) = client.emit(Topic::MenuAction(MenuActionPayload {
                app_id: APP_ID.into(),
                action_id: instance::RAISE.into(),
            })) {
                tracing::warn!("asking the running Projects to show itself failed: {e:?}");
            }
        }
        return Ok(());
    }

    // Must precede any tmux helper — do not share sola-terminal's server.
    tmux::configure(TMUX_SOCKET, TMUX_UNIT, TMUX_PREFIX);
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::IdleFade,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowFloating,
            TopicKind::WindowChrome,
            TopicKind::TabWindow,
            TopicKind::CloseTab,
            TopicKind::NotificationActivate,
            TopicKind::AgentIslandAction,
        ])
        .calls(calls::OWNER, calls::methods())
        .install();

    // A window per project: each is a tab in the shell's tab bar.
    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .run()
}

struct App {
    projects: Vec<catalog::Project>,
    tabs: Vec<catalog::Tab>,
    selected: String,
    focused: String,
    runtimes: HashMap<String, PaneRuntime>,
    /// Last applied grid per pane. Skip TIOCSWINSZ when a divider drag
    /// has not changed cols/rows (same as sola-terminal).
    pane_grids: HashMap<String, (u16, u16)>,
    theme: Theme,
    palette: Palette,
    metrics: CellMetrics,
    cursor_on: bool,
    keyboard_mods: keyboard::Modifiers,
    keys_held_mods: keyboard::Modifiers,
    float: sola_kit::FloatState,
    /// A window per session, each a tab in the shell's tab bar.
    windows: windows::SessionWindows,
    pane_status: HashMap<String, status::PaneStatus>,
    /// Previous pane id if we renamed a *path-matched* orphan onto `ws-main`.
    adopted_from: Option<String>,
    spawn: cards::SpawnDraft,
    add: cards::AddDraft,
    end: cards::EndDraft,
    /// One of the app's windows has keyboard focus (desk cards wait).
    window_focused: bool,
    /// The desk is idle and fading (`Topic::IdleFade`): a focused tab is
    /// not being looked at, so its session's island shows.
    idle_fading: bool,
    pending_waits: Vec<PendingWait>,
    /// What each session needs from you: its island in the top row.
    islands: islands::Islands,
    /// Panes on another computer whose ssh ended with their session out
    /// of reach, and why. Their tab says so and offers to reconnect.
    remote_down: HashMap<String, String>,
    /// Each host's link ([`link`]).
    links: HashMap<String, link::Link>,
    /// Remote panes waiting for their link to attach.
    remote_pending: HashSet<String>,
    /// Hosts whose status tick is out, and when each last went out.
    remote_busy: HashSet<String>,
    remote_ticked: HashMap<String, Instant>,
}

struct PendingWait {
    pane: String,
    want: status::AgentStatus,
    fresh: bool,
    armed: bool,
    reply: sola_call::ReplyTx,
    deadline: Instant,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    PtyOutput(String),
    PtyExit(String),
    Title(String, String),
    Input(iced::Event),
    SelectionChanged,
    Scrolled(String),
    OpenUrl(String),
    WheelToPty(String, Vec<u8>),
    Pasted(Option<String>),
    BlinkTick,
    Window(window::Id, window::Event),
    TitleDrag(window::Id),
    TitleResize(window::Id, window::Direction),
    TitleClose(window::Id),
    DismissDialog,
    SpawnName(String),
    Spawn,
    AddPath(String),
    AddProject,
    /// The End Session card's End Session.
    EndConfirmed,
    /// `tab.rm`: end the session once the reply is out.
    EndSession(String),
    RestartShell(String),
    PaneFocused(String),
    Ignore,
    Hook(hooks::Incoming),
    /// Another computer (or this one) looked at session `pane` (its id on
    /// `host`): its island goes here too.
    Seen {
        host: Option<String>,
        pane: String,
    },
    /// Read a session's screen for its island (the prompt's choices, or a
    /// body), a moment after it began to need you.
    IslandScan(String),
    Osc(String, sola_terminal::osc9999::OscStatus),
    PresenceTick,
    Call(sola_call::Incoming),
    /// The start's wait for a project is over: quit if none opened.
    QuitIfIdle,
    /// A remote pane's ssh ended: is its session there (`Ok(true)`), gone
    /// (`Ok(false)`), or out of reach (`Err`)? `lived` is how long the
    /// ssh ran.
    RemoteExited {
        pane: String,
        state: Result<bool, String>,
        lived: Duration,
    },
    /// A host's link came up, failed, or ended ([`link`]).
    Link(link::LinkEvent),
    /// A host's status tick: each pane as it was and is ([`remote`]).
    RemoteTick {
        host: String,
        panes: Vec<(String, status::PaneStatus, status::PaneStatus)>,
    },
    /// A host's sessions, listed for opening project `project` there.
    HostSessions {
        project: String,
        found: Result<Vec<sessions::HostSession>, String>,
    },
    /// A resumable remote pane's attach, planned off the UI thread.
    RemoteAttach {
        pane: String,
        exec: Vec<String>,
    },
    /// `tab.exec` on a remote tab: is its session alive there?
    RemoteExec {
        tab: String,
        pane: String,
        agent: String,
        prompt: Option<String>,
        reply: sola_call::ReplyTx,
        alive: Result<bool, String>,
    },
    /// Whether a remote project's root is a git checkout.
    RemoteKind {
        tab: String,
        git: bool,
    },
    /// A remote pane's screen and transcript message, for its island.
    IslandScanned {
        pane: String,
        screen: String,
        message: Option<String>,
    },
}

/// PTY taken off the iced state so `tmux kill-session` / `git worktree remove`
/// can run off the UI thread. `close()` matches [`App::teardown_pane`].
struct DetachedPane {
    backend: Option<PtyBackend>,
    session: String,
    /// The computer its session is on, when it is not this one.
    host: Option<String>,
}

impl DetachedPane {
    /// [`Self::close`], waiting until a remote session is gone too (call it
    /// off the UI thread).
    fn close_blocking(self) {
        match self.host.clone() {
            None => self.close(),
            Some(host) => {
                let session = self.session.clone();
                if let Some(backend) = self.backend {
                    backend.close();
                }
                rtmux::kill_session(&host, &session);
            }
        }
    }

    fn close(self) {
        if let Some(backend) = self.backend {
            backend.close();
            return;
        }
        match self.host {
            None => tmux::kill_session(&self.session),
            Some(host) => {
                let session = self.session;
                std::thread::spawn(move || rtmux::kill_session(&host, &session));
            }
        }
    }
}

/// End `panes`' sessions off the UI thread (`tmux kill-session` here, or
/// over ssh on its computer).
fn close_io_task(panes: Vec<DetachedPane>, tab_id: String) -> Task<Msg> {
    Task::perform(
        async move {
            if let Err(e) = tokio::task::spawn_blocking(move || {
                for pane in panes {
                    pane.close_blocking();
                }
            })
            .await
            {
                tracing::error!(%e, tab = %tab_id, "session teardown worker panicked");
            }
        },
        |()| Msg::Ignore,
    )
}

/// [`App::resume_argv`] for a pane's status on `m`: its Grok or Claude
/// session, if that is still on disk there.
fn resume_argv_on(m: &Machine, st: &status::PaneStatus) -> Option<Vec<String>> {
    let sid = st.resume_session_id()?;
    if st.is_grok() {
        if !status::grok_session_exists(m, sid) {
            tracing::warn!(session = %sid, "stored grok session not on disk; starting a shell");
            return None;
        }
        return Some(cli::grok_resume_argv(sid));
    }
    if st.is_claude() {
        if !status::claude_session_exists(m, sid) {
            tracing::warn!(session = %sid, "stored claude session not on disk; starting a shell");
            return None;
        }
        return Some(cli::claude_resume_argv(sid));
    }
    None
}

/// A session [`App::plan_tab`] checked, to be added.
#[derive(Debug, Clone)]
struct NewTab {
    tab: catalog::Tab,
    select: bool,
}

/// Ask `host` whether remote pane `pane`'s session lives, off the UI
/// thread (ssh may take its connect timeout).
fn remote_exit_task(pane: String, host: String, lived: Duration) -> Task<Msg> {
    let session = catalog::session_name(&pane);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || rtmux::session_state(&host, &session))
                .await
                .unwrap_or_else(|e| Err(format!("session check panicked: {e}")))
        },
        move |state| Msg::RemoteExited {
            pane: pane.clone(),
            state,
            lived,
        },
    )
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        hooks::start();
        let _ = sola_terminal::osc9999::sender();
        let catalog = catalog::load();
        let selected = catalog
            .selected
            .clone()
            .filter(|id| catalog.tabs.iter().any(|w| w.id == *id))
            .or_else(|| catalog.tabs.first().map(|w| w.id.clone()))
            .unwrap_or_default();
        let adopted_from = catalog
            .tabs
            .iter()
            .find(|w| w.id == catalog::LIVE_ID)
            .and_then(|w| {
                let claimed: HashSet<String> =
                    catalog.tabs.iter().map(|x| x.id.clone()).collect();
                catalog::adopt_orphan_session(&w.path, &claimed)
            });
        let mut pane_status = HashMap::new();
        for ws in &catalog.tabs {
            // Saved under its id before sessions were keyed by host.
            let st = status::hydrate(&ws.id).or_else(|| status::hydrate(catalog::bare_id(&ws.id)));
            if let Some(st) = st {
                pane_status.insert(ws.id.clone(), st);
            }
        }
        let focused = selected.clone();
        let mut app = Self {
            projects: catalog.projects,
            tabs: catalog.tabs,
            selected: selected.clone(),
            focused,
            runtimes: HashMap::new(),
            pane_grids: HashMap::new(),
            theme: default_theme(),
            palette: Palette::from_kit_theme(&Atoms::default()),
            metrics: CellMetrics::for_font(15.0, fonts::mono_metrics()),
            cursor_on: true,
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
            float: sola_kit::FloatState::new(APP_ID),
            windows: windows::SessionWindows::new(APP_ID),
            pane_status,
            adopted_from,
            spawn: cards::SpawnDraft::default(),
            add: cards::AddDraft::default(),
            end: cards::EndDraft::default(),
            window_focused: true,
            idle_fading: false,
            pending_waits: Vec::new(),
            islands: islands::Islands::default(),
            remote_down: HashMap::new(),
            links: HashMap::new(),
            remote_pending: HashSet::new(),
            remote_busy: HashSet::new(),
            remote_ticked: HashMap::new(),
        };
        app.link_hosts();
        app.sync_all_rows();
        let windows = app.open_all_windows();
        // Nothing open: stay a moment for the launcher's `project.open`
        // (it starts Projects, then asks), then quit.
        let idle = if app.windows.is_empty() {
            Task::perform(tokio::time::sleep(IDLE_QUIT_AFTER), |_| Msg::QuitIfIdle)
        } else {
            Task::none()
        };
        (app, Task::batch([windows, idle]))
    }

    /// A session window's label: its project's folder, the session when
    /// it is not the project's main one (its title, else its name), and
    /// the computer it runs on — always named, this one too (Joshua,
    /// 2026-10-30: "tab should always show the host even if local"). The
    /// shell's bar draws the parts apart.
    fn label(&self, window: window::Id) -> Option<WindowLabel> {
        let tab = self.tab_for_pane(self.windows.tab_of(window)?)?;
        let p = catalog::find_project(&self.projects, &tab.project_id)?;
        let session = (!catalog::is_main(tab))
            .then(|| tab.title.clone().unwrap_or_else(|| tab.name.clone()));
        Some(WindowLabel {
            project: catalog::island_name(p).to_string(),
            session,
            host: p.host.clone().unwrap_or_else(machine::this_host),
        })
    }

    /// A project window's title (its tab in the shell's bar): its
    /// [`Self::label`] as [`WindowLabel::title`] writes it.
    fn title(&self, window: window::Id) -> String {
        self.label(window)
            .map(|l| l.title())
            .unwrap_or_else(|| WINDOW_TITLE.into())
    }

    /// The name in a project window's own frame: the same parts as its tab.
    fn frame_title(&self, window: window::Id) -> String {
        self.title(window)
    }

    fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.chrome(window), &self.theme)
    }

    /// How to frame `window`: tiled, floating, or bare in Cinema.
    fn chrome(&self, window: window::Id) -> sola_kit::Chrome {
        self.windows
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            emulator::title_subscription().map(|(id, t)| Msg::Title(id, t)),
            hooks::subscription().map(|e| match e {
                hooks::Event::Hook(incoming) => Msg::Hook(incoming),
                hooks::Event::Seen { host, pane } => Msg::Seen { host, pane },
            }),
            link::subscription().map(Msg::Link),
            sola_kit::call_subscription().map(Msg::Call),
            sola_terminal::osc9999::subscription().map(|(id, payload)| Msg::Osc(id, payload)),
            iced::time::every(Duration::from_secs(1)).map(|_| Msg::PresenceTick),
            window::events().map(|(id, e)| Msg::Window(id, e)),
            event::listen_with(|ev, status, _| match &ev {
                Event::Keyboard(_) => Some(Msg::Input(ev.clone())),
                // Mouse is handled by TermView / sidebar / the split-drag
                // sub below. Mapping ignored motion to Input presented the
                // whole window on every pixel (`on_input` ignores mouse).
                Event::Mouse(_) => None,
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(ev.clone())),
                _ => None,
            }),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.update_msg(msg);
        self.windows.publish_activity(&self.tabs);
        let scans = self.sync_islands();
        Task::batch([task, scans])
    }

    fn update_msg(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::Window(id, event) => self.on_window(id, event),
            Msg::TitleDrag(id) => sola_kit::drag(Some(id)),
            Msg::TitleResize(id, dir) => sola_kit::drag_resize(Some(id), dir),
            Msg::TitleClose(id) => self.close_window(id),
            Msg::Ignore => Task::none(),
            Msg::Call(incoming) => self.on_call(incoming),
            Msg::Hook(incoming) => {
                let Some(id) = self.resolve_hook_pane(&incoming) else {
                    tracing::debug!(
                        pane = %incoming.pane_id,
                        cwd = ?incoming.cwd,
                        session = ?incoming.mapped.session_id,
                        "hook for unknown pane"
                    );
                    return Task::none();
                };
                self.islands
                    .note_hook(&id, incoming.ask.clone(), incoming.transcript.clone());
                let prev = self
                    .pane_status
                    .get(&id)
                    .map(|s| s.status)
                    .unwrap_or_default();
                // A remote pane's files are read by its host's tick.
                let cwd = self
                    .tab_for_pane(&id)
                    .filter(|w| w.host.is_none())
                    .map(|w| w.path.clone());
                let st = self.pane_status.entry(id.clone()).or_default();
                st.apply_hook(&incoming);
                if incoming.mapped.compacted || incoming.mapped.session_id.is_some() {
                    if let Some(cwd) = cwd.as_deref() {
                        st.refresh_compaction(&Machine::Local, cwd);
                    }
                }
                if incoming.mapped.compacted && st.compaction_count == 0 {
                    st.compaction_count = 1;
                }
                let now = st.status;
                let unconfirmed = st.restored_unconfirmed;
                status::persist_all(&self.pane_status);
                self.sync_row(&id);
                self.maybe_toast_status(&id, prev, now, unconfirmed);
                self.flush_waits();
                Task::none()
            }
            Msg::Seen { host, pane } => {
                let key = catalog::key_for(&pane, host.as_deref());
                self.islands.answer(&key);
                Task::none()
            }
            Msg::IslandScan(id) => self.scan_island(&id),
            Msg::Osc(id, payload) => {
                if let Some(id) = self.resolve_pane(&id) {
                    let prev = self
                        .pane_status
                        .get(&id)
                        .map(|s| s.status)
                        .unwrap_or_default();
                    let st = self.pane_status.entry(id.clone()).or_default();
                    st.apply_osc(&payload);
                    let now = st.status;
                    let unconfirmed = st.restored_unconfirmed;
                    status::persist_all(&self.pane_status);
                    self.sync_row(&id);
                    self.maybe_toast_status(&id, prev, now, unconfirmed);
                    self.flush_waits();
                }
                Task::none()
            }
            Msg::PresenceTick => {
                // A remote pane's processes are not here to scan.
                let ids: Vec<String> = self
                    .tabs
                    .iter()
                    .filter(|w| w.host.is_none())
                    .map(|w| w.id.clone())
                    .collect();
                let mut claimed: HashSet<String> = HashSet::new();
                for id in &ids {
                    let Some(st) = self.pane_status.get(id) else {
                        continue;
                    };
                    if !st.is_codex() {
                        continue;
                    }
                    if let Some(sid) = st.owner_session.clone() {
                        if status::codex_rollout_exists(&Machine::Local, &sid) {
                            claimed.insert(sid);
                        }
                    }
                }
                let mut dirty = false;
                for id in ids {
                    let tmux_session = catalog::session_name(&id);
                    let who = presence::scan_session(&tmux_session);
                    let cwd = self.tab_for_pane(&id).map(|w| w.path.clone());
                    let st = self.pane_status.entry(id).or_default();
                    let before = (st.status, st.agent.clone());
                    st.apply_presence(who);
                    if st.is_codex() {
                        if let Some(cwd) = cwd.as_deref() {
                            if st.observe_codex_turn(&Machine::Local, cwd, &claimed) {
                                dirty = true;
                            }
                            if let Some(sid) = st.owner_session.clone() {
                                claimed.insert(sid);
                            }
                        }
                    }
                    if let Some(cwd) = cwd.as_deref() {
                        st.refresh_compaction(&Machine::Local, cwd);
                    }
                    if (st.status, st.agent.clone()) != before {
                        dirty = true;
                    }
                }
                if dirty {
                    status::persist_all(&self.pane_status);
                }
                self.sync_all_rows();
                self.flush_waits();
                self.remote_ticks()
            }
            Msg::PaneFocused(pane_id) => {
                // Hover focuses for typing. It must not spawn a shell —
                // only the Start new shell button attaches a missing PTY.
                // Only in the window keys go to: hovering a session in
                // another window must not take the keyboard.
                if self.windows.focused.is_some()
                    && self.windows.window_of(&pane_id) == self.windows.focused
                {
                    self.set_focus(&pane_id);
                }
                Task::none()
            }
            Msg::DismissDialog => {
                self.spawn = cards::SpawnDraft::default();
                self.add = cards::AddDraft::default();
                self.end = cards::EndDraft::default();
                Task::none()
            }
            Msg::SpawnName(s) => {
                self.spawn.name = s;
                self.spawn.error = None;
                Task::none()
            }
            Msg::Spawn => self.spawn_sibling(),
            Msg::EndSession(id) => self.end_session(&id),
            Msg::EndConfirmed => match std::mem::take(&mut self.end).tab {
                Some(id) => self.end_session(&id),
                None => Task::none(),
            },
            Msg::AddPath(s) => {
                self.add.name = s;
                self.add.error = None;
                Task::none()
            }
            Msg::AddProject => self.add_project(),
            Msg::RestartShell(id) => self.attach_pane(&id, &[]),
            Msg::PtyOutput(id) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::PtyExit(id) => {
                tracing::info!(pane = %id, "pane PTY exited");
                // Drop the client without `close()` (plain Drop keeps tmux).
                let lived = self.runtimes.remove(&id).map(|rt| rt.backend.age());
                // Tab already dropped (CLI / hover / its project closed).
                // The session is being killed off-thread — do not
                // list-sessions or reattach.
                if self.tab_for_pane(&id).is_none() {
                    self.pane_status.remove(&id);
                    status::persist_all(&self.pane_status);
                    return Task::none();
                }
                if let Some(host) = self.tab_for_pane(&id).and_then(|w| w.host.clone()) {
                    return remote_exit_task(id, host, lived.unwrap_or_default());
                }
                let want = catalog::session_name(&id);
                let live = tmux::list_sessions().unwrap_or_default();
                if live.iter().any(|s| s == &want) {
                    tracing::info!(
                        pane = %id,
                        session = %want,
                        "tmux client exited; session lives, reattaching"
                    );
                    return self.attach_pane(&id, &[]);
                }
                if let Some(st) = self.pane_status.get_mut(&id) {
                    st.status = status::AgentStatus::Idle;
                    st.agent = None;
                    st.tool = None;
                    st.owner_session = None;
                }
                // Its window stays, offering Start new shell.
                self.pane_grids.remove(&id);
                self.sync_row(&id);
                status::persist_all(&self.pane_status);
                Task::none()
            }
            Msg::RemoteExited { pane, state, lived } => self.on_remote_exit(pane, state, lived),
            Msg::QuitIfIdle => {
                if self.windows.is_empty() {
                    tracing::info!("no project opened; quitting");
                    return iced::exit();
                }
                Task::none()
            }
            Msg::Link(ev) => self.on_link(ev),
            Msg::RemoteTick { host, panes } => self.on_remote_tick(host, panes),
            Msg::RemoteAttach { pane, exec } => self.attach_planned(&pane, exec),
            Msg::HostSessions { project, found } => self.on_host_sessions(&project, found),
            Msg::RemoteExec {
                tab,
                pane,
                agent,
                prompt,
                reply,
                alive,
            } => self.on_remote_exec(tab, pane, agent, prompt, reply, alive),
            Msg::RemoteKind { tab, git } => {
                if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == tab) {
                    let kind = if git { catalog::Kind::Main } else { catalog::Kind::Folder };
                    if ws.kind != kind && ws.kind != catalog::Kind::Worktree {
                        ws.kind = kind;
                        self.persist_catalog();
                    }
                }
                Task::none()
            }
            Msg::IslandScanned {
                pane,
                screen,
                message,
            } => self.apply_island_scan(&pane, &screen, message),
            Msg::Title(id, title) => {
                tracing::debug!(pane = %id, %title, "pane title");
                Task::none()
            }
            Msg::BlinkTick => {
                self.cursor_on = !self.cursor_on;
                Task::none()
            }
            Msg::Input(event) => self.on_input(event),
            Msg::SelectionChanged => {
                if let Some(rt) = self.runtimes.get(&self.focused) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::Scrolled(id) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::OpenUrl(uri) => {
                links::open_url(&uri);
                Task::none()
            }
            Msg::WheelToPty(id, bytes) => {
                if let Some(rt) = self.runtimes.get(&id) {
                    rt.backend.write(&bytes);
                }
                Task::none()
            }
            Msg::Pasted(text) => self.on_pasted(text),
        }
    }

    fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let pane: Element<'_, Msg> = match self.windows.tab_of(window) {
            Some(id) if self.tabs.iter().any(|w| w.id == id) => self.render_leaf(id),
            _ => cards::empty_pane(),
        };
        // Clipped: a terminal canvas paints outside its bounds (rows
        // above the top while a grid catches up with its size).
        let body: Element<'_, Msg> = container(pane)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .into();

        let body: Element<'_, Msg> = match (self.windows.focused == Some(window))
            .then(|| cards::overlay(&self.spawn, &self.add, &self.end, &self.projects))
            .flatten()
        {
            Some(veil) => stack![body, veil].into(),
            None => body,
        };

        let bg = self.palette.bg;
        let framed = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            });

        sola_kit::window_frame(
            self.chrome(window),
            self.frame_title(window),
            Msg::TitleDrag(window),
            Msg::TitleClose(window),
            move |dir| Msg::TitleResize(window, dir),
            framed.into(),
        )
    }

    /// At start: a window for each session open here when Projects last
    /// ran (the selected one in front). One whose folder here is gone is
    /// forgotten; its session, if any, is left alone.
    fn open_all_windows(&mut self) -> Task<Msg> {
        let gone: Vec<String> = self
            .tabs
            .iter()
            .filter(|w| w.host.is_none() && !w.path.is_dir())
            .map(|w| w.id.clone())
            .collect();
        for id in &gone {
            tracing::info!(tab = %id, "its folder is gone; forgetting it");
            self.forget_tab(id);
        }
        let ids: Vec<String> = self.tabs.iter().map(|w| w.id.clone()).collect();
        let front = ids
            .iter()
            .find(|id| **id == self.selected)
            .or_else(|| ids.first())
            .cloned();
        let tasks: Vec<Task<Msg>> = ids
            .iter()
            .map(|id| self.open_window(id, front.as_deref() != Some(id.as_str())))
            .collect();
        Task::batch(tasks)
    }

    /// Open session `tab_id`'s window and attach it.
    fn open_window(&mut self, tab_id: &str, background: bool) -> Task<Msg> {
        if self.windows.has(tab_id) {
            return Task::none();
        }
        let open = self.windows.open(APP_ID, tab_id, background).map(|_| Msg::Ignore);
        let attach = self.attach_pane(tab_id, &[]);
        self.publish_menu();
        Task::batch([open, attach])
    }

    /// Close a session's window (its close dot, Close Window, the tab bar's
    /// stoplight, `project.close`): this computer stops watching it. The
    /// session keeps running where it is (Joshua, 2026-10-30: "We should
    /// have to deliberatly tell sessions to close") — End Session ends it.
    /// The last window closing quits.
    fn close_window(&mut self, window: window::Id) -> Task<Msg> {
        let tab = self.windows.forget(window);
        tracing::info!(tab = ?tab, "session window closed; the session stays");
        if let Some(id) = tab {
            self.forget_tab(&id);
        }
        self.publish_menu();
        if self.windows.is_empty() {
            return Task::batch([window::close(window), iced::exit()]);
        }
        Task::batch([window::close(window), Task::none()])
    }

    /// Stop watching session `id` here: its client let go (tmux keeps the
    /// session), its row, and its project when no other session of it is
    /// open here.
    fn forget_tab(&mut self, id: &str) {
        // Dropping the client only detaches it.
        self.runtimes.remove(id);
        self.pane_status.remove(id);
        self.pane_grids.remove(id);
        self.remote_pending.remove(id);
        self.remote_down.remove(id);
        let project = self.tabs.iter().find(|w| w.id == id).map(|w| w.project_id.clone());
        self.tabs.retain(|w| w.id != id);
        if let Some(project) = project {
            if self.tabs.iter().all(|w| w.project_id != project) {
                self.projects.retain(|p| p.id != project);
            }
        }
        if self.selected == id {
            self.selected = self.windows.focused_tab().unwrap_or_default().to_string();
            self.focused = self.selected.clone();
        }
        self.persist_catalog();
        status::persist_all(&self.pane_status);
        self.unlink_unused();
    }

    /// End session `id`: its tmux killed where it runs (off the UI
    /// thread), so every computer watching loses it; its window here
    /// closes. Its folder and any worktree stay.
    fn end_session(&mut self, id: &str) -> Task<Msg> {
        if self.tabs.iter().all(|w| w.id != id) {
            return Task::none();
        }
        tracing::info!(tab = %id, "ending session");
        let detached = self.detach_pane(id);
        let window = self.windows.window_of(id);
        if let Some(w) = window {
            self.windows.forget(w);
        }
        self.forget_tab(id);
        self.publish_menu();
        let close = window.map_or_else(Task::none, window::close);
        let quit = if self.windows.is_empty() { iced::exit() } else { Task::none() };
        Task::batch([close, close_io_task(vec![detached], id.to_string()).chain(quit)])
    }

    /// End Session from the menu: asked first while its agent is working
    /// or waiting on you.
    fn ask_end_session(&mut self, id: &str) -> Task<Msg> {
        let Some(tab) = self.tabs.iter().find(|w| w.id == id) else {
            return Task::none();
        };
        let busy = matches!(
            tab.status,
            status::AgentStatus::Working | status::AgentStatus::Waiting
        );
        if !busy {
            return self.end_session(id);
        }
        let label = self
            .windows
            .window_of(id)
            .and_then(|w| self.label(w))
            .map(|l| match l.session {
                Some(s) => format!("{} › {s}", l.project),
                None => l.project,
            })
            .unwrap_or_else(|| tab.name.clone());
        self.spawn = cards::SpawnDraft::default();
        self.add = cards::AddDraft::default();
        self.end = cards::EndDraft {
            tab: Some(id.to_string()),
            label,
            host: tab.host.clone().unwrap_or_else(machine::this_host),
            waiting: tab.status == status::AgentStatus::Waiting,
        };
        Task::none()
    }

    /// `project.close`: close every window of the project's sessions here.
    /// The sessions keep running.
    fn close_project(&mut self, project_id: &str) -> Task<Msg> {
        let ids: Vec<String> = self
            .tabs
            .iter()
            .filter(|w| w.project_id == project_id)
            .map(|w| w.id.clone())
            .collect();
        let windows: Vec<window::Id> = ids.iter().filter_map(|id| self.windows.window_of(id)).collect();
        Task::batch(windows.into_iter().map(|w| self.close_window(w)))
    }

    /// Window lifecycle: sizes, focus, and the close button / request.
    fn on_window(&mut self, id: window::Id, event: window::Event) -> Task<Msg> {
        match event {
            window::Event::Opened { size, .. } | window::Event::Resized(size) => {
                self.windows.set_size(id, size);
                self.resize_all_panes();
                Task::none()
            }
            window::Event::Focused => {
                self.windows.focused = Some(id);
                self.window_focused = true;
                // Keys and menu actions go to this window's session now.
                if let Some(tab) = self.windows.tab_of(id).map(str::to_string) {
                    if tab != self.selected {
                        self.selected = tab.clone();
                        self.focused = tab;
                        self.persist_catalog();
                    }
                }
                Task::none()
            }
            window::Event::Unfocused => {
                self.window_focused = false;
                // Super release is often eaten by River while we are
                // unfocused (switcher, another window). Drop the latch so
                // the next focus does not treat every key as ⌘.
                self.keyboard_mods = keyboard::Modifiers::empty();
                self.keys_held_mods = keyboard::Modifiers::empty();
                Task::none()
            }
            window::Event::CloseRequested => self.close_window(id),
            window::Event::Closed => {
                // Closed under us (not through close_window).
                if let Some(tab) = self.windows.forget(id) {
                    self.forget_tab(&tab);
                    self.publish_menu();
                }
                if self.windows.is_empty() {
                    return iced::exit();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The app menu, with Project → Open for each project without a window.
    /// The app menu; Project → Open lists the folders of `~/Projects` not
    /// open here.
    fn publish_menu(&self) {
        let open: HashSet<&PathBuf> = self
            .projects
            .iter()
            .filter(|p| p.host.is_none())
            .map(|p| &p.root)
            .collect();
        let folders: Vec<(String, String)> = catalog::project_folders()
            .into_iter()
            .filter(|(_, path)| !open.contains(path))
            .map(|(name, path)| (path.to_string_lossy().into_owned(), name))
            .collect();
        sola_kit::menu::publish(menu::app_menu(&folders));
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        self.float.update(m);
        self.windows.update(m);
        // Cinema drops the titlebar: the panes get its rows.
        if matches!(Topic::parse(m), Some(Topic::WindowChrome(_))) {
            self.resize_all_panes();
        }
        // The tab bar's stoplight: as the window's own close.
        if let Some(window) = self.windows.close_request(m) {
            return self.on_window(window, window::Event::CloseRequested);
        }
        if let Some(Topic::NotificationActivate(a)) = Topic::parse(m) {
            if a.app_id == APP_ID {
                if let Some(tab) = cli::notice_pane(&a.id)
                    .and_then(|pane| self.tab_for_pane(pane))
                    .map(|w| w.id.clone())
                {
                    return self.show_tab(&tab);
                }
            }
            return Task::none();
        }
        if let Some(Topic::AgentIslandAction(a)) = Topic::parse(m) {
            return self.on_island_action(a);
        }
        if let Some(Topic::IdleFade(f)) = Topic::parse(m) {
            // Retracted (sola-river gone): not fading.
            self.idle_fading = m.sticky && f.fading;
            return Task::none();
        }
        if apply_theme_update(m, &mut self.theme) {
            if let Some(Topic::Theme(bus)) = Topic::parse(m) {
                self.palette = Palette::from_kit_theme(&atoms_from_bus_theme(&bus));
                self.metrics = CellMetrics::for_font(self.metrics.font_size, fonts::mono_metrics());
                self.resize_all_panes();
                // The grids are cached in the old colors (a resize clears
                // only the panes whose size changed).
                for rt in self.runtimes.values() {
                    rt.cache.clear();
                }
            }
            return Task::none();
        }
        if is_self_quit(m, APP_ID) {
            return iced::exit();
        }
        if let Some(Topic::MenuAction(p)) = Topic::parse(m) {
            if p.app_id == APP_ID {
                return match p.action_id.as_str() {
                    "spawn-sibling" => match self.selected_project_id() {
                        Some(id) => self.open_spawn(&id),
                        None => self.open_add(),
                    },
                    instance::RAISE => {
                        let tab = Some(self.selected.clone())
                            .filter(|t| !t.is_empty())
                            .or_else(|| self.tabs.first().map(|w| w.id.clone()));
                        match tab {
                            Some(id) => self.show_tab(&id),
                            None => Task::none(),
                        }
                    }
                    "add-project" => self.open_add(),
                    "close-window" => match self.windows.focused {
                        Some(window) => self.close_window(window),
                        None => Task::none(),
                    },
                    open if open.starts_with(menu::OPEN_PREFIX) => {
                        let path = open[menu::OPEN_PREFIX.len()..].to_string();
                        match self.cli_open_project(&path, None) {
                            Ok((_, task)) => task,
                            Err(e) => {
                                tracing::warn!(%path, "Project → Open: {e}");
                                Task::none()
                            }
                        }
                    }
                    "end-session" => {
                        let id = self.selected.clone();
                        self.ask_end_session(&id)
                    }
                    "copy" => self.copy_selection(),
                    "paste" => self.paste_clipboard(),
                    _ => Task::none(),
                };
            }
        }
        Task::none()
    }

    /// Grid (cols, rows) a session should have in its window.
    fn pane_grid(&self, pane_id: &str) -> (u16, u16) {
        let Some(window) = self.windows.window_of(pane_id) else {
            return (DEFAULT_COLS, DEFAULT_ROWS);
        };
        let content = self.content_rect(window);
        let (c, r) = term_view::cols_rows_for(iced::Size::new(content.w, content.h), self.metrics);
        (c.max(2), r.max(1))
    }

    fn render_leaf(&self, pane_id: &str) -> Element<'_, Msg> {
        let inner: Element<'_, Msg> = match self.runtimes.get(pane_id) {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cursor_snap: rt.emulator.cursor_snap(),
                    selection_track: rt.emulator.selection_track(),
                    cache: &rt.cache,
                    palette: &self.palette,
                    metrics: self.metrics,
                    cursor_on: self.cursor_on,
                    active: pane_id == self.focused,
                    on_select: Msg::SelectionChanged,
                    on_scroll: Msg::Scrolled(pane_id.to_string()),
                    on_open_url: Box::new(Msg::OpenUrl),
                    on_wheel_pty: Box::new({
                        let pid = pane_id.to_string();
                        move |bytes| Msg::WheelToPty(pid.clone(), bytes)
                    }),
                };
                canvas(view).width(Length::Fill).height(Length::Fill).into()
            }
            None if self.remote_pending.contains(pane_id) => {
                let host = self
                    .tab_for_pane(pane_id)
                    .and_then(|w| w.host.as_deref())
                    .unwrap_or_default();
                cards::connecting_pane(host)
            }
            None if self.remote_down.contains_key(pane_id) => {
                let host = self
                    .tab_for_pane(pane_id)
                    .and_then(|w| w.host.as_deref())
                    .unwrap_or_default();
                cards::unreachable_pane(pane_id, host, &self.remote_down[pane_id])
            }
            None => cards::exited_pane(pane_id),
        };
        mouse_area(inner)
            .on_enter(Msg::PaneFocused(pane_id.to_string()))
            .into()
    }

    /// Record keyboard focus. Does not attach a PTY (hover must not spawn).
    fn set_focus(&mut self, tab_id: &str) {
        if self.tabs.iter().any(|w| w.id == tab_id) && self.selected != tab_id {
            self.selected = tab_id.to_string();
            self.focused = tab_id.to_string();
            self.persist_catalog();
        }
    }

    fn attach_pane(&mut self, id: &str, exec: &[&str]) -> Task<Msg> {
        if self.runtimes.contains_key(id) {
            self.resize_all_panes();
            return Task::none();
        }
        let Some(ws) = self.tabs.iter().find(|w| w.id == id) else {
            return Task::none();
        };
        let (cols, rows) = self.pane_grid(id);
        let cols = if cols == 0 { DEFAULT_COLS } else { cols };
        let rows = if rows == 0 { DEFAULT_ROWS } else { rows };
        if let Some(host) = ws.host.clone() {
            let cwd = ws.path.to_string_lossy().into_owned();
            return self.attach_remote_pane(id, &host, &cwd, cols, rows, exec);
        }
        let tmux_session = catalog::bind_session(id, &ws.path);
        if tmux_session.is_empty() {
            tracing::error!(pane = %id, "refusing attach; leftover tmux is another checkout");
            return Task::none();
        }
        let cwd = ws.path.to_string_lossy().into_owned();
        // New tmux (reboot / first attach): if this pane had Grok running,
        // `grok -r <session>` restores the conversation. `-A` reattach
        // ignores exec, so a live tmux is left as-is.
        let resume = if exec.is_empty() && !tmux::has_session(&tmux_session) {
            self.resume_argv(id)
        } else {
            None
        };
        if let Some(args) = resume.as_ref() {
            if let (Some(bin), Some(sid)) = (args.first(), args.last()) {
                tracing::info!(pane = %id, agent = %bin, session = %sid, "resuming agent session");
            }
        }
        let exec_refs: Vec<&str> = match resume.as_ref() {
            Some(args) => args.iter().map(String::as_str).collect(),
            None => exec.to_vec(),
        };

        let listener = Listener::new(
            id.to_string(),
            sola_terminal::pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let em = Emulator::new(cols, rows, listener);
        let term = em.term();
        let cursor = em.cursor_snap();

        let env = [("SOLA_PANE_ID", id), (catalog::SOLA_WS_PATH, cwd.as_str())];
        let new_session = !tmux::has_session(&tmux_session);
        let backend = match PtyBackend::spawn_or_attach_with_env(
            id,
            &tmux_session,
            cols,
            rows,
            Some(&cwd),
            term,
            cursor,
            emulator::notify_sender(),
            emulator::exit_sender(),
            &env,
            &exec_refs,
        ) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("spawn_or_attach failed: {e}");
                return Task::none();
            }
        };
        if new_session {
            sessions::stamp(&tmux_session, &self.session_options(id));
        }
        // Another computer may watch it too: its size follows whoever
        // typed last, not this window alone.
        tmux::follow_latest(&tmux_session);
        backend.resize_client(cols, rows);
        backend.sigwinch();
        self.runtimes.insert(
            id.to_string(),
            PaneRuntime {
                emulator: em,
                backend,
                cache: canvas::Cache::default(),
            },
        );
        self.resize_all_panes();
        Task::none()
    }

    /// What session `id` says it is, for any computer that lists it
    /// ([`sessions`]).
    fn session_options(&self, id: &str) -> Vec<(String, String)> {
        let Some(tab) = self.tab_for_pane(id) else {
            return Vec::new();
        };
        let root = catalog::find_project(&self.projects, &tab.project_id)
            .map_or_else(|| tab.path.clone(), |p| p.root.clone());
        sessions::options(&root, &tab.name, tab.title.as_deref(), cli::kind_str(tab.kind))
    }

    /// `grok -r` or `claude --resume` when last-status still names that
    /// agent and the session is on disk. A live tmux is left as-is.
    fn resume_argv(&self, pane_id: &str) -> Option<Vec<String>> {
        resume_argv_on(&Machine::Local, self.pane_status.get(pane_id)?)
    }

    fn resolve_hook_pane(&self, incoming: &hooks::Incoming) -> Option<String> {
        // A hook says its pane's id on its own computer.
        let header = self
            .resolve_pane(&catalog::key_for(&incoming.pane_id, incoming.host.as_deref()))
            .filter(|id| self.tab_for_pane(id).is_some_and(|w| w.host == incoming.host));
        let header_path = header
            .as_deref()
            .and_then(|id| self.tab_for_pane(id))
            .map(|ws| ws.path.clone());
        // A hook names a folder on the computer it came from: only that
        // computer's tabs can be its pane.
        let located: Vec<(String, std::path::PathBuf)> = self
            .tabs
            .iter()
            .filter(|ws| ws.host == incoming.host)
            .map(|ws| (ws.id.clone(), ws.path.clone()))
            .collect();
        let owned: Vec<(String, std::path::PathBuf, Option<String>, Option<String>)> = located
            .into_iter()
            .map(|(id, path)| {
                let st = self.pane_status.get(&id);
                (
                    id,
                    path,
                    st.and_then(|s| s.agent.clone()),
                    st.and_then(|s| s.owner_session.clone()),
                )
            })
            .collect();
        let leaves: Vec<hooks::event::HookLeaf<'_>> = owned
            .iter()
            .map(|(id, path, agent, session)| hooks::event::HookLeaf {
                id,
                path,
                agent: agent.as_deref(),
                session_id: session.as_deref(),
            })
            .collect();
        hooks::event::resolve_hook_target(
            header.as_deref(),
            header_path.as_deref(),
            incoming.cwd.as_deref().map(std::path::Path::new),
            incoming.mapped.session_id.as_deref(),
            &incoming.agent,
            &leaves,
        )
        .and_then(|id| self.resolve_pane(&id))
    }

    fn resolve_pane(&self, id: &str) -> Option<String> {
        if self.tabs.iter().any(|w| w.id == id) {
            return Some(id.to_string());
        }
        if self.adopted_from.as_deref() == Some(id)
            && self.tabs.iter().any(|w| w.id == catalog::LIVE_ID)
        {
            return Some(catalog::LIVE_ID.into());
        }
        None
    }

    fn tab_for_pane(&self, pane_id: &str) -> Option<&catalog::Tab> {
        self.tabs.iter().find(|w| w.id == pane_id)
    }

    /// Copy a session's status onto its row (its mark in the shell's bar).
    fn sync_row(&mut self, pane_id: &str) {
        let st = self.pane_status.get(pane_id);
        let rolled = status::rollup_tracked(st.into_iter());
        let agent = status::loudest_agent(st.into_iter());
        if let Some(ws) = catalog::find_tab_mut(&mut self.tabs, pane_id) {
            ws.status = rolled;
            ws.agent = agent;
        }
    }

    fn sync_all_rows(&mut self) {
        let ids: Vec<String> = self.tabs.iter().map(|w| w.id.clone()).collect();
        for id in ids {
            self.sync_row(&id);
        }
    }

    fn persist_catalog(&self) {
        let projects = self.projects.clone();
        catalog::save(&catalog::Catalog {
            version: 1,
            selected: if self.selected.is_empty() {
                None
            } else {
                Some(self.selected.clone())
            },
            projects,
            tabs: self.tabs.clone(),
        });
    }

    fn select_tab(&mut self, id: &str) -> Task<Msg> {
        if self.tabs.iter().all(|w| w.id != id) {
            return Task::none();
        }
        self.selected = id.to_string();
        self.focused = id.to_string();
        self.persist_catalog();
        self.attach_pane(id, &[])
    }

    /// Bring every session of project `project_id` open here up (each
    /// window opened if it has none), the first in front.
    fn show_project(&mut self, project_id: &str) -> Task<Msg> {
        let ids: Vec<String> = self
            .tabs
            .iter()
            .filter(|w| w.project_id == project_id)
            .map(|w| w.id.clone())
            .collect();
        let mut tasks = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if self.windows.has(id) {
                if i == 0 {
                    self.windows.show(id);
                }
            } else {
                tasks.push(self.open_window(id, i != 0));
            }
        }
        Task::batch(tasks)
    }

    /// Bring session `id`'s window up (opening it if it has none here):
    /// `tab.select`, `--select`, a desk-card click.
    fn show_tab(&mut self, id: &str) -> Task<Msg> {
        if self.tabs.iter().all(|w| w.id != id) {
            return Task::none();
        }
        let select = self.select_tab(id);
        if self.windows.has(id) {
            self.windows.show(id);
            select
        } else {
            Task::batch([select, self.open_window(id, false)])
        }
    }

    /// The New Session card for project `project_id` (in the focused
    /// session's window).
    fn open_spawn(&mut self, project_id: &str) -> Task<Msg> {
        let Some(project) = self.projects.iter().find(|p| p.id == project_id) else {
            return Task::none();
        };
        self.add = cards::AddDraft::default();
        self.end = cards::EndDraft::default();
        self.spawn = cards::SpawnDraft::open(project_id, project.host.clone());
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(cards::SPAWN_INPUT_ID))
    }

    fn open_add(&mut self) -> Task<Msg> {
        self.spawn = cards::SpawnDraft::default();
        self.end = cards::EndDraft::default();
        self.add = cards::AddDraft {
            open: true,
            name: String::new(),
            error: None,
        };
        iced::widget::operation::focus::<Msg>(iced::widget::Id::new(cards::ADD_INPUT_ID))
    }

    fn selected_project_id(&self) -> Option<String> {
        self.tabs
            .iter()
            .find(|w| w.id == self.selected)
            .map(|w| w.project_id.clone())
            .or_else(|| self.projects.first().map(|p| p.id.clone()))
    }

    /// The card's Create: a new session in the project's folder, in a
    /// window of its own.
    fn spawn_sibling(&mut self) -> Task<Msg> {
        let Some(project_id) = self.spawn.project_id.clone() else {
            return Task::none();
        };
        let name = self.spawn.name.clone();
        match self.plan_tab(&project_id, &name, None, None, None, true) {
            Ok(new) => {
                self.spawn = cards::SpawnDraft::default();
                let id = self.add_planned_tab(new);
                self.open_window(&id, false)
            }
            Err(e) => {
                self.spawn.error = Some(e);
                Task::none()
            }
        }
    }

    /// A new session, checked: its id (unique among this computer's view
    /// of its host), its name and title; it starts in the project's folder.
    /// Nothing made yet: its tmux is made when it attaches. Projects makes
    /// no worktree; a session makes its own when its work needs one
    /// (Joshua, 2026-10-30: "I don't want any tie between a project window
    /// and a worktree").
    fn plan_tab(
        &self,
        project_q: &str,
        name: &str,
        parent: Option<String>,
        agent: Option<&str>,
        title: Option<&str>,
        select: bool,
    ) -> Result<NewTab, String> {
        let project = catalog::resolve_project(&self.projects, project_q)?.clone();
        let name = match name.trim() {
            "" => catalog::next_session_name(&self.tabs, &project.id),
            n => n.to_string(),
        };
        let slug = spawn::slug(&name);
        if slug.is_empty() {
            return Err("name needs a letter or number".into());
        }
        if catalog::session_name_taken(&self.tabs, &project.id, &name) {
            return Err(format!("a session '{name}' is already open in this project"));
        }
        let agent = cli::first_class_agent(agent)?;
        let host = project.host.as_deref();
        let mut taken: HashSet<String> = self
            .tabs
            .iter()
            .filter(|w| w.host.as_deref() == host)
            .map(|w| catalog::bare_id(&w.id).to_string())
            .collect();
        // Sessions here that another computer opened are this host's too.
        if host.is_none() {
            taken.extend(catalog::live_local_ids());
        }
        let id = catalog::key_for(&catalog::unique_id("ws", &slug, &taken), host);
        let parent = match parent {
            Some(p) => Some(catalog::resolve_tab(&self.tabs, &p)?.id.clone()),
            None => None,
        };
        let title = title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let tab = catalog::Tab {
            id,
            project_id: project.id.clone(),
            name,
            title,
            path: project.root.clone(),
            host: project.host.clone(),
            kind: catalog::Kind::Root,
            parent,
            layout: None,
            active_pane: None,
            status: status::AgentStatus::Idle,
            agent: agent.map(str::to_string),
        };
        Ok(NewTab { tab, select })
    }

    /// Add a [`NewTab`]. Its id is made again if another session took it
    /// meanwhile.
    fn add_planned_tab(&mut self, new: NewTab) -> String {
        let mut ws = new.tab;
        if self.tabs.iter().any(|w| w.id == ws.id) {
            let host = ws.host.clone();
            let taken: HashSet<String> = self
                .tabs
                .iter()
                .filter(|w| w.host == host)
                .map(|w| catalog::bare_id(&w.id).to_string())
                .collect();
            let slug = spawn::slug(&ws.name);
            ws.id = catalog::key_for(&catalog::unique_id("ws", &slug, &taken), host.as_deref());
        }
        let id = ws.id.clone();
        self.tabs.push(ws);
        catalog::apply_spawn_focus(&mut self.selected, &mut self.focused, &id, new.select);
        self.persist_catalog();
        id
    }

    /// Follow every session's status into its island; send the islands.
    /// Returns screen reads for sessions that newly need you.
    fn sync_islands(&mut self) -> Task<Msg> {
        let now = now_millis() as u64;
        let mut scans = Vec::new();
        for (id, st) in &self.pane_status {
            // A status restored from before a restart says nothing yet.
            let status = (!st.restored_unconfirmed).then_some(st.status);
            if self.islands.track(id, status, now) {
                scans.push(id.clone());
            }
        }
        let known: Vec<String> = self.pane_status.keys().cloned().collect();
        let live: HashSet<String> = known
            .iter()
            .filter(|p| self.tab_for_pane(p).is_some())
            .cloned()
            .collect();
        self.islands.retain(|p| live.contains(p));
        let mut looked = HashMap::new();
        let mut place = HashMap::new();
        for pane in self.islands.panes() {
            looked.insert(pane.clone(), self.looked_at(&pane));
            let (project, name) = self.island_place(&pane);
            place.insert(pane, (project.unwrap_or_default(), name));
        }
        for pane in self.islands.sync(&looked, &place, &known) {
            // Looked at here: every computer watching it lets it go.
            self.announce_seen(&pane);
        }
        Task::batch(scans.into_iter().map(|id| island_scan_later(id, ISLAND_SCAN_MS)))
    }

    /// You are looking at session `pane`: its window has the keyboard and
    /// the desk is not idle-fading (then you are away, and its island
    /// shows).
    fn looked_at(&self, pane: &str) -> bool {
        if self.idle_fading {
            return false;
        }
        let window = self.windows.window_of(pane);
        self.window_focused && window.is_some() && self.windows.focused == window
    }

    /// The prompt draws a moment after its hook: read its choices (a
    /// question's, a permission's) or, with nothing from the hook, the end
    /// of the screen as the body. Tries again while the prompt is not up.
    fn scan_island(&mut self, id: &str) -> Task<Msg> {
        if self.tab_for_pane(id).is_some_and(|w| w.host.is_some()) {
            return self.remote_island_scan(id);
        }
        let screen = tmux::capture_scrollback(&catalog::session_name(id)).unwrap_or_default();
        let message = self.islands.last_message(id);
        self.apply_island_scan(id, &screen, message)
    }

    /// [`Self::scan_island`] with the screen and the transcript's last
    /// message read (here, or fetched from another computer).
    fn apply_island_scan(&mut self, id: &str, screen: &str, message: Option<String>) -> Task<Msg> {
        let Some(need) = self.islands.need_mut(id) else {
            return Task::none();
        };
        need.scans += 1;
        if need.state == IslandState::Waiting {
            // The prompt is at the bottom; an older list further up is not it.
            need.choices = islands::choices_on_screen(&islands::screen_tail(&screen, 40));
            if need.choices.is_empty() && need.scans < ISLAND_SCAN_TRIES {
                return island_scan_later(id.to_string(), ISLAND_RESCAN_MS);
            }
        }
        if need.state == IslandState::Done && !need.asked {
            // The transcript may not have had the last message yet.
            match message {
                Some(m) => {
                    need.body = m;
                    need.asked = true;
                }
                None if need.scans < ISLAND_SCAN_TRIES => {
                    need.body = islands::screen_tail(&screen, ISLAND_TAIL_LINES);
                    return island_scan_later(id.to_string(), ISLAND_RESCAN_MS);
                }
                None => need.body = islands::screen_tail(&screen, ISLAND_TAIL_LINES),
            }
            return Task::none();
        }
        if need.body.is_empty() {
            need.body = islands::screen_tail(&screen, ISLAND_TAIL_LINES);
        }
        Task::none()
    }

    /// The shell's island: an answer typed into the session, a reply, or
    /// its tab pulled up.
    fn on_island_action(&mut self, a: AgentIslandAction) -> Task<Msg> {
        let pane = a.key;
        // Answered or pulled up here: every computer lets it go.
        self.announce_seen(&pane);
        match a.action {
            IslandAction::Show => {
                self.islands.answer(&pane);
                let Some(tab) = self.tab_for_pane(&pane).map(|t| t.id.clone()) else {
                    return Task::none();
                };
                self.show_tab(&tab)
            }
            IslandAction::Choose(i) => {
                let Some(need) = self.islands.answer(&pane) else {
                    return Task::none();
                };
                let Some(choice) = need.offered().get(i as usize).copied().cloned() else {
                    return Task::none();
                };
                tracing::info!(%pane, choice = %choice.label, "island answer");
                let key = choice.key.to_string();
                self.tmux_job(&pane, "island answer", move |t| t.send_literal(&key));
                Task::none()
            }
            IslandAction::Reply(text) => {
                let Some(need) = self.islands.answer(&pane) else {
                    return Task::none();
                };
                // A question's own answer: pick its "Type something" first.
                let own = (need.state == IslandState::Waiting)
                    .then(|| need.choices.iter().find(|c| islands::is_free_text(&c.label)))
                    .flatten()
                    .map(|c| c.key.to_string());
                tracing::info!(%pane, "island reply");
                self.tmux_job(&pane, "island reply", move |t| {
                    if let Some(key) = own {
                        let _ = t.send_literal(&key);
                        std::thread::sleep(ISLAND_TYPE_SETTLE);
                    }
                    t.send_prompt(&text, true)
                });
                Task::none()
            }
        }
    }

    fn toast_place(&self, pane_id: &str) -> (Option<String>, String) {
        let Some(ws) = self.tab_for_pane(pane_id) else {
            return (None, pane_id.to_string());
        };
        let project =
            catalog::find_project(&self.projects, &ws.project_id).map(|p| p.name.clone());
        (project, cli::rail_label(ws))
    }

    /// As [`Self::toast_place`], with the project as its island names it
    /// ([`catalog::island_name`]).
    fn island_place(&self, pane_id: &str) -> (Option<String>, String) {
        let Some(ws) = self.tab_for_pane(pane_id) else {
            return (None, pane_id.to_string());
        };
        let project = catalog::find_project(&self.projects, &ws.project_id)
            .map(|p| catalog::island_name(p).to_string());
        (project, cli::rail_label(ws))
    }

    fn emit_notice(&self, title: String, body: String, tag: String) {
        if let Ok(mut client) = bus().lock() {
            let _ = client.emit(Topic::AppNotification(AppNotification {
                id: format!("ws-{tag}-{}", now_millis()),
                app_id: APP_ID.into(),
                source: "Projects".into(),
                title,
                body,
                tag: Some(tag),
                tab_id: None,
                url: None,
            }));
        }
    }

    fn maybe_toast_status(
        &self,
        id: &str,
        prev: status::AgentStatus,
        now: status::AgentStatus,
        unconfirmed: bool,
    ) {
        if now == prev {
            return;
        }
        if unconfirmed || self.window_focused {
            return;
        }
        let (project, tab) = self.toast_place(id);
        let agent = self
            .pane_status
            .get(id)
            .and_then(|s| s.agent.as_deref())
            .unwrap_or("agent");
        let Some(notice) = cli::status_notice(project.as_deref(), &tab, agent, now) else {
            return;
        };
        let kind = match now {
            status::AgentStatus::Done => "done",
            status::AgentStatus::Waiting => "waiting",
            _ => return,
        };
        self.emit_notice(notice.title, notice.body, format!("{kind}-{id}"));
    }

    fn on_call(&mut self, inc: sola_call::Incoming) -> Task<Msg> {
        if inc.method == "pane.wait" {
            return self.cli_wait(inc);
        }
        if let Some(task) = self.remote_call(&inc) {
            return task;
        }
        let (result, task) = self.dispatch_call(&inc.method, &inc.params);
        match result {
            Ok(data) => inc.reply.ok(data),
            Err(e) => inc.reply.err(e),
        }
        task
    }

    fn dispatch_call(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> (Result<serde_json::Value, String>, Task<Msg>) {
        match method {
            "ps" => (Ok(self.ps_json()), Task::none()),
            "project.list" => {
                let projects: Vec<serde_json::Value> = self
                    .projects
                    .iter()
                    .map(|p| {
                        let mut json = cli::project_json(p);
                        let open = self
                            .tabs
                            .iter()
                            .any(|w| w.project_id == p.id && self.windows.has(&w.id));
                        json["open"] = serde_json::json!(open);
                        json
                    })
                    .collect();
                (
                    Ok(serde_json::json!({ "projects": projects })),
                    Task::none(),
                )
            }
            "project.close" => {
                let Some(q) = param_str(params, "project") else {
                    return (Err("missing project".into()), Task::none());
                };
                match catalog::resolve_project(&self.projects, &q).map(|p| p.id.clone()) {
                    Ok(id) => {
                        let task = self.close_project(&id);
                        (Ok(serde_json::json!({ "project": id, "closed": true })), task)
                    }
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "project.open" => {
                let Some(path) = param_str(params, "path") else {
                    return (Err("missing path".into()), Task::none());
                };
                let host = param_str(params, "host").filter(|h| !h.trim().is_empty());
                match self.cli_open_project(&path, host.as_deref()) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.list" => (
                self.cli_tab_list(param_str(params, "project").as_deref()),
                Task::none(),
            ),
            "tab.spawn" => {
                let Some(project) = param_str(params, "project") else {
                    return (Err("missing project".into()), Task::none());
                };
                let name = param_str(params, "name").unwrap_or_default();
                match self.cli_spawn(
                    &project,
                    &name,
                    param_str(params, "agent").as_deref(),
                    param_str(params, "prompt").as_deref(),
                    param_str(params, "prompt-file").as_deref(),
                    param_str(params, "parent"),
                    param_str(params, "title").as_deref(),
                    params
                        .get("select")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                ) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.rm" => {
                let Some(ws) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                match catalog::resolve_tab(&self.tabs, &ws).map(|w| w.id.clone()) {
                    // Reply first; teardown on the next tick so solactl
                    // (maybe running in the session we are about to end)
                    // gets {ok:true} instead of a hang / timeout.
                    Ok(id) => (
                        Ok(serde_json::json!({ "ok": true })),
                        Task::done(Msg::EndSession(id)),
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.select" => {
                let Some(q) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                match self.cli_select(&q) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "tab.set" => {
                let Some(q) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                (
                    self.cli_set(
                        &q,
                        params.get("name").and_then(|v| v.as_str()),
                        params.get("title").and_then(|v| v.as_str()),
                    ),
                    Task::none(),
                )
            }
            "tab.exec" => {
                let Some(q) = param_str(params, "tab") else {
                    return (Err("missing tab".into()), Task::none());
                };
                match self.cli_exec(
                    &q,
                    param_str(params, "agent").as_deref(),
                    param_str(params, "prompt").as_deref(),
                    param_str(params, "prompt-file").as_deref(),
                ) {
                    Ok((data, task)) => (Ok(data), task),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "pane.list" => (
                self.cli_pane_list(param_str(params, "tab").as_deref()),
                Task::none(),
            ),
            "pane.send" => {
                let Some(text) = param_str(params, "text") else {
                    return (Err("missing text".into()), Task::none());
                };
                let enter = params
                    .get("enter")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                match self.cli_send(param_str(params, "pane").as_deref(), &text, enter) {
                    Ok(pane) => (
                        Ok(serde_json::json!({ "ok": true, "pane": pane })),
                        Task::none(),
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "pane.read" => {
                let lines = params
                    .get("lines")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                match self.cli_read(param_str(params, "pane").as_deref(), lines) {
                    Ok((pane, text)) => (
                        Ok(serde_json::json!({ "text": text, "pane": pane })),
                        Task::none(),
                    ),
                    Err(e) => (Err(e), Task::none()),
                }
            }
            "whoami" => (
                self.cli_whoami(
                    param_str(params, "pane").as_deref(),
                    param_str(params, "path").as_deref(),
                ),
                Task::none(),
            ),
            other => (Err(format!("unknown method {other}")), Task::none()),
        }
    }

    fn ps_json(&self) -> serde_json::Value {
        let projects: Vec<serde_json::Value> = self
            .projects
            .iter()
            .map(|p| {
                let tabs: Vec<serde_json::Value> =
                    catalog::ordered_for_project(&p.id, &self.tabs)
                        .into_iter()
                        .map(|w| cli::tab_json(w, Some(&self.selected)))
                        .collect();
                let mut row = cli::project_json(p);
                row["tabs"] = serde_json::Value::Array(tabs);
                row
            })
            .collect();
        serde_json::json!({ "projects": projects, "selected": self.selected })
    }

    fn cli_tab_list(&self, project: Option<&str>) -> Result<serde_json::Value, String> {
        let filter = match project {
            Some(q) => Some(catalog::resolve_project(&self.projects, q)?.id.clone()),
            None => None,
        };
        let list: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .filter(|w| filter.as_ref().is_none_or(|id| w.project_id == *id))
            .map(|w| cli::tab_json(w, None))
            .collect();
        Ok(serde_json::json!({ "tabs": list }))
    }

    fn cli_spawn(
        &mut self,
        project: &str,
        name: &str,
        agent: Option<&str>,
        prompt: Option<&str>,
        prompt_file: Option<&str>,
        parent: Option<String>,
        title: Option<&str>,
        select: bool,
    ) -> Result<(serde_json::Value, Task<Msg>), String> {
        let prompt = cli::read_prompt(prompt, prompt_file)?;
        let agent = match (cli::first_class_agent(agent)?, prompt.as_deref()) {
            (Some(a), _) => Some(a),
            (None, Some(_)) => Some("grok"),
            (None, None) => None,
        };
        let new = self.plan_tab(project, name, parent, agent, title, select)?;
        let id = self.add_planned_tab(new);
        // Its own window, behind unless `--select`.
        let open = self.windows.open(APP_ID, &id, !select).map(|_| Msg::Ignore);
        self.publish_menu();
        let attach = if let Some(a) = agent {
            let args = cli::agent_argv(a, prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&id, &refs)
        } else {
            self.attach_pane(&id, &[])
        };
        let data = self
            .tabs
            .iter()
            .find(|w| w.id == id)
            .map(|w| cli::spawn_json(w, select))
            .unwrap_or_else(|| serde_json::json!({ "id": id, "selected": select }));
        Ok((data, Task::batch([open, attach])))
    }

    fn cli_select(&mut self, q: &str) -> Result<(serde_json::Value, Task<Msg>), String> {
        let id = catalog::resolve_tab(&self.tabs, q)?
            .id
            .clone();
        let task = self.show_tab(&id);
        Ok((serde_json::json!({ "id": id, "selected": true }), task))
    }

    /// `tab.set`: a session's name (unique in its project here) and
    /// title. Its id and its tmux stay.
    fn cli_set(
        &mut self,
        q: &str,
        name: Option<&str>,
        title: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let id = catalog::resolve_tab(&self.tabs, q)?.id.clone();
        if let Some(raw) = name {
            let raw = raw.trim();
            if spawn::slug(raw).is_empty() {
                return Err("name needs a letter or number".into());
            }
            let project = catalog::resolve_tab(&self.tabs, &id)?.project_id.clone();
            if self
                .tabs
                .iter()
                .any(|w| w.id != id && w.project_id == project && w.name == raw)
            {
                return Err(format!("a session '{raw}' is already open in this project"));
            }
            if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == id) {
                ws.name = raw.to_string();
            }
        }
        if let Some(raw) = title {
            if let Some(ws) = self.tabs.iter_mut().find(|w| w.id == id) {
                cli::apply_rail_title(ws, raw);
            }
        }
        self.persist_catalog();
        let ws = catalog::resolve_tab(&self.tabs, &id)?;
        Ok(cli::tab_json(ws, Some(&self.selected)))
    }

    fn cli_exec(
        &mut self,
        q: &str,
        agent: Option<&str>,
        prompt: Option<&str>,
        prompt_file: Option<&str>,
    ) -> Result<(serde_json::Value, Task<Msg>), String> {
        let agent = cli::first_class_agent(agent)?.unwrap_or("grok");
        let prompt = cli::read_prompt(prompt, prompt_file)?;
        let ws_id = catalog::resolve_tab(&self.tabs, q)?
            .id
            .clone();
        let pane = self.preferred_pane_for(Some(&ws_id), Some(agent))?;
        self.local_pane(&pane)?;
        let is_same = self
            .pane_status
            .get(&pane)
            .and_then(|s| s.agent.as_deref())
            .is_some_and(|a| a.eq_ignore_ascii_case(agent));
        if is_same {
            if let Some(text) = prompt.as_deref() {
                self.write_pane(&pane, text, true)?;
            }
            return Ok((
                serde_json::json!({
                    "tab": ws_id,
                    "pane": pane,
                    "started": false,
                    "sent": prompt.is_some(),
                }),
                Task::none(),
            ));
        }
        let session = catalog::session_name(&pane);
        let new_session = !tmux::has_session(&session);
        let task = if new_session {
            let args = cli::agent_argv(agent, prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&pane, &refs)
        } else {
            let attach = self.attach_pane(&pane, &[]);
            let line = cli::agent_shell_line(agent, prompt.as_deref());
            self.write_pane(&pane, &line, true)?;
            attach
        };
        Ok((
            serde_json::json!({
                "tab": ws_id,
                "pane": pane,
                "started": true,
                "sent": false,
            }),
            task,
        ))
    }


    /// `project.open` (the launcher's project chooser): the project at
    /// `path` in front. Its window shows if open (the shell brings up its
    /// workspace), else opens in front (it lands in the showing workspace);
    /// a folder that is not a project yet is added first, on its root tab.
    /// With `host`, `path` is an absolute folder on that computer.
    fn cli_open_project(
        &mut self,
        path: &str,
        host: Option<&str>,
    ) -> Result<(serde_json::Value, Task<Msg>), String> {
        let known = match host {
            Some(host) => {
                let host = sola_core::ssh::check_host(host)?;
                let root = catalog::remote_root(path)?;
                self.projects
                    .iter()
                    .find(|p| p.host.as_deref() == Some(host) && p.root == root)
                    .map(|p| p.id.clone())
            }
            None => {
                let root = catalog::expand_user_path(path.trim())
                    .canonicalize()
                    .map_err(|e| format!("path: {e}"))?;
                if !catalog::in_projects_dir(&root) {
                    return Err(format!("{} is not a folder of ~/Projects", root.display()));
                }
                self.projects
                    .iter()
                    .find(|p| p.host.is_none() && p.root == root)
                    .map(|p| p.id.clone())
            }
        };
        let project_id = match (known, host) {
            (Some(id), _) => id,
            (None, Some(host)) => self.register_remote_project(host, path)?,
            (None, None) => self.register_project(path)?,
        };
        let was_open = self
            .tabs
            .iter()
            .any(|w| w.project_id == project_id && self.windows.has(&w.id));
        // Its sessions on its computer, whichever computer opened them:
        // listed here at once, or over ssh off the UI thread.
        let task = match host {
            None => {
                let found = sessions::list_local();
                self.on_host_sessions(&project_id, found)
            }
            Some(host) => {
                let (host, project) = (host.to_string(), project_id.clone());
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || sessions::list_remote(&host))
                            .await
                            .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    move |found| Msg::HostSessions {
                        project: project.clone(),
                        found,
                    },
                )
            }
        };
        let project = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| "project vanished".to_string())?;
        let mut data = cli::project_json(project);
        data["opened"] = serde_json::json!(!was_open);
        Ok((data, task))
    }

    fn cli_pane_list(&self, tab: Option<&str>) -> Result<serde_json::Value, String> {
        let id = match tab {
            Some(q) => catalog::resolve_tab(&self.tabs, q)?
                .id
                .clone(),
            None => self.selected.clone(),
        };
        if id.is_empty() {
            return Err("no tab selected".into());
        }
        let leaves = self
            .tabs
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.layout().leaves())
            .unwrap_or_else(|| vec![id.clone()]);
        let panes: Vec<serde_json::Value> = leaves
            .into_iter()
            .map(|pid| {
                let st = self.pane_status.get(&pid);
                cli::pane_json(
                    &pid,
                    st.map(|s| s.status).unwrap_or_default(),
                    st.and_then(|s| s.agent.as_deref()),
                    st.and_then(|s| s.owner_session.as_deref()),
                )
            })
            .collect();
        Ok(serde_json::json!({ "panes": panes }))
    }

    fn pane_agents(&self, leaves: &[String]) -> Vec<(String, Option<String>)> {
        leaves
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    self.pane_status.get(id).and_then(|s| s.agent.clone()),
                )
            })
            .collect()
    }

    fn preferred_pane(&self, hint: Option<&str>) -> Result<String, String> {
        self.preferred_pane_for(hint, Some("grok"))
    }

    fn preferred_pane_for(
        &self,
        hint: Option<&str>,
        prefer: Option<&str>,
    ) -> Result<String, String> {
        if let Some(q) = hint {
            if self.tabs.iter().any(|w| w.owns_pane(q) && w.id != q) {
                return Ok(q.to_string());
            }
            let ws = catalog::resolve_tab(&self.tabs, q)?;
            let leaves = ws.layout().leaves();
            let agents = self.pane_agents(&leaves);
            return Ok(cli::prefer_agent_pane(
                &leaves,
                &agents,
                &ws.active_pane_id(),
                None,
                prefer,
            ));
        }
        if !self.focused.is_empty() {
            return Ok(self.focused.clone());
        }
        if !self.selected.is_empty() {
            return self.preferred_pane_for(Some(&self.selected), prefer);
        }
        Err("no pane".into())
    }

    fn cli_pane_id(&self, pane: Option<&str>) -> Result<String, String> {
        self.preferred_pane(pane)
    }

    /// These sync paths reach this computer's tmux; a remote pane's calls
    /// go through [`App::remote_call`]. Never send to a remote pane's
    /// session name here.
    fn local_pane(&self, id: &str) -> Result<(), String> {
        match self.tab_for_pane(id).and_then(|w| w.host.as_deref()) {
            Some(host) => Err(self
                .machine_for(Some(host))
                .err()
                .unwrap_or_else(|| format!("pane '{id}' runs on {host}"))),
            None => Ok(()),
        }
    }

    fn write_pane(&self, id: &str, text: &str, enter: bool) -> Result<(), String> {
        self.local_pane(id)?;
        let session = catalog::session_name(id);
        tmux::send_prompt(&session, text, enter)
    }

    fn cli_send(&self, pane: Option<&str>, text: &str, enter: bool) -> Result<String, String> {
        let id = self.cli_pane_id(pane)?;
        self.write_pane(&id, text, enter)?;
        Ok(id)
    }

    fn cli_read(&self, pane: Option<&str>, lines: Option<u32>) -> Result<(String, String), String> {
        let id = self.cli_pane_id(pane)?;
        self.local_pane(&id)?;
        let session = catalog::session_name(&id);
        let text = tmux::capture_scrollback(&session)?;
        Ok((id, cli::last_lines(&text, lines)))
    }

    fn cli_whoami(
        &self,
        pane: Option<&str>,
        path: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let ws = if let Some(q) = pane {
            catalog::resolve_tab(&self.tabs, q)?
        } else if let Some(p) = path {
            catalog::resolve_tab(&self.tabs, p)?
        } else {
            return Err(
                "not in a Projects pane (pass --pane/--path or run from a Projects PTY)".into(),
            );
        };
        let pane_id = if let Some(q) = pane {
            if ws.owns_pane(q) {
                q.to_string()
            } else {
                self.preferred_pane(Some(&ws.id))?
            }
        } else {
            self.preferred_pane(Some(&ws.id))?
        };
        let st = self.pane_status.get(&pane_id);
        let project = catalog::resolve_project(&self.projects, &ws.project_id).ok();
        Ok(serde_json::json!({
            "pane": pane_id,
            "tab": ws.id,
            "tab_name": cli::display_name(ws),
            "project": ws.project_id,
            "project_name": project.map(|p| p.name.as_str()),
            "path": ws.path,
            "kind": cli::kind_str(ws.kind),
            "status": cli::status_str(st.map(|s| s.status).unwrap_or(ws.status)),
            "agent": st.and_then(|s| s.agent.clone()).or_else(|| ws.agent.clone()),
            "session_id": st.and_then(|s| s.owner_session.clone()),
        }))
    }

    fn cli_wait(&mut self, inc: sola_call::Incoming) -> Task<Msg> {
        let want = match param_str(&inc.params, "status") {
            Some(s) => match cli::parse_status(&s) {
                Ok(st) => st,
                Err(e) => {
                    inc.reply.err(e);
                    return Task::none();
                }
            },
            None => status::AgentStatus::Done,
        };
        let pane = match self.cli_pane_id(param_str(&inc.params, "pane").as_deref()) {
            Ok(id) => id,
            Err(e) => {
                inc.reply.err(e);
                return Task::none();
            }
        };
        let secs = cli::wait_timeout_secs(inc.params.get("timeout").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        }));
        let fresh = inc
            .params
            .get("fresh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let now = self
            .pane_status
            .get(&pane)
            .map(|s| s.status)
            .unwrap_or_default();
        if !fresh && now == want {
            inc.reply.ok(serde_json::json!({
                "pane": pane,
                "status": cli::status_str(want),
            }));
            return Task::none();
        }
        self.pending_waits.push(PendingWait {
            pane,
            want,
            fresh,
            armed: !fresh || now != want,
            reply: inc.reply,
            deadline: Instant::now() + Duration::from_secs(secs),
        });
        Task::none()
    }

    fn flush_waits(&mut self) {
        let now = Instant::now();
        let mut keep = Vec::new();
        for mut wait in self.pending_waits.drain(..) {
            if now >= wait.deadline {
                wait.reply.err(format!(
                    "timeout waiting for pane {} to be {}",
                    wait.pane,
                    cli::status_str(wait.want)
                ));
                continue;
            }
            let status = self
                .pane_status
                .get(&wait.pane)
                .map(|s| s.status)
                .unwrap_or_default();
            if wait.fresh && !wait.armed {
                if status != wait.want {
                    wait.armed = true;
                }
                keep.push(wait);
                continue;
            }
            if status == wait.want {
                wait.reply.ok(serde_json::json!({
                    "pane": wait.pane,
                    "status": cli::status_str(wait.want),
                }));
                continue;
            }
            keep.push(wait);
        }
        self.pending_waits = keep;
    }

    /// New Project: `~/Projects/<name>` (made if it is not there), opened
    /// in front on its root tab.
    fn add_project(&mut self) -> Task<Msg> {
        let made = catalog::check_project_name(&self.add.name).and_then(|name| {
            let path = catalog::projects_dir().join(name);
            std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(path)
        });
        match made.and_then(|path| self.cli_open_project(&path.to_string_lossy(), None)) {
            Ok((_, task)) => {
                self.add = cards::AddDraft::default();
                task
            }
            Err(e) => {
                self.add.error = Some(e);
                Task::none()
            }
        }
    }

    /// A folder of `~/Projects` here becomes a project in this catalog
    /// (no session yet: [`Self::on_host_sessions`] finds or makes them).
    fn register_project(&mut self, raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("folder path required".into());
        }
        let root = match catalog::expand_user_path(raw).canonicalize() {
            Ok(p) if p.is_dir() => p,
            Ok(_) => return Err("not a folder".into()),
            Err(e) => return Err(format!("path: {e}")),
        };
        if self.projects.iter().any(|p| p.host.is_none() && p.root == root) {
            return Err("already a project".into());
        }
        let slug = spawn::slug(
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("project"),
        );
        let taken_p: HashSet<String> = self.projects.iter().map(|p| p.id.clone()).collect();
        let project_id = catalog::unique_id("proj", &slug, &taken_p);
        let (project, _) = catalog::project_from_root(&root, &project_id, "");
        self.projects.push(project);
        self.persist_catalog();
        Ok(project_id)
    }

    /// [`Self::register_project`] for folder `path` on `host` (checked by
    /// the caller's listing, not here: it is not on this computer).
    fn register_remote_project(&mut self, host: &str, path: &str) -> Result<String, String> {
        let host = sola_core::ssh::check_host(host)?;
        let root = catalog::remote_root(path)?;
        let folder = root.file_name().and_then(|s| s.to_str()).unwrap_or("project");
        let slug = spawn::slug(&format!("{host}-{folder}"));
        let taken_p: HashSet<String> = self.projects.iter().map(|p| p.id.clone()).collect();
        let project_id = catalog::unique_id("proj", &slug, &taken_p);
        let (project, _) = catalog::project_from_remote(host, &root, &project_id, "");
        tracing::info!(project = %project_id, %host, root = %root.display(), "remote project");
        self.projects.push(project);
        self.ensure_link(host);
        self.persist_catalog();
        Ok(project_id)
    }

    /// Project `project_id`'s computer listed its sessions: each one of
    /// this project not open here gets a window; with none anywhere, its
    /// main session starts (its id free on that computer). Then its
    /// windows come up, the first in front.
    fn on_host_sessions(
        &mut self,
        project_id: &str,
        found: Result<Vec<sessions::HostSession>, String>,
    ) -> Task<Msg> {
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return Task::none();
        };
        let found = found.unwrap_or_else(|e| {
            tracing::warn!(project = %project_id, "could not list its sessions: {e}");
            Vec::new()
        });
        let host = project.host.as_deref();
        for s in found.iter().filter(|s| s.project.as_deref() == Some(project.root.as_path())) {
            let key = catalog::key_for(&s.id, host);
            if self.tabs.iter().any(|w| w.id == key) {
                continue;
            }
            let mut name = s.name.clone().unwrap_or_else(|| catalog::name_from_id(&s.id));
            if catalog::session_name_taken(&self.tabs, project_id, &name) {
                name = format!("{name} ({})", s.id);
            }
            tracing::info!(project = %project_id, session = %key, "a session there; opening it here");
            self.tabs.push(catalog::Tab {
                id: key,
                project_id: project_id.to_string(),
                name,
                title: s.title.clone(),
                path: project.root.clone(),
                host: project.host.clone(),
                kind: catalog::kind_from(s.kind.as_deref(), &s.id),
                parent: None,
                layout: None,
                active_pane: None,
                status: status::AgentStatus::Idle,
                agent: None,
            });
        }
        let mut learn = Task::none();
        if self.tabs.iter().all(|w| w.project_id != project_id) {
            let live: HashSet<String> = found.iter().map(|s| s.id.clone()).collect();
            let key = self.add_main_tab(&project, &live);
            if host.is_some() {
                learn = self.learn_remote_root(&key);
            }
        }
        if self.selected.is_empty() {
            if let Some(first) = self.tabs.iter().find(|w| w.project_id == project_id) {
                self.selected = first.id.clone();
                self.focused = first.id.clone();
            }
        }
        self.persist_catalog();
        Task::batch([learn, self.show_project(project_id)])
    }

    /// A new main session for `project`: `ws-main` here when that is free
    /// (as before), `ws-<folder>` on another computer; never an id `live`
    /// (that computer's sessions) or this catalog has there.
    fn add_main_tab(&mut self, project: &catalog::Project, live: &HashSet<String>) -> String {
        let host = project.host.as_deref();
        let mut taken: HashSet<String> = self
            .tabs
            .iter()
            .filter(|w| w.host.as_deref() == host)
            .map(|w| catalog::bare_id(&w.id).to_string())
            .collect();
        taken.extend(live.iter().cloned());
        let (_, mut tab) = match host {
            None => {
                let id = catalog::main_tab_id(&project.root, &taken);
                catalog::project_from_root(&project.root, &project.id, &id)
            }
            Some(h) => {
                let folder = project.root.file_name().and_then(|s| s.to_str()).unwrap_or("project");
                let id = catalog::unique_id("ws", &spawn::slug(folder), &taken);
                catalog::project_from_remote(h, &project.root, &project.id, &id)
            }
        };
        tab.id = catalog::key_for(catalog::bare_id(&tab.id), host);
        let key = tab.id.clone();
        self.tabs.push(tab);
        key
    }

    /// Stop drawing this pane. Caller must `close()` the result (on the
    /// iced thread only when the tmux session name may be reused next,
    /// e.g. last-leaf ⌘W).
    fn detach_pane(&mut self, id: &str) -> DetachedPane {
        let backend = self.runtimes.remove(id).map(|rt| rt.backend);
        self.pane_status.remove(id);
        self.pane_grids.remove(id);
        self.remote_down.remove(id);
        DetachedPane {
            backend,
            session: catalog::session_name(id),
            host: self.tab_for_pane(id).and_then(|w| w.host.clone()),
        }
    }

    /// Where a session window's pane sits, in window coordinates: under
    /// the kit frame's titlebar (none in Cinema). Without the titlebar the
    /// grid had two rows more than fit, the last ones below the window's
    /// edge.
    fn content_rect(&self, window: window::Id) -> term_state::Rect {
        let size = self.windows.size(window).unwrap_or(DEFAULT_WINDOW);
        let top = match self.chrome(window) {
            sola_kit::Chrome::Bare => 0.0,
            _ => sola_kit::components::window_frame::HEIGHT,
        };
        term_state::Rect {
            x: 0.0,
            y: top,
            w: size.width,
            h: (size.height - top).max(0.0),
        }
    }

    /// Size each window's session to the window.
    fn resize_all_panes(&mut self) {
        let targets: Vec<(String, u16, u16)> = self
            .windows
            .iter()
            .map(|(_, id)| {
                let (c, r) = self.pane_grid(id);
                (id.to_string(), c, r)
            })
            .collect();
        for (pane_id, cols, rows) in targets {
            if self.pane_grids.get(&pane_id) == Some(&(cols, rows)) {
                continue;
            }
            let Some(rt) = self.runtimes.get(&pane_id) else {
                continue;
            };
            rt.emulator.resize(cols, rows);
            rt.backend.resize_client(cols, rows);
            rt.backend.sigwinch();
            rt.cache.clear();
            self.pane_grids.insert(pane_id, (cols, rows));
        }
    }

    fn dialog_open(&self) -> bool {
        self.spawn.is_open() || self.add.open || self.end.is_open()
    }

    fn copy_selection(&self) -> Task<Msg> {
        if self.spawn.is_open() && !self.spawn.name.is_empty() {
            return iced::clipboard::write(self.spawn.name.clone());
        }
        if self.add.open && !self.add.name.is_empty() {
            return iced::clipboard::write(self.add.name.clone());
        }
        let Some(rt) = self.runtimes.get(&self.focused) else {
            return Task::none();
        };
        let text = { rt.emulator.term().lock().selection_to_string() };
        match text {
            Some(s) if !s.is_empty() => iced::clipboard::write(s),
            _ => Task::none(),
        }
    }

    fn paste_clipboard(&self) -> Task<Msg> {
        iced::clipboard::read().map(Msg::Pasted)
    }

    fn on_pasted(&mut self, text: Option<String>) -> Task<Msg> {
        let Some(text) = text else {
            return Task::none();
        };
        if self.spawn.is_open() {
            self.spawn.name.push_str(&text.replace('\n', ""));
            return Task::none();
        }
        if self.add.open {
            self.add.name.push_str(&text.replace('\n', ""));
            return Task::none();
        }
        let Some(rt) = self.runtimes.get(&self.focused) else {
            return Task::none();
        };
        let mode = { *rt.emulator.term().lock().mode() };
        rt.backend.write(&input::paste(&text, mode));
        Task::none()
    }

    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
        // Modifier tracking must run even while a card is open. ⌘T and ⌘N open those dialogs; if Super-up is
        // dropped here, `keys_held_mods` / a latched snapshot keep LOGO
        // and every later key is swallowed until quit.
        if let iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
            self.keyboard_mods = mods;
            return Task::none();
        }
        if let iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key, physical_key, ..
        }) = &event
        {
            self.apply_modifier_key(key, physical_key, false);
            return Task::none();
        }

        if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) = &event
        {
            if self.dialog_open() {
                self.spawn = cards::SpawnDraft::default();
                self.add = cards::AddDraft::default();
                self.end = cards::EndDraft::default();
                return Task::none();
            }
        }
        if self.dialog_open() {
            return Task::none();
        }

        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            modifiers,
            location,
            text,
            repeat,
            ..
        }) = event
        else {
            return Task::none();
        };

        self.apply_modifier_key(&key, &physical_key, true);
        if modifier_key_bit(&key, &physical_key).is_some() {
            return Task::none();
        }

        let modifiers = input::merge_modifiers(modifiers, self.keyboard_mods, self.keys_held_mods);
        if modifiers.logo() {
            return Task::none();
        }

        let Some(rt) = self.runtimes.get(&self.focused) else {
            return Task::none();
        };
        let mut mode = { *rt.emulator.term().lock().mode() };
        if extkeys::level(&self.focused) >= 1 {
            mode |= alacritty_terminal::term::TermMode::DISAMBIGUATE_ESC_CODES;
        }
        let mods = Mods::from(modifiers);
        let enter_key = match (&key, &modified_key) {
            (keyboard::Key::Named(keyboard::key::Named::Enter), _)
            | (_, keyboard::Key::Named(keyboard::key::Named::Enter)) => {
                keyboard::Key::Named(keyboard::key::Named::Enter)
            }
            _ => modified_key.clone(),
        };
        let Some(bytes) = input::resolve_bytes(&input::KeyInput {
            key: &key,
            modified_key: &enter_key,
            mods,
            mode,
            location,
            text: text.as_deref(),
            repeat,
            modify_other_keys: extkeys::level(&self.focused) >= 1,
        }) else {
            return Task::none();
        };
        {
            let term = rt.emulator.term();
            let mut guard = term.lock();
            if guard.grid().display_offset() != 0 {
                guard.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            }
        }
        rt.backend.write(&bytes);
        Task::none()
    }

    fn apply_modifier_key(
        &mut self,
        key: &keyboard::Key,
        physical: &keyboard::key::Physical,
        pressed: bool,
    ) {
        let Some(bit) = modifier_key_bit(key, physical) else {
            return;
        };
        if pressed {
            self.keys_held_mods = self.keys_held_mods | bit;
        } else {
            self.keys_held_mods = self.keys_held_mods & !bit;
        }
    }
}

fn modifier_key_bit(
    key: &keyboard::Key,
    physical: &keyboard::key::Physical,
) -> Option<keyboard::Modifiers> {
    use keyboard::key::{Code, Named, Physical};
    if let keyboard::Key::Named(n) = key {
        match n {
            Named::Shift => return Some(keyboard::Modifiers::SHIFT),
            Named::Control => return Some(keyboard::Modifiers::CTRL),
            Named::Alt => return Some(keyboard::Modifiers::ALT),
            Named::Super | Named::Meta => return Some(keyboard::Modifiers::LOGO),
            _ => {}
        }
    }
    match physical {
        Physical::Code(Code::ShiftLeft | Code::ShiftRight) => Some(keyboard::Modifiers::SHIFT),
        Physical::Code(Code::ControlLeft | Code::ControlRight) => Some(keyboard::Modifiers::CTRL),
        Physical::Code(Code::AltLeft | Code::AltRight) => Some(keyboard::Modifiers::ALT),
        Physical::Code(Code::SuperLeft | Code::SuperRight) => Some(keyboard::Modifiers::LOGO),
        _ => None,
    }
}

fn param_str(params: &serde_json::Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn param_bool(params: &serde_json::Value, key: &str) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}
