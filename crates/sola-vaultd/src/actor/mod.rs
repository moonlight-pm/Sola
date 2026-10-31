//! The vault's one owner. Every request that touches the account runs
//! here, one at a time, on its own thread: signing in, unlocking,
//! locking, syncing, reading and writing. It publishes the status
//! (a watch) and item events (a broadcast) for the socket, the bus and
//! the call plane, and locks the vault when it has been idle or the
//! screen locks.

mod account;
mod requests;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sola_vault_proto::{ErrorCode, Event, Reply, Request, Response, Server, Settings, Status};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::info;
use zeroize::Zeroizing;

use crate::idle::IdleClock;
use crate::peer::{Peer, Role};
use crate::prefs::Prefs;
use crate::tickets::Tickets;
use crate::vault::errors::{Result, err, locked, signed_out};
use crate::vault::{Session, VaultService, session};

/// How often the screen lock is looked for while the vault is open.
const SCREEN_POLL: Duration = Duration::from_secs(3);
/// A login waiting for its second step is forgotten after this.
const PENDING_TTL: Duration = Duration::from_secs(10 * 60);

/// What the actor is asked.
pub enum Input {
    /// A socket request from `peer`.
    Request {
        req: Request,
        peer: Peer,
        reply: oneshot::Sender<Response>,
    },
    /// The desk's idle fade and its settings, from the bus.
    Desk(DeskSignal),
    /// The call plane's `lock` and `sync`.
    Call(CallJob, oneshot::Sender<std::result::Result<(), String>>),
}

pub enum DeskSignal {
    Fade(bool),
    Look { fade_idle: bool, fade_after_min: u16 },
}

pub enum CallJob {
    Lock,
    Sync,
}

/// Where the actor tells what changed.
#[derive(Clone)]
pub struct Outlets {
    pub status: watch::Sender<Status>,
    pub events: broadcast::Sender<Event>,
}

enum Account {
    SignedOut,
    Locked(Session),
    Unlocked(Box<VaultService>),
}

/// A login waiting for its second step: what it needs to finish.
struct Pending {
    email: String,
    password: Zeroizing<String>,
    server: Server,
    since: Instant,
}

/// The last sync, as the status shows it.
#[derive(Default)]
struct SyncState {
    at: Option<u64>,
    syncing: bool,
    error: Option<String>,
    items: u32,
    unreadable: u32,
}

pub struct Vault {
    dir: PathBuf,
    prefs: Prefs,
    settings: Settings,
    account: Account,
    pending: Option<Pending>,
    tickets: Tickets,
    idle: IdleClock,
    next_screen_poll: Instant,
    sync: SyncState,
    out: Outlets,
}

impl Vault {
    /// Start signed in and locked when a session is on disk, else signed
    /// out.
    pub fn new(dir: PathBuf, prefs: Prefs, settings: Settings, out: Outlets) -> Self {
        let account = match session::load(&dir) {
            Some(s) => {
                info!(server = %s.server.label(), "vault: signed in, locked");
                Account::Locked(s)
            }
            None => {
                info!("vault: signed out");
                Account::SignedOut
            }
        };
        let now = Instant::now();
        let vault = Self {
            dir,
            prefs,
            settings,
            account,
            pending: None,
            tickets: Tickets::default(),
            idle: IdleClock::new(now),
            next_screen_poll: now,
            sync: SyncState::default(),
            out,
        };
        vault.publish();
        vault
    }

    pub fn status(&self) -> Status {
        let (signed_in, unlocked, email, server) = match &self.account {
            Account::SignedOut => match &self.pending {
                Some(p) => (false, false, p.email.clone(), p.server.clone()),
                // The account last signed in, for the sign-in form.
                None => (false, false, self.prefs.last_email.clone().unwrap_or_default(), Server::default()),
            },
            Account::Locked(s) => (true, false, s.email.clone(), s.server.clone()),
            Account::Unlocked(v) => (true, true, v.session().email.clone(), v.session().server.clone()),
        };
        Status {
            signed_in,
            unlocked,
            email,
            server,
            synced_at: self.sync.at,
            syncing: self.sync.syncing,
            sync_error: self.sync.error.clone(),
            items: self.sync.items,
            unreadable: self.sync.unreadable,
            two_factor_pending: self.pending.is_some(),
        }
    }

    fn publish(&self) {
        let now = self.status();
        self.out.status.send_if_modified(|s| {
            if *s == now {
                false
            } else {
                *s = now;
                true
            }
        });
    }

    fn event(&self, event: Event) {
        let _ = self.out.events.send(event);
    }

    fn svc(&self) -> Result<&VaultService> {
        match &self.account {
            Account::Unlocked(v) => Ok(v),
            Account::Locked(_) => Err(locked()),
            Account::SignedOut => Err(signed_out()),
        }
    }

    async fn handle(&mut self, input: Input) {
        match input {
            Input::Request { req, peer, reply } => {
                let counts = !matches!(
                    req,
                    Request::Hello { .. } | Request::Status | Request::Settings | Request::Subscribe
                );
                if counts && peer.role == Some(Role::Full) {
                    self.idle.used(Instant::now());
                }
                let result = self.request(req, &peer).await;
                self.end_refused_session();
                self.publish();
                let _ = reply.send(match result {
                    Ok(r) => Response::Ok(r),
                    Err(e) => Response::Err(e),
                });
            }
            Input::Desk(DeskSignal::Fade(fading)) => self.idle.fading(fading, Instant::now()),
            Input::Desk(DeskSignal::Look {
                fade_idle,
                fade_after_min,
            }) => self.idle.look(fade_idle, fade_after_min),
            Input::Call(job, reply) => {
                let result = match job {
                    CallJob::Lock => {
                        self.lock("asked on the call plane");
                        Ok(())
                    }
                    CallJob::Sync => self.sync_now().await.map_err(|e| e.message),
                };
                self.end_refused_session();
                self.publish();
                let _ = reply.send(result);
            }
        }
    }

    /// The next time [`Self::tick`] has something to look at.
    fn next_wake(&self) -> Option<Instant> {
        let mut wake = self.pending.as_ref().map(|p| p.since + PENDING_TTL);
        if matches!(self.account, Account::Unlocked(_)) {
            let idle = self
                .settings
                .idle_lock_minutes
                .and_then(|m| self.idle.deadline(m));
            let screen = self.settings.lock_on_screen_lock.then_some(self.next_screen_poll);
            for t in [idle, screen].into_iter().flatten() {
                wake = Some(wake.map_or(t, |w| w.min(t)));
            }
        }
        wake
    }

    /// Forget a stale pending login; lock when idle or the screen locked.
    fn tick(&mut self) {
        let now = Instant::now();
        if self.pending.as_ref().is_some_and(|p| now >= p.since + PENDING_TTL) {
            info!("vault: the pending login's second step timed out");
            self.pending = None;
        }
        if !matches!(self.account, Account::Unlocked(_)) {
            return;
        }
        if let Some(m) = self.settings.idle_lock_minutes {
            if self.idle.deadline(m).is_some_and(|d| now >= d) {
                self.lock("idle");
                return;
            }
        }
        if self.settings.lock_on_screen_lock && now >= self.next_screen_poll {
            self.next_screen_poll = now + SCREEN_POLL;
            if crate::screen_lock::locked() {
                self.lock("the screen locked");
            }
        }
    }

    /// Drop the keys; keep the session (signed in, locked).
    fn lock(&mut self, why: &str) {
        let account = std::mem::replace(&mut self.account, Account::SignedOut);
        self.account = match account {
            Account::Unlocked(v) => {
                info!(%why, "vault: locked");
                Account::Locked(v.into_session())
            }
            other => other,
        };
        self.tickets.clear();
    }

    /// Bitwarden refused the session's refresh token: the account must
    /// sign in again.
    fn end_refused_session(&mut self) {
        let ended = matches!(&self.account, Account::Unlocked(v) if v.take_session_ended());
        if ended {
            info!("vault: Bitwarden ended the session; signed out");
            self.sign_out();
        }
    }

    fn sign_out(&mut self) {
        if let Some(email) = match &self.account {
            Account::Locked(s) => Some(s.email.clone()),
            Account::Unlocked(v) => Some(v.session().email.clone()),
            Account::SignedOut => None,
        } {
            self.prefs.remember_email(&self.dir, &email);
        }
        session::clear(&self.dir);
        self.account = Account::SignedOut;
        self.pending = None;
        self.tickets.clear();
        self.sync = SyncState::default();
    }

    /// Sync now, tell what changed, and recount.
    async fn sync_now(&mut self) -> Result<()> {
        self.sync_with(Vec::new()).await
    }

    /// [`Self::sync_now`], counting `ids` as changed besides (a save).
    async fn sync_with(&mut self, mut ids: Vec<String>) -> Result<()> {
        if self.svc().is_err() {
            return self.svc().map(|_| ());
        }
        self.sync.syncing = true;
        self.publish();
        let result = self.svc()?.sync().await;
        self.sync.syncing = false;
        match result {
            Ok(changes) => {
                self.sync.at = Some(crate::vault::read::unix_now());
                self.sync.error = None;
                self.recount().await;
                ids.extend(changes.ids);
                ids.sort();
                ids.dedup();
                if !ids.is_empty() {
                    self.event(Event::ItemsChanged { ids });
                }
                if changes.folders {
                    self.event(Event::Folders);
                }
                Ok(())
            }
            Err(e) => {
                self.sync.error = Some(e.message.clone());
                if !ids.is_empty() {
                    self.event(Event::ItemsChanged { ids });
                }
                Err(e)
            }
        }
    }

    async fn recount(&mut self) {
        let Ok(svc) = self.svc() else { return };
        if let Ok(all) = svc.all().await {
            self.sync.items = all
                .successes
                .iter()
                .filter(|c| c.deleted_date.is_none() && c.archived_date.is_none())
                .count() as u32;
            self.sync.unreadable = all.failures.len() as u32;
        }
    }

    async fn request(&mut self, req: Request, peer: &Peer) -> Result<Reply> {
        match req {
            Request::Hello { .. } | Request::Status | Request::Subscribe => {
                // Answered on the connection.
                Err(err(ErrorCode::Internal, "not for the vault's owner"))
            }
            Request::Settings => Ok(Reply::Settings(self.settings.clone())),
            Request::SetSettings { settings } => {
                crate::settings::save(&self.dir, &settings)
                    .map_err(|e| err(ErrorCode::Internal, format!("Settings not saved ({e}).")))?;
                info!(?settings, "vault: settings changed");
                self.settings = settings;
                Ok(Reply::Done)
            }
            req => self.account_request(req, peer).await,
        }
    }
}

/// Run the actor until every sender is gone.
pub async fn run(mut vault: Vault, mut rx: mpsc::UnboundedReceiver<Input>) {
    loop {
        let wake = vault.next_wake();
        let sleep = async {
            match wake {
                Some(t) => tokio::time::sleep_until(t.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            input = rx.recv() => match input {
                Some(input) => vault.handle(input).await,
                None => break,
            },
            () = sleep => vault.tick(),
        }
        vault.publish();
    }
    info!("vault: owner stopped");
}
