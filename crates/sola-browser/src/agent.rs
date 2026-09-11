//! Chrome-side `solactl browser` dispatch.
//!
//! Page verbs go to the helper over `engine.sock` (CEF wraps CDP). Chrome
//! keeps the ref map; YAML never includes backend ids.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::{App, BLANK_URL, Msg};
use crate::ax::{self, RefEntry};
use crate::engine::{Cmd, Engine, NavCmd, TabId, TabInfo};
use crate::groups::TabGroup;
use iced::Task;
use sola_call::Incoming;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub id: u64,
    pub tab: u64,
    pub op: AgentOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOp {
    Snapshot {
        interactive: bool,
        subtree_backend: Option<i32>,
        json: bool,
    },
    Click {
        backend_node_id: i32,
        role: String,
        name: String,
    },
    Hover {
        backend_node_id: i32,
        role: String,
        name: String,
    },
    Type {
        backend_node_id: i32,
        role: String,
        name: String,
        text: String,
        submit: bool,
    },
    Fill {
        backend_node_id: i32,
        role: String,
        name: String,
        text: String,
    },
    Select {
        backend_node_id: i32,
        role: String,
        name: String,
        values: Vec<String>,
    },
    Nav(NavCmd),
    FindPage {
        text: String,
        forward: bool,
        next: bool,
    },
    Screenshot {
        path: String,
    },
    /// `document.readyState` + `location.href` (wait --load).
    ReadyState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReply {
    pub id: u64,
    #[serde(default)]
    pub tab: u64,
    pub ok: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub yaml: Option<String>,
    #[serde(default)]
    pub refs: Vec<RefEntry>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub focused: Option<String>,
    #[serde(default)]
    pub dialog_open: bool,
    #[serde(default)]
    pub json: bool,
    #[serde(default)]
    pub path: Option<String>,
    /// `document.readyState` from a `ReadyState` probe (`complete` / `interactive` / `loading`).
    #[serde(default)]
    pub ready: Option<String>,
}

pub type AgentHandle = std::sync::Arc<std::sync::Mutex<Vec<AgentReply>>>;

#[derive(Clone)]
struct LastSnap {
    yaml: String,
    refs: HashMap<String, RefEntry>,
}

struct PendingWait {
    inc: Incoming,
    tab: TabId,
    text: Option<String>,
    deadline: Instant,
    awaiting_snap: bool,
    probed: bool,
    saw_busy: bool,
    pending_nav: Option<PendingNav>,
}

#[derive(Clone)]
enum PendingNav {
    Url(String),
    AnyLoad,
}

struct ReadyInfo {
    state: String,
    url: String,
}

pub struct AgentState {
    next_id: u64,
    pending: HashMap<u64, Incoming>,
    wait_snaps: HashMap<u64, TabId>,
    snaps: HashMap<TabId, LastSnap>,
    ready: HashMap<TabId, ReadyInfo>,
    pending_nav: HashMap<TabId, PendingNav>,
    last_tab: Option<TabId>,
    waits: Vec<PendingWait>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            next_id: 1,
            pending: HashMap::new(),
            wait_snaps: HashMap::new(),
            snaps: HashMap::new(),
            ready: HashMap::new(),
            pending_nav: HashMap::new(),
            last_tab: None,
            waits: Vec::new(),
        }
    }
}

impl AgentState {
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.waits.is_empty()
    }
}

fn param_str(params: &serde_json::Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn param_bool(params: &serde_json::Value, key: &str) -> bool {
    params
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn param_i64(params: &serde_json::Value, key: &str) -> Option<i64> {
    params.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

impl<E: Engine> App<E> {
    pub fn on_call(&mut self, inc: Incoming) -> Task<Msg> {
        match inc.method.as_str() {
            "wait" => self.cli_wait(inc),
            other => {
                match self.dispatch_call(other, &inc.params) {
                    CallOut::Reply(Ok(v)) => inc.reply.ok(v),
                    CallOut::Reply(Err(e)) => inc.reply.err(e),
                    CallOut::Engine(req) => {
                        self.agent.next_id = self.agent.next_id.saturating_add(1);
                        self.agent.pending.insert(req.id, inc);
                        let _ = self.cmd_tx.send(Cmd::Agent(req));
                    }
                }
                self.wake_if_pending()
            }
        }
    }

    pub fn drain_agent(&mut self) -> Task<Msg> {
        let replies: Vec<AgentReply> = self
            .engine
            .agent_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for r in replies {
            let tab = TabId(if r.tab == 0 {
                self.agent.last_tab.unwrap_or(self.cached_active).0
            } else {
                r.tab
            });
            if r.ok {
                if let Some(state) = r.ready.clone() {
                    let info = ReadyInfo {
                        state,
                        url: r.url.clone().unwrap_or_default(),
                    };
                    let busy = info.state != "complete" || is_blank_url(&info.url);
                    self.agent.ready.insert(tab, info);
                    for w in &mut self.agent.waits {
                        if w.tab == tab && busy {
                            w.saw_busy = true;
                        }
                    }
                }
                if r.yaml.is_some() {
                    self.store_snap(tab, &r);
                }
            }
            if self.agent.wait_snaps.remove(&r.id).is_some() {
                for w in &mut self.agent.waits {
                    if w.tab == tab {
                        w.awaiting_snap = false;
                    }
                }
            }
            if let Some(inc) = self.agent.pending.remove(&r.id) {
                self.finish_engine_reply(inc, r);
            }
        }
        self.poll_waits();
        self.wake_if_pending()
    }

    fn wake_if_pending(&self) -> Task<Msg> {
        if !self.agent.has_pending() {
            return Task::none();
        }
        Task::perform(
            async {
                tokio::time::sleep(Duration::from_millis(120)).await;
            },
            |_| Msg::Tick,
        )
    }

    fn store_snap(&mut self, tab: TabId, r: &AgentReply) {
        let Some(yaml) = r.yaml.clone() else {
            return;
        };
        let mut refs = HashMap::new();
        for e in &r.refs {
            refs.insert(e.r#ref.clone(), e.clone());
        }
        self.agent.snaps.insert(
            tab,
            LastSnap { yaml, refs },
        );
        self.agent.last_tab = Some(tab);
        for w in &mut self.agent.waits {
            if w.tab == tab {
                w.awaiting_snap = false;
                w.probed = true;
            }
        }
    }

    fn finish_engine_reply(&mut self, inc: Incoming, r: AgentReply) {
        if !r.ok {
            inc.reply.err(r.error.unwrap_or_else(|| "engine error".into()));
            return;
        }
        if let Some(yaml) = r.yaml.clone() {
            let tab = TabId(if r.tab == 0 {
                self.agent.last_tab.unwrap_or(self.cached_active).0
            } else {
                r.tab
            });
            let info = self.cached_tabs.iter().find(|t| t.id == tab);
            let url = r
                .url
                .clone()
                .or_else(|| info.map(|t| t.url.clone()))
                .unwrap_or_default();
            let title = r
                .title
                .clone()
                .or_else(|| info.map(|t| t.title.clone()))
                .unwrap_or_default();
            if r.json {
                let snap = ax::Snapshot {
                    url,
                    title,
                    focused: r.focused.clone(),
                    dialog_open: r.dialog_open,
                    yaml: yaml.clone(),
                    refs: r.refs.clone(),
                };
                inc.reply.ok(snap.as_debug_json());
                return;
            }
            let mut body = String::new();
            if !url.is_empty() {
                body.push_str("url: ");
                body.push_str(&url);
                body.push('\n');
            }
            if !title.is_empty() {
                body.push_str("title: ");
                body.push_str(&title);
                body.push('\n');
            }
            if let Some(f) = &r.focused {
                body.push_str("focused: ");
                body.push_str(f);
                body.push('\n');
            }
            if r.dialog_open {
                body.push_str("dialog: open\n");
            }
            body.push_str(&yaml);
            inc.reply.ok(serde_json::json!({
                "tab": tab.0,
                "snapshot": body,
            }));
            return;
        }
        if let Some(path) = r.path {
            inc.reply.ok(serde_json::json!({ "ok": true, "path": path }));
            return;
        }
        inc.reply.ok(serde_json::json!({ "ok": true }));
    }

    fn dispatch_call(&mut self, method: &str, params: &serde_json::Value) -> CallOut {
        match method {
            "tabs" => CallOut::Reply(Ok(self.tabs_json())),
            "tab.open" => CallOut::Reply(self.cli_tab_open(params)),
            "tab.close" => CallOut::Reply(self.cli_tab_close(params)),
            "tab.focus" => CallOut::Reply(self.cli_tab_focus(params)),
            "tab.move" => CallOut::Reply(self.cli_tab_move(params)),
            "group.list" => CallOut::Reply(Ok(self.groups_json())),
            "group.create" => CallOut::Reply(self.cli_group_create(params)),
            "group.rename" => CallOut::Reply(self.cli_group_rename(params)),
            "group.recolor" => CallOut::Reply(self.cli_group_recolor(params)),
            "group.collapse" => CallOut::Reply(self.cli_group_collapse(params)),
            "goto" => self.cli_goto(params),
            "back" => self.cli_nav(params, NavCmd::Back),
            "forward" => self.cli_nav(params, NavCmd::Forward),
            "reload" => {
                let nav = if param_bool(params, "hard") {
                    NavCmd::ReloadIgnoreCache
                } else {
                    NavCmd::Reload
                };
                self.cli_nav(params, nav)
            }
            "stop" => self.cli_nav(params, NavCmd::Stop),
            "find.page" => self.cli_find_page(params),
            "snapshot" => self.cli_snapshot(params),
            "find" => CallOut::Reply(self.cli_find_snap(params)),
            "click" | "hover" | "type" | "fill" | "select" => self.cli_act(method, params),
            "screenshot" => self.cli_screenshot(params),
            other => CallOut::Reply(Err(format!("unknown method {other}"))),
        }
    }

    fn tabs_json(&self) -> serde_json::Value {
        let tabs: Vec<serde_json::Value> = self
            .cached_tabs
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id.0,
                    "url": t.url,
                    "title": t.title,
                    "active": t.id == self.cached_active,
                    "loading": t.is_loading,
                    "group": self.groups.of_tab(t.id),
                })
            })
            .collect();
        serde_json::json!({
            "tabs": tabs,
            "active": self.cached_active.0,
        })
    }

    fn groups_json(&self) -> serde_json::Value {
        let groups: Vec<serde_json::Value> = self
            .groups
            .groups
            .iter()
            .map(|g| group_json(g, &self.groups, &self.cached_tabs))
            .collect();
        serde_json::json!({ "groups": groups })
    }

    fn cli_tab_open(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let url = param_str(params, "url").ok_or_else(|| "missing url".to_string())?;
        let select = param_bool(params, "select");
        self.open_tab(url.clone(), select);
        let id = self.cached_tabs.last().map(|t| t.id).unwrap_or(self.cached_active);
        if let Some(g) = param_str(params, "group") {
            let gid = self.resolve_group(&g)?;
            self.groups.add_to(id, &gid);
            self.groups.normalize(&mut self.cached_tabs);
            self.persist_session();
        }
        self.agent
            .pending_nav
            .insert(id, PendingNav::Url(url));
        Ok(serde_json::json!({ "id": id.0, "selected": select }))
    }

    fn cli_tab_close(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.resolve_tab(param_str(params, "tab").as_deref())?;
        // Reuse CloseTab path via a synthetic update would recurse; inline.
        if self.cached_tabs.len() <= 1 {
            self.open_tab(BLANK_URL.to_string(), true);
        }
        self.remember_closed(id);
        let was_active = self.cached_active == id;
        if was_active {
            if let Some(new_active) = self.pick_new_active_after_close(id) {
                self.switch_active_tab(new_active);
            }
        }
        self.slot.forget_tab(id);
        self.slot.drop_paint_tabs.lock().unwrap().push(id.0);
        self.slot.need_park_prime.lock().unwrap().remove(&id.0);
        let _ = self.cmd_tx.send(Cmd::CloseTab(id));
        self.closed_tabs.insert(id);
        self.cached_tabs.retain(|t| t.id != id);
        self.groups.on_tab_closed(id);
        self.favicons.remove(&id);
        self.agent.snaps.remove(&id);
        self.agent.ready.remove(&id);
        self.agent.pending_nav.remove(&id);
        self.persist_session();
        Ok(serde_json::json!({ "ok": true }))
    }

    fn cli_tab_focus(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.resolve_tab(param_str(params, "tab").as_deref())?;
        self.switch_active_tab(id);
        self.persist_session();
        Ok(serde_json::json!({ "id": id.0 }))
    }

    fn cli_tab_move(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.resolve_tab(param_str(params, "tab").as_deref())?;
        match param_str(params, "group") {
            Some(g) => {
                let gid = self.resolve_group(&g)?;
                self.groups.add_to(id, &gid);
            }
            None => self.groups.ungroup(id),
        }
        self.groups.normalize(&mut self.cached_tabs);
        self.persist_session();
        Ok(serde_json::json!({ "ok": true, "id": id.0 }))
    }

    fn cli_group_create(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.resolve_tab(param_str(params, "tab").as_deref())?;
        let gid = self.groups.new_group(id);
        if let Some(name) = param_str(params, "name") {
            self.groups.rename(&gid, name);
        }
        self.groups.normalize(&mut self.cached_tabs);
        self.persist_session();
        Ok(serde_json::json!({ "id": gid, "tab": id.0 }))
    }

    fn cli_group_rename(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let gid = self.resolve_group(param_str(params, "group").as_deref().unwrap_or(""))?;
        let name = param_str(params, "name").ok_or_else(|| "missing name".to_string())?;
        self.groups.rename(&gid, name);
        self.persist_session();
        Ok(serde_json::json!({ "ok": true, "id": gid }))
    }

    fn cli_group_recolor(&mut self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let gid = self.resolve_group(param_str(params, "group").as_deref().unwrap_or(""))?;
        let color = param_str(params, "color");
        self.groups.set_color(&gid, color);
        self.persist_session();
        Ok(serde_json::json!({ "ok": true, "id": gid }))
    }

    fn cli_group_collapse(
        &mut self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let gid = self.resolve_group(param_str(params, "group").as_deref().unwrap_or(""))?;
        self.groups.set_collapsed(&gid, param_bool(params, "collapsed"));
        self.persist_session();
        Ok(serde_json::json!({ "ok": true, "id": gid }))
    }

    fn cli_goto(&mut self, params: &serde_json::Value) -> CallOut {
        let Some(url) = param_str(params, "url") else {
            return CallOut::Reply(Err("missing url".into()));
        };
        let url = crate::util::normalize_url(&url);
        match self.resolve_tab(param_str(params, "tab").as_deref()) {
            Ok(tab) => {
                if param_bool(params, "select") {
                    self.switch_active_tab(tab);
                }
                self.engine_nav(tab, NavCmd::LoadUrl(url))
            }
            Err(e) => CallOut::Reply(Err(e)),
        }
    }

    fn cli_nav(&mut self, params: &serde_json::Value, nav: NavCmd) -> CallOut {
        match self.resolve_tab(param_str(params, "tab").as_deref()) {
            Ok(tab) => self.engine_nav(tab, nav),
            Err(e) => CallOut::Reply(Err(e)),
        }
    }

    fn engine_nav(&mut self, tab: TabId, nav: NavCmd) -> CallOut {
        let pending = match &nav {
            NavCmd::LoadUrl(url) => PendingNav::Url(url.clone()),
            _ => PendingNav::AnyLoad,
        };
        self.agent.pending_nav.insert(tab, pending);
        let id = self.agent.next_id;
        self.agent.next_id += 1;
        self.agent.last_tab = Some(tab);
        CallOut::Engine(AgentRequest {
            id,
            tab: tab.0,
            op: AgentOp::Nav(nav),
        })
    }

    fn cli_find_page(&mut self, params: &serde_json::Value) -> CallOut {
        let Some(text) = param_str(params, "text") else {
            return CallOut::Reply(Err("missing text".into()));
        };
        match self.resolve_tab(param_str(params, "tab").as_deref()) {
            Ok(tab) => {
                let id = self.agent.next_id;
                self.agent.next_id += 1;
                self.agent.last_tab = Some(tab);
                CallOut::Engine(AgentRequest {
                    id,
                    tab: tab.0,
                    op: AgentOp::FindPage {
                        text,
                        forward: !param_bool(params, "back"),
                        next: true,
                    },
                })
            }
            Err(e) => CallOut::Reply(Err(e)),
        }
    }

    fn cli_snapshot(&mut self, params: &serde_json::Value) -> CallOut {
        match self.resolve_tab(param_str(params, "tab").as_deref()) {
            Ok(tab) => {
                let subtree = param_str(params, "ref").and_then(|r| {
                    self.lookup_ref(tab, &r).map(|e| e.backend_node_id)
                });
                if param_str(params, "ref").is_some() && subtree.is_none() {
                    return CallOut::Reply(Err(
                        "ref not found in the current page snapshot. Try capturing new snapshot."
                            .into(),
                    ));
                }
                let id = self.agent.next_id;
                self.agent.next_id += 1;
                self.agent.last_tab = Some(tab);
                CallOut::Engine(AgentRequest {
                    id,
                    tab: tab.0,
                    op: AgentOp::Snapshot {
                        interactive: param_bool(params, "interactive"),
                        subtree_backend: subtree,
                        json: param_bool(params, "json"),
                    },
                })
            }
            Err(e) => CallOut::Reply(Err(e)),
        }
    }

    fn cli_find_snap(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let q = param_str(params, "text").ok_or_else(|| "missing text".to_string())?;
        let tab = self.resolve_tab(param_str(params, "tab").as_deref())?;
        let snap = self
            .agent
            .snaps
            .get(&tab)
            .ok_or_else(|| "no snapshot for this tab; run snapshot first".to_string())?;
        let hits = ax::find_in_yaml(&snap.yaml, &q);
        Ok(serde_json::json!({ "tab": tab.0, "hits": hits }))
    }

    fn cli_act(&mut self, method: &str, params: &serde_json::Value) -> CallOut {
        let Some(r) = param_str(params, "ref") else {
            return CallOut::Reply(Err("missing ref".into()));
        };
        let tab = match self.resolve_tab_or_last(param_str(params, "tab").as_deref()) {
            Ok(t) => t,
            Err(e) => return CallOut::Reply(Err(e)),
        };
        let entry = match self.lookup_ref(tab, &r) {
            Some(e) => e.clone(),
            None => {
                return CallOut::Reply(Err(
                    "ref not found in the current page snapshot. Try capturing new snapshot."
                        .into(),
                ));
            }
        };
        let id = self.agent.next_id;
        self.agent.next_id += 1;
        self.agent.last_tab = Some(tab);
        let op = match method {
            "click" => AgentOp::Click {
                backend_node_id: entry.backend_node_id,
                role: entry.role.clone(),
                name: entry.name.clone(),
            },
            "hover" => AgentOp::Hover {
                backend_node_id: entry.backend_node_id,
                role: entry.role.clone(),
                name: entry.name.clone(),
            },
            "type" => {
                let Some(text) = param_str(params, "text") else {
                    return CallOut::Reply(Err("missing text".into()));
                };
                AgentOp::Type {
                    backend_node_id: entry.backend_node_id,
                    role: entry.role.clone(),
                    name: entry.name.clone(),
                    text,
                    submit: param_bool(params, "submit"),
                }
            }
            "fill" => {
                let Some(text) = param_str(params, "text") else {
                    return CallOut::Reply(Err("missing text".into()));
                };
                AgentOp::Fill {
                    backend_node_id: entry.backend_node_id,
                    role: entry.role.clone(),
                    name: entry.name.clone(),
                    text,
                }
            }
            "select" => {
                let Some(raw) = param_str(params, "values") else {
                    return CallOut::Reply(Err("missing values".into()));
                };
                let values: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                AgentOp::Select {
                    backend_node_id: entry.backend_node_id,
                    role: entry.role.clone(),
                    name: entry.name.clone(),
                    values,
                }
            }
            other => return CallOut::Reply(Err(format!("unknown act {other}"))),
        };
        CallOut::Engine(AgentRequest {
            id,
            tab: tab.0,
            op,
        })
    }

    fn cli_screenshot(&mut self, params: &serde_json::Value) -> CallOut {
        match self.resolve_tab(param_str(params, "tab").as_deref()) {
            Ok(tab) => {
                let path = param_str(params, "path").unwrap_or_else(default_shot_path);
                let id = self.agent.next_id;
                self.agent.next_id += 1;
                self.agent.last_tab = Some(tab);
                CallOut::Engine(AgentRequest {
                    id,
                    tab: tab.0,
                    op: AgentOp::Screenshot { path },
                })
            }
            Err(e) => CallOut::Reply(Err(e)),
        }
    }

    fn cli_wait(&mut self, inc: Incoming) -> Task<Msg> {
        let tab = match self.resolve_tab(param_str(&inc.params, "tab").as_deref()) {
            Ok(t) => t,
            Err(e) => {
                inc.reply.err(e);
                return Task::none();
            }
        };
        let secs = param_i64(&inc.params, "timeout")
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(crate::calls::WAIT_DEFAULT_SECS);
        let text = param_str(&inc.params, "text");
        let pending_nav = self.agent.pending_nav.remove(&tab);
        self.agent.waits.push(PendingWait {
            inc,
            tab,
            text,
            deadline: Instant::now() + Duration::from_secs(secs.max(1)),
            awaiting_snap: false,
            probed: false,
            saw_busy: false,
            pending_nav,
        });
        self.poll_waits();
        self.wake_if_pending()
    }

    fn poll_waits(&mut self) {
        let now = Instant::now();
        let mut i = 0;
        while i < self.agent.waits.len() {
            if now >= self.agent.waits[i].deadline {
                let w = self.agent.waits.remove(i);
                w.inc.reply.err("wait timed out");
                continue;
            }
            let tab = self.agent.waits[i].tab;
            let loading = self
                .cached_tabs
                .iter()
                .find(|t| t.id == tab)
                .map(|t| t.is_loading)
                .unwrap_or(true);
            if let Some(text) = self.agent.waits[i].text.clone() {
                if self.agent.waits[i].probed {
                    if let Some(snap) = self.agent.snaps.get(&tab) {
                        if !ax::find_in_yaml(&snap.yaml, &text).is_empty() {
                            let w = self.agent.waits.remove(i);
                            w.inc.reply.ok(serde_json::json!({ "ok": true, "tab": tab.0 }));
                            continue;
                        }
                    }
                }
                if !self.agent.waits[i].awaiting_snap {
                    self.agent.waits[i].awaiting_snap = true;
                    let id = self.agent.next_id;
                    self.agent.next_id += 1;
                    self.agent.last_tab = Some(tab);
                    self.agent.wait_snaps.insert(id, tab);
                    let _ = self.cmd_tx.send(Cmd::Agent(AgentRequest {
                        id,
                        tab: tab.0,
                        op: AgentOp::Snapshot {
                            interactive: false,
                            subtree_backend: None,
                            json: false,
                        },
                    }));
                }
            } else if load_wait_done(
                loading,
                self.agent.ready.get(&tab),
                self.agent.waits[i].pending_nav.as_ref(),
                self.agent.waits[i].saw_busy,
            ) {
                let w = self.agent.waits.remove(i);
                w.inc.reply.ok(serde_json::json!({ "ok": true, "tab": tab.0 }));
                continue;
            } else if !self.agent.waits[i].awaiting_snap {
                self.agent.waits[i].awaiting_snap = true;
                let id = self.agent.next_id;
                self.agent.next_id += 1;
                self.agent.last_tab = Some(tab);
                self.agent.wait_snaps.insert(id, tab);
                let _ = self.cmd_tx.send(Cmd::Agent(AgentRequest {
                    id,
                    tab: tab.0,
                    op: AgentOp::ReadyState,
                }));
            }
            i += 1;
        }
    }

    fn lookup_ref(&self, tab: TabId, r: &str) -> Option<&RefEntry> {
        let key = r.trim().trim_start_matches('@');
        self.agent.snaps.get(&tab).and_then(|s| s.refs.get(key))
    }

    fn resolve_tab_or_last(&self, q: Option<&str>) -> Result<TabId, String> {
        if q.is_some() {
            return self.resolve_tab(q);
        }
        if let Some(t) = self.agent.last_tab {
            if self.cached_tabs.iter().any(|x| x.id == t) {
                return Ok(t);
            }
        }
        self.resolve_tab(None)
    }

    fn resolve_tab(&self, q: Option<&str>) -> Result<TabId, String> {
        let Some(q) = q.filter(|s| !s.is_empty()) else {
            if self.cached_tabs.iter().any(|t| t.id == self.cached_active) {
                return Ok(self.cached_active);
            }
            return self
                .cached_tabs
                .first()
                .map(|t| t.id)
                .ok_or_else(|| "no tabs".to_string());
        };
        if let Ok(n) = q.parse::<u64>() {
            if let Some(t) = self.cached_tabs.iter().find(|t| t.id.0 == n) {
                return Ok(t.id);
            }
        }
        let ql = q.to_ascii_lowercase();
        let hits: Vec<&TabInfo> = self
            .cached_tabs
            .iter()
            .filter(|t| {
                t.url.to_ascii_lowercase().contains(&ql)
                    || t.title.to_ascii_lowercase().contains(&ql)
            })
            .collect();
        match hits.len() {
            1 => Ok(hits[0].id),
            0 => Err(format!("no tab matching {q}")),
            n => Err(format!("{n} tabs match {q}; pass an id")),
        }
    }

    fn resolve_group(&self, q: &str) -> Result<String, String> {
        if q.is_empty() {
            return Err("missing group".into());
        }
        if let Some(g) = self.groups.group(q) {
            return Ok(g.id.clone());
        }
        let ql = q.to_ascii_lowercase();
        let hits: Vec<&TabGroup> = self
            .groups
            .groups
            .iter()
            .filter(|g| g.name.to_ascii_lowercase() == ql || g.name.to_ascii_lowercase().contains(&ql))
            .collect();
        match hits.len() {
            1 => Ok(hits[0].id.clone()),
            0 => Err(format!("no group matching {q}")),
            n => Err(format!("{n} groups match {q}; pass an id")),
        }
    }
}

enum CallOut {
    Reply(Result<serde_json::Value, String>),
    Engine(AgentRequest),
}

fn group_json(g: &TabGroup, groups: &crate::groups::Groups, tabs: &[TabInfo]) -> serde_json::Value {
    let members: Vec<u64> = tabs
        .iter()
        .filter(|t| groups.of_tab(t.id) == Some(g.id.as_str()))
        .map(|t| t.id.0)
        .collect();
    serde_json::json!({
        "id": g.id,
        "name": g.name,
        "collapsed": g.collapsed,
        "color": g.color,
        "tabs": members,
    })
}

fn default_shot_path() -> String {
    let mut p = PathBuf::from(std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{home}/.cache")
    }));
    p.push("sola");
    p.push("browser");
    let _ = std::fs::create_dir_all(&p);
    p.push(format!("shot-{}.png", crate::engine::monotonic_ms()));
    p.to_string_lossy().into_owned()
}

impl AgentReply {
    pub fn fail(id: u64, tab: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            tab,
            ok: false,
            error: Some(error.into()),
            yaml: None,
            refs: Vec::new(),
            url: None,
            title: None,
            focused: None,
            dialog_open: false,
            json: false,
            path: None,
            ready: None,
        }
    }
}

fn is_blank_url(url: &str) -> bool {
    let u = url.trim();
    u.is_empty() || u == crate::app::BLANK_URL || u == "about:blank/"
}

fn url_matches_pending(ready: &str, want: &str) -> bool {
    let a = normalize_wait_url(ready);
    let b = normalize_wait_url(want);
    !a.is_empty() && !b.is_empty() && (a == b || a.starts_with(&b) || b.starts_with(&a))
}

fn normalize_wait_url(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

/// Wait --load: trust `document.readyState` once the helper has sampled it.
/// CEF `is_loading` sticks `true` on background OSR tabs after the document
/// is done, so the spinner is only the fast path before the first sample.
/// After goto/tab.open, require the new URL — the previous document is also
/// `complete` until navigation starts.
fn load_wait_done(
    spinner: bool,
    ready: Option<&ReadyInfo>,
    pending: Option<&PendingNav>,
    saw_busy: bool,
) -> bool {
    match pending {
        Some(PendingNav::Url(want)) => ready
            .map(|r| r.state == "complete" && url_matches_pending(&r.url, want))
            .unwrap_or(false),
        Some(PendingNav::AnyLoad) => {
            saw_busy
                && ready
                    .map(|r| r.state == "complete" && !is_blank_url(&r.url))
                    .unwrap_or(false)
        }
        None => {
            if let Some(r) = ready {
                if is_blank_url(&r.url) {
                    return false;
                }
                return r.state == "complete";
            }
            !spinner
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(state: &str, url: &str) -> ReadyInfo {
        ReadyInfo {
            state: state.into(),
            url: url.into(),
        }
    }

    #[test]
    fn load_wait_uses_spinner_before_sample() {
        assert!(!load_wait_done(true, None, None, false));
        assert!(load_wait_done(false, None, None, false));
    }

    #[test]
    fn load_wait_ignores_blank_document() {
        assert!(!load_wait_done(
            false,
            Some(&ready("complete", "about:blank")),
            None,
            false
        ));
        assert!(!load_wait_done(true, Some(&ready("complete", "")), None, false));
    }

    #[test]
    fn load_wait_complete_committed_url_wins_over_spinner() {
        assert!(load_wait_done(
            true,
            Some(&ready("complete", "https://example.com/")),
            None,
            false
        ));
        assert!(!load_wait_done(
            true,
            Some(&ready("loading", "https://example.com/")),
            None,
            false
        ));
        assert!(!load_wait_done(
            true,
            Some(&ready("interactive", "https://example.com/")),
            None,
            false
        ));
    }

    #[test]
    fn load_wait_goto_ignores_previous_document() {
        let pending = PendingNav::Url("https://www.wikipedia.org/".into());
        assert!(!load_wait_done(
            true,
            Some(&ready("complete", "https://example.com/")),
            Some(&pending),
            false
        ));
        assert!(load_wait_done(
            true,
            Some(&ready("complete", "https://www.wikipedia.org")),
            Some(&pending),
            false
        ));
    }

    #[test]
    fn load_wait_reload_needs_a_busy_blip() {
        let pending = PendingNav::AnyLoad;
        assert!(!load_wait_done(
            true,
            Some(&ready("complete", "https://example.com/")),
            Some(&pending),
            false
        ));
        assert!(load_wait_done(
            true,
            Some(&ready("complete", "https://example.com/")),
            Some(&pending),
            true
        ));
    }
}
