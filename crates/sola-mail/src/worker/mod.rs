//! Mail worker thread: owns IMAP session, IDLE, and SMTP.

mod cmds;
mod write;

use cmds::compact_cmds;
pub use cmds::{LinkState, MailCmd, MailEvent};
use write::WriteCmd;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sola_bus::topics::MailConfig;
use tracing::{debug, info, warn};

use crate::bridge;
use crate::protocol::boxes::{self, MailboxMap};
use crate::protocol::types::MessageSummary;
use crate::protocol::{
    Account, Folder, IdleChange, ImapClient, account, rule_matches, sender, start_idle,
};

struct ImapSlot {
    account: Account,
    map: MailboxMap,
    /// List / fetch / mark-read / STATUS. Never waits on MOVE.
    client: Arc<Mutex<ImapClient>>,
    write_tx: mpsc::Sender<WriteCmd>,
    write: Option<std::thread::JoinHandle<()>>,
    idle: Option<crate::protocol::IdleHandle>,
    keepalive: Option<std::thread::JoinHandle<()>>,
    keepalive_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
}

struct WorkerState {
    config: Option<MailConfig>,
    slots: Vec<ImapSlot>,
    /// Move rules shared with IDLE callbacks.
    idle_move_rules: Arc<Mutex<Vec<sola_bus::topics::MailRule>>>,
}

pub fn start() {
    std::thread::Builder::new()
        .name("sola-mail-worker".into())
        .spawn(run)
        .expect("spawn mail worker");
}

fn run() {
    let cmd_rx = bridge::take_cmd_rx();
    let mut state = WorkerState {
        config: None,
        slots: Vec::new(),
        idle_move_rules: Arc::new(Mutex::new(Vec::new())),
    };

    let mut pending: Vec<MailCmd> = Vec::new();
    loop {
        if pending.is_empty() {
            match cmd_rx.recv() {
                Ok(first) => pending.push(first),
                Err(_) => break,
            }
        }
        while let Ok(more) = cmd_rx.try_recv() {
            pending.push(more);
        }
        pending = compact_cmds(std::mem::take(&mut pending));
        if pending.is_empty() {
            continue;
        }
        let cmd = pending.remove(0);
        match cmd {
            MailCmd::Shutdown => {
                teardown(&mut state);
                break;
            }
            MailCmd::Reconfigure(cfg) => {
                let n = Account::imap_accounts(&cfg).len();
                state.config = Some(cfg);
                if n > 0 {
                    do_connect(&mut state);
                } else {
                    teardown(&mut state);
                    bridge::emit(MailEvent::NotConfigured);
                }
            }
            MailCmd::ListFolders => do_list_folders(&state),
            MailCmd::ListMessages {
                folder,
                offset,
                limit,
            } => do_list_messages(&state, folder, offset, limit),
            MailCmd::Search { query } => do_search(&state, query),
            MailCmd::FetchBody {
                account,
                folder,
                uid,
            } => do_fetch_body(&state, account, folder, uid),
            MailCmd::MarkRead {
                account,
                folder,
                uid,
            } => do_mark_read(&state, account, folder, uid),
            MailCmd::Move {
                account,
                folder,
                uid,
                dest,
            } => do_move(&state, account, folder, uid, dest),
            MailCmd::EmptyFolder { folder } => do_empty(&state, folder),
            MailCmd::Send {
                from,
                to,
                cc,
                subject,
                body,
                in_reply_to,
                attachments,
            } => do_send(
                &state,
                from,
                to,
                cc,
                subject,
                body,
                in_reply_to,
                attachments,
            ),
        }
    }
    debug!("mail worker stopped");
}

fn teardown(state: &mut WorkerState) {
    for slot in &mut state.slots {
        if let Some(flag) = slot.keepalive_stop.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(h) = slot.keepalive.take() {
            let _ = h.join();
        }
        if let Some(idle) = slot.idle.take() {
            idle.stop();
        }
        let _ = slot.write_tx.send(WriteCmd::Shutdown);
        if let Some(h) = slot.write.take() {
            let _ = h.join();
        }
    }
    state.slots.clear();
}

fn slot_by_id<'a>(state: &'a WorkerState, account: &str) -> Result<&'a ImapSlot, String> {
    let key = sola_bus::topics::mail_addr_key(account);
    state
        .slots
        .iter()
        .find(|s| s.account.id() == key || s.account.id() == account)
        .ok_or_else(|| format!("No IMAP session for {account}"))
}

fn remote_box(slot: &ImapSlot, canonical: &str) -> String {
    boxes::remote(&slot.map, canonical)
        .unwrap_or(canonical)
        .to_string()
}

fn do_connect(state: &mut WorkerState) {
    teardown(state);

    let Some(cfg) = state.config.clone() else {
        bridge::emit(MailEvent::NotConfigured);
        return;
    };
    let accounts = Account::imap_accounts(&cfg);
    if accounts.is_empty() {
        bridge::emit(MailEvent::NotConfigured);
        return;
    }

    let move_rules: Vec<_> = cfg
        .rules
        .iter()
        .filter(|r| r.action == "move")
        .cloned()
        .collect();
    *state
        .idle_move_rules
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = move_rules.clone();

    let n = accounts.len();
    let (tx, rx) = std::sync::mpsc::channel();
    for account in accounts {
        let email = account.email.clone();
        let id = account.id();
        bridge::emit(MailEvent::AccountLink {
            account: id.clone(),
            email: email.clone(),
            state: LinkState::Connecting,
        });
        let rules = move_rules.clone();
        let idle_rules = Arc::clone(&state.idle_move_rules);
        let tx = tx.clone();
        let _ = std::thread::Builder::new()
            .name(format!("sola-mail-c-{id}"))
            .spawn(move || {
                let result = connect_slot(account, &rules, &idle_rules);
                let _ = tx.send((id, email, result));
            });
    }
    drop(tx);

    let mut slots = Vec::with_capacity(n);
    while let Ok((id, email, result)) = rx.recv() {
        match result {
            Ok(slot) => {
                info!(account = id.as_str(), "IMAP ready");
                bridge::emit(MailEvent::AccountLink {
                    account: id,
                    email,
                    state: LinkState::Ready,
                });
                slots.push(slot);
            }
            Err(e) => {
                warn!(account = id.as_str(), "IMAP connect failed: {e}");
                bridge::emit(MailEvent::AccountLink {
                    account: id,
                    email,
                    state: LinkState::Failed(e),
                });
            }
        }
    }

    if slots.is_empty() {
        return;
    }

    let folders = merge_canonical_folders(&slots);
    let smart = merge_smart_counts(&slots, &cfg.rules);
    let mut folders = folders;
    if let Some(inbox) = folders.iter_mut().find(|f| f.name == "INBOX") {
        inbox.total = inbox.total.saturating_sub(smart.inbox_total_deduction);
        inbox.unread = inbox.unread.saturating_sub(smart.inbox_unread_deduction);
    }

    let from_addresses = cfg.from_addresses();

    let rules = cfg.rules.clone();
    state.slots = slots;
    bridge::emit(MailEvent::Connected {
        folders,
        smart_counts: smart.folders,
        from_addresses,
        rules,
    });
}

fn connect_slot(
    account: Account,
    move_rules: &[sola_bus::topics::MailRule],
    idle_move_rules: &Arc<Mutex<Vec<sola_bus::topics::MailRule>>>,
) -> Result<ImapSlot, String> {
    let mut client = ImapClient::connect(&account).map_err(|e| e.to_string())?;
    let mut listed = client.list_mailbox_names().map_err(|e| e.to_string())?;
    let mut map = boxes::map_mailboxes(&listed);
    if boxes::should_create_archive(&map, &account.imap_host, &listed) {
        match client.create_mailbox("Archive") {
            Ok(()) => {
                info!(account = account.id().as_str(), "created Archive mailbox");
                if let Ok(again) = client.list_mailbox_names() {
                    listed = again;
                    map = boxes::map_mailboxes(&listed);
                }
            }
            Err(e) => warn!(
                account = account.id().as_str(),
                "CREATE Archive failed: {e}"
            ),
        }
    }
    info!(
        account = account.id().as_str(),
        boxes = map.len(),
        "IMAP mailbox map"
    );

    let inbox_remote = boxes::remote(&map, "INBOX").unwrap_or("INBOX").to_string();
    let dest_map = map.clone();
    let moved = apply_move_rules(
        &mut client,
        &inbox_remote,
        &dest_map,
        move_rules,
        APPLY_ON_CONNECT,
    );
    if moved > 0 {
        info!(
            moved,
            account = account.id().as_str(),
            "move rules applied on connect"
        );
    }

    let client_arc = Arc::new(Mutex::new(client));
    let write_client = match ImapClient::connect(&account) {
        Ok(c) => Some(c),
        Err(e) => {
            warn!(
                account = account.id().as_str(),
                "write IMAP session failed ({e}); moves share the list connection"
            );
            None
        }
    };
    let shared_for_write = if write_client.is_none() {
        Some(Arc::clone(&client_arc))
    } else {
        None
    };
    let (write_tx, write) =
        write::spawn(account.clone(), map.clone(), write_client, shared_for_write);
    let shared_rules = Arc::clone(idle_move_rules);
    let idle_map = map.clone();
    let idle_account = account.clone();
    let idle = start_idle(account.clone(), move |change, idle_client| match change {
        IdleChange::Arrived { new_count } => {
            let rules = shared_rules.lock().unwrap_or_else(|e| e.into_inner());
            if !rules.is_empty() {
                let inbox = boxes::remote(&idle_map, "INBOX").unwrap_or("INBOX");
                let _ = apply_move_rules(idle_client, inbox, &idle_map, &rules, new_count.max(20));
            }
            let _ = idle_account.id();
            bridge::emit(MailEvent::NewMail);
        }
        IdleChange::Removed { gone } => {
            tracing::debug!(gone, "IDLE: remote deletes/expunge — refreshing UI");
            bridge::emit(MailEvent::NewMail);
        }
        IdleChange::Touched => {}
    });

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let ka_client = Arc::clone(&client_arc);
    let keepalive = std::thread::Builder::new()
        .name(format!("sola-mail-ka-{}", account.id()))
        .spawn(move || {
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                for _ in 0..2400 {
                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let mut c = ka_client.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = c.noop() {
                    warn!("Keepalive NOOP failed: {e}");
                }
            }
        })
        .ok();

    Ok(ImapSlot {
        account,
        map,
        client: client_arc,
        write_tx,
        write: Some(write),
        idle: Some(idle),
        keepalive,
        keepalive_stop: Some(stop),
    })
}

fn merge_canonical_folders(slots: &[ImapSlot]) -> Vec<Folder> {
    let mut folders: Vec<Folder> = boxes::CANONICAL
        .iter()
        .map(|name| Folder {
            name: (*name).to_string(),
            unread: 0,
            total: 0,
        })
        .collect();
    for slot in slots {
        let mut c = slot.client.lock().unwrap_or_else(|e| e.into_inner());
        for f in &mut folders {
            let Some(remote) = boxes::remote(&slot.map, &f.name) else {
                continue;
            };
            match c.folder_status(remote) {
                Ok((unread, total)) => {
                    f.unread = f.unread.saturating_add(unread);
                    f.total = f.total.saturating_add(total);
                }
                Err(e) => warn!(
                    account = slot.account.id().as_str(),
                    remote, "STATUS failed: {e}"
                ),
            }
        }
    }
    folders
}

struct SmartMerge {
    folders: Vec<Folder>,
    inbox_total_deduction: u32,
    inbox_unread_deduction: u32,
}

fn merge_smart_counts(slots: &[ImapSlot], rules: &[sola_bus::topics::MailRule]) -> SmartMerge {
    let mut acc = SmartMerge {
        folders: Vec::new(),
        inbox_total_deduction: 0,
        inbox_unread_deduction: 0,
    };
    for slot in slots {
        let mut c = slot.client.lock().unwrap_or_else(|e| e.into_inner());
        match c.count_smart_mailboxes(rules) {
            Ok(s) => {
                acc.inbox_total_deduction = acc
                    .inbox_total_deduction
                    .saturating_add(s.inbox_total_deduction);
                acc.inbox_unread_deduction = acc
                    .inbox_unread_deduction
                    .saturating_add(s.inbox_unread_deduction);
                for f in s.folders {
                    if let Some(e) = acc.folders.iter_mut().find(|x| x.name == f.name) {
                        e.unread = e.unread.saturating_add(f.unread);
                        e.total = e.total.saturating_add(f.total);
                    } else {
                        acc.folders.push(f);
                    }
                }
            }
            Err(e) => warn!(
                account = slot.account.id().as_str(),
                "smart counts failed: {e}"
            ),
        }
    }
    acc
}

fn do_list_folders(state: &WorkerState) {
    if state.slots.is_empty() {
        return;
    }
    let rules = state
        .config
        .as_ref()
        .map(|c| c.rules.clone())
        .unwrap_or_default();
    let mut folders = merge_canonical_folders(&state.slots);
    let smart = merge_smart_counts(&state.slots, &rules);
    if let Some(inbox) = folders.iter_mut().find(|f| f.name == "INBOX") {
        inbox.total = inbox.total.saturating_sub(smart.inbox_total_deduction);
        inbox.unread = inbox.unread.saturating_sub(smart.inbox_unread_deduction);
    }
    bridge::emit(MailEvent::Folders {
        folders,
        smart_counts: smart.folders,
    });
}

fn do_list_messages(state: &WorkerState, folder: String, offset: u32, limit: u32) {
    if state.slots.is_empty() {
        return;
    }
    let rules = state
        .config
        .as_ref()
        .map(|c| c.rules.clone())
        .unwrap_or_default();
    let fetch = offset.saturating_add(limit);
    let mut all = Vec::new();
    let mut total = 0u32;
    for slot in &state.slots {
        let mut c = slot.client.lock().unwrap_or_else(|e| e.into_inner());
        let result = if let Some(name) = folder.strip_prefix("smart:") {
            c.list_smart_mailbox(name, &rules, 0, fetch)
        } else {
            let remote = remote_box(slot, &folder);
            if folder == "INBOX" {
                // Smart-mailbox filtering is INBOX-only and per-account.
                c.list_inbox_filtered(&rules, 0, fetch)
                    .or_else(|_| c.list_messages(&remote, 0, fetch))
            } else {
                c.list_messages(&remote, 0, fetch)
            }
        };
        match result {
            Ok((mut messages, n)) => {
                total = total.saturating_add(n);
                let id = slot.account.id();
                for m in &mut messages {
                    m.stamp_account(&id);
                }
                all.extend(messages);
            }
            Err(e) => warn!(
                account = slot.account.id().as_str(),
                folder = folder.as_str(),
                "list failed: {e}"
            ),
        }
    }
    all.sort_by(MessageSummary::cmp_recency);
    let skip = offset as usize;
    let take = limit as usize;
    let messages = all.into_iter().skip(skip).take(take).collect();
    bridge::emit(MailEvent::Messages {
        folder,
        messages,
        total,
        offset,
    });
}

fn do_search(state: &WorkerState, query: String) {
    if query.trim().is_empty() {
        bridge::emit(MailEvent::Error {
            context: "search".into(),
            message: "empty query".into(),
        });
        return;
    }
    let mut all = Vec::new();
    for slot in &state.slots {
        let mut c = slot.client.lock().unwrap_or_else(|e| e.into_inner());
        let folders = ["INBOX", "Sent", "Archive"];
        let id = slot.account.id();
        for canon in folders {
            let remote = remote_box(slot, canon);
            match c.search_messages(&remote, &query) {
                Ok((mut messages, _)) => {
                    for m in &mut messages {
                        m.stamp_account(&id);
                    }
                    all.extend(messages);
                }
                Err(e) => warn!(
                    account = id.as_str(),
                    remote = remote.as_str(),
                    "search failed: {e}"
                ),
            }
        }
    }
    all.sort_by(MessageSummary::cmp_recency);
    all.truncate(200);
    let total = all.len() as u32;
    bridge::emit(MailEvent::SearchResults {
        messages: all,
        total,
    });
}

fn do_fetch_body(state: &WorkerState, account: String, folder: String, uid: u32) {
    let slot = match slot_by_id(state, &account) {
        Ok(s) => s,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "fetch_body".into(),
                message: e,
            });
            return;
        }
    };
    let remote = remote_box(slot, &folder);
    let mut c = slot.client.lock().unwrap_or_else(|e| e.into_inner());
    match c.fetch_body(&remote, uid) {
        Ok(mut body) => {
            body.account = slot.account.id();
            bridge::emit(MailEvent::Body(body));
        }
        Err(e) => bridge::emit(MailEvent::Error {
            context: "fetch_body".into(),
            message: e.to_string(),
        }),
    }
}

fn do_mark_read(state: &WorkerState, account: String, folder: String, uid: u32) {
    let slot = match slot_by_id(state, &account) {
        Ok(s) => s,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "mark_read".into(),
                message: e,
            });
            return;
        }
    };
    let remote = remote_box(slot, &folder);
    let mut c = slot.client.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = c.mark_read(&remote, uid) {
        bridge::emit(MailEvent::Error {
            context: "mark_read".into(),
            message: e.to_string(),
        });
    }
}

fn do_move(state: &WorkerState, account: String, folder: String, uid: u32, dest: String) {
    let slot = match slot_by_id(state, &account) {
        Ok(s) => s,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "move".into(),
                message: e,
            });
            return;
        }
    };
    if slot
        .write_tx
        .send(WriteCmd::Move { folder, uid, dest })
        .is_err()
    {
        bridge::emit(MailEvent::MoveFailed {
            account,
            uid,
            message: "Write worker gone".into(),
        });
    }
}

fn do_empty(state: &WorkerState, folder: String) {
    let mut any = false;
    for slot in &state.slots {
        if boxes::remote(&slot.map, &folder).is_none() {
            continue;
        }
        if slot
            .write_tx
            .send(WriteCmd::Empty {
                folder: folder.clone(),
            })
            .is_ok()
        {
            any = true;
        }
    }
    if !any {
        bridge::emit(MailEvent::Error {
            context: "empty_folder".into(),
            message: "No IMAP session for that folder".into(),
        });
    }
}

fn do_send(
    state: &WorkerState,
    from: String,
    to: String,
    cc: String,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    attachments: Vec<crate::protocol::MailAttachment>,
) {
    let Some(cfg) = state.config.clone() else {
        bridge::emit(MailEvent::Error {
            context: "send".into(),
            message: "Not connected".into(),
        });
        return;
    };
    let account = account::smtp_for(&cfg, &from);
    if !account.can_send() {
        bridge::emit(MailEvent::Error {
            context: "send".into(),
            message: format!("No SMTP account for {from}"),
        });
        return;
    }
    let sent_slot = slot_by_id(state, &account.id())
        .ok()
        .or_else(|| state.slots.first());
    let Some(slot) = sent_slot else {
        bridge::emit(MailEvent::Error {
            context: "send".into(),
            message: "Not connected".into(),
        });
        return;
    };
    let sent_remote = remote_box(slot, "Sent");
    let write_tx = slot.write_tx.clone();
    let cc_opt = if cc.trim().is_empty() {
        None
    } else {
        Some(cc.as_str())
    };
    match sender::send_mail(
        &account,
        &from,
        &to,
        cc_opt,
        &subject,
        &body,
        in_reply_to.as_deref(),
        &attachments,
    ) {
        Ok(raw) => {
            if write_tx
                .send(WriteCmd::Append {
                    remote: sent_remote,
                    raw,
                })
                .is_err()
            {
                warn!("Failed to queue Sent append");
            }
            bridge::emit(MailEvent::Sent);
        }
        Err(e) => bridge::emit(MailEvent::Error {
            context: "send".into(),
            message: e.to_string(),
        }),
    }
}

/// Newest INBOX envelopes to scan when applying move rules on connect.
/// Apocrypha used 500; older matches wait for a later IDLE sweep of the
/// newest page.
const APPLY_ON_CONNECT: u32 = 500;

/// Apply `action == "move"` rules to a page of newest INBOX messages.
/// Returns how many messages were moved.
fn apply_move_rules(
    client: &mut ImapClient,
    inbox_remote: &str,
    map: &MailboxMap,
    rules: &[sola_bus::topics::MailRule],
    page: u32,
) -> u32 {
    let move_rules: Vec<_> = rules
        .iter()
        .filter(|r| r.action == "move" && r.dest.as_deref().is_some_and(|d| !d.trim().is_empty()))
        .cloned()
        .collect();
    if move_rules.is_empty() || page == 0 {
        return 0;
    }
    let messages = match client.list_messages(inbox_remote, 0, page) {
        Ok((msgs, _)) => msgs,
        Err(e) => {
            warn!("move rules: failed to list {inbox_remote}: {e}");
            return 0;
        }
    };
    let mut moved = 0u32;
    for msg in &messages {
        for rule in &move_rules {
            let Some(dest) = rule
                .dest
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
            else {
                continue;
            };
            if !rule_matches(rule, &msg.from, &msg.subject, &msg.to) {
                continue;
            }
            let dest_remote = boxes::remote(map, dest).unwrap_or(dest);
            match client.move_message(inbox_remote, msg.uid, dest_remote) {
                Ok(_) => {
                    info!(
                        uid = msg.uid,
                        dest = dest_remote,
                        rule = rule.name.as_str(),
                        "move rule applied"
                    );
                    moved += 1;
                }
                Err(e) => {
                    warn!(
                        "move rule {}: uid {} to {dest_remote}: {e}",
                        rule.name, msg.uid
                    );
                }
            }
            break;
        }
    }
    moved
}

// Silence unused import warning if MailConfig only used via re-export paths.
#[allow(dead_code)]
fn _cfg_ty(_: MailConfig) {}
