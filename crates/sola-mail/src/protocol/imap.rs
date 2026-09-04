use std::collections::HashSet;

use imap::types::Fetch;
use tracing::{debug, warn};

use super::account::Account;
use super::attachments::collect_attachments;
use super::types::{Folder, MessageBody, MessageSummary};
use sola_bus::topics::{MailRule, MailRuleCondition};

type ImapSession = imap::Session<rustls_connector::TlsStream<std::net::TcpStream>>;

/// Envelope + BODYSTRUCTURE (paperclip) + forwarded-for. No SEARCH.
const SUMMARY_ITEMS: &str =
    "(UID FLAGS ENVELOPE BODYSTRUCTURE BODY.PEEK[HEADER.FIELDS (X-Forwarded-For)])";

pub struct ImapClient {
    session: ImapSession,
    config: Account,
    selected_folder: Option<String>,
}

impl ImapClient {
    /// Connect to the IMAP server over TLS and login.
    pub fn connect(config: &Account) -> anyhow::Result<Self> {
        let session = new_session(config)?;
        debug!("IMAP connected to {}", config.imap_host);

        Ok(Self {
            session,
            config: config.clone(),
            selected_folder: None,
        })
    }

    /// Send an IMAP NOOP to keep the connection alive.
    ///
    /// Uses `with_reconnect` so a dead connection is transparently replaced.
    pub fn noop(&mut self) -> anyhow::Result<()> {
        self.with_reconnect(|s| {
            s.session.noop()?;
            Ok(())
        })
    }

    /// CREATE a mailbox (Gmail: a user label).
    pub fn create_mailbox(&mut self, name: &str) -> anyhow::Result<()> {
        let name = name.to_string();
        self.with_reconnect(move |s| {
            s.session.create(&name)?;
            Ok(())
        })
    }

    /// LIST names + attributes (no STATUS). Used to map canonical boxes.
    pub fn list_mailbox_names(
        &mut self,
    ) -> anyhow::Result<Vec<crate::protocol::boxes::ListedMailbox>> {
        self.with_reconnect(|s| {
            let mailboxes = s.session.list(None, Some("*"))?;
            let mut out = Vec::new();
            for mb in mailboxes.iter() {
                let attrs = mb
                    .attributes()
                    .iter()
                    .map(|a| match a {
                        imap::types::NameAttribute::NoSelect => "\\Noselect".to_string(),
                        imap::types::NameAttribute::NoInferiors => "\\Noinferiors".to_string(),
                        imap::types::NameAttribute::Marked => "\\Marked".to_string(),
                        imap::types::NameAttribute::Unmarked => "\\Unmarked".to_string(),
                        imap::types::NameAttribute::Custom(c) => c.to_string(),
                    })
                    .collect();
                out.push(crate::protocol::boxes::ListedMailbox {
                    name: mb.name().to_string(),
                    attrs,
                });
            }
            Ok(out)
        })
    }

    /// Unread/total for one mailbox via STATUS (no SELECT).
    pub fn folder_status(&mut self, name: &str) -> anyhow::Result<(u32, u32)> {
        let name = name.to_string();
        self.with_reconnect(move |s| {
            s.session.status(&name, "(MESSAGES UNSEEN)")?;
            let mut total = 0u32;
            let mut unread = 0u32;
            while let Ok(resp) = s.session.unsolicited_responses.try_recv() {
                if let imap::types::UnsolicitedResponse::Status {
                    mailbox: ref mb,
                    attributes,
                } = resp
                {
                    if mb != &name {
                        continue;
                    }
                    for attr in attributes {
                        match attr {
                            imap::types::StatusAttribute::Messages(n) => total = n,
                            imap::types::StatusAttribute::Unseen(n) => unread = n,
                            _ => {}
                        }
                    }
                }
            }
            Ok((unread, total))
        })
    }

    /// List all folders with unread/total counts via `STATUS` (no SELECT).
    ///
    /// Uses STATUS rather than SELECT+SEARCH per folder: some servers/crates
    /// leave the session desynced after repeated SEARCH failures, and the
    /// imap client panics on tag mismatch (killing the mail worker).
    /// Message lists still exclude `\Deleted` via `UNDELETED` SEARCH.
    pub fn list_folders(&mut self) -> anyhow::Result<Vec<Folder>> {
        self.with_reconnect(|s| {
            let mailboxes = s.session.list(None, Some("*"))?;
            let mut folders = Vec::new();

            for mb in mailboxes.iter() {
                let name = mb.name().to_string();
                // Skip \Noselect mailboxes (can't STATUS).
                if mb
                    .attributes()
                    .iter()
                    .any(|a| matches!(a, imap::types::NameAttribute::NoSelect))
                {
                    continue;
                }
                match s.session.status(&name, "(MESSAGES UNSEEN)") {
                    Ok(_) => {
                        // The imap crate routes STATUS responses to the
                        // unsolicited channel, so drain it for the attributes.
                        let mut total = 0u32;
                        let mut unread = 0u32;
                        while let Ok(resp) = s.session.unsolicited_responses.try_recv() {
                            if let imap::types::UnsolicitedResponse::Status {
                                mailbox: ref mb,
                                attributes,
                            } = resp
                            {
                                if mb != &name {
                                    continue;
                                }
                                for attr in attributes {
                                    match attr {
                                        imap::types::StatusAttribute::Messages(n) => total = n,
                                        imap::types::StatusAttribute::Unseen(n) => unread = n,
                                        _ => {}
                                    }
                                }
                            }
                        }
                        folders.push(Folder {
                            name,
                            unread,
                            total,
                        });
                    }
                    Err(e) if is_imap_connection_error(&e) => {
                        return Err(anyhow::Error::from(e));
                    }
                    Err(e) => {
                        warn!("STATUS failed for {name}: {e}");
                        folders.push(Folder {
                            name,
                            unread: 0,
                            total: 0,
                        });
                    }
                }
            }

            super::types::sort_folders(&mut folders);
            Ok(folders)
        })
    }

    /// Select a folder and fetch a page of message summaries (most recent first).
    ///
    /// Uses `SELECT` + sequence `FETCH`, not `UID SEARCH`. This server's
    /// SEARCH replies (`ESEARCH` / empty-mailbox forms) make the rust imap
    /// crate return `Unable to parse status response` and then panic on the
    /// next command's tag (`a3` vs `a4`). `\Deleted` is dropped in
    /// [`parse_summary`].
    pub fn list_messages(
        &mut self,
        folder: &str,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
        let folder = folder.to_string();
        self.with_reconnect(move |s| {
            let exists = s.select_folder(&folder)?;
            fetch_page_by_seq(&mut s.session, exists, offset, limit)
        })
    }

    /// List INBOX messages, excluding any UID matched by a smart_mailbox rule.
    pub fn list_inbox_filtered(
        &mut self,
        rules: &[MailRule],
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
        let rules = rules.to_vec();
        self.with_reconnect(move |s| {
            let exists = s.select_folder("INBOX")?;
            if exists == 0 {
                return Ok((Vec::new(), 0));
            }
            let fetches = s.session.fetch(format!("1:{exists}"), "(UID FLAGS)")?;
            let excluded = smart_mailbox_uids(&mut s.session, &rules);
            let mut kept: Vec<u32> = fetches
                .iter()
                .filter(|f| {
                    !f.flags()
                        .iter()
                        .any(|flag| matches!(flag, imap::types::Flag::Deleted))
                })
                .filter_map(|f| f.uid)
                .filter(|uid| !excluded.contains(uid))
                .collect();
            kept.sort_unstable_by(|a, b| b.cmp(a));
            let total = kept.len() as u32;
            fetch_envelopes(&mut s.session, &kept, offset, limit, total)
        })
    }

    /// List INBOX messages matching a single smart_mailbox rule (by name).
    pub fn list_smart_mailbox(
        &mut self,
        rule_name: &str,
        rules: &[MailRule],
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
        let Some(rule) = rules
            .iter()
            .find(|r| r.action == "smart_mailbox" && r.name == rule_name)
            .cloned()
        else {
            return Ok((Vec::new(), 0));
        };
        let query = build_imap_search(&rule.conditions);
        if query.is_empty() {
            return Ok((Vec::new(), 0));
        }
        self.with_reconnect(move |s| {
            s.ensure_selected("INBOX")?;
            let mut uids: Vec<u32> = s.session.uid_search(&query)?.into_iter().collect();
            uids.sort_unstable_by(|a, b| b.cmp(a));
            let total = uids.len() as u32;
            fetch_envelopes(&mut s.session, &uids, offset, limit, total)
        })
    }

    /// Search messages in a folder by subject, from, or body text.
    /// Search across multiple folders (INBOX, Sent, Archive) matching
    /// from, to, subject, and body content. Returns deduplicated results.
    pub fn search_all_folders(
        &mut self,
        query: &str,
    ) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
        let folders = ["INBOX", "Sent", "Archive"];
        let mut all_messages: Vec<MessageSummary> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for folder in &folders {
            match self.search_messages(folder, query) {
                Ok((messages, _)) => {
                    for msg in messages {
                        if seen_ids.insert(msg.uid) {
                            all_messages.push(msg);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Search in {folder} failed: {e}");
                }
            }
        }

        // Sort by date descending, cap at 200
        all_messages.sort_by(|a, b| b.date.cmp(&a.date));
        let total = all_messages.len() as u32;
        all_messages.truncate(200);
        Ok((all_messages, total))
    }

    pub fn search_messages(
        &mut self,
        folder: &str,
        query: &str,
    ) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
        let folder = folder.to_string();
        let query = query.to_string();
        self.with_reconnect(move |s| {
            s.ensure_selected(&folder)?;

            // Escape IMAP quoted-string special characters
            let safe = query.replace('\\', "\\\\").replace('"', "\\\"");
            // IMAP SEARCH: OR across subject, from, to, and body (TEXT),
            // restricted to non-deleted messages.
            // TEXT searches both headers and body content per RFC 3501
            let imap_query = format!(
                "UNDELETED OR OR OR (SUBJECT \"{safe}\") (FROM \"{safe}\") (TO \"{safe}\") (TEXT \"{safe}\")"
            );
            let uids = match s.session.uid_search(&imap_query) {
                Ok(uids) => uids,
                Err(e) => {
                    // Some IMAP servers send unparseable responses for
                    // empty/unusual search results — treat as no matches.
                    warn!("IMAP SEARCH failed (treating as empty): {e}");
                    return Ok((Vec::new(), 0));
                }
            };
            let total = uids.len() as u32;

            if uids.is_empty() {
                return Ok((Vec::new(), 0));
            }

            // Sort descending (most recent first), cap at 200
            let mut uid_vec: Vec<u32> = uids.into_iter().collect();
            uid_vec.sort_unstable_by(|a, b| b.cmp(a));
            uid_vec.truncate(200);

            // Build UID fetch string: "uid1,uid2,uid3"
            let uid_str = uid_vec
                .iter()
                .map(|u| u.to_string())
                .collect::<Vec<_>>()
                .join(",");

            let fetches = s.session.uid_fetch(&uid_str, SUMMARY_ITEMS)?;

            let mut messages: Vec<MessageSummary> =
                fetches.iter().filter_map(parse_summary).collect();

            // Re-sort by UID descending (fetch order isn't guaranteed)
            messages.sort_unstable_by(|a, b| b.uid.cmp(&a.uid));

            Ok((messages, total))
        })
    }

    /// Fetch the full body of a message by UID.
    pub fn fetch_body(&mut self, folder: &str, uid: u32) -> anyhow::Result<MessageBody> {
        let folder = folder.to_string();
        self.with_reconnect(move |s| {
            s.ensure_selected(&folder)?;

            let fetches = s
                .session
                .uid_fetch(uid.to_string(), "(UID FLAGS ENVELOPE BODY[])")?;

            let fetch = fetches
                .iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Message UID {uid} not found"))?;

            parse_body(fetch, uid)
        })
    }

    /// Mark a message as read (\Seen flag).
    pub fn mark_read(&mut self, folder: &str, uid: u32) -> anyhow::Result<()> {
        let folder = folder.to_string();
        self.with_reconnect(move |s| {
            s.ensure_selected(&folder)?;
            s.session.uid_store(uid.to_string(), "+FLAGS (\\Seen)")?;
            Ok(())
        })
    }

    /// Move a message to another folder. Returns the UID in `dest`.
    ///
    /// IMAP UIDs are per-mailbox. Undo must use this destination UID, never
    /// the source UID against the dest folder (Trash often already has a
    /// different message with that number).
    ///
    /// Prefers `UID MOVE` (COPYUID). Falls back to COPY + DELETE + EXPUNGE.
    /// If COPYUID is missing (tagged OK is dropped by the imap crate), scans
    /// recent dest envelopes for the Message-ID peeked before the move.
    ///
    /// Note: COPY+EXPUNGE is not atomic. If the connection dies mid-operation
    /// (e.g. after COPY but before EXPUNGE), the retry may duplicate in dest.
    /// This is preferable to losing mail.
    pub fn move_message(
        &mut self,
        folder: &str,
        uid: u32,
        dest: &str,
    ) -> anyhow::Result<Option<u32>> {
        let folder = folder.to_string();
        let dest = dest.to_string();
        self.with_reconnect(move |s| {
            s.ensure_selected(&folder)?;
            let ident = peek_identity(&mut s.session, uid)?;
            let dest_q = quote_mailbox(&dest);
            let response = match s
                .session
                .run_command_and_read_response(&format!("UID MOVE {uid} {dest_q}"))
            {
                Ok(r) => r,
                Err(e) if matches!(e, imap::Error::Bad(_) | imap::Error::No(_)) => {
                    debug!("UID MOVE unavailable ({e}); COPY+EXPUNGE");
                    let r = s
                        .session
                        .run_command_and_read_response(&format!("UID COPY {uid} {dest_q}"))?;
                    s.session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)")?;
                    s.session.expunge()?;
                    r
                }
                Err(e) => return Err(e.into()),
            };
            let mut dest_uid = parse_copyuid_dest(&response);
            if dest_uid.is_none() {
                dest_uid = find_copied_uid(s, &dest, &ident)?;
            }
            if dest_uid.is_none() {
                warn!(
                    uid,
                    dest = dest.as_str(),
                    "move succeeded but destination UID was not identified"
                );
            }
            Ok(dest_uid)
        })
    }

    /// Permanently delete all messages in a folder (flag \Deleted + EXPUNGE).
    ///
    /// Refuses to empty protected folders (INBOX, Sent, Archive, Drafts).
    pub fn empty_folder(&mut self, folder: &str) -> anyhow::Result<()> {
        const PROTECTED: &[&str] = &["INBOX", "Sent", "Archive", "Drafts"];
        if PROTECTED.iter().any(|f| f.eq_ignore_ascii_case(folder)) {
            anyhow::bail!("Cannot empty protected folder: {folder}");
        }
        let folder = folder.to_string();
        let result = self.with_reconnect({
            let folder = folder.clone();
            move |s| {
                // Batch so a large Trash does not sit on one STORE/EXPUNGE
                // past the socket timeout (that surfaces as EAGAIN / os error 11).
                const BATCH: u32 = 200;
                loop {
                    let mailbox = s.session.select(&folder)?;
                    if mailbox.exists == 0 {
                        break;
                    }
                    let end = mailbox.exists.min(BATCH);
                    s.session.store(format!("1:{end}"), "+FLAGS (\\Deleted)")?;
                    s.session.expunge()?;
                    while s.session.unsolicited_responses.try_recv().is_ok() {}
                }
                s.selected_folder = None;
                Ok(())
            }
        });
        // Fresh session so the next list/select starts at tag a1.
        if result.is_ok() {
            if let Err(e) = self.reconnect() {
                warn!("IMAP reconnect after empty {folder} failed: {e}");
            }
        }
        result
    }

    /// Append a raw message to a folder with the \Seen flag.
    pub fn append_to_sent(&mut self, folder: &str, message: &[u8]) -> anyhow::Result<()> {
        let folder = folder.to_string();
        let message = message.to_vec();
        self.with_reconnect(move |s| {
            s.session
                .append_with_flags(&folder, &message, &[imap::types::Flag::Seen])?;
            Ok(())
        })
    }

    /// Count messages matching each smart mailbox rule using IMAP SEARCH.
    ///
    /// Returns per-rule counts plus the total unique INBOX messages/unseen
    /// that match any smart mailbox rule (for adjusting INBOX display counts).
    pub fn count_smart_mailboxes(
        &mut self,
        rules: &[MailRule],
    ) -> anyhow::Result<SmartMailboxCounts> {
        let smart_rules: Vec<&MailRule> = rules
            .iter()
            .filter(|r| r.action == "smart_mailbox")
            .collect();

        if smart_rules.is_empty() {
            return Ok(SmartMailboxCounts {
                folders: Vec::new(),
                inbox_total_deduction: 0,
                inbox_unread_deduction: 0,
            });
        }

        self.with_reconnect(|s| {
            s.ensure_selected("INBOX")?;

            let mut folders = Vec::new();
            let mut all_uids: HashSet<u32> = HashSet::new();
            let mut all_unseen_uids: HashSet<u32> = HashSet::new();

            for rule in &smart_rules {
                let query = build_imap_search(&rule.conditions);
                if query.is_empty() {
                    folders.push(Folder {
                        name: rule.name.clone(),
                        total: 0,
                        unread: 0,
                    });
                    continue;
                }

                let uids = match s.session.uid_search(&query) {
                    Ok(uids) => uids,
                    Err(e) => {
                        warn!("SEARCH failed for smart mailbox {}: {e}", rule.name);
                        folders.push(Folder {
                            name: rule.name.clone(),
                            total: 0,
                            unread: 0,
                        });
                        continue;
                    }
                };
                let total = uids.len() as u32;
                all_uids.extend(&uids);

                let unseen_query = format!("UNSEEN {query}");
                let unseen_uids = match s.session.uid_search(&unseen_query) {
                    Ok(uids) => uids,
                    Err(e) => {
                        warn!("SEARCH UNSEEN failed for smart mailbox {}: {e}", rule.name);
                        HashSet::new()
                    }
                };
                let unread = unseen_uids.len() as u32;
                all_unseen_uids.extend(&unseen_uids);

                folders.push(Folder {
                    name: rule.name.clone(),
                    total,
                    unread,
                });
            }

            Ok(SmartMailboxCounts {
                folders,
                inbox_total_deduction: all_uids.len() as u32,
                inbox_unread_deduction: all_unseen_uids.len() as u32,
            })
        })
    }

    /// Ensure the given folder is selected, re-selecting if needed.
    fn ensure_selected(&mut self, folder: &str) -> anyhow::Result<()> {
        if self.selected_folder.as_deref() != Some(folder) {
            self.select_folder(folder)?;
        }
        Ok(())
    }

    /// Always `SELECT` so `EXISTS` is current. Returns the exists count.
    fn select_folder(&mut self, folder: &str) -> anyhow::Result<u32> {
        let mailbox = self.session.select(folder)?;
        while self.session.unsolicited_responses.try_recv().is_ok() {}
        self.selected_folder = Some(folder.to_string());
        Ok(mailbox.exists)
    }

    /// Reconnect to the IMAP server, replacing the dead session.
    fn reconnect(&mut self) -> anyhow::Result<()> {
        debug!("IMAP reconnecting to {}", self.config.imap_host);
        self.session = new_session(&self.config)?;
        self.selected_folder = None;
        debug!("IMAP reconnected");
        Ok(())
    }

    /// Run an operation, reconnecting and retrying once on connection errors.
    ///
    /// Also catches panics (e.g. imap crate tag assertion failures) and treats
    /// them as connection errors, since a panic leaves the session in a corrupt state.
    fn with_reconnect<T, F>(&mut self, op: F) -> anyhow::Result<T>
    where
        F: Fn(&mut Self) -> anyhow::Result<T>,
    {
        match Self::run_or_catch_panic(&op, self) {
            Ok(val) => Ok(val),
            Err(e) if is_connection_error(&e) || e.to_string().contains("imap panic") => {
                warn!("IMAP error, reconnecting once: {e}");
                self.reconnect()?;
                // Retry under catch_unwind too — a bare `op(self)` would kill
                // the worker thread if the imap crate panics again on tag mismatch.
                Self::run_or_catch_panic(&op, self)
            }
            Err(e) => Err(e),
        }
    }

    /// Run an IMAP operation, catching panics and converting them to anyhow errors.
    fn run_or_catch_panic<T, F>(op: &F, this: &mut Self) -> anyhow::Result<T>
    where
        F: Fn(&mut Self) -> anyhow::Result<T>,
    {
        // Suppress the default panic hook output — we handle it via reconnect
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(this)));
        std::panic::set_hook(prev_hook);

        match result {
            Ok(result) => result,
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "session desynced (tag mismatch)".into());
                Err(anyhow::anyhow!("imap panic: {msg}"))
            }
        }
    }
}

/// Create a new authenticated IMAP session with TCP timeouts.
///
/// Timeouts prevent indefinite blocking on the GTK main thread when the
/// IMAP server becomes unresponsive. Without these, a hung TCP read would
/// freeze the entire desktop (no mouse, no keyboard, no rendering).
fn new_session(config: &Account) -> anyhow::Result<ImapSession> {
    use std::net::TcpStream;
    use std::time::Duration;

    let addr = format!("{}:{}", config.imap_host, config.imap_port);
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| anyhow::anyhow!("IMAP TCP connect to {addr} failed: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))?;

    let connector = rustls_connector::RustlsConnector::new_with_native_certs()
        .map_err(|e| anyhow::anyhow!("TLS connector init failed: {e}"))?;
    let tls_stream = connector
        .connect(&config.imap_host, tcp)
        .map_err(|e| anyhow::anyhow!("IMAP TLS handshake failed: {e}"))?;

    let client = imap::Client::new(tls_stream);
    let session = client
        .login(&config.username, &config.password)
        .map_err(|e| anyhow::anyhow!("IMAP login failed: {}", e.0))?;

    Ok(session)
}

/// Check if an imap::Error indicates a dead connection.
fn is_imap_connection_error(err: &imap::Error) -> bool {
    matches!(err, imap::Error::ConnectionLost | imap::Error::Io(_))
}

/// Check if an error looks like a connection failure (worth retrying).
fn is_connection_error(err: &anyhow::Error) -> bool {
    if let Some(imap_err) = err.downcast_ref::<imap::Error>() {
        return is_imap_connection_error(imap_err);
    }

    // Fallback: check error message for connection-related strings
    let msg = err.to_string().to_lowercase();
    msg.contains("broken pipe")
        || msg.contains("connection reset")
        || msg.contains("connection closed")
        || msg.contains("unexpected eof")
        || msg.contains("timed out")
        || msg.contains("not connected")
        || msg.contains("temporarily unavailable")
        || msg.contains("os error 11")
        || msg.contains("would block")
}

/// How many newest dest-folder messages to scan when COPYUID is missing.
const DEST_UID_SCAN: u32 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
struct MailIdent {
    message_id: Option<String>,
    from: String,
    subject: String,
    date: String,
}

impl MailIdent {
    fn from_fetch(fetch: &Fetch) -> Option<Self> {
        let env = fetch.envelope()?;
        Some(Self {
            message_id: normalize_message_id(
                env.message_id
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or(""),
            ),
            from: env
                .from
                .as_ref()
                .and_then(|addrs| addrs.first())
                .map(|a| {
                    let name = a
                        .name
                        .as_ref()
                        .and_then(|n| std::str::from_utf8(n).ok())
                        .unwrap_or("");
                    let mailbox = a
                        .mailbox
                        .as_ref()
                        .and_then(|m| std::str::from_utf8(m).ok())
                        .unwrap_or("");
                    let host = a
                        .host
                        .as_ref()
                        .and_then(|h| std::str::from_utf8(h).ok())
                        .unwrap_or("");
                    if name.is_empty() {
                        format!("{mailbox}@{host}")
                    } else {
                        format!("{name} <{mailbox}@{host}>")
                    }
                })
                .unwrap_or_default(),
            subject: env
                .subject
                .and_then(|b| std::str::from_utf8(b).ok())
                .unwrap_or("")
                .to_string(),
            date: env
                .date
                .and_then(|b| std::str::from_utf8(b).ok())
                .unwrap_or("")
                .to_string(),
        })
    }

    fn matches(&self, other: &Self) -> bool {
        match (&self.message_id, &other.message_id) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => {
                if self.from.is_empty() && self.subject.is_empty() {
                    false
                } else {
                    self.from == other.from
                        && self.subject == other.subject
                        && self.date == other.date
                }
            }
        }
    }
}

fn normalize_message_id(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches(|c| c == '<' || c == '>');
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn quote_mailbox(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

fn peek_identity(session: &mut ImapSession, uid: u32) -> anyhow::Result<MailIdent> {
    let fetches = session.uid_fetch(uid.to_string(), "(UID ENVELOPE)")?;
    let fetch = fetches
        .iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Message UID {uid} not found"))?;
    MailIdent::from_fetch(fetch).ok_or_else(|| anyhow::anyhow!("No envelope for UID {uid}"))
}

fn find_copied_uid(
    client: &mut ImapClient,
    dest: &str,
    ident: &MailIdent,
) -> anyhow::Result<Option<u32>> {
    let exists = client.select_folder(dest)?;
    if exists == 0 {
        return Ok(None);
    }
    let start = exists
        .saturating_sub(DEST_UID_SCAN.saturating_sub(1))
        .max(1);
    let fetches = client
        .session
        .fetch(format!("{start}:{exists}"), "(UID ENVELOPE)")?;
    let mut best = None;
    for fetch in fetches.iter() {
        let Some(uid) = fetch.uid else { continue };
        let Some(other) = MailIdent::from_fetch(fetch) else {
            continue;
        };
        if ident.matches(&other) {
            best = Some(best.map_or(uid, |b: u32| b.max(uid)));
        }
    }
    Ok(best)
}

/// Dest UID from an IMAP COPYUID response code (RFC 4315).
///
/// The rust imap crate drops the tagged OK line, so this only sees COPYUID
/// when the server sends it untagged (typical for UID MOVE).
fn parse_copyuid_dest(response: &[u8]) -> Option<u32> {
    let text = String::from_utf8_lossy(response);
    for line in text.split(['\r', '\n']) {
        if let Some(uid) = copyuid_dest_from_line(line) {
            return Some(uid);
        }
    }
    None
}

fn copyuid_dest_from_line(line: &str) -> Option<u32> {
    let upper = line.to_ascii_uppercase();
    let idx = upper.find("[COPYUID ")?;
    let rest = line.get(idx + "[COPYUID ".len()..)?;
    let end = rest.find(']')?;
    let inner = rest[..end].trim();
    let mut parts = inner.splitn(3, char::is_whitespace);
    let _uidvalidity = parts.next()?;
    let _src = parts.next()?;
    let dest_set = parts.next()?.trim();
    first_uid_in_set(dest_set)
}

fn first_uid_in_set(set: &str) -> Option<u32> {
    let first = set.split(',').next()?.trim();
    let start = first.split(':').next()?.trim();
    start.parse().ok()
}

/// Parse a FETCH response into a MessageSummary.
fn parse_summary(fetch: &Fetch) -> Option<MessageSummary> {
    let uid = fetch.uid?;
    let envelope = fetch.envelope()?;

    macro_rules! first_addr {
        ($field:expr) => {
            $field
                .as_ref()
                .and_then(|addrs| addrs.first())
                .map(|a| {
                    let name = a
                        .name
                        .as_ref()
                        .and_then(|n| std::str::from_utf8(n).ok())
                        .unwrap_or("");
                    let mailbox = a
                        .mailbox
                        .as_ref()
                        .and_then(|m| std::str::from_utf8(m).ok())
                        .unwrap_or("");
                    let host = a
                        .host
                        .as_ref()
                        .and_then(|h| std::str::from_utf8(h).ok())
                        .unwrap_or("");
                    if name.is_empty() {
                        format!("{mailbox}@{host}")
                    } else {
                        format!("{name} <{mailbox}@{host}>")
                    }
                })
                .unwrap_or_default()
        };
    }

    let from = first_addr!(envelope.from);
    let to = first_addr!(envelope.to);

    let subject = envelope
        .subject
        .as_ref()
        .and_then(|s| std::str::from_utf8(s).ok())
        .unwrap_or("")
        .to_string();

    let date = envelope
        .date
        .as_ref()
        .and_then(|d| std::str::from_utf8(d).ok())
        .unwrap_or("")
        .to_string();
    let date_sort = super::date::date_sort_key(&date);

    // Soft-deleted messages should not appear in the UI list.
    if fetch
        .flags()
        .iter()
        .any(|f| matches!(f, imap::types::Flag::Deleted))
    {
        return None;
    }

    let seen = fetch
        .flags()
        .iter()
        .any(|f| matches!(f, imap::types::Flag::Seen));

    let forwarded_for = fetch.header().and_then(|hdr| {
        let text = std::str::from_utf8(hdr).ok()?;
        let value = text
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("x-forwarded-for:"))?;
        let value = value[16..].trim();
        // Header may contain multiple addresses (space or comma separated);
        // the first is the original recipient whose mail was forwarded.
        let first = value
            .split([' ', ','])
            .find(|s| !s.is_empty())
            .unwrap_or(value);
        Some(first.to_string())
    });

    Some(MessageSummary {
        account: String::new(),
        uid,
        from,
        to,
        subject,
        date,
        date_sort,
        seen,
        forwarded_for,
        has_attachment: fetch.bodystructure().is_some_and(structure_has_attachment),
    })
}

fn structure_has_attachment(bs: &imap_proto::types::BodyStructure<'_>) -> bool {
    use imap_proto::types::BodyStructure::*;
    match bs {
        Multipart { bodies, .. } => bodies.iter().any(structure_has_attachment),
        Message { .. } => true,
        Text { common, .. } => leaf_is_file(common, true),
        Basic { common, .. } => leaf_is_file(common, false),
    }
}

fn leaf_is_file(common: &imap_proto::types::BodyContentCommon<'_>, is_text: bool) -> bool {
    if let Some(disp) = &common.disposition {
        if disp.ty.eq_ignore_ascii_case("attachment") {
            return true;
        }
        if disp.ty.eq_ignore_ascii_case("inline") {
            return false;
        }
    }
    if has_structure_filename(common) {
        return true;
    }
    !is_text
}

fn has_structure_filename(common: &imap_proto::types::BodyContentCommon<'_>) -> bool {
    filename_in(
        common
            .disposition
            .as_ref()
            .and_then(|d| d.params.as_deref()),
    ) || filename_in(common.ty.params.as_deref())
}

fn filename_in(params: Option<&[(&str, &str)]>) -> bool {
    params.is_some_and(|ps| {
        ps.iter().any(|(k, v)| {
            !v.is_empty() && (k.eq_ignore_ascii_case("filename") || k.eq_ignore_ascii_case("name"))
        })
    })
}

/// Parse a FETCH response with body into a MessageBody.
fn parse_body(fetch: &Fetch, uid: u32) -> anyhow::Result<MessageBody> {
    let body_bytes = fetch
        .body()
        .ok_or_else(|| anyhow::anyhow!("No body in fetch response"))?;

    let parsed = mail_parser::MessageParser::default()
        .parse(body_bytes)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse message body"))?;

    let from = parsed
        .from()
        .and_then(|a| a.first())
        .map(|a| {
            a.name()
                .map(|n| format!("{n} <{}>", a.address().unwrap_or_default()))
                .unwrap_or_else(|| a.address().unwrap_or_default().to_string())
        })
        .unwrap_or_default();

    let to = parsed
        .to()
        .map(|list| {
            list.iter()
                .map(|a| a.address().unwrap_or_default().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let cc = parsed
        .cc()
        .map(|list| {
            list.iter()
                .map(|a| a.address().unwrap_or_default().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let subject = parsed.subject().unwrap_or("").to_string();

    let date = parsed.date().map(|d| d.to_rfc3339()).unwrap_or_default();

    let html = parsed.body_html(0).map(|s| s.to_string());
    let text = parsed
        .body_text(0)
        .map(|s| s.to_string())
        .unwrap_or_default();

    let in_reply_to = parsed.in_reply_to().as_text().map(|s| s.to_string());
    let message_id = parsed.message_id().map(|s| s.to_string());
    let attachments = collect_attachments(&parsed);

    Ok(MessageBody {
        account: String::new(),
        uid,
        from,
        to,
        cc,
        subject,
        date,
        html,
        text,
        in_reply_to,
        message_id,
        attachments,
    })
}

/// Result of counting messages per smart mailbox rule via IMAP SEARCH.
pub struct SmartMailboxCounts {
    /// Per-smart-mailbox folder counts.
    pub folders: Vec<Folder>,
    /// Total unique INBOX messages matching any smart mailbox rule.
    pub inbox_total_deduction: u32,
    /// Total unique unseen INBOX messages matching any smart mailbox rule.
    pub inbox_unread_deduction: u32,
}

/// Return the union of UIDs in INBOX matched by any smart_mailbox rule.
/// Caller must have INBOX selected.
fn smart_mailbox_uids(session: &mut ImapSession, rules: &[MailRule]) -> HashSet<u32> {
    let mut union: HashSet<u32> = HashSet::new();
    for rule in rules.iter().filter(|r| r.action == "smart_mailbox") {
        let query = build_imap_search(&rule.conditions);
        if query.is_empty() {
            continue;
        }
        match session.uid_search(&query) {
            Ok(uids) => union.extend(&uids),
            Err(e) => warn!("SEARCH failed for smart mailbox {}: {e}", rule.name),
        }
    }
    union
}

/// Page of envelopes via sequence numbers (highest seq = newest). No SEARCH.
fn fetch_page_by_seq(
    session: &mut ImapSession,
    exists: u32,
    offset: u32,
    limit: u32,
) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
    if exists == 0 || limit == 0 {
        return Ok((Vec::new(), exists));
    }
    let end = exists.saturating_sub(offset);
    if end == 0 {
        return Ok((Vec::new(), exists));
    }
    let start = end.saturating_sub(limit.saturating_sub(1)).max(1);
    let fetches = session.fetch(format!("{start}:{end}"), SUMMARY_ITEMS)?;
    let mut messages: Vec<MessageSummary> = fetches.iter().filter_map(parse_summary).collect();
    messages.sort_unstable_by(|a, b| b.uid.cmp(&a.uid));
    Ok((messages, exists))
}

/// Fetch envelope summaries for a paginated slice of a UID list (already sorted desc).
fn fetch_envelopes(
    session: &mut ImapSession,
    uids_desc: &[u32],
    offset: u32,
    limit: u32,
    total: u32,
) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
    if limit == 0 || uids_desc.is_empty() {
        return Ok((Vec::new(), total));
    }
    let start = offset as usize;
    if start >= uids_desc.len() {
        return Ok((Vec::new(), total));
    }
    let end = (start + limit as usize).min(uids_desc.len());
    let seq = uids_desc[start..end]
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session.uid_fetch(&seq, SUMMARY_ITEMS)?;
    let mut messages: Vec<MessageSummary> = fetches.iter().filter_map(parse_summary).collect();
    messages.sort_unstable_by(|a, b| b.uid.cmp(&a.uid));
    Ok((messages, total))
}

/// Build an IMAP SEARCH query string from mail rule conditions.
///
/// Conditions are ANDed (IMAP SEARCH criteria listed together are implicitly ANDed).
/// IMAP SEARCH only supports substring matching on header fields, so "domain" and
/// "address" match types are approximated as substring searches. Counts may slightly
/// overcount compared to the precise frontend filtering.
///
/// Soft-deleted messages are filtered out later via [`parse_summary`] / list paths;
/// we intentionally do **not** prefix `UNDELETED` here — some servers mishandle
/// combined criteria and desync the imap crate's command tags.
fn build_imap_search(conditions: &[MailRuleCondition]) -> String {
    conditions
        .iter()
        .filter_map(|c| {
            let safe = c.value.replace('\\', "\\\\").replace('"', "\\\"");
            match c.field.as_str() {
                "from" => Some(format!("FROM \"{safe}\"")),
                "subject" => Some(format!("SUBJECT \"{safe}\"")),
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copyuid_from_untagged_move() {
        let raw = b"* OK [COPYUID 1511554416 142 4567] Moved UIDs.\r\n";
        assert_eq!(parse_copyuid_dest(raw), Some(4567));
    }

    #[test]
    fn copyuid_from_tagged_ok() {
        let raw = b"a1 OK [COPYUID 38505 304 3956] Done\r\n";
        assert_eq!(parse_copyuid_dest(raw), Some(3956));
    }

    #[test]
    fn copyuid_dest_set_takes_first_uid() {
        let raw = b"* OK [COPYUID 1511554416 142,399 41:42] Moved UIDs.\r\n";
        assert_eq!(parse_copyuid_dest(raw), Some(41));
    }

    #[test]
    fn copyuid_absent() {
        assert_eq!(parse_copyuid_dest(b"a1 OK COPY completed\r\n"), None);
        assert_eq!(parse_copyuid_dest(b""), None);
    }

    #[test]
    fn quote_mailbox_escapes() {
        assert_eq!(quote_mailbox("Trash"), "\"Trash\"");
        assert_eq!(quote_mailbox(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn ident_matches_message_id() {
        let a = MailIdent {
            message_id: Some("abc@x".into()),
            from: "a@x".into(),
            subject: "s".into(),
            date: "d".into(),
        };
        let b = MailIdent {
            message_id: Some("ABC@x".into()),
            from: "other".into(),
            subject: "nope".into(),
            date: "nope".into(),
        };
        let c = MailIdent {
            message_id: Some("other@x".into()),
            from: "a@x".into(),
            subject: "s".into(),
            date: "d".into(),
        };
        assert!(a.matches(&b));
        assert!(!a.matches(&c));
    }

    #[test]
    fn ident_weak_match_without_message_id() {
        let a = MailIdent {
            message_id: None,
            from: "a@x".into(),
            subject: "Hello".into(),
            date: "1 Jan".into(),
        };
        let b = a.clone();
        let empty = MailIdent {
            message_id: None,
            from: String::new(),
            subject: String::new(),
            date: String::new(),
        };
        assert!(a.matches(&b));
        assert!(!empty.matches(&a));
        assert!(!a.matches(&empty));
    }

    #[test]
    fn normalize_strips_angle_brackets() {
        assert_eq!(normalize_message_id(" <id@host> "), Some("id@host".into()));
        assert_eq!(normalize_message_id("   "), None);
    }
}
