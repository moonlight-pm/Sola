//! One authenticated IMAP session. Every operation reconnects and retries
//! once on a dead connection (or an imap-crate tag-mismatch panic).

use std::cell::Cell;
use std::collections::HashSet;
use std::net::{TcpStream, ToSocketAddrs};
use std::panic::AssertUnwindSafe;
use std::sync::Once;
use std::time::Duration;

use imap::types::Flag;
use sola_bus::topics::MailRule;
use tracing::{debug, warn};

use super::account::Account;
use super::boxes::ListedMailbox;
use super::dest_uid::{DEST_UID_SCAN, MailIdent, parse_copyuid_dest, quote_mailbox};
use super::parse::{BODY_ITEMS, SUMMARY_ITEMS, parse_body, parse_summary};
use super::rules::{imap_search, is_smart};
use super::types::{Folder, MailId, MessageBody, MessageSummary};

pub type TlsStream = rustls_connector::TlsStream<TcpStream>;
type ImapSession = imap::Session<TlsStream>;

/// Bounded so a hung server can't wedge a worker thread.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Search results per mailbox.
pub const SEARCH_CAP: usize = 200;
/// Largest sequence window one page pass fetches.
const MAX_WINDOW: u32 = 500;

/// Open a TLS stream to the account's IMAP host with bounded connect and
/// I/O timeouts.
pub fn open_tls(account: &Account) -> anyhow::Result<TlsStream> {
    let addr = format!("{}:{}", account.imap_host, account.imap_port);
    let tcp = connect_tcp(&addr)?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))?;
    let connector = rustls_connector::RustlsConnector::new_with_native_certs()
        .map_err(|e| anyhow::anyhow!("TLS setup failed: {e}"))?;
    connector
        .connect(&account.imap_host, tcp)
        .map_err(|e| anyhow::anyhow!("TLS handshake with {} failed: {e}", account.imap_host))
}

fn connect_tcp(addr: &str) -> anyhow::Result<TcpStream> {
    let addrs = addr
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("can't resolve {addr}: {e}"))?;
    let mut last = None;
    for sa in addrs {
        match TcpStream::connect_timeout(&sa, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e) => anyhow::anyhow!("can't reach {addr}: {e}"),
        None => anyhow::anyhow!("can't resolve {addr}: no addresses"),
    })
}

thread_local! {
    static QUIET_PANIC: Cell<bool> = const { Cell::new(false) };
}

/// Chain a panic hook that stays silent while an IMAP operation runs
/// under `catch_unwind` (the panic becomes an error and is logged as
/// one). Install once, after logging init, so every other panic still
/// reaches the log hook. Swapping the global hook per call raced across
/// session threads and could leave panics silenced for good.
pub fn install_panic_filter() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET_PANIC.with(Cell::get) {
                prev(info);
            }
        }));
    });
}

/// Server extensions this session can lean on.
#[derive(Debug, Clone, Copy, Default)]
struct Caps {
    uidplus: bool,
}

/// One page of a mailbox, newest first by UID.
pub struct Rows {
    pub rows: Vec<MessageSummary>,
    pub total: u32,
}

/// Per-rule smart mailbox counts plus how much INBOX they hide.
pub struct SmartCounts {
    pub folders: Vec<Folder>,
    pub inbox_total_deduction: u32,
    pub inbox_unread_deduction: u32,
}

pub struct ImapClient {
    session: ImapSession,
    account: Account,
    caps: Caps,
    selected: Option<String>,
}

impl ImapClient {
    /// Connect over TLS and log in.
    pub fn connect(account: &Account) -> anyhow::Result<Self> {
        let (session, caps) = new_session(account)?;
        debug!(host = account.imap_host.as_str(), "IMAP connected");
        Ok(Self {
            session,
            account: account.clone(),
            caps,
            selected: None,
        })
    }

    pub fn noop(&mut self) -> anyhow::Result<()> {
        self.with_reconnect(|s| Ok(s.session.noop()?))
    }

    /// CREATE a mailbox (Gmail: a user label).
    pub fn create_mailbox(&mut self, name: &str) -> anyhow::Result<()> {
        self.with_reconnect(|s| Ok(s.session.create(name)?))
    }

    /// LIST names + attributes. Used to map canonical boxes.
    pub fn list_mailbox_names(&mut self) -> anyhow::Result<Vec<ListedMailbox>> {
        use imap::types::NameAttribute;
        self.with_reconnect(|s| {
            let mailboxes = s.session.list(None, Some("*"))?;
            Ok(mailboxes
                .iter()
                .map(|mb| ListedMailbox {
                    name: mb.name().to_string(),
                    attrs: mb
                        .attributes()
                        .iter()
                        .map(|a| match a {
                            NameAttribute::NoSelect => "\\Noselect".to_string(),
                            NameAttribute::NoInferiors => "\\Noinferiors".to_string(),
                            NameAttribute::Marked => "\\Marked".to_string(),
                            NameAttribute::Unmarked => "\\Unmarked".to_string(),
                            NameAttribute::Custom(c) => c.to_string(),
                        })
                        .collect(),
                })
                .collect())
        })
    }

    /// `(unread, total)` via STATUS (no SELECT).
    pub fn status(&mut self, name: &str) -> anyhow::Result<(u32, u32)> {
        use imap::types::{StatusAttribute, UnsolicitedResponse};
        self.with_reconnect(|s| {
            // The imap crate routes STATUS to the unsolicited channel.
            s.session.status(name, "(MESSAGES UNSEEN)")?;
            let (mut unread, mut total) = (0, 0);
            while let Ok(resp) = s.session.unsolicited_responses.try_recv() {
                if let UnsolicitedResponse::Status {
                    mailbox,
                    attributes,
                } = resp
                {
                    if mailbox != name {
                        continue;
                    }
                    for attr in attributes {
                        match attr {
                            StatusAttribute::Messages(n) => total = n,
                            StatusAttribute::Unseen(n) => unread = n,
                            _ => {}
                        }
                    }
                }
            }
            Ok((unread, total))
        })
    }

    /// `limit` rows of `remote` older than UID `before` (newest when
    /// `None`), skipping `exclude`. Walks sequence windows down from the
    /// top — `SELECT` + `FETCH`, not `UID SEARCH`: this server's SEARCH
    /// replies (`ESEARCH` / empty-mailbox forms) make the imap crate fail
    /// to parse and then panic on the next tag.
    pub fn page(
        &mut self,
        remote: &str,
        canonical: &str,
        before: Option<u32>,
        limit: u32,
        exclude: &HashSet<u32>,
    ) -> anyhow::Result<Rows> {
        let id = self.account.id();
        self.with_reconnect(|s| {
            let exists = s.select_folder(remote)?;
            let total = exists.saturating_sub(exclude.len() as u32);
            let mut end = match before {
                None => exists,
                Some(uid) => s.seq_before(uid, exists)?,
            };
            let mut rows: Vec<MessageSummary> = Vec::new();
            let mut window = limit;
            while end > 0 && rows.len() < limit as usize && limit > 0 {
                let start = end.saturating_sub(window.saturating_sub(1)).max(1);
                let fetches = s.session.fetch(format!("{start}:{end}"), SUMMARY_ITEMS)?;
                rows.extend(
                    fetches
                        .iter()
                        .filter(|f| f.uid.is_some_and(|u| !exclude.contains(&u)))
                        .filter_map(|f| parse_summary(f, &id, canonical)),
                );
                end = start - 1;
                window = (window * 2).min(MAX_WINDOW);
            }
            rows.sort_unstable_by_key(|m| std::cmp::Reverse(m.id.uid));
            rows.truncate(limit as usize);
            Ok(Rows { rows, total })
        })
    }

    /// INBOX UIDs matched by any smart mailbox rule.
    pub fn smart_uids(&mut self, rules: &[MailRule]) -> anyhow::Result<HashSet<u32>> {
        self.with_reconnect(|s| {
            s.ensure_selected("INBOX")?;
            let mut union = HashSet::new();
            for rule in rules.iter().filter(|r| is_smart(r)) {
                let query = imap_search(&rule.conditions);
                if query.is_empty() {
                    continue;
                }
                match s.session.uid_search(&query) {
                    Ok(uids) => union.extend(uids),
                    Err(e) => warn!(rule = rule.name.as_str(), "smart SEARCH failed: {e}"),
                }
            }
            Ok(union)
        })
    }

    /// One smart mailbox (INBOX rows matching `rule`), older than `before`.
    pub fn smart_page(
        &mut self,
        rule: &MailRule,
        before: Option<u32>,
        limit: u32,
    ) -> anyhow::Result<Rows> {
        let query = imap_search(&rule.conditions);
        if query.is_empty() {
            return Ok(Rows {
                rows: Vec::new(),
                total: 0,
            });
        }
        self.with_reconnect(|s| {
            s.ensure_selected("INBOX")?;
            let mut uids: Vec<u32> = s.session.uid_search(&query)?.into_iter().collect();
            uids.sort_unstable_by(|a, b| b.cmp(a));
            let total = uids.len() as u32;
            let page: Vec<u32> = uids
                .into_iter()
                .filter(|u| before.is_none_or(|b| *u < b))
                .take(limit as usize)
                .collect();
            let rows = s.fetch_uids(&page, "INBOX")?;
            Ok(Rows { rows, total })
        })
    }

    /// Per-rule counts and the INBOX deduction they imply.
    pub fn smart_counts(&mut self, rules: &[MailRule]) -> anyhow::Result<SmartCounts> {
        let smart: Vec<&MailRule> = rules.iter().filter(|r| is_smart(r)).collect();
        let mut out = SmartCounts {
            folders: Vec::new(),
            inbox_total_deduction: 0,
            inbox_unread_deduction: 0,
        };
        if smart.is_empty() {
            return Ok(out);
        }
        self.with_reconnect(|s| {
            s.ensure_selected("INBOX")?;
            let mut all: HashSet<u32> = HashSet::new();
            let mut unseen_all: HashSet<u32> = HashSet::new();
            out.folders.clear();
            for rule in &smart {
                let query = imap_search(&rule.conditions);
                let (uids, unseen) = if query.is_empty() {
                    (HashSet::new(), HashSet::new())
                } else {
                    let uids = s.session.uid_search(&query).unwrap_or_else(|e| {
                        warn!(rule = rule.name.as_str(), "smart SEARCH failed: {e}");
                        HashSet::new()
                    });
                    let unseen = s
                        .session
                        .uid_search(format!("UNSEEN {query}"))
                        .unwrap_or_else(|e| {
                            warn!(rule = rule.name.as_str(), "smart SEARCH UNSEEN failed: {e}");
                            HashSet::new()
                        });
                    (uids, unseen)
                };
                out.folders.push(Folder {
                    name: rule.name.clone(),
                    total: uids.len() as u32,
                    unread: unseen.len() as u32,
                });
                all.extend(uids);
                unseen_all.extend(unseen);
            }
            out.inbox_total_deduction = all.len() as u32;
            out.inbox_unread_deduction = unseen_all.len() as u32;
            Ok(())
        })?;
        Ok(out)
    }

    /// Subject / From / To / body text matches in one mailbox, newest
    /// first, capped at [`SEARCH_CAP`].
    pub fn search(
        &mut self,
        remote: &str,
        canonical: &str,
        query: &str,
    ) -> anyhow::Result<Vec<MessageSummary>> {
        let safe = query.replace('\\', "\\\\").replace('"', "\\\"");
        // TEXT covers headers and body (RFC 3501).
        let criteria = format!(
            "UNDELETED OR OR OR (SUBJECT \"{safe}\") (FROM \"{safe}\") (TO \"{safe}\") (TEXT \"{safe}\")"
        );
        self.with_reconnect(|s| {
            s.ensure_selected(remote)?;
            let uids = match s.session.uid_search(&criteria) {
                Ok(uids) => uids,
                Err(e) => {
                    // Some servers send unparseable replies for empty results.
                    warn!(remote, "IMAP SEARCH failed (treating as empty): {e}");
                    return Ok(Vec::new());
                }
            };
            let mut uids: Vec<u32> = uids.into_iter().collect();
            uids.sort_unstable_by(|a, b| b.cmp(a));
            uids.truncate(SEARCH_CAP);
            s.fetch_uids(&uids, canonical)
        })
    }

    /// The full message for `id`, which lives in `remote`.
    pub fn fetch_body(&mut self, remote: &str, id: &MailId) -> anyhow::Result<MessageBody> {
        self.with_reconnect(|s| {
            s.ensure_selected(remote)?;
            let fetches = s.session.uid_fetch(id.uid.to_string(), BODY_ITEMS)?;
            let fetch = fetches
                .iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("the message is no longer on the server"))?;
            parse_body(fetch, id.clone())
        })
    }

    /// Add or clear one system flag (`\Seen`, `\Flagged`).
    pub fn set_flag(&mut self, remote: &str, uid: u32, flag: &str, on: bool) -> anyhow::Result<()> {
        let op = if on { "+FLAGS" } else { "-FLAGS" };
        self.with_reconnect(|s| {
            s.ensure_selected(remote)?;
            s.session
                .uid_store(uid.to_string(), format!("{op} ({flag})"))?;
            Ok(())
        })
    }

    /// Move a message to another mailbox. Returns its UID in `dest`.
    ///
    /// IMAP UIDs are per-mailbox. Undo must use this destination UID, never
    /// the source UID against the dest folder (Trash often already has a
    /// different message with that number).
    ///
    /// Prefers `UID MOVE` (COPYUID). Falls back to COPY + `\Deleted` +
    /// `UID EXPUNGE` (plain EXPUNGE only without UIDPLUS — it would purge
    /// every `\Deleted` message in the folder). If COPYUID is missing,
    /// scans recent dest envelopes for the Message-ID peeked before.
    pub fn move_message(
        &mut self,
        remote: &str,
        uid: u32,
        dest: &str,
    ) -> anyhow::Result<Option<u32>> {
        self.with_reconnect(|s| {
            s.ensure_selected(remote)?;
            let ident = s.peek_identity(uid)?;
            let dest_q = quote_mailbox(dest);
            let response = match s
                .session
                .run_command_and_read_response(format!("UID MOVE {uid} {dest_q}"))
            {
                Ok(r) => r,
                Err(e @ (imap::Error::Bad(_) | imap::Error::No(_))) => {
                    debug!("UID MOVE unavailable ({e}); COPY + expunge");
                    let r = s
                        .session
                        .run_command_and_read_response(format!("UID COPY {uid} {dest_q}"))?;
                    s.expunge_uid(uid)?;
                    r
                }
                Err(e) => return Err(e.into()),
            };
            let mut dest_uid = parse_copyuid_dest(&response);
            if dest_uid.is_none() {
                dest_uid = s.find_copied_uid(dest, &ident)?;
            }
            if dest_uid.is_none() {
                warn!(
                    uid,
                    dest, "move succeeded but destination UID was not identified"
                );
            }
            Ok(dest_uid)
        })
    }

    /// Permanently remove one message (a replaced draft). Without UIDPLUS
    /// the message stays flagged `\Deleted` (hidden from lists) rather
    /// than expunging other clients' deletes.
    pub fn delete_message(&mut self, remote: &str, uid: u32) -> anyhow::Result<()> {
        self.with_reconnect(|s| {
            s.ensure_selected(remote)?;
            if s.caps.uidplus {
                s.expunge_uid(uid)
            } else {
                s.session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)")?;
                Ok(())
            }
        })
    }

    /// Permanently delete everything in a folder (flag `\Deleted` + EXPUNGE).
    /// Refuses the folders that must never be emptied.
    pub fn empty_folder(&mut self, remote: &str) -> anyhow::Result<()> {
        const PROTECTED: &[&str] = &["INBOX", "Sent", "Archive", "Drafts"];
        if PROTECTED.iter().any(|f| f.eq_ignore_ascii_case(remote)) {
            anyhow::bail!("refusing to empty {remote}");
        }
        let result = self.with_reconnect(|s| {
            // Batch so a large Trash does not sit on one STORE/EXPUNGE
            // past the socket timeout (that surfaces as EAGAIN / os error 11).
            const BATCH: u32 = 200;
            loop {
                let mailbox = s.session.select(remote)?;
                if mailbox.exists == 0 {
                    break;
                }
                let end = mailbox.exists.min(BATCH);
                s.session.store(format!("1:{end}"), "+FLAGS (\\Deleted)")?;
                s.session.expunge()?;
                while s.session.unsolicited_responses.try_recv().is_ok() {}
            }
            s.selected = None;
            Ok(())
        });
        // Fresh session so the next list/select starts at tag a1.
        if result.is_ok()
            && let Err(e) = self.reconnect()
        {
            warn!(remote, "IMAP reconnect after empty failed: {e}");
        }
        result
    }

    /// Append a raw message with `flags`.
    pub fn append(
        &mut self,
        remote: &str,
        message: &[u8],
        flags: &[Flag<'_>],
    ) -> anyhow::Result<()> {
        self.with_reconnect(|s| Ok(s.session.append_with_flags(remote, message, flags)?))
    }

    fn fetch_uids(&mut self, uids: &[u32], canonical: &str) -> anyhow::Result<Vec<MessageSummary>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let id = self.account.id();
        let fetches = self.session.uid_fetch(&set, SUMMARY_ITEMS)?;
        let mut rows: Vec<MessageSummary> = fetches
            .iter()
            .filter_map(|f| parse_summary(f, &id, canonical))
            .collect();
        rows.sort_unstable_by_key(|m| std::cmp::Reverse(m.id.uid));
        Ok(rows)
    }

    /// Highest sequence number whose UID is below `uid` (the selected
    /// mailbox). `UID FETCH n:*` returns the last message even when `n`
    /// is above every UID, so filter on the UIDs that came back.
    fn seq_before(&mut self, uid: u32, exists: u32) -> anyhow::Result<u32> {
        let fetches = self.session.uid_fetch(format!("{uid}:*"), "(UID)")?;
        let first = fetches
            .iter()
            .filter(|f| f.uid.is_some_and(|u| u >= uid))
            .map(|f| f.message)
            .min();
        Ok(first.map_or(exists, |seq| seq.saturating_sub(1)))
    }

    fn expunge_uid(&mut self, uid: u32) -> anyhow::Result<()> {
        self.session
            .uid_store(uid.to_string(), "+FLAGS (\\Deleted)")?;
        if self.caps.uidplus {
            self.session.uid_expunge(uid.to_string())?;
        } else {
            warn!(uid, "server lacks UIDPLUS; plain EXPUNGE");
            self.session.expunge()?;
        }
        Ok(())
    }

    fn peek_identity(&mut self, uid: u32) -> anyhow::Result<MailIdent> {
        let fetches = self.session.uid_fetch(uid.to_string(), "(UID ENVELOPE)")?;
        let fetch = fetches
            .iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("message UID {uid} not found"))?;
        MailIdent::from_fetch(fetch).ok_or_else(|| anyhow::anyhow!("no envelope for UID {uid}"))
    }

    fn find_copied_uid(&mut self, dest: &str, ident: &MailIdent) -> anyhow::Result<Option<u32>> {
        let exists = self.select_folder(dest)?;
        if exists == 0 {
            return Ok(None);
        }
        let start = exists.saturating_sub(DEST_UID_SCAN - 1).max(1);
        let fetches = self
            .session
            .fetch(format!("{start}:{exists}"), "(UID ENVELOPE)")?;
        Ok(fetches
            .iter()
            .filter(|f| MailIdent::from_fetch(f).is_some_and(|other| ident.matches(&other)))
            .filter_map(|f| f.uid)
            .max())
    }

    fn ensure_selected(&mut self, folder: &str) -> anyhow::Result<()> {
        if self.selected.as_deref() != Some(folder) {
            self.select_folder(folder)?;
        }
        Ok(())
    }

    /// Always `SELECT` so `EXISTS` is current. Returns the exists count.
    fn select_folder(&mut self, folder: &str) -> anyhow::Result<u32> {
        let mailbox = self.session.select(folder)?;
        while self.session.unsolicited_responses.try_recv().is_ok() {}
        self.selected = Some(folder.to_string());
        Ok(mailbox.exists)
    }

    fn reconnect(&mut self) -> anyhow::Result<()> {
        debug!(host = self.account.imap_host.as_str(), "IMAP reconnecting");
        let (session, caps) = new_session(&self.account)?;
        self.session = session;
        self.caps = caps;
        self.selected = None;
        Ok(())
    }

    /// Run an operation, reconnecting and retrying once on connection
    /// errors. Panics (imap crate tag assertions) count as connection
    /// errors: they leave the session corrupt.
    fn with_reconnect<T>(
        &mut self,
        mut op: impl FnMut(&mut Self) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        match run_caught(|| op(self)) {
            Err(e) if is_connection_error(&e) => {
                warn!(
                    host = self.account.imap_host.as_str(),
                    "IMAP error, reconnecting once: {e}"
                );
                self.reconnect()?;
                run_caught(|| op(self))
            }
            other => other,
        }
    }
}

fn new_session(account: &Account) -> anyhow::Result<(ImapSession, Caps)> {
    let tls = open_tls(account)?;
    let mut session = imap::Client::new(tls)
        .login(&account.username, &account.password)
        .map_err(|(e, _)| anyhow::anyhow!("login failed: {e}"))?;
    let caps = match session.capabilities() {
        Ok(c) => Caps {
            uidplus: c.has_str("UIDPLUS"),
        },
        Err(e) => {
            warn!(host = account.imap_host.as_str(), "CAPABILITY failed: {e}");
            Caps::default()
        }
    };
    Ok((session, caps))
}

fn run_caught<T>(op: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    QUIET_PANIC.with(|q| q.set(true));
    let result = std::panic::catch_unwind(AssertUnwindSafe(op));
    QUIET_PANIC.with(|q| q.set(false));
    result.unwrap_or_else(|panic| {
        let msg = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_else(|| "session desynced (tag mismatch)".into());
        Err(anyhow::anyhow!("imap panic: {msg}"))
    })
}

/// Worth one reconnect + retry.
fn is_connection_error(err: &anyhow::Error) -> bool {
    if let Some(imap_err) = err.downcast_ref::<imap::Error>() {
        return matches!(imap_err, imap::Error::ConnectionLost | imap::Error::Io(_));
    }
    let msg = err.to_string().to_lowercase();
    [
        "imap panic",
        "broken pipe",
        "connection reset",
        "connection closed",
        "unexpected eof",
        "timed out",
        "not connected",
        "temporarily unavailable",
        "os error 11",
        "would block",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}
