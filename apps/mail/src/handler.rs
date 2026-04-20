use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::MailConfig;
use crate::idle;
use crate::imap::ImapClient;
use crate::rules::MailRule;
use crate::state::MailState;
use crate::wicket;

pub struct MailHandler {
    pub state: Arc<MailState>,
}

#[async_trait::async_trait]
impl sola_app::AppHandler for MailHandler {
    async fn dispatch(&self, cmd: &str, args: &Value) -> Value {
        let result = match cmd {
            "mail_connect" => cmd_mail_connect(&self.state).await,
            "mail_test_connection" => cmd_mail_test_connection(args).await,
            "mail_list_folders" => cmd_mail_list_folders(&self.state).await,
            "mail_list_messages" => cmd_mail_list_messages(&self.state, args).await,
            "mail_search" => cmd_mail_search(&self.state, args).await,
            "mail_fetch_body" => cmd_mail_fetch_body(&self.state, args).await,
            "mail_send" => cmd_mail_send(&self.state, args).await,
            "mail_move" => cmd_mail_move(&self.state, args).await,
            "mail_mark_read" => cmd_mail_mark_read(&self.state, args).await,
            "mail_empty_folder" => cmd_mail_empty_folder(&self.state, args).await,
            "apply_rules" => cmd_apply_rules(&self.state).await,
            "open_url" => return json!({ "error": "not wired" }),
            _ => Err(format!("unknown command: {cmd}")),
        };

        match result {
            Ok(v) => v,
            Err(e) => json!({ "error": e }),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get a clone of the client Arc, returning an error if not connected.
async fn get_client(
    state: &Arc<MailState>,
) -> Result<Arc<std::sync::Mutex<ImapClient>>, String> {
    state
        .client
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not connected".to_string())
}

/// Emit a JSON event through the state's event channel (best-effort).
fn emit_event(state: &MailState, payload: Value) {
    let _ = state.event_tx.send(payload.to_string());
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Test IMAP credentials without storing them.
async fn cmd_mail_test_connection(args: &Value) -> Result<Value, String> {
    let imap_host = args["imap_host"].as_str().unwrap_or("").to_string();
    let imap_port = args["imap_port"].as_u64().unwrap_or(993) as u16;
    let username = args["username"].as_str().unwrap_or("").to_string();
    let password = args["password"].as_str().unwrap_or("").to_string();

    if imap_host.is_empty() || username.is_empty() || password.is_empty() {
        return Ok(json!({
            "success": false,
            "error": "IMAP host, username, and password are required",
        }));
    }

    let test_config = MailConfig {
        email: String::new(),
        imap_host,
        imap_port,
        smtp_host: String::new(),
        smtp_port: 587,
        username,
        password,
        rules: Vec::new(),
    };

    let result = tokio::task::spawn_blocking(move || ImapClient::connect(&test_config))
        .await
        .map_err(|e| format!("join error: {e}"))?;

    match result {
        Ok(_) => Ok(json!({ "success": true })),
        Err(e) => Ok(json!({ "success": false, "error": e.to_string() })),
    }
}

/// Connect to IMAP, start keepalive + IDLE, return folders/from_addresses/rules.
async fn cmd_mail_connect(state: &Arc<MailState>) -> Result<Value, String> {
    // Abort any prior keepalive + IDLE on reconnect
    if let Some(handle) = state.keepalive_abort.lock().await.take() {
        handle.abort();
    }
    if let Some(handle) = state.idle_handle.lock().await.take() {
        tokio::task::spawn_blocking(move || handle.stop());
    }

    let config = MailConfig::load().map_err(|e| e.to_string())?;

    if config.imap_host.is_empty() {
        return Err("No mail config — configure in Settings".to_string());
    }

    // Connect to IMAP (blocking call)
    let config_clone = config.clone();
    let client = tokio::task::spawn_blocking(move || {
        ImapClient::connect(&config_clone).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("join error: {e}"))??;

    let client_arc = Arc::new(std::sync::Mutex::new(client));

    // Store client + config
    *state.client.lock().await = Some(Arc::clone(&client_arc));
    *state.config.write().await = Some(config.clone());

    // Fetch folders + smart counts
    let config_for_folders = config.clone();
    let client_arc_for_folders = Arc::clone(&client_arc);
    let (mut folders, smart_folders) = tokio::task::spawn_blocking(move || {
        let mut client = client_arc_for_folders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let folders = client.list_folders().map_err(|e| e.to_string())?;
        let smart = client
            .count_smart_mailboxes(&config_for_folders.rules)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((folders, smart))
    })
    .await
    .map_err(|e| format!("join error: {e}"))??;

    if let Some(inbox) = folders.iter_mut().find(|f| f.name == "INBOX") {
        inbox.total = inbox.total.saturating_sub(smart_folders.inbox_total_deduction);
        inbox.unread = inbox
            .unread
            .saturating_sub(smart_folders.inbox_unread_deduction);
    }

    // Fetch from-addresses via wicket, fallback to config.email
    let host = config.imap_host.clone();
    let user = config.username.clone();
    let pass = config.password.clone();
    let email_fallback = config.email.clone();
    let from_addresses = tokio::task::spawn_blocking(move || {
        let addrs = wicket::fetch_from_addresses(&host, &user, &pass);
        if addrs.is_empty() { vec![email_fallback] } else { addrs }
    })
    .await
    .unwrap_or_else(|_| vec![config.email.clone()]);

    // Store move rules for IDLE
    let initial_move_rules: Vec<MailRule> = config
        .rules
        .iter()
        .filter(|r| r.action == "move")
        .cloned()
        .collect();
    *state
        .idle_move_rules
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = initial_move_rules;

    // Start IDLE watcher
    let shared_rules = Arc::clone(&state.idle_move_rules);
    let event_tx = state.event_tx.clone();
    let idle_handle = idle::start_idle(config.clone(), move |new_count, idle_client| {
        let rules = shared_rules.lock().unwrap_or_else(|e| e.into_inner());
        let remaining = if !rules.is_empty() {
            apply_move_rules_on_idle(idle_client, &rules, new_count)
        } else {
            new_count
        };
        if remaining > 0 {
            let _ = event_tx.send(json!({ "event": "mail:new" }).to_string());
        }
    });
    *state.idle_handle.lock().await = Some(idle_handle);

    // Start keepalive task (NOOP every 240s)
    let client_arc_for_keepalive = Arc::clone(&client_arc);
    let keepalive = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(240));
        interval.tick().await; // skip first immediate tick
        loop {
            interval.tick().await;
            let arc = Arc::clone(&client_arc_for_keepalive);
            let _ = tokio::task::spawn_blocking(move || {
                let mut client = arc.lock().unwrap_or_else(|e| e.into_inner());
                if let Err(e) = client.noop() {
                    tracing::warn!("Keepalive NOOP failed: {e}");
                }
            })
            .await;
        }
    });
    *state.keepalive_abort.lock().await = Some(keepalive);

    Ok(json!({
        "folders": folders,
        "smart_counts": smart_folders.folders,
        "from_addresses": from_addresses,
        "rules": config.rules,
    }))
}

/// List IMAP folders with counts.
async fn cmd_mail_list_folders(state: &Arc<MailState>) -> Result<Value, String> {
    let client_arc = get_client(state).await?;
    let rules = state
        .config
        .read()
        .await
        .as_ref()
        .map(|c| c.rules.clone())
        .unwrap_or_default();

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        let mut folders = client.list_folders().map_err(|e| e.to_string())?;
        let smart = client
            .count_smart_mailboxes(&rules)
            .map_err(|e| e.to_string())?;
        if let Some(inbox) = folders.iter_mut().find(|f| f.name == "INBOX") {
            inbox.total = inbox.total.saturating_sub(smart.inbox_total_deduction);
            inbox.unread = inbox.unread.saturating_sub(smart.inbox_unread_deduction);
        }
        Ok(json!({
            "folders": folders,
            "smart_counts": smart.folders,
        }))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// List messages in a folder.
async fn cmd_mail_list_messages(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let folder = args["folder"].as_str().unwrap_or("INBOX").to_string();
    let offset = args["offset"].as_u64().unwrap_or(0) as u32;
    let limit = args["limit"].as_u64().unwrap_or(50) as u32;
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        let (messages, total) = client
            .list_messages(&folder, offset, limit)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "messages": messages, "total": total }))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Search across folders.
async fn cmd_mail_search(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let query = args["query"].as_str().unwrap_or("").to_string();
    if query.is_empty() {
        return Err("mail_search: empty query".to_string());
    }
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        let (messages, total) = client
            .search_all_folders(&query)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "messages": messages, "total": total }))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Fetch full message body.
async fn cmd_mail_fetch_body(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let folder = args["folder"].as_str().unwrap_or("INBOX").to_string();
    let uid = args["uid"].as_u64().unwrap_or(0) as u32;
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        let body = client.fetch_body(&folder, uid).map_err(|e| e.to_string())?;
        serde_json::to_value(&body).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Mark message as read.
async fn cmd_mail_mark_read(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let folder = args["folder"].as_str().unwrap_or("INBOX").to_string();
    let uid = args["uid"].as_u64().unwrap_or(0) as u32;
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        client.mark_read(&folder, uid).map_err(|e| e.to_string())?;
        Ok(json!("ok"))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Move message between folders.
async fn cmd_mail_move(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let folder = args["folder"].as_str().unwrap_or("INBOX").to_string();
    let uid = args["uid"].as_u64().unwrap_or(0) as u32;
    let dest = args["dest"].as_str().unwrap_or("Trash").to_string();
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        client
            .move_message(&folder, uid, &dest)
            .map_err(|e| e.to_string())?;
        Ok(json!("ok"))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Empty a folder.
async fn cmd_mail_empty_folder(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let folder = args["folder"].as_str().unwrap_or("");
    if folder.is_empty() {
        return Err("mail_empty_folder: folder required".to_string());
    }
    let folder = folder.to_string();
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        client.empty_folder(&folder).map_err(|e| e.to_string())?;
        Ok(json!("ok"))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Send via SMTP + append to Sent.
async fn cmd_mail_send(state: &Arc<MailState>, args: &Value) -> Result<Value, String> {
    let from = args["from"].as_str().unwrap_or("").to_string();
    let to = args["to"].as_str().unwrap_or("").to_string();
    let cc = args["cc"].as_str().map(|s| s.to_string());
    let subject = args["subject"].as_str().unwrap_or("").to_string();
    let body_text = args["body"].as_str().unwrap_or("").to_string();
    let in_reply_to = args["in_reply_to"].as_str().map(|s| s.to_string());

    let config = state.config.read().await.clone().ok_or("Not connected")?;
    let client_arc = get_client(state).await?;

    tokio::task::spawn_blocking(move || {
        let raw_message = crate::sender::send_mail(
            &config,
            &from,
            &to,
            cc.as_deref(),
            &subject,
            &body_text,
            in_reply_to.as_deref(),
        )
        .map_err(|e| e.to_string())?;

        // Append to Sent folder (best-effort)
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        if let Err(e) = client.append_to_sent("Sent", &raw_message) {
            tracing::warn!("Failed to save to Sent folder: {e}");
        }
        Ok(json!("ok"))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Apply move rules to existing INBOX messages.
async fn cmd_apply_rules(state: &Arc<MailState>) -> Result<Value, String> {
    let config = state.config.read().await.clone().ok_or("Not connected")?;

    let move_rules: Vec<_> = config
        .rules
        .into_iter()
        .filter(|r| r.action == "move")
        .collect();

    if move_rules.is_empty() {
        return Ok(json!({ "moved": 0 }));
    }

    let client_arc = get_client(state).await?;

    let moved = tokio::task::spawn_blocking(move || {
        let mut client = client_arc.lock().unwrap_or_else(|e| {
            tracing::warn!("IMAP mutex was poisoned, recovering");
            e.into_inner()
        });
        let (messages, _) = client
            .list_messages("INBOX", 0, 500)
            .map_err(|e| e.to_string())?;
        let mut moved = 0u32;
        for msg in &messages {
            for rule in &move_rules {
                if let Some(dest) = &rule.dest {
                    if crate::rules::rule_matches(rule, &msg.from, &msg.subject, &msg.to) {
                        if let Err(e) = client.move_message("INBOX", msg.uid, dest) {
                            tracing::warn!("apply_rules: move uid {} to {dest}: {e}", msg.uid);
                        } else {
                            moved += 1;
                        }
                        break;
                    }
                }
            }
        }
        Ok::<_, String>(moved)
    })
    .await
    .map_err(|e| format!("join error: {e}"))??;

    if moved > 0 {
        emit_event(state, json!({ "event": "mail:new" }));
    }

    Ok(json!({ "moved": moved }))
}

// ---------------------------------------------------------------------------
// IDLE helper (mirrors mail_bridge::apply_move_rules_on_idle from Cogsworth)
// ---------------------------------------------------------------------------

/// Apply move rules to recent INBOX messages during IDLE.
/// Returns count of messages that were NOT moved (remaining new).
fn apply_move_rules_on_idle(
    client: &mut ImapClient,
    rules: &[MailRule],
    new_count: u32,
) -> u32 {
    let messages = match client.list_messages("INBOX", 0, new_count.max(20)) {
        Ok((msgs, _)) => msgs,
        Err(e) => {
            tracing::warn!("IDLE move rules: failed to list messages: {e}");
            return new_count;
        }
    };

    let mut moved = 0u32;
    for msg in messages.iter().take(new_count as usize) {
        for rule in rules {
            if let Some(dest) = &rule.dest {
                if crate::rules::rule_matches(rule, &msg.from, &msg.subject, &msg.to) {
                    if let Err(e) = client.move_message("INBOX", msg.uid, dest) {
                        tracing::warn!("IDLE move: uid {} to {dest}: {e}", msg.uid);
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
