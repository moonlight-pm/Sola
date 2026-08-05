//! Mail worker thread: owns IMAP session, IDLE, and SMTP.

mod cmds;

pub use cmds::{MailCmd, MailEvent};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sola_bus::topics::MailConfig;
use tracing::{debug, warn};

use crate::bridge;
use crate::protocol::{
    start_idle, Account, IdleChange, ImapClient, rule_matches, sender, wicket,
};

struct WorkerState {
    account: Option<Account>,
    client: Option<Arc<Mutex<ImapClient>>>,
    idle: Option<crate::protocol::IdleHandle>,
    keepalive: Option<std::thread::JoinHandle<()>>,
    keepalive_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Move rules shared with IDLE callback.
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
        account: None,
        client: None,
        idle: None,
        keepalive: None,
        keepalive_stop: None,
        idle_move_rules: Arc::new(Mutex::new(Vec::new())),
    };

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            MailCmd::Shutdown => {
                teardown(&mut state);
                break;
            }
            MailCmd::Reconfigure(cfg) => {
                state.account = Some(Account::from_config(&cfg));
                // Auto-connect when credentials are present.
                if state.account.as_ref().is_some_and(|a| a.is_configured()) {
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
            MailCmd::FetchBody { folder, uid } => do_fetch_body(&state, folder, uid),
            MailCmd::MarkRead { folder, uid } => do_mark_read(&state, folder, uid),
            MailCmd::Move { folder, uid, dest } => do_move(&state, folder, uid, dest),
            MailCmd::EmptyFolder { folder } => do_empty(&state, folder),
            MailCmd::Send {
                from,
                to,
                cc,
                subject,
                body,
                in_reply_to,
            } => do_send(&state, from, to, cc, subject, body, in_reply_to),
        }
    }
    debug!("mail worker stopped");
}

fn teardown(state: &mut WorkerState) {
    if let Some(flag) = state.keepalive_stop.take() {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(h) = state.keepalive.take() {
        let _ = h.join();
    }
    if let Some(idle) = state.idle.take() {
        idle.stop();
    }
    state.client = None;
}

fn get_client(state: &WorkerState) -> Result<Arc<Mutex<ImapClient>>, String> {
    state
        .client
        .clone()
        .ok_or_else(|| "Not connected".to_string())
}

fn do_connect(state: &mut WorkerState) {
    teardown(state);

    let Some(account) = state.account.clone() else {
        bridge::emit(MailEvent::NotConfigured);
        return;
    };
    if !account.is_configured() {
        bridge::emit(MailEvent::NotConfigured);
        return;
    }

    let client = match ImapClient::connect(&account) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "connect".into(),
                message: e.to_string(),
            });
            return;
        }
    };
    let client_arc = Arc::new(Mutex::new(client));

    let (folders, smart) = {
        let mut c = client_arc.lock().unwrap_or_else(|e| e.into_inner());
        let folders = match c.list_folders() {
            Ok(f) => f,
            Err(e) => {
                bridge::emit(MailEvent::Error {
                    context: "list_folders".into(),
                    message: e.to_string(),
                });
                return;
            }
        };
        let smart = match c.count_smart_mailboxes(&account.rules) {
            Ok(s) => s,
            Err(e) => {
                bridge::emit(MailEvent::Error {
                    context: "smart_counts".into(),
                    message: e.to_string(),
                });
                return;
            }
        };
        (folders, smart)
    };

    let mut folders = folders;
    if let Some(inbox) = folders.iter_mut().find(|f| f.name == "INBOX") {
        inbox.total = inbox.total.saturating_sub(smart.inbox_total_deduction);
        inbox.unread = inbox.unread.saturating_sub(smart.inbox_unread_deduction);
    }

    let from_addresses = {
        let addrs = wicket::fetch_from_addresses(
            &account.imap_host,
            &account.username,
            &account.password,
        );
        if addrs.is_empty() {
            vec![account.email.clone()]
        } else {
            addrs
        }
    };

    let move_rules: Vec<_> = account
        .rules
        .iter()
        .filter(|r| r.action == "move")
        .cloned()
        .collect();
    *state.idle_move_rules.lock().unwrap_or_else(|e| e.into_inner()) = move_rules;

    let shared_rules = Arc::clone(&state.idle_move_rules);
    let idle = start_idle(account.clone(), move |change, idle_client| {
        match change {
            IdleChange::Arrived { new_count } => {
                let rules = shared_rules.lock().unwrap_or_else(|e| e.into_inner());
                if !rules.is_empty() {
                    let _ = apply_move_rules_on_idle(idle_client, &rules, new_count);
                }
                // Always refresh UI — even if every arrival was auto-moved away.
                bridge::emit(MailEvent::NewMail);
            }
            IdleChange::Removed { gone } => {
                tracing::debug!(gone, "IDLE: remote deletes/expunge — refreshing UI");
                bridge::emit(MailEvent::NewMail);
            }
            IdleChange::Touched => {
                // Flag-only / keepalive: no EXISTS change. Periodic PollRefresh
                // covers stragglers without thrashing on every IDLE nudge.
            }
        }
    });

    // Keepalive NOOP every 240s on a std thread.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let ka_client = Arc::clone(&client_arc);
    let keepalive = std::thread::Builder::new()
        .name("sola-mail-keepalive".into())
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

    state.client = Some(client_arc);
    state.idle = Some(idle);
    state.keepalive = keepalive;
    state.keepalive_stop = Some(stop);

    bridge::emit(MailEvent::Connected {
        folders,
        smart_counts: smart.folders,
        from_addresses,
        rules: account.rules.clone(),
    });
}

fn do_list_folders(state: &WorkerState) {
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "list_folders".into(),
                message: e,
            });
            return;
        }
    };
    let rules = state
        .account
        .as_ref()
        .map(|a| a.rules.clone())
        .unwrap_or_default();
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    let mut folders = match c.list_folders() {
        Ok(f) => f,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "list_folders".into(),
                message: e.to_string(),
            });
            return;
        }
    };
    let smart = match c.count_smart_mailboxes(&rules) {
        Ok(s) => s,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "smart_counts".into(),
                message: e.to_string(),
            });
            return;
        }
    };
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
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "list_messages".into(),
                message: e,
            });
            return;
        }
    };
    let rules = state
        .account
        .as_ref()
        .map(|a| a.rules.clone())
        .unwrap_or_default();
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    let result = if folder == "INBOX" {
        c.list_inbox_filtered(&rules, offset, limit)
    } else if let Some(name) = folder.strip_prefix("smart:") {
        c.list_smart_mailbox(name, &rules, offset, limit)
    } else {
        c.list_messages(&folder, offset, limit)
    };
    match result {
        Ok((messages, total)) => bridge::emit(MailEvent::Messages {
            folder,
            messages,
            total,
            offset,
        }),
        Err(e) => bridge::emit(MailEvent::Error {
            context: "list_messages".into(),
            message: e.to_string(),
        }),
    }
}

fn do_search(state: &WorkerState, query: String) {
    if query.trim().is_empty() {
        bridge::emit(MailEvent::Error {
            context: "search".into(),
            message: "empty query".into(),
        });
        return;
    }
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "search".into(),
                message: e,
            });
            return;
        }
    };
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    match c.search_all_folders(&query) {
        Ok((messages, total)) => bridge::emit(MailEvent::SearchResults { messages, total }),
        Err(e) => bridge::emit(MailEvent::Error {
            context: "search".into(),
            message: e.to_string(),
        }),
    }
}

fn do_fetch_body(state: &WorkerState, folder: String, uid: u32) {
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "fetch_body".into(),
                message: e,
            });
            return;
        }
    };
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    match c.fetch_body(&folder, uid) {
        Ok(body) => bridge::emit(MailEvent::Body(body)),
        Err(e) => bridge::emit(MailEvent::Error {
            context: "fetch_body".into(),
            message: e.to_string(),
        }),
    }
}

fn do_mark_read(state: &WorkerState, folder: String, uid: u32) {
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "mark_read".into(),
                message: e,
            });
            return;
        }
    };
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = c.mark_read(&folder, uid) {
        bridge::emit(MailEvent::Error {
            context: "mark_read".into(),
            message: e.to_string(),
        });
    }
}

fn do_move(state: &WorkerState, folder: String, uid: u32, dest: String) {
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "move".into(),
                message: e,
            });
            return;
        }
    };
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    match c.move_message(&folder, uid, &dest) {
        Ok(()) => bridge::emit(MailEvent::Moved),
        Err(e) => bridge::emit(MailEvent::Error {
            context: "move".into(),
            message: e.to_string(),
        }),
    }
}

fn do_empty(state: &WorkerState, folder: String) {
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "empty_folder".into(),
                message: e,
            });
            return;
        }
    };
    let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
    match c.empty_folder(&folder) {
        Ok(()) => bridge::emit(MailEvent::Emptied),
        Err(e) => bridge::emit(MailEvent::Error {
            context: "empty_folder".into(),
            message: e.to_string(),
        }),
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
) {
    let Some(account) = state.account.clone() else {
        bridge::emit(MailEvent::Error {
            context: "send".into(),
            message: "Not connected".into(),
        });
        return;
    };
    let client = match get_client(state) {
        Ok(c) => c,
        Err(e) => {
            bridge::emit(MailEvent::Error {
                context: "send".into(),
                message: e,
            });
            return;
        }
    };
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
    ) {
        Ok(raw) => {
            let mut c = client.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = c.append_to_sent("Sent", &raw) {
                warn!("Failed to save to Sent folder: {e}");
            }
            bridge::emit(MailEvent::Sent);
        }
        Err(e) => bridge::emit(MailEvent::Error {
            context: "send".into(),
            message: e.to_string(),
        }),
    }
}

fn apply_move_rules_on_idle(
    client: &mut ImapClient,
    rules: &[sola_bus::topics::MailRule],
    new_count: u32,
) -> u32 {
    let messages = match client.list_messages("INBOX", 0, new_count.max(20)) {
        Ok((msgs, _)) => msgs,
        Err(e) => {
            warn!("IDLE move rules: failed to list messages: {e}");
            return new_count;
        }
    };
    let mut moved = 0u32;
    for msg in messages.iter().take(new_count as usize) {
        for rule in rules {
            if let Some(dest) = &rule.dest {
                if rule_matches(rule, &msg.from, &msg.subject, &msg.to) {
                    if let Err(e) = client.move_message("INBOX", msg.uid, dest) {
                        warn!("IDLE move: uid {} to {dest}: {e}", msg.uid);
                    } else {
                        moved += 1;
                    }
                    break;
                }
            }
        }
    }
    new_count.saturating_sub(moved)
}

// Silence unused import warning if MailConfig only used via re-export paths.
#[allow(dead_code)]
fn _cfg_ty(_: MailConfig) {}
