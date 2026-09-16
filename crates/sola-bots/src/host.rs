//! In-process bot manager: catalog + dialog + ACP children.

use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};

use crate::acp::AcpSession;
use crate::catalog::{self, BotRecord, Catalog};
use crate::foundation;
use crate::paths;

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
            let home = paths::bot_home(&rec.slug);
            let _ = foundation::seed_home(&home, &rec.name);
            let dialog = load_dialog(&rec.slug);
            let mut live_row = empty_live();
            live_row.dialog = dialog;
            live.insert(rec.id.clone(), live_row);
        }
        Arc::new(Self {
            inner: Mutex::new(Inner { catalog, live }),
            list_cache: Mutex::new(None),
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

    fn list_json_fresh(&self) -> serde_json::Value {
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

    pub fn transcript_json(&self, key: &str) -> Result<serde_json::Value, String> {
        let g = self.inner.lock().unwrap();
        let rec = g.catalog.find(key).ok_or_else(|| format!("unknown bot {key}"))?;
        let live = g.live.get(&rec.id).ok_or_else(|| format!("unknown bot {key}"))?;
        Ok(json!({
            "id": rec.id,
            "name": rec.name,
            "status": match live.status {
                BotStatus::Working => "working",
                BotStatus::Error => "error",
                BotStatus::Idle => "idle",
            },
            "error": live.error,
            "turns": live.dialog,
        }))
    }

    pub fn create(self: &Arc<Self>, name: &str) -> Result<serde_json::Value, String> {
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
                vendor: "grok".into(),
                model: "grok-4.6".into(),
                grok_session_id: None,
                created: now.clone(),
                last_used: now,
            };
            let home = paths::bot_home(&slug);
            foundation::seed_home(&home, name).map_err(|e| e.to_string())?;
            g.catalog.bots.push(rec.clone());
            g.catalog.save().map_err(|e| e.to_string())?;
            g.live.insert(rec.id.clone(), empty_live());
            info!(%slug, "bot created");
            rec
        };
        let home = paths::bot_home(&rec.slug);
        let intro = foundation::intro_prompt(&rec.name, &rec.slug, &home);
        let _ = self.prompt(&rec.id, &intro, false);
        Ok(json!({
            "id": rec.id,
            "name": rec.name,
            "slug": rec.slug,
            "home": home.display().to_string(),
        }))
    }

    pub fn rm(&self, key: &str) -> Result<serde_json::Value, String> {
        let (id, slug, session_id, acp) = {
            let mut g = self.inner.lock().unwrap();
            let rec = g
                .catalog
                .find(key)
                .cloned()
                .ok_or_else(|| format!("unknown bot {key}"))?;
            let live = g.live.remove(&rec.id);
            g.catalog.bots.retain(|b| b.id != rec.id);
            g.catalog.save().map_err(|e| e.to_string())?;
            (
                rec.id,
                rec.slug.clone(),
                rec.grok_session_id.clone(),
                live.map(|l| l.acp),
            )
        };
        thread::Builder::new()
            .name(format!("bot-rm-{slug}"))
            .spawn(move || {
                if let Some(slot) = acp {
                    if let Ok(mut g) = slot.lock() {
                        *g = None;
                    }
                }
                let home = paths::bot_home(&slug);
                if home.exists() {
                    let _ = fs::remove_dir_all(&home);
                }
                if let Some(sid) = session_id {
                    let _ = std::process::Command::new(paths::grok_bin())
                        .args(["sessions", "delete", &sid])
                        .status();
                }
                info!(%slug, "bot removed");
            })
            .ok();
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
        let (id, slug, model, acp_slot, home, cancel) = {
            let mut g = self.inner.lock().unwrap();
            let rec = g
                .catalog
                .find(key)
                .cloned()
                .ok_or_else(|| format!("unknown bot {key}"))?;
            let live = g
                .live
                .get_mut(&rec.id)
                .ok_or_else(|| format!("unknown bot {key}"))?;
            if live.status == BotStatus::Working {
                return Err("bot is already working".into());
            }
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
                rec.id.clone(),
                rec.slug.clone(),
                rec.model.clone(),
                Arc::clone(&live.acp),
                paths::bot_home(&rec.slug),
                Arc::clone(&live.cancel),
            )
        };

        let host = Arc::clone(self);
        let prompt = text.to_string();
        let reply_id = id.clone();
        thread::Builder::new()
            .name(format!("bot-{slug}"))
            .spawn(move || {
                let result = run_turn(&host, &id, &slug, &model, &home, &acp_slot, &cancel, &prompt);
                if result.as_ref().is_err_and(|e| e.contains("disconnect")) {
                    if let Ok(mut slot) = acp_slot.lock() {
                        *slot = None;
                    }
                }
                let mut g = host.inner.lock().unwrap();
                if let Some(live) = g.live.get_mut(&id) {
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
                    save_dialog(&slug, &live.dialog);
                }
                if let Some(rec) = g.catalog.find_mut(&id) {
                    rec.last_used = now_rfc();
                    let _ = g.catalog.save();
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(json!({ "ok": true, "id": reply_id, "status": "working" }))
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
        let sid = rec.grok_session_id.clone();
        if let (Some(sid), Ok(slot)) = (sid, live.acp.lock()) {
            if let Some(sess) = slot.as_ref() {
                sess.cancel(&sid);
            }
        }
        Ok(json!({ "ok": true, "id": rec.id, "status": "cancelling" }))
    }
}

fn run_turn(
    host: &Host,
    id: &str,
    slug: &str,
    model: &str,
    home: &std::path::Path,
    acp_slot: &Arc<Mutex<Option<Arc<AcpSession>>>>,
    cancel: &Arc<AtomicBool>,
    prompt: &str,
) -> Result<(), String> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        match run_turn_once(host, id, slug, model, home, acp_slot, cancel, prompt) {
            Ok(()) => return Ok(()),
            Err(e) if e.contains("cancelled") => return Err(e),
            Err(e)
                if e.contains("disconnect")
                    || e.contains("writer gone")
                    || e.contains("timed out") =>
            {
                let partial = {
                    let g = host.inner.lock().unwrap();
                    g.live.get(id).and_then(|l| l.dialog.last()).is_some_and(|t| {
                        t.role == "assistant" && !t.text.trim().is_empty()
                    })
                };
                warn!(%slug, %partial, "acp died ({e})");
                if let Ok(mut g) = acp_slot.lock() {
                    *g = None;
                }
                if partial {
                    // Keep the streamed text. Do not re-send the user prompt.
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(400));
            }
            Err(e) => return Err(e),
        }
    }
}

fn run_turn_once(
    host: &Host,
    id: &str,
    slug: &str,
    model: &str,
    home: &std::path::Path,
    acp_slot: &Arc<Mutex<Option<Arc<AcpSession>>>>,
    cancel: &Arc<AtomicBool>,
    prompt: &str,
) -> Result<(), String> {
    let (sess, session_id) = ensure_session(host, id, slug, model, home, acp_slot)?;

    let cancel = Arc::clone(cancel);
    sess.prompt(
        &session_id,
        prompt,
        move || cancel.load(Ordering::SeqCst),
        |ev| {
        if let Some(delta) = ev.text_delta {
            let mut g = host.inner.lock().unwrap();
            if let Some(live) = g.live.get_mut(id) {
                if let Some(last) = live.dialog.last_mut() {
                    if last.role == "assistant" {
                        last.text.push_str(&delta);
                    }
                }
                if live.last_flush.elapsed() >= Duration::from_millis(250) {
                    save_dialog(slug, &live.dialog);
                    live.last_flush = Instant::now();
                }
            }
        }
        if let Some(err) = ev.error {
            error!(%slug, "turn error: {err}");
        }
    })
    .map_err(|e| e.to_string())?;
    let _ = Duration::from_secs(0);
    Ok(())
}

fn ensure_session(
    host: &Host,
    id: &str,
    slug: &str,
    model: &str,
    home: &std::path::Path,
    acp_slot: &Arc<Mutex<Option<Arc<AcpSession>>>>,
) -> Result<(Arc<AcpSession>, String), String> {
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
    let existing = {
        let g = host.inner.lock().unwrap();
        g.catalog
            .find(id)
            .and_then(|r| r.grok_session_id.clone())
    };
    let session_id = if !fresh {
        existing.ok_or_else(|| "missing session id".to_string())?
    } else if let Some(sid) = existing {
        match sess.load_session(&sid) {
            Ok(()) => sid,
            Err(e) => {
                warn!(%slug, "session/load failed ({e}); new session");
                let sid = sess.new_session(home).map_err(|e| e.to_string())?;
                persist_session(host, id, &sid);
                sid
            }
        }
    } else {
        let sid = sess.new_session(home).map_err(|e| e.to_string())?;
        persist_session(host, id, &sid);
        sid
    };
    Ok((sess, session_id))
}

fn persist_session(host: &Host, id: &str, sid: &str) {
    let mut g = host.inner.lock().unwrap();
    if let Some(rec) = g.catalog.find_mut(id) {
        rec.grok_session_id = Some(sid.to_string());
        let _ = g.catalog.save();
    }
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
