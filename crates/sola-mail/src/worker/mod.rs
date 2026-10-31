//! Mail worker. A coordinator thread owns the config and the account
//! set; each connected account runs a read session, a write session and
//! an IDLE watcher ([`account::AccountHandle`]). UI commands fan out to
//! the accounts and their replies merge back here, so one slow account
//! never holds up another and a body fetch never waits on a list.

mod account;
mod cmds;
mod images;
mod merge;
mod read;
mod send;
mod write;

pub use cmds::{Flag, LinkState, MailCmd, MailEvent, Op, Page};

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use sola_bus::topics::{MailConfig, MailRule, mail_addr_key};
use tracing::{debug, info, warn};

use crate::bridge;
use crate::protocol::account::smtp_for;
use crate::protocol::imap::{Rows, SEARCH_CAP};
use crate::protocol::sender::{Outgoing, build_message};
use crate::protocol::{Account, FolderRef, MessageSummary};
use account::AccountHandle;
use merge::AccountCounts;
use read::ReadReq;
use send::SendPlan;
use write::WriteCmd;

/// Rules shared with every session (rule edits do not reconnect).
pub type Rules = Arc<RwLock<Vec<MailRule>>>;
/// Idle sessions NOOP this often.
pub const KEEPALIVE: Duration = Duration::from_secs(240);
/// Canonical boxes a search covers (plus the open Junk / Trash).
const SEARCH_BOXES: &[&str] = &["INBOX", "Sent", "Archive", "Drafts"];
/// Seconds before reconnecting a failed account, by attempt.
const BACKOFF: &[u64] = &[15, 30, 60, 120, 300];

/// Everything the coordinator reacts to.
pub enum Input {
    Cmd(MailCmd),
    Up {
        id: String,
        try_id: u64,
        result: Result<Box<AccountHandle>, String>,
    },
    Retry {
        id: String,
        try_id: u64,
    },
    Part {
        job: u64,
        account: String,
        part: Part,
    },
}

/// One account's reply to a fan-out job.
pub enum Part {
    Rows(Result<Rows, String>),
    Counts(Result<AccountCounts, String>),
    Search {
        rows: Vec<MessageSummary>,
        capped: bool,
    },
    Emptied(Result<(), String>),
}

impl Part {
    fn error(&self) -> Option<&str> {
        match self {
            Part::Rows(Err(e)) | Part::Counts(Err(e)) | Part::Emptied(Err(e)) => Some(e),
            _ => None,
        }
    }
}

pub fn start() {
    std::thread::Builder::new()
        .name("sola-mail-worker".into())
        .spawn(run)
        .expect("spawn mail worker");
}

fn run() {
    let (tx, rx) = bridge::take_input();
    let mut c = Coordinator::new(tx);
    while let Ok(input) = rx.recv() {
        if !c.handle(input) {
            break;
        }
    }
    debug!("mail worker stopped");
}

enum Link {
    Connecting,
    Up(Box<AccountHandle>),
    Failed,
}

struct Slot {
    account: Account,
    /// Bumped per connect attempt; a stale `Up` / `Retry` is dropped.
    try_id: u64,
    attempts: u32,
    link: Link,
}

enum JobKind {
    List {
        epoch: u64,
        folder: FolderRef,
        page: Page,
        limit: u32,
    },
    Counts,
    Search {
        epoch: u64,
    },
    Empty {
        folder: String,
    },
}

struct Job {
    kind: JobKind,
    waiting: HashSet<String>,
    parts: Vec<(String, Part)>,
}

enum Req {
    Read(ReadReq),
    Write(WriteCmd),
}

struct Coordinator {
    tx: Sender<Input>,
    config: Option<MailConfig>,
    rules: Rules,
    slots: Vec<Slot>,
    jobs: HashMap<u64, Job>,
    next_job: u64,
    next_try: u64,
}

impl Coordinator {
    fn new(tx: Sender<Input>) -> Self {
        Self {
            tx,
            config: None,
            rules: Arc::new(RwLock::new(Vec::new())),
            slots: Vec::new(),
            jobs: HashMap::new(),
            next_job: 1,
            next_try: 1,
        }
    }

    /// `false` once shut down.
    fn handle(&mut self, input: Input) -> bool {
        match input {
            Input::Cmd(MailCmd::Shutdown) => {
                self.shutdown();
                return false;
            }
            Input::Cmd(cmd) => self.command(cmd),
            Input::Up { id, try_id, result } => self.on_up(&id, try_id, result),
            Input::Retry { id, try_id } => {
                if let Some(i) = self.slot_index(&id)
                    && self.slots[i].try_id == try_id
                    && matches!(self.slots[i].link, Link::Failed)
                {
                    self.connect(i);
                }
            }
            Input::Part { job, account, part } => self.on_part(job, account, part),
        }
        true
    }

    fn command(&mut self, cmd: MailCmd) {
        match cmd {
            MailCmd::Reconfigure(cfg) => self.reconfigure(cfg),
            MailCmd::Retry => {
                for i in 0..self.slots.len() {
                    if matches!(self.slots[i].link, Link::Failed) {
                        self.connect(i);
                    }
                }
            }
            MailCmd::ListFolders => {
                self.start_job(JobKind::Counts, |job, _| {
                    Some(Req::Read(ReadReq::Counts { job }))
                });
            }
            MailCmd::List {
                epoch,
                folder,
                page,
                cursors,
                limit,
            } => {
                let kind = JobKind::List {
                    epoch,
                    folder: folder.clone(),
                    page,
                    limit,
                };
                let started = self.start_job(kind, |job, h| {
                    let id = h.id();
                    let before = cursors.iter().find(|(a, _)| *a == id).map(|(_, uid)| *uid);
                    Some(Req::Read(ReadReq::List {
                        job,
                        folder: folder.clone(),
                        before,
                        limit,
                    }))
                });
                if !started {
                    fail(Op::List, "no account is connected yet");
                }
            }
            MailCmd::Search { epoch, query, also } => {
                let mut folders: Vec<String> = SEARCH_BOXES.iter().map(|s| s.to_string()).collect();
                if let Some(extra) = also.filter(|f| !folders.contains(f)) {
                    folders.push(extra);
                }
                let started = self.start_job(JobKind::Search { epoch }, |job, _| {
                    Some(Req::Read(ReadReq::Search {
                        job,
                        query: query.clone(),
                        folders: folders.clone(),
                    }))
                });
                if !started {
                    fail(Op::Search, "no account is connected yet");
                }
            }
            MailCmd::FetchBody(id) => {
                let sent = self
                    .handle_for(&id.account)
                    .is_some_and(|h| h.read(ReadReq::Body(id.clone())));
                if !sent {
                    fail(Op::Open(id), "that account is not connected");
                }
            }
            MailCmd::FetchImages { id, urls } => images::spawn(id, urls),
            MailCmd::SetFlag { id, flag, on } => {
                let sent = self.handle_for(&id.account).is_some_and(|h| {
                    h.read(ReadReq::Flag {
                        id: id.clone(),
                        flag,
                        on,
                    })
                });
                if !sent {
                    fail(Op::Flag(id), "that account is not connected");
                }
            }
            MailCmd::Move { id, dest } => {
                let sent = self.handle_for(&id.account).is_some_and(|h| {
                    h.write(WriteCmd::Move {
                        id: id.clone(),
                        dest: dest.clone(),
                    })
                });
                if !sent {
                    bridge::emit(MailEvent::MoveFailed {
                        id,
                        message: "that account is not connected".into(),
                    });
                }
            }
            MailCmd::Empty { folder } => {
                let kind = JobKind::Empty {
                    folder: folder.clone(),
                };
                let started = self.start_job(kind, |job, h| {
                    h.has(&folder).then(|| {
                        Req::Write(WriteCmd::Empty {
                            job,
                            folder: folder.clone(),
                        })
                    })
                });
                if !started {
                    fail(
                        Op::Erase,
                        &format!("no connected account has a {folder} mailbox"),
                    );
                }
            }
            MailCmd::Send(msg) => self.send(msg),
            MailCmd::SaveDraft(msg) => self.save_draft(msg),
            MailCmd::Shutdown => {}
        }
    }

    // ── Accounts ──────────────────────────────────────────────────────

    fn reconfigure(&mut self, cfg: MailConfig) {
        let rules_changed = self
            .config
            .as_ref()
            .is_none_or(|old| old.rules != cfg.rules);
        *self.rules.write().unwrap_or_else(|e| e.into_inner()) = cfg.rules.clone();
        let desired = Account::imap_accounts(&cfg);

        let mut dropped = false;
        let mut i = 0;
        while i < self.slots.len() {
            let current = &self.slots[i].account;
            let keep = desired
                .iter()
                .any(|a| a.id() == current.id() && a.conn_key() == current.conn_key());
            if keep {
                i += 1;
            } else {
                let slot = self.slots.remove(i);
                self.drop_slot(slot);
                dropped = true;
            }
        }
        for account in desired {
            match self.slot_index(&account.id()) {
                Some(i) => self.slots[i].account = account,
                None => {
                    self.slots.push(Slot {
                        account,
                        try_id: 0,
                        attempts: 0,
                        link: Link::Connecting,
                    });
                    self.connect(self.slots.len() - 1);
                }
            }
        }
        self.config = Some(cfg);

        if self.slots.is_empty() {
            bridge::emit(MailEvent::NotConfigured);
        } else if (dropped || rules_changed) && self.up().next().is_some() {
            bridge::emit(MailEvent::Changed);
        }
    }

    fn connect(&mut self, i: usize) {
        let try_id = self.next_try;
        self.next_try += 1;
        let slot = &mut self.slots[i];
        slot.try_id = try_id;
        slot.link = Link::Connecting;
        let account = slot.account.clone();
        let id = account.id();
        bridge::emit(MailEvent::AccountLink {
            account: id.clone(),
            email: account.email.clone(),
            state: LinkState::Connecting,
        });
        let (rules, tx) = (Arc::clone(&self.rules), self.tx.clone());
        let started = std::thread::Builder::new()
            .name(format!("sola-mail-c-{id}"))
            .spawn(move || {
                let result = AccountHandle::connect(account, rules, tx.clone()).map(Box::new);
                let _ = tx.send(Input::Up { id, try_id, result });
            });
        if let Err(e) = started {
            warn!("spawn connect thread: {e}");
        }
    }

    fn on_up(&mut self, id: &str, try_id: u64, result: Result<Box<AccountHandle>, String>) {
        let Some(i) = self
            .slot_index(id)
            .filter(|i| self.slots[*i].try_id == try_id)
        else {
            // Superseded (config changed or a newer attempt): discard.
            if let Ok(handle) = result {
                reap(*handle);
            }
            return;
        };
        let email = self.slots[i].account.email.clone();
        match result {
            Ok(handle) => {
                info!(account = id, "IMAP ready");
                let slot = &mut self.slots[i];
                slot.link = Link::Up(handle);
                slot.attempts = 0;
                bridge::emit(MailEvent::AccountLink {
                    account: id.to_string(),
                    email,
                    state: LinkState::Ready,
                });
                bridge::emit(MailEvent::Changed);
            }
            Err(e) => {
                warn!(account = id, "IMAP connect failed: {e}");
                let slot = &mut self.slots[i];
                slot.link = Link::Failed;
                let wait = BACKOFF[(slot.attempts as usize).min(BACKOFF.len() - 1)];
                slot.attempts += 1;
                bridge::emit(MailEvent::AccountLink {
                    account: id.to_string(),
                    email,
                    state: LinkState::Failed(e),
                });
                let (tx, id) = (self.tx.clone(), id.to_string());
                let _ = std::thread::Builder::new()
                    .name("sola-mail-retry".into())
                    .spawn(move || {
                        std::thread::sleep(Duration::from_secs(wait));
                        let _ = tx.send(Input::Retry { id, try_id });
                    });
            }
        }
    }

    fn drop_slot(&mut self, slot: Slot) {
        let id = slot.account.id();
        if let Link::Up(handle) = slot.link {
            reap(*handle);
        }
        let waiting: Vec<u64> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.waiting.contains(&id))
            .map(|(k, _)| *k)
            .collect();
        for job in waiting {
            if let Some(j) = self.jobs.get_mut(&job) {
                j.waiting.remove(&id);
                if j.waiting.is_empty() {
                    let j = self.jobs.remove(&job).expect("present");
                    finish(j);
                }
            }
        }
    }

    fn shutdown(&mut self) {
        let joins: Vec<_> = self
            .slots
            .drain(..)
            .filter_map(|s| match s.link {
                Link::Up(h) => Some(std::thread::spawn(move || h.shutdown())),
                _ => None,
            })
            .collect();
        for j in joins {
            let _ = j.join();
        }
    }

    fn slot_index(&self, id: &str) -> Option<usize> {
        self.slots.iter().position(|s| s.account.id() == id)
    }

    fn up(&self) -> impl Iterator<Item = &AccountHandle> {
        self.slots.iter().filter_map(|s| match &s.link {
            Link::Up(h) => Some(h.as_ref()),
            _ => None,
        })
    }

    fn handle_for(&self, account: &str) -> Option<&AccountHandle> {
        let key = mail_addr_key(account);
        self.up().find(|h| {
            let id = h.id();
            id == key || id == account
        })
    }

    // ── Jobs ──────────────────────────────────────────────────────────

    /// Send one request per connected account (`make` may skip one).
    /// `false` when no account took part.
    fn start_job(
        &mut self,
        kind: JobKind,
        mut make: impl FnMut(u64, &AccountHandle) -> Option<Req>,
    ) -> bool {
        let job = self.next_job;
        self.next_job += 1;
        let mut waiting = HashSet::new();
        for h in self.up() {
            let sent = match make(job, h) {
                Some(Req::Read(r)) => h.read(r),
                Some(Req::Write(w)) => h.write(w),
                None => false,
            };
            if sent {
                waiting.insert(h.id());
            }
        }
        if waiting.is_empty() {
            return false;
        }
        self.jobs.insert(
            job,
            Job {
                kind,
                waiting,
                parts: Vec::new(),
            },
        );
        true
    }

    fn on_part(&mut self, job: u64, account: String, part: Part) {
        let Some(j) = self.jobs.get_mut(&job) else {
            return;
        };
        if !j.waiting.remove(&account) {
            return;
        }
        if let Some(e) = part.error() {
            warn!(account = account.as_str(), "{e}");
        }
        j.parts.push((account, part));
        if j.waiting.is_empty() {
            let j = self.jobs.remove(&job).expect("present");
            finish(j);
        }
    }

    // ── Outgoing ──────────────────────────────────────────────────────

    fn send(&self, msg: Outgoing) {
        let Some(cfg) = &self.config else {
            return fail(Op::Send, "mail is not set up");
        };
        let smtp = smtp_for(cfg, &msg.from);
        if !smtp.can_send() {
            return fail(
                Op::Send,
                &format!(
                    "no outgoing (SMTP) server is set up for {}",
                    msg.from.trim()
                ),
            );
        }
        let smtp_id = smtp.id();
        let sent = self
            .handle_for(&smtp_id)
            .or_else(|| self.up().next())
            .filter(|h| h.has("Sent") && !(smtp.smtp_saves_sent() && h.id() == smtp_id))
            .map(AccountHandle::writer);
        let replaced = msg
            .replaces
            .as_ref()
            .and_then(|id| Some((self.handle_for(&id.account)?.writer(), id.clone())));
        send::spawn(
            SendPlan {
                smtp,
                sent,
                replaced,
            },
            msg,
        );
    }

    fn save_draft(&self, msg: Outgoing) {
        let raw = match build_message(&msg, true) {
            Ok(m) => m.formatted(),
            Err(e) => return fail(Op::SaveDraft, &e.to_string()),
        };
        let target = self
            .up()
            .find(|h| h.account.owns_from(&msg.from))
            .or_else(|| self.up().next())
            .filter(|h| h.has("Drafts"));
        let Some(target) = target else {
            return fail(Op::SaveDraft, "no connected account has a Drafts mailbox");
        };
        // The old copy goes only once the new one landed. A draft moving
        // to another account's Drafts deletes the old copy right away.
        let (same, other): (Option<_>, Option<_>) = match msg.replaces.clone() {
            Some(id) if id.account == target.id() => (Some(id), None),
            Some(id) => (None, Some(id)),
            None => (None, None),
        };
        target.write(WriteCmd::Append {
            folder: "Drafts".into(),
            raw,
            draft: true,
            replaces: same,
        });
        if let Some(id) = other
            && let Some(h) = self.handle_for(&id.account)
        {
            h.write(WriteCmd::Delete(id));
        }
    }
}

fn finish(job: Job) {
    match job.kind {
        JobKind::List {
            epoch,
            folder,
            page,
            limit,
        } => {
            let mut failed = Vec::new();
            let mut ok = Vec::new();
            let mut last_err = String::new();
            for (account, part) in job.parts {
                match part {
                    Part::Rows(Ok(rows)) => ok.push(rows),
                    Part::Rows(Err(e)) => {
                        failed.push(account);
                        last_err = e;
                    }
                    _ => {}
                }
            }
            if ok.is_empty() {
                return fail(Op::List, &last_err);
            }
            let (rows, total) = merge::pages(ok, limit);
            bridge::emit(MailEvent::Messages {
                epoch,
                folder,
                page,
                rows,
                total,
                failed,
            });
        }
        JobKind::Counts => {
            let parts: Vec<AccountCounts> = job
                .parts
                .into_iter()
                .filter_map(|(_, p)| match p {
                    Part::Counts(Ok(c)) => Some(c),
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                return;
            }
            let (folders, smart) = merge::counts(parts);
            bridge::emit(MailEvent::Folders { folders, smart });
        }
        JobKind::Search { epoch } => {
            let mut rows = Vec::new();
            let mut capped = false;
            for (_, part) in job.parts {
                if let Part::Search { rows: r, capped: c } = part {
                    rows.extend(r);
                    capped |= c;
                }
            }
            rows.sort_by(MessageSummary::cmp_recency);
            capped |= rows.len() > SEARCH_CAP;
            rows.truncate(SEARCH_CAP);
            bridge::emit(MailEvent::SearchResults {
                epoch,
                rows,
                capped,
            });
        }
        JobKind::Empty { folder } => {
            let failed = job
                .parts
                .into_iter()
                .filter_map(|(account, p)| match p {
                    Part::Emptied(Err(e)) => Some(format!("{account}: {e}")),
                    _ => None,
                })
                .collect();
            bridge::emit(MailEvent::Emptied { folder, failed });
        }
    }
}

fn fail(op: Op, message: &str) {
    // A list before any account is up is routine; everything else is a
    // failure someone will ask about later.
    if op == Op::List {
        debug!(?op, "{message}");
    } else {
        warn!(?op, "{message}");
    }
    bridge::emit(MailEvent::Failed {
        op,
        message: message.to_string(),
    });
}

/// Stop an account's threads off the coordinator (a session may be
/// mid-command for a few seconds).
fn reap(handle: AccountHandle) {
    let _ = std::thread::Builder::new()
        .name("sola-mail-reap".into())
        .spawn(move || handle.shutdown());
}
