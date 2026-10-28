//! In-process bot manager: the bots in `~/Bots`, their dialogs, and the
//! AI each runs on (Grok's ACP child, a `claude -p` per turn, Gab's API).

use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};

use crate::acp::AcpSession;
use crate::ai::Ai;
use crate::catalog::{self, BotRecord, Catalog};
use crate::paths;
use crate::sse;
use crate::{claude, foundation, gab};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BotStatus {
    Idle,
    Working,
    Error,
}

impl Default for BotStatus {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogTurn {
    pub role: String,
    pub text: String,
}

#[derive(Clone)]
struct Live {
    status: BotStatus,
    error: Option<String>,
    dialog: Vec<DialogTurn>,
    acp: Arc<Mutex<Option<Arc<AcpSession>>>>,
    cancel: Arc<AtomicBool>,
    last_flush: Instant,
}

fn empty_live() -> Live {
    Live {
        status: BotStatus::Idle,
        error: None,
        dialog: Vec::new(),
        acp: Arc::new(Mutex::new(None)),
        cancel: Arc::new(AtomicBool::new(false)),
        last_flush: Instant::now(),
    }
}

pub struct Host {
    inner: Mutex<Inner>,
    list_cache: Mutex<Option<(Instant, serde_json::Value)>>,
    subs: Mutex<Vec<mpsc::Sender<Vec<u8>>>>,
}

struct Inner {
    catalog: Catalog,
    live: HashMap<String, Live>,
}

impl Host {
    pub fn start() -> Arc<Self> {
        let catalog = Catalog::load();
        let mut live = HashMap::new();
        for rec in &catalog.bots {
            live.insert(rec.id.clone(), live_for(rec));
        }
        Arc::new(Self {
            inner: Mutex::new(Inner { catalog, live }),
            list_cache: Mutex::new(None),
            subs: Mutex::new(Vec::new()),
        })
    }

    pub fn list_json(&self) -> serde_json::Value {
        if let Ok(c) = self.list_cache.lock() {
            if let Some((t, v)) = c.as_ref() {
                if t.elapsed() < Duration::from_millis(400) {
                    return v.clone();
                }
            }
        }
        let v = self.list_json_fresh();
        if let Ok(mut c) = self.list_cache.lock() {
            *c = Some((Instant::now(), v.clone()));
        }
        v
    }

    /// Pick up bot directories made or removed by hand.
    fn rescan(&self) {
        let mut g = self.inner.lock().unwrap();
        let found = g.catalog.rescan();
        for id in &found.removed {
            g.live.remove(id);
            info!(%id, "bot directory gone");
        }
        for rec in found.added {
            info!(slug = %rec.slug, "bot directory found");
            g.live.insert(rec.id.clone(), live_for(&rec));
        }
    }

    fn list_json_fresh(&self) -> serde_json::Value {
        self.rescan();
        let g = self.inner.lock().unwrap();
        let bots: Vec<serde_json::Value> = g
            .catalog
            .bots
            .iter()
            .map(|r| {
                let st = g.live.get(&r.id);
                json!({
                    "id": r.id,
                    "name": r.name,
                    "slug": r.slug,
                    "ai": r.ai.map(Ai::id),
                    "ai_label": r.ai.map(Ai::label),
                    "model": r.model,
                    "status": match st.map(|s| &s.status) {
                        Some(BotStatus::Working) => "working",
                        Some(BotStatus::Error) => "error",
                        _ => "idle",
                    },
                    "error": st.and_then(|s| s.error.clone()),
                    "home": paths::bot_home(&r.slug).display().to_string(),
                })
            })
            .collect();
        json!({ "bots": bots })
    }

    pub fn poll_json(&self, key: Option<&str>) -> serde_json::Value {
        let list = self.list_json();
        let transcript = key.and_then(|k| self.transcript_json(k).ok());
        json!({
            "bots": list.get("bots").cloned().unwrap_or(json!([])),
            "transcript": transcript,
        })
    }

    /// Snapshot then live frames. Drop the receiver to disconnect.
    pub fn subscribe(&self) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        let snap = self.poll_json(None);
        let _ = tx.send(sse::frame("snapshot", &snap));
        self.subs.lock().unwrap().push(tx);
        rx
    }

    fn fanout(&self, bytes: Vec<u8>) {
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|tx| tx.send(bytes.clone()).is_ok());
    }

    fn emit_list(&self) {
        self.invalidate_list_cache();
        let v = self.list_json();
        self.fanout(sse::frame("bots", &v));
    }

    fn emit_removed(&self, id: &str) {
        self.fanout(sse::frame("removed", &json!({ "id": id })));
    }

    fn emit_transcript(&self, id: &str) {
        if let Ok(v) = self.transcript_json(id) {
            self.fanout(sse::frame("transcript", &v));
        }
    }

    fn emit_delta(&self, id: &str, text: &str) {
        self.fanout(sse::frame("delta", &json!({ "id": id, "text": text })));
    }

    pub fn transcript_json(&self, key: &str) -> Result<serde_json::Value, String> {
        let g = self.inner.lock().unwrap();
        let rec = g
            .catalog
            .find(key)
            .ok_or_else(|| format!("unknown bot {key}"))?;
        let live = g
            .live
            .get(&rec.id)
            .ok_or_else(|| format!("unknown bot {key}"))?;
        Ok(json!({
            "id": rec.id,
            "name": rec.name,
            "ai": rec.ai.map(Ai::id),
            "status": match live.status {
                BotStatus::Working => "working",
                BotStatus::Error => "error",
                BotStatus::Idle => "idle",
            },
            "error": live.error,
            "turns": live.dialog,
        }))
    }

    /// Make a bot on `ai`: `~/Bots/<slug>/`, its `bot.json`, its seed
    /// files, and a first turn that introduces it.
    pub fn create(self: &Arc<Self>, name: &str, ai: Ai) -> Result<serde_json::Value, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("name required".into());
        }
        let rec = {
            let mut g = self.inner.lock().unwrap();
            let slug = catalog::unique_slug(&g.catalog, &catalog::slugify(name));
            let now = now_rfc();
            let rec = BotRecord {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.to_string(),
                slug: slug.clone(),
                ai: Some(ai),
                model: String::new(),
                session: None,
                created: now.clone(),
                last_used: now,
            };
            foundation::seed_home(&paths::bot_home(&slug), name, ai).map_err(|e| e.to_string())?;
            catalog::save(&rec).map_err(|e| e.to_string())?;
            g.catalog.insert(rec.clone());
            g.live.insert(rec.id.clone(), empty_live());
            info!(%slug, ai = ai.id(), "bot created");
            rec
        };
        Ok(self.introduce(&rec))
    }

    /// Choose the AI of a bot that has none (a directory made by hand).
    /// It is fixed after: a bot does not change AI.
    pub fn setup(self: &Arc<Self>, key: &str, ai: Ai) -> Result<serde_json::Value, String> {
        let (old_id, rec) = {
            let mut g = self.inner.lock().unwrap();
            let old = g
                .catalog
                .find(key)
                .cloned()
                .ok_or_else(|| format!("unknown bot {key}"))?;
            if let Some(had) = old.ai {
                return Err(format!("{} already runs on {}", old.name, had.label()));
            }
            let now = now_rfc();
            let rec = BotRecord {
                id: uuid::Uuid::new_v4().to_string(),
                ai: Some(ai),
                created: now.clone(),
                last_used: now,
                ..old.clone()
            };
            let home = paths::bot_home(&rec.slug);
            foundation::seed_home(&home, &rec.name, ai).map_err(|e| e.to_string())?;
            catalog::save(&rec).map_err(|e| e.to_string())?;
            if let Some(slot) = g.catalog.find_mut(&old.id) {
                *slot = rec.clone();
            }
            let live = g.live.remove(&old.id).unwrap_or_else(empty_live);
            g.live.insert(rec.id.clone(), live);
            info!(slug = %rec.slug, ai = ai.id(), "bot set up");
            (old.id, rec)
        };
        self.emit_removed(&old_id);
        Ok(self.introduce(&rec))
    }

    /// Announce a new bot and run its first, hidden turn.
    fn introduce(self: &Arc<Self>, rec: &BotRecord) -> serde_json::Value {
        self.invalidate_list_cache();
        self.emit_list();
        let home = paths::bot_home(&rec.slug);
        if let Some(ai) = rec.ai {
            let intro = foundation::intro_prompt(&rec.name, &rec.slug, &home, ai);
            let _ = self.prompt(&rec.id, &intro, false);
        }
        json!({
            "id": rec.id,
            "name": rec.name,
            "slug": rec.slug,
            "ai": rec.ai.map(Ai::id),
            "home": home.display().to_string(),
        })
    }

    pub fn rm(&self, key: &str) -> Result<serde_json::Value, String> {
        let (id, slug, session_id, acp) = {
            let mut g = self.inner.lock().unwrap();
            let rec = g
                .catalog
                .find(key)
                .cloned()
                .ok_or_else(|| format!("unknown bot {key}"))?;
            // The directory is the bot: it goes under the lock, so a
            // rescan cannot find it again between the two.
            let home = paths::bot_home(&rec.slug);
            if home.exists() {
                fs::remove_dir_all(&home).map_err(|e| e.to_string())?;
            }
            let live = g.live.remove(&rec.id);
            g.catalog.bots.retain(|b| b.id != rec.id);
            // Grok keeps its sessions apart from the home; Claude Code's
            // stay in its own history.
            let grok_session = rec.session.clone().filter(|_| rec.ai == Some(Ai::Grok));
            (rec.id, rec.slug.clone(), grok_session, live.map(|l| l.acp))
        };
        // Stop the ACP child before returning.
        if let Some(slot) = acp {
            if let Ok(mut g) = slot.lock() {
                *g = None;
            }
        }
        self.invalidate_list_cache();
        self.emit_removed(&id);
        self.emit_list();
        if let Some(sid) = session_id {
            thread::Builder::new()
                .name(format!("bot-rm-session-{slug}"))
                .spawn(move || {
                    let _ = std::process::Command::new(paths::grok_bin())
                        .args(["sessions", "delete", &sid])
                        .status();
                    info!(%slug, "bot removed");
                })
                .ok();
        } else {
            info!(%slug, "bot removed");
        }
        Ok(json!({ "ok": true, "id": id }))
    }

    pub fn send(self: &Arc<Self>, key: &str, text: &str) -> Result<serde_json::Value, String> {
        self.prompt(key, text, true)
    }

    fn prompt(
        self: &Arc<Self>,
        key: &str,
        text: &str,
        show_user: bool,
    ) -> Result<serde_json::Value, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("text required".into());
        }
        let (rec, ai, history, acp_slot, cancel) = {
            let mut g = self.inner.lock().unwrap();
            let rec = g
                .catalog
                .find(key)
                .cloned()
                .ok_or_else(|| format!("unknown bot {key}"))?;
            let ai = rec
                .ai
                .ok_or_else(|| format!("choose an AI for {} first", rec.name))?;
            let live = g
                .live
                .get_mut(&rec.id)
                .ok_or_else(|| format!("unknown bot {key}"))?;
            if live.status == BotStatus::Working {
                return Err("bot is already working".into());
            }
            let history = live.dialog.clone();
            live.status = BotStatus::Working;
            live.error = None;
            live.cancel.store(false, Ordering::SeqCst);
            if show_user {
                live.dialog.push(DialogTurn {
                    role: "user".into(),
                    text: text.to_string(),
                });
            }
            live.dialog.push(DialogTurn {
                role: "assistant".into(),
                text: String::new(),
            });
            save_dialog(&rec.slug, &live.dialog);
            (
                rec.clone(),
                ai,
                history,
                Arc::clone(&live.acp),
                Arc::clone(&live.cancel),
            )
        };
        let id = rec.id.clone();
        let slug = rec.slug.clone();
        let home = paths::bot_home(&slug);
        self.invalidate_list_cache();
        self.emit_list();
        self.emit_transcript(&id);

        let host = Arc::clone(self);
        let mut prompt = text.to_string();
        if ai.has_tools() && foundation::refresh_agents(&home) {
            prompt = format!("{}\n\n{prompt}", foundation::ADOPT);
            foundation::stamp(&home);
        }
        let reply_id = id.clone();
        let thread_id = id.clone();
        let thread_slug = slug.clone();
        let spawn = thread::Builder::new()
            .name(format!("bot-{slug}"))
            .spawn(move || {
                let turn = TurnCtx {
                    host: &host,
                    id: &thread_id,
                    slug: &thread_slug,
                    home: &home,
                    cancel: &cancel,
                };
                let result = match ai {
                    Ai::Grok => run_grok(&turn, &rec.model, &acp_slot, &prompt),
                    Ai::Claude => run_claude(&turn, &rec.model, &prompt),
                    Ai::Gab => run_gab(&turn, &rec, &history, &prompt),
                };
                if result.as_ref().is_err_and(|e| acp_retryable(e)) {
                    if let Ok(mut slot) = acp_slot.lock() {
                        *slot = None;
                    }
                }
                let mut g = host.inner.lock().unwrap();
                if let Some(live) = g.live.get_mut(&thread_id) {
                    match result {
                        Ok(()) => {
                            live.status = BotStatus::Idle;
                            live.error = None;
                        }
                        Err(e) if e.contains("cancelled") => {
                            live.status = BotStatus::Idle;
                            live.error = None;
                        }
                        Err(e) => {
                            live.status = BotStatus::Error;
                            live.error = Some(e);
                        }
                    }
                    save_dialog(&thread_slug, &live.dialog);
                }
                if let Some(rec) = g.catalog.find_mut(&thread_id) {
                    rec.last_used = now_rfc();
                    let _ = catalog::save(rec);
                }
                drop(g);
                host.invalidate_list_cache();
                host.emit_transcript(&thread_id);
                host.emit_list();
            });
        if let Err(e) = spawn {
            self.revert_failed_spawn(&id, &slug, show_user, e.to_string());
            return Err(e.to_string());
        }

        Ok(json!({ "ok": true, "id": reply_id, "status": "working" }))
    }

    fn revert_failed_spawn(&self, id: &str, slug: &str, show_user: bool, err: String) {
        let mut g = self.inner.lock().unwrap();
        if let Some(live) = g.live.get_mut(id) {
            live.status = BotStatus::Idle;
            live.error = Some(err);
            let _ = live.dialog.pop();
            if show_user {
                let _ = live.dialog.pop();
            }
            save_dialog(slug, &live.dialog);
        }
        drop(g);
        self.invalidate_list_cache();
        self.emit_transcript(id);
        self.emit_list();
    }

    fn invalidate_list_cache(&self) {
        if let Ok(mut c) = self.list_cache.lock() {
            *c = None;
        }
    }

    pub fn cancel(&self, key: &str) -> Result<serde_json::Value, String> {
        let g = self.inner.lock().unwrap();
        let rec = g
            .catalog
            .find(key)
            .cloned()
            .ok_or_else(|| format!("unknown bot {key}"))?;
        let live = g
            .live
            .get(&rec.id)
            .ok_or_else(|| format!("unknown bot {key}"))?;
        live.cancel.store(true, Ordering::SeqCst);
        // Grok hears a cancel; the other AIs' turns watch the flag.
        let sid = rec.session.clone().filter(|_| rec.ai == Some(Ai::Grok));
        if let (Some(sid), Ok(slot)) = (sid, live.acp.lock()) {
            if let Some(sess) = slot.as_ref() {
                sess.cancel(&sid);
            }
        }
        Ok(json!({ "ok": true, "id": rec.id, "status": "cancelling" }))
    }
}

fn acp_retryable(e: &str) -> bool {
    e.contains("disconnect")
        || e.contains("writer gone")
        || e.contains("timed out")
        || e.contains("spawn grok")
}

/// What every AI's turn needs to land its reply in the dialog.
struct TurnCtx<'a> {
    host: &'a Host,
    id: &'a str,
    slug: &'a str,
    home: &'a std::path::Path,
    cancel: &'a Arc<AtomicBool>,
}

impl TurnCtx<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Add `delta` to the reply being written, and pass it to listeners.
    fn append(&self, delta: &str) {
        {
            let mut g = self.host.inner.lock().unwrap();
            if let Some(live) = g.live.get_mut(self.id) {
                if let Some(last) = live.dialog.last_mut() {
                    if last.role == "assistant" {
                        last.text.push_str(delta);
                    }
                }
                if live.last_flush.elapsed() >= Duration::from_millis(250) {
                    save_dialog(self.slug, &live.dialog);
                    live.last_flush = Instant::now();
                }
            }
        }
        self.host.emit_delta(self.id, delta);
    }

    fn session(&self) -> Option<String> {
        let g = self.host.inner.lock().unwrap();
        g.catalog.find(self.id).and_then(|r| r.session.clone())
    }

    fn keep_session(&self, sid: &str) {
        persist_session(self.host, self.id, sid);
    }
}

/// A Grok turn: the bot's ACP child (spawned, or reused) and session.
fn run_grok(
    turn: &TurnCtx<'_>,
    model: &str,
    acp_slot: &Arc<Mutex<Option<Arc<AcpSession>>>>,
    prompt: &str,
) -> Result<(), String> {
    let slug = turn.slug;
    if turn.cancelled() {
        return Err("cancelled".into());
    }
    // Retry spawn/handshake only. Never re-send session/prompt — that
    // would duplicate tool calls if Grok died after accepting the turn.
    let mut tries = 0u32;
    let (sess, session_id) = loop {
        if turn.cancelled() {
            return Err("cancelled".into());
        }
        match ensure_session(turn, model, acp_slot) {
            Ok(pair) => break pair,
            Err(e) if acp_retryable(&e) && tries < 2 => {
                tries += 1;
                warn!(%slug, tries, "acp spawn/handshake failed ({e}); retry");
                if let Ok(mut g) = acp_slot.lock() {
                    *g = None;
                }
                thread::sleep(Duration::from_millis(400));
            }
            Err(e) => return Err(e),
        }
    };

    let cancel = Arc::clone(turn.cancel);
    let result = sess
        .prompt(
            &session_id,
            prompt,
            move || cancel.load(Ordering::SeqCst),
            |ev| {
                if let Some(delta) = ev.text_delta {
                    turn.append(&delta);
                }
                if let Some(err) = ev.error {
                    error!(%slug, "turn error: {err}");
                }
            },
        )
        .map_err(|e| e.to_string());
    match result {
        Ok(_) => Ok(()),
        Err(e) if e.contains("cancelled") => Err(e),
        Err(e) => {
            let partial = said_something(turn);
            warn!(%slug, %partial, "acp prompt ended ({e})");
            if let Ok(mut g) = acp_slot.lock() {
                *g = None;
            }
            if partial { Ok(()) } else { Err(e) }
        }
    }
}

/// A Claude turn: `claude -p` on the bot's session, started on its first
/// turn and again if Claude Code no longer has it.
fn run_claude(turn: &TurnCtx<'_>, model: &str, prompt: &str) -> Result<(), String> {
    let run = |session: &str, resume: bool| {
        let t = claude::Turn {
            home: turn.home,
            model,
            session,
            resume,
            prompt,
        };
        claude::run(&t, || turn.cancelled(), |text| turn.append(text))
    };
    let fresh = || {
        let sid = uuid::Uuid::new_v4().to_string();
        turn.keep_session(&sid);
        sid
    };
    match turn.session() {
        Some(sid) => match run(&sid, true) {
            Err(e) if claude::session_missing(&e) => {
                warn!(slug = %turn.slug, "claude session {sid} gone; a new one");
                run(&fresh(), false)
            }
            other => other,
        },
        None => run(&fresh(), false),
    }
}

/// A Gab AI turn: the dialog so far and the new message, over the API.
fn run_gab(
    turn: &TurnCtx<'_>,
    rec: &BotRecord,
    history: &[DialogTurn],
    prompt: &str,
) -> Result<(), String> {
    let system = foundation::chat_system(&rec.name, turn.home);
    let mut messages = vec![gab::Message {
        role: "system",
        content: &system,
    }];
    for t in history {
        if !t.text.trim().is_empty() && (t.role == "user" || t.role == "assistant") {
            messages.push(gab::Message {
                role: &t.role,
                content: &t.text,
            });
        }
    }
    messages.push(gab::Message {
        role: "user",
        content: prompt,
    });
    gab::run(&rec.model, &messages, || turn.cancelled(), |text| turn.append(text))
}

/// Whether the reply being written has any text yet.
fn said_something(turn: &TurnCtx<'_>) -> bool {
    let g = turn.host.inner.lock().unwrap();
    g.live
        .get(turn.id)
        .and_then(|l| l.dialog.last())
        .is_some_and(|t| t.role == "assistant" && !t.text.trim().is_empty())
}

fn ensure_session(
    turn: &TurnCtx<'_>,
    model: &str,
    acp_slot: &Arc<Mutex<Option<Arc<AcpSession>>>>,
) -> Result<(Arc<AcpSession>, String), String> {
    let (slug, home) = (turn.slug, turn.home);
    let sess = {
        let mut guard = acp_slot.lock().map_err(|e| e.to_string())?;
        if let Some(s) = guard.as_ref() {
            if s.child_exited() {
                *guard = None;
            }
        }
        let fresh = guard.is_none();
        if fresh {
            info!(%slug, "spawning grok acp");
            let spawned = AcpSession::spawn(home, model).map_err(|e| e.to_string())?;
            *guard = Some(Arc::new(spawned));
        }
        let sess = Arc::clone(guard.as_ref().unwrap());
        (sess, fresh)
    };
    let (sess, fresh) = sess;
    let existing = turn.session();
    let session_id = if !fresh {
        existing.ok_or_else(|| "missing session id".to_string())?
    } else if let Some(sid) = existing {
        match sess.load_session(&sid) {
            Ok(()) => sid,
            Err(e) => {
                warn!(%slug, "session/load failed ({e}); new session");
                let sid = sess.new_session(home).map_err(|e| e.to_string())?;
                turn.keep_session(&sid);
                sid
            }
        }
    } else {
        let sid = sess.new_session(home).map_err(|e| e.to_string())?;
        turn.keep_session(&sid);
        sid
    };
    Ok((sess, session_id))
}

fn persist_session(host: &Host, id: &str, sid: &str) {
    let mut g = host.inner.lock().unwrap();
    if let Some(rec) = g.catalog.find_mut(id) {
        rec.session = Some(sid.to_string());
        if let Err(e) = catalog::save(rec) {
            warn!(slug = %rec.slug, "session not saved: {e}");
        }
    }
}

/// A bot's live row: its dialog from disk, its home seeded when its AI
/// is known.
fn live_for(rec: &BotRecord) -> Live {
    if let Some(ai) = rec.ai {
        let _ = foundation::seed_home(&paths::bot_home(&rec.slug), &rec.name, ai);
    }
    let mut live = empty_live();
    live.dialog = load_dialog(&rec.slug);
    live
}

fn load_dialog(slug: &str) -> Vec<DialogTurn> {
    let path = paths::dialog_path(slug);
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_dialog(slug: &str, turns: &[DialogTurn]) {
    let path = paths::dialog_path(slug);
    let Some(dir) = path.parent() else {
        return;
    };
    let _ = fs::create_dir_all(dir);
    let Ok(bytes) = serde_json::to_vec_pretty(turns) else {
        return;
    };
    let tmp = dir.join("dialog.json.tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

fn now_rfc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_is_spawn_and_disconnect_not_unknown_bot() {
        assert!(acp_retryable("acp disconnected"));
        assert!(acp_retryable("initialize timed out"));
        assert!(acp_retryable("spawn grok (/opt/x) failed: No such file"));
        assert!(!acp_retryable("unknown bot suno"));
        assert!(!acp_retryable("bot is already working"));
    }
}
