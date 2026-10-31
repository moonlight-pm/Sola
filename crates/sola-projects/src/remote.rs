//! Projects on another computer, as the app runs them, so they work as
//! projects here do: their links ([`crate::link`]: their hooks come here,
//! so marks, islands and `pane.wait` work), attaching their tabs (with an
//! agent's resume), a status tick for their panes (presence, Codex turns,
//! `×N`, gone worktrees), and the calls and island answers that reach
//! their tmux, git and startup scripts. What waits on ssh runs off the UI
//! thread, through a [`Machine::Remote`]; a call there replies when done.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::canvas;
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::pty::PtyBackend;
use sola_terminal::state::PaneRuntime;
use sola_terminal::{remote as rtmux, tmux};

use crate::link::{self, Link, LinkEvent};
use crate::machine::Machine;
use crate::presence::Presence;
use crate::status::{self, PaneStatus};
use crate::{App, Msg, catalog, cli, presence, spawn};

/// A host that could not be linked is tried again after this.
const RETRY_AFTER: Duration = Duration::from_secs(20);
/// How often a host's status tick goes out. Its hooks bring status at
/// once; the tick is for what has no hook (an agent exiting, `×N`), and
/// each look there is an ssh round trip.
const TICK_EVERY: Duration = Duration::from_secs(3);

/// Where a pane's tmux is: here, or on a host (its link up).
#[derive(Clone)]
pub(crate) struct PaneTmux {
    machine: Machine,
    session: String,
}

impl PaneTmux {
    pub(crate) fn capture(&self) -> Result<String, String> {
        match self.machine.host() {
            None => tmux::capture_scrollback(&self.session),
            Some(host) => rtmux::capture_scrollback(host, &self.session),
        }
    }

    pub(crate) fn send_literal(&self, text: &str) -> Result<(), String> {
        match self.machine.host() {
            None => tmux::send_literal(&self.session, text),
            Some(host) => rtmux::send_literal(host, &self.session, text),
        }
    }

    pub(crate) fn send_prompt(&self, text: &str, enter: bool) -> Result<(), String> {
        match self.machine.host() {
            None => tmux::send_prompt(&self.session, text, enter),
            Some(host) => rtmux::send_prompt(host, &self.session, text, enter),
        }
    }

    fn alive(&self) -> Result<bool, String> {
        match self.machine.host() {
            None => Ok(tmux::has_session(&self.session)),
            Some(host) => rtmux::session_state(host, &self.session),
        }
    }

    pub(crate) fn is_local(&self) -> bool {
        self.machine.is_local()
    }
}

impl App {
    /// The machine for `host` (`None`: here). Another computer's only
    /// while its link is up.
    pub(crate) fn machine_for(&self, host: Option<&str>) -> Result<Machine, String> {
        let Some(host) = host else {
            return Ok(Machine::Local);
        };
        match self.links.get(host) {
            Some(Link::Up { machine, .. }) => Ok(machine.clone()),
            Some(Link::Connecting) => Err(format!("still connecting to {host}")),
            Some(Link::Down { why, .. }) => Err(format!("can't reach {host}: {why}")),
            None => Err(format!("not linked to {host}")),
        }
    }

    /// Pane `pane`'s tmux.
    pub(crate) fn pane_tmux(&self, pane: &str) -> Result<PaneTmux, String> {
        let host = self.tab_for_pane(pane).and_then(|w| w.host.clone());
        Ok(PaneTmux {
            machine: self.machine_for(host.as_deref())?,
            session: catalog::session_name(pane),
        })
    }

    /// Run `job` on pane `pane`'s tmux: here at once, there off the UI
    /// thread. A failure is logged under `what`.
    pub(crate) fn tmux_job(
        &self,
        pane: &str,
        what: &'static str,
        job: impl FnOnce(&PaneTmux) -> Result<(), String> + Send + 'static,
    ) {
        let t = match self.pane_tmux(pane) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(%pane, "{what}: {e}");
                return;
            }
        };
        let pane = pane.to_string();
        let local = t.is_local();
        let run = move || {
            if let Err(e) = job(&t) {
                tracing::warn!(%pane, "{what} failed: {e}");
            }
        };
        if local {
            run();
        } else {
            std::thread::spawn(run);
        }
    }

    /// Hosts with a project open.
    fn remote_hosts(&self) -> BTreeSet<String> {
        self.projects.iter().filter_map(|p| p.host.clone()).collect()
    }

    /// Link each host the catalog has a project on (at boot, and every
    /// tick for one down a while).
    pub(crate) fn link_hosts(&mut self) {
        for host in self.remote_hosts() {
            let due = match self.links.get(&host) {
                None => true,
                Some(Link::Down { since, .. }) => since.elapsed() >= RETRY_AFTER,
                Some(_) => false,
            };
            if due {
                self.ensure_link(&host);
            }
        }
    }

    /// Link `host` unless it is linked or connecting.
    pub(crate) fn ensure_link(&mut self, host: &str) {
        if matches!(self.links.get(host), Some(Link::Up { .. } | Link::Connecting)) {
            return;
        }
        let sock = match self.link_socks.get(host) {
            Some(s) => s.clone(),
            None => match crate::hooks::listen_host(host) {
                Ok(s) => {
                    self.link_socks.insert(host.to_string(), s.clone());
                    s
                }
                Err(e) => {
                    tracing::error!(%host, "hook socket for its link: {e}");
                    self.links.insert(
                        host.to_string(),
                        Link::Down {
                            why: format!("hook socket: {e}"),
                            since: Instant::now(),
                        },
                    );
                    return;
                }
            },
        };
        tracing::info!(%host, "linking");
        self.links.insert(host.to_string(), Link::Connecting);
        link::spawn(host.to_string(), sock);
    }

    /// Unlink a host no project is on any more.
    pub(crate) fn unlink_unused(&mut self) {
        let used = self.remote_hosts();
        let gone: Vec<String> = self
            .links
            .keys()
            .filter(|h| !used.contains(*h))
            .cloned()
            .collect();
        for host in gone {
            if let Some(Link::Up { pid, .. }) = self.links.remove(&host) {
                tracing::info!(%host, "no project there any more; unlinking");
                link::stop(pid);
            }
        }
    }

    pub(crate) fn on_link(&mut self, ev: LinkEvent) -> Task<Msg> {
        match ev {
            LinkEvent::Up { host, remote, pid } => {
                if !self.remote_hosts().contains(&host) {
                    link::stop(pid);
                    return Task::none();
                }
                self.links.insert(
                    host.clone(),
                    Link::Up {
                        machine: Machine::Remote(remote),
                        pid,
                    },
                );
                // Panes that waited for it, or said it was out of reach.
                let waiting: Vec<String> = self
                    .remote_pending
                    .iter()
                    .chain(self.remote_down.keys())
                    .filter(|p| self.host_of_pane(p).as_deref() == Some(host.as_str()))
                    .cloned()
                    .collect();
                let mut tasks: Vec<Task<Msg>> = waiting.iter().map(|p| self.attach_pane(p, &[])).collect();
                // Its projects' roots: git checkouts, or folders.
                let roots: Vec<String> = self
                    .tabs
                    .iter()
                    .filter(|w| w.host.as_deref() == Some(host.as_str()))
                    .filter(|w| matches!(w.kind, catalog::Kind::Main | catalog::Kind::Folder))
                    .map(|w| w.id.clone())
                    .collect();
                tasks.extend(roots.iter().map(|id| self.learn_remote_root(id)));
                Task::batch(tasks)
            }
            LinkEvent::Failed { host, why } => {
                self.down(&host, why);
                Task::none()
            }
            LinkEvent::Lost { host, pid, why } => {
                if matches!(self.links.get(&host), Some(Link::Up { pid: p, .. }) if *p == pid) {
                    self.down(&host, why);
                }
                Task::none()
            }
        }
    }

    fn down(&mut self, host: &str, why: String) {
        let pending: Vec<String> = self
            .remote_pending
            .iter()
            .filter(|p| self.host_of_pane(p).as_deref() == Some(host))
            .cloned()
            .collect();
        for pane in pending {
            self.remote_pending.remove(&pane);
            self.remote_down.insert(pane, why.clone());
        }
        self.links.insert(
            host.to_string(),
            Link::Down {
                why,
                since: Instant::now(),
            },
        );
    }

    fn host_of_pane(&self, pane: &str) -> Option<String> {
        self.tab_for_pane(pane).and_then(|w| w.host.clone())
    }

    /// [`App::attach_pane`] for a tab on `host`: once its link is up, its
    /// ssh there attaches the pane's session, or makes it in `cwd`. A new
    /// session where Grok or Claude was resumes it, as here.
    pub(crate) fn attach_remote_pane(
        &mut self,
        id: &str,
        host: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
        exec: &[&str],
    ) -> Task<Msg> {
        self.remote_down.remove(id);
        let machine = match self.links.get(host) {
            Some(Link::Up { machine, .. }) => machine.clone(),
            _ => {
                self.remote_pending.insert(id.to_string());
                self.ensure_link(host);
                return Task::none();
            }
        };
        self.remote_pending.remove(id);
        let resumable = self
            .pane_status
            .get(id)
            .filter(|s| s.resume_session_id().is_some())
            .cloned();
        match (exec.is_empty(), resumable) {
            (true, Some(st)) => plan_attach(id.to_string(), machine, st),
            _ => self.spawn_remote_pty(id, &machine, cwd, cols, rows, exec),
        }
    }

    /// A remote pane's ssh ended. Its session still there: reattach (once
    /// the ssh had run a while — else it would only fail again). Gone: the
    /// shell exited, as for a pane here. Out of reach: say so on the tab
    /// and leave the session be.
    pub(crate) fn on_remote_exit(
        &mut self,
        pane: String,
        state: Result<bool, String>,
        lived: Duration,
    ) -> Task<Msg> {
        // Dropped meanwhile, or reattached (Reconnect).
        if self.tab_for_pane(&pane).is_none() || self.runtimes.contains_key(&pane) {
            return Task::none();
        }
        match state {
            Ok(true) if lived >= crate::REMOTE_REATTACH_AFTER => {
                tracing::info!(pane = %pane, "ssh ended; remote session lives, reattaching");
                self.attach_pane(&pane, &[])
            }
            Ok(true) => {
                tracing::warn!(pane = %pane, ?lived, "remote session lives but would not attach");
                self.remote_down
                    .insert(pane, "The session is there, but would not attach.".into());
                Task::none()
            }
            // Its window stays, offering Start new shell.
            Ok(false) => {
                if let Some(st) = self.pane_status.get_mut(&pane) {
                    *st = PaneStatus::default();
                }
                self.sync_row(&pane);
                Task::none()
            }
            Err(why) => {
                tracing::warn!(pane = %pane, "remote pane out of reach: {why}");
                self.remote_down.insert(pane, why);
                Task::none()
            }
        }
    }

    /// The plan for a resumable pane came back: attach with it.
    pub(crate) fn attach_planned(&mut self, id: &str, exec: Vec<String>) -> Task<Msg> {
        if self.runtimes.contains_key(id) {
            return Task::none();
        }
        let Some(ws) = self.tab_for_pane(id) else {
            return Task::none();
        };
        let cwd = ws.path.to_string_lossy().into_owned();
        let Ok(machine) = self.machine_for(ws.host.as_deref()) else {
            return self.attach_pane(id, &[]);
        };
        let (cols, rows) = self.pane_grid(id);
        let cols = if cols == 0 { crate::DEFAULT_COLS } else { cols };
        let rows = if rows == 0 { crate::DEFAULT_ROWS } else { rows };
        if let (Some(bin), Some(sid)) = (exec.first(), exec.last()) {
            tracing::info!(pane = %id, agent = %bin, session = %sid, "resuming agent session there");
        }
        let refs: Vec<&str> = exec.iter().map(String::as_str).collect();
        self.spawn_remote_pty(id, &machine, &cwd, cols, rows, &refs)
    }

    fn spawn_remote_pty(
        &mut self,
        id: &str,
        machine: &Machine,
        cwd: &str,
        cols: u16,
        rows: u16,
        exec: &[&str],
    ) -> Task<Msg> {
        let Machine::Remote(r) = machine else {
            return Task::none();
        };
        let listener = Listener::new(
            id.to_string(),
            sola_terminal::pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let em = Emulator::new(cols, rows, listener);
        let hook_sock = link::remote_socket(r).to_string_lossy().into_owned();
        let env = [
            ("SOLA_PANE_ID", catalog::bare_id(id)),
            ("SOLA_WS_HOOKS_SOCK", hook_sock.as_str()),
            (catalog::SOLA_WS_PATH, cwd),
        ];
        let opts = self.session_options(id);
        let opts: Vec<(&str, &str)> = opts.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let backend = match PtyBackend::spawn_remote(
            id,
            &r.host,
            &catalog::session_name(id),
            cols,
            rows,
            cwd,
            em.term(),
            em.cursor_snap(),
            emulator::notify_sender(),
            emulator::exit_sender(),
            &env,
            &opts,
            exec,
        ) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(pane = %id, host = %r.host, "remote attach failed: {e}");
                self.remote_down.insert(id.to_string(), e.to_string());
                return Task::none();
            }
        };
        tracing::info!(pane = %id, host = %r.host, %cwd, "attaching remote pane");
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

    /// The presence tick's part for other computers: for each linked host
    /// (one look at a time), its panes' presence, Codex turns and `×N`
    /// ([`remote_tick`]).
    pub(crate) fn remote_ticks(&mut self) -> Task<Msg> {
        self.link_hosts();
        let mut tasks = Vec::new();
        let hosts: Vec<(String, Machine)> = self
            .links
            .iter()
            .filter_map(|(h, l)| match l {
                Link::Up { machine, .. } => Some((h.clone(), machine.clone())),
                _ => None,
            })
            .collect();
        for (host, machine) in hosts {
            let due = self
                .remote_ticked
                .get(&host)
                .is_none_or(|t| t.elapsed() >= TICK_EVERY);
            if !due || !self.remote_busy.insert(host.clone()) {
                continue;
            }
            self.remote_ticked.insert(host.clone(), Instant::now());
            let panes: Vec<(String, PathBuf, PaneStatus)> = self
                .tabs
                .iter()
                .filter(|w| w.host.as_deref() == Some(host.as_str()))
                .map(|w| {
                    let st = self.pane_status.get(&w.id).cloned().unwrap_or_default();
                    (w.id.clone(), w.path.clone(), st)
                })
                .collect();
            tasks.push(Task::perform(
                async move {
                    let h = host.clone();
                    let done = tokio::task::spawn_blocking(move || remote_tick(&machine, panes))
                        .await
                        .unwrap_or_default();
                    (h, done)
                },
                |(host, panes)| Msg::RemoteTick { host, panes },
            ));
        }
        Task::batch(tasks)
    }

    /// A host's tick came back. A pane a hook changed meanwhile keeps the
    /// hook's word (the next tick looks again).
    pub(crate) fn on_remote_tick(
        &mut self,
        host: String,
        panes: Vec<(String, PaneStatus, PaneStatus)>,
    ) -> Task<Msg> {
        self.remote_busy.remove(&host);
        let mut dirty = false;
        for (id, before, after) in panes {
            if self.tab_for_pane(&id).is_none() {
                continue;
            }
            let now = self.pane_status.get(&id).cloned().unwrap_or_default();
            if now != before {
                continue;
            }
            if (now.status, &now.agent) != (after.status, &after.agent) {
                dirty = true;
            }
            self.pane_status.insert(id, after);
        }
        if dirty {
            status::persist_all(&self.pane_status);
        }
        self.sync_all_rows();
        self.flush_waits();
        Task::none()
    }

    /// A call that reaches another computer's pane, tmux, git or scripts:
    /// done off the UI thread, replied when done. `None`: not one, the
    /// usual dispatch takes it (and reports any error).
    pub(crate) fn remote_call(&mut self, inc: &sola_call::Incoming) -> Option<Task<Msg>> {
        let params = &inc.params;
        let reply = inc.reply.clone();
        match inc.method.as_str() {
            "pane.send" | "pane.read" => {
                let pane = self.cli_pane_id(crate::param_str(params, "pane").as_deref()).ok()?;
                self.host_of_pane(&pane)?;
                let t = match self.pane_tmux(&pane) {
                    Ok(t) => t,
                    Err(e) => {
                        reply.err(e);
                        return Some(Task::none());
                    }
                };
                let send = inc.method == "pane.send";
                let text = crate::param_str(params, "text");
                let enter = crate::param_bool(params, "enter");
                let lines = params.get("lines").and_then(|v| v.as_u64()).map(|n| n as u32);
                std::thread::spawn(move || {
                    if send {
                        let Some(text) = text else {
                            return reply.err("missing text");
                        };
                        match t.send_prompt(&text, enter) {
                            Ok(()) => reply.ok(serde_json::json!({ "pane": pane })),
                            Err(e) => reply.err(e),
                        }
                    } else {
                        match t.capture() {
                            Ok(text) => {
                                let text = cli::last_lines(&text, lines);
                                reply.ok(serde_json::json!({ "text": text, "pane": pane }))
                            }
                            Err(e) => reply.err(e),
                        }
                    }
                });
                Some(Task::none())
            }
            "tab.exec" => {
                let q = crate::param_str(params, "tab")?;
                let ws = catalog::resolve_tab(&self.tabs, &q).ok()?;
                ws.host.as_ref()?;
                let ws_id = ws.id.clone();
                Some(self.remote_exec(
                    ws_id,
                    crate::param_str(params, "agent"),
                    crate::param_str(params, "prompt"),
                    crate::param_str(params, "prompt-file"),
                    reply,
                ))
            }
            _ => None,
        }
    }

    /// `tab.exec` on a remote tab, as [`App::cli_exec`] does here.
    fn remote_exec(
        &mut self,
        ws_id: String,
        agent: Option<String>,
        prompt: Option<String>,
        prompt_file: Option<String>,
        reply: sola_call::ReplyTx,
    ) -> Task<Msg> {
        let agent = match cli::first_class_agent(agent.as_deref()) {
            Ok(a) => a.unwrap_or("grok").to_string(),
            Err(e) => {
                reply.err(e);
                return Task::none();
            }
        };
        let prompt = match cli::read_prompt(prompt.as_deref(), prompt_file.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                reply.err(e);
                return Task::none();
            }
        };
        let pane = match self.preferred_pane_for(Some(&ws_id), Some(&agent)) {
            Ok(p) => p,
            Err(e) => {
                reply.err(e);
                return Task::none();
            }
        };
        let t = match self.pane_tmux(&pane) {
            Ok(t) => t,
            Err(e) => {
                reply.err(e);
                return Task::none();
            }
        };
        let same = self
            .pane_status
            .get(&pane)
            .and_then(|s| s.agent.as_deref())
            .is_some_and(|a| a.eq_ignore_ascii_case(&agent));
        if same {
            std::thread::spawn(move || {
                let sent = prompt.is_some();
                let done = match prompt.as_deref() {
                    Some(text) => t.send_prompt(text, true),
                    None => Ok(()),
                };
                match done {
                    Ok(()) => reply.ok(serde_json::json!({
                        "tab": ws_id, "pane": pane, "started": false, "sent": sent,
                    })),
                    Err(e) => reply.err(e),
                }
            });
            return Task::none();
        }
        Task::perform(
            async move {
                let alive = tokio::task::spawn_blocking(move || t.alive())
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                (ws_id, pane, agent, prompt, reply, alive)
            },
            |(tab, pane, agent, prompt, reply, alive)| Msg::RemoteExec {
                tab,
                pane,
                agent,
                prompt,
                reply,
                alive,
            },
        )
    }

    /// [`Self::remote_exec`]'s session check came back: start the agent.
    pub(crate) fn on_remote_exec(
        &mut self,
        tab: String,
        pane: String,
        agent: String,
        prompt: Option<String>,
        reply: sola_call::ReplyTx,
        alive: Result<bool, String>,
    ) -> Task<Msg> {
        let alive = match alive {
            Ok(a) => a,
            Err(e) => {
                reply.err(e);
                return Task::none();
            }
        };
        let task = if alive {
            let attach = self.attach_pane(&pane, &[]);
            let line = cli::agent_shell_line(&agent, prompt.as_deref());
            self.tmux_job(&pane, "tab.exec", move |t| t.send_prompt(&line, true));
            attach
        } else {
            let args = cli::agent_argv(&agent, prompt.as_deref());
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.attach_pane(&pane, &refs)
        };
        reply.ok(serde_json::json!({ "tab": tab, "pane": pane, "started": true, "sent": false }));
        task
    }

    /// Learn off the UI thread what a remote project's root is (a git
    /// checkout, or a folder).
    pub(crate) fn learn_remote_root(&self, tab_id: &str) -> Task<Msg> {
        let Some(ws) = self.tabs.iter().find(|w| w.id == tab_id) else {
            return Task::none();
        };
        let Ok(machine) = self.machine_for(ws.host.as_deref()) else {
            return Task::none();
        };
        let (tab, root) = (ws.id.clone(), ws.path.clone());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || spawn::is_git_checkout(&machine, &root))
                    .await
                    .unwrap_or(true)
            },
            move |git| Msg::RemoteKind {
                tab: tab.clone(),
                git,
            },
        )
    }

    /// An island scan for a remote pane: its screen and its transcript's
    /// last message, fetched off the UI thread.
    pub(crate) fn remote_island_scan(&self, id: &str) -> Task<Msg> {
        let Ok(t) = self.pane_tmux(id) else {
            return Task::none();
        };
        let transcript = self.islands.transcript_of(id).map(PathBuf::from);
        let machine = t.machine.clone();
        let pane = id.to_string();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let screen = t.capture().unwrap_or_default();
                    let message = transcript.and_then(|p| crate::islands::message_at(&machine, &p));
                    (screen, message)
                })
                .await
                .unwrap_or_default()
            },
            move |(screen, message)| Msg::IslandScanned {
                pane: pane.clone(),
                screen,
                message,
            },
        )
    }
}

/// Off the UI thread: a resumable remote pane's session alive there
/// (attach as it is), else its agent's resume if that session is still
/// there (as [`App::resume_argv`] here).
fn plan_attach(id: String, machine: Machine, st: PaneStatus) -> Task<Msg> {
    let session = catalog::session_name(&id);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let alive = machine
                    .host()
                    .map(|h| rtmux::session_state(h, &session) == Ok(true))
                    .unwrap_or(false);
                if alive {
                    return Vec::new();
                }
                crate::resume_argv_on(&machine, &st).unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        },
        move |exec| Msg::RemoteAttach {
            pane: id.clone(),
            exec,
        },
    )
}

/// Off the UI thread, for one host: its panes' presence (one look) and
/// Codex turns and `×N` from its files. Each pane comes back as it was
/// and as it is now.
fn remote_tick(
    m: &Machine,
    panes: Vec<(String, PathBuf, PaneStatus)>,
) -> Vec<(String, PaneStatus, PaneStatus)> {
    let sessions: Vec<String> = panes.iter().map(|(id, ..)| catalog::session_name(id)).collect();
    let who = presence::scan_remote(m, &sessions);
    let mut claimed: HashSet<String> = HashSet::new();
    for (_, _, st) in &panes {
        if let (true, Some(sid)) = (st.is_codex(), st.owner_session.as_deref()) {
            if status::codex_rollout_exists(m, sid) {
                claimed.insert(sid.to_string());
            }
        }
    }
    let mut out = Vec::with_capacity(panes.len());
    for (id, cwd, before) in panes {
        let mut st = before.clone();
        let seen = who.get(&catalog::session_name(&id)).copied().unwrap_or(Presence::Unknown);
        st.apply_presence(seen);
        if st.is_codex() {
            st.observe_codex_turn(m, &cwd, &claimed);
            if let Some(sid) = st.owner_session.clone() {
                claimed.insert(sid);
            }
        }
        st.refresh_compaction(m, &cwd);
        out.push((id, before, st));
    }
    out
}
