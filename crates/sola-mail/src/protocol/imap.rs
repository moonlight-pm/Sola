use std::collections::HashSet;

use imap::types::Fetch;
use tracing::{debug, warn};

use super::account::Account;
use super::types::{Folder, MessageBody, MessageSummary};
use sola_bus::topics::{MailRule, MailRuleCondition};

type ImapSession = imap::Session<rustls_connector::TlsStream<std::net::TcpStream>>;

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

    /// List all folders with unread/total counts.
    pub fn list_folders(&mut self) -> anyhow::Result<Vec<Folder>> {
        self.with_reconnect(|s| {
            let mailboxes = s.session.list(None, Some("*"))?;
            let mut folders = Vec::new();

            for mb in mailboxes.iter() {
                let name = mb.name().to_string();
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

            Ok(folders)
        })
    }

    /// Select a folder and fetch a page of message summaries (most recent first).
    pub fn list_messages(
        &mut self,
        folder: &str,
        offset: u32,
        limit: u32,
    ) -> anyhow::Result<(Vec<MessageSummary>, u32)> {
        let folder = folder.to_string();
        self.with_reconnect(move |s| {
            let mailbox = s.session.select(&folder)?;
            s.selected_folder = Some(folder.clone());

            let total = mailbox.exists;
            if total == 0 || limit == 0 {
                return Ok((Vec::new(), total));
            }

            // Sequence numbers from the end: newest messages have highest numbers
            let end = total.saturating_sub(offset);
            if end == 0 {
                return Ok((Vec::new(), total));
            }
            let start = end.saturating_sub(limit - 1).max(1);

            let range = format!("{start}:{end}");
            let fetches = s.session.fetch(
                &range,
                "(UID FLAGS ENVELOPE BODY.PEEK[HEADER.FIELDS (X-Forwarded-For)])",
            )?;

            let mut messages: Vec<MessageSummary> =
                fetches.iter().filter_map(parse_summary).collect();

            // Most recent first
            messages.sort_unstable_by(|a, b| b.uid.cmp(&a.uid));

            Ok((messages, total))
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
            s.ensure_selected("INBOX")?;
            let all_uids = s.session.uid_search("ALL")?;
            let excluded = smart_mailbox_uids(&mut s.session, &rules);
            let mut kept: Vec<u32> = all_uids
                .into_iter()
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
            // IMAP SEARCH: OR across subject, from, to, and body (TEXT)
            // TEXT searches both headers and body content per RFC 3501
            let imap_query = format!(
                "OR OR OR (SUBJECT \"{safe}\") (FROM \"{safe}\") (TO \"{safe}\") (TEXT \"{safe}\")"
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

            let fetches = s.session.uid_fetch(
                &uid_str,
                "(UID FLAGS ENVELOPE BODY.PEEK[HEADER.FIELDS (X-Forwarded-For)])",
            )?;

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

    /// Move a message to another folder (COPY + DELETE + EXPUNGE).
    ///
    /// Note: this is not atomic. If the connection dies mid-operation (e.g. after COPY
    /// but before EXPUNGE), the retry may create a duplicate in the destination folder.
    /// This is preferable to losing mail.
    pub fn move_message(&mut self, folder: &str, uid: u32, dest: &str) -> anyhow::Result<()> {
        let folder = folder.to_string();
        let dest = dest.to_string();
        self.with_reconnect(move |s| {
            s.ensure_selected(&folder)?;
            s.session.uid_copy(uid.to_string(), &dest)?;
            s.session.uid_store(uid.to_string(), "+FLAGS (\\Deleted)")?;
            s.session.expunge()?;
            Ok(())
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
        self.with_reconnect(move |s| {
            let mailbox = s.session.select(&folder)?;
            s.selected_folder = Some(folder.clone());
            if mailbox.exists == 0 {
                return Ok(());
            }
            s.session.store("1:*", "+FLAGS (\\Deleted)")?;
            s.session.expunge()?;
            Ok(())
        })
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
            self.session.select(folder)?;
            self.selected_folder = Some(folder.to_string());
        }
        Ok(())
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
            Err(e) if is_connection_error(&e) => {
                warn!("IMAP connection error, reconnecting: {e}");
                self.reconnect()?;
                op(self)
            }
            Err(e) if e.to_string().contains("imap panic") => {
                warn!("IMAP session corrupted (panic), reconnecting: {e}");
                self.reconnect()?;
                op(self)
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
                    .map(|s| s.as_str())
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown");
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
        uid,
        from,
        to,
        subject,
        date,
        seen,
        forwarded_for,
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

    Ok(MessageBody {
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
    let fetches = session.uid_fetch(
        &seq,
        "(UID FLAGS ENVELOPE BODY.PEEK[HEADER.FIELDS (X-Forwarded-For)])",
    )?;
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
