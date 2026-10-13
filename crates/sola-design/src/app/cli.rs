//! Call owner `design` (`solactl design …`). Specs live in `crate::calls`;
//! handlers run here on the iced thread. Page work (screenshot, inspect)
//! goes to the board's tab over CDP and replies when the helper answers.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::Task;
use serde_json::{Value, json};
use sola_browser::agent::{AgentOp, AgentReply};
use sola_browser::cef::engine::InputEvent;
use sola_browser::engine::{Cmd, Engine, TabId};
use sola_call::Incoming;

use super::{App, Msg, Pending};
use crate::boards::Board;
use crate::calls::WAIT_DEFAULT_SECS;
use crate::camera::Camera;
use crate::canvas;
use crate::inspect;
use crate::loads::LOAD_GRACE;
use crate::projects::{self, CanvasRef};
use crate::select;

/// CSS px per inch (PDF paper size).
const CSS_DPI: f64 = 96.0;
const NO_CANVAS: &str =
    "no canvas given and none on screen (pass --canvas <path>, or solactl design open <path>)";

/// Page call waiting on the helper.
pub(super) enum CallKind {
    Screenshot {
        artboard: String,
        path: PathBuf,
        width: u32,
        height: u32,
        scale: f64,
    },
    Inspect {
        artboard: String,
    },
    Pdf {
        artboard: String,
        path: PathBuf,
    },
}

/// `wait` / `reload` waiting for its boards to load.
pub(super) struct Wait {
    inc: Incoming,
    /// `(tab, artboard id)`.
    tabs: Vec<(u64, String)>,
    saw_loading: HashSet<u64>,
    started: Instant,
    deadline: Instant,
}

impl Wait {
    pub(super) fn tabs(&self) -> impl Iterator<Item = u64> + '_ {
        self.tabs.iter().map(|(tab, _)| *tab)
    }
}

impl App {
    pub(super) fn on_call(&mut self, inc: Incoming) -> Task<Msg> {
        let method = inc.method.clone();
        let p = inc.params.clone();
        let out = match method.as_str() {
            "project.list" => Ok(self.projects_json()),
            "project.add" => self.call_project_add(&p),
            "open" => self.call_open(&p),
            "canvas.new" => self.call_canvas_new(&p),
            "status" => self.target(&p).map(|key| self.status_json(&key)),
            "selection" => {
                if let Some(sel) = &self.selection {
                    self.glows.elements(sel.tab, [sel.picked.rect], Instant::now());
                }
                Ok(self.selection.as_ref().map_or(Value::Null, |s| s.to_json()))
            }
            "view" => self.target(&p).and_then(|key| self.call_view(&key, &p)),
            "share" | "share.rotate" => self.target(&p).and_then(|key| {
                let share = self.share.as_ref().ok_or("sharing is off (see the Design log)")?;
                let url = if method == "share" { share.url(&key) } else { share.rotate(&key) };
                let url = url.ok_or("this canvas has no share link yet")?;
                Ok(json!({"url": url, "tunnel": share.tunnel_configured()}))
            }),
            "wait" => return self.begin_wait(inc, false),
            "reload" => return self.begin_wait(inc, true),
            "screenshot" | "inspect" | "pdf" => {
                let started = self.target(&p).and_then(|key| match method.as_str() {
                    "screenshot" => self.start_screenshot(&key, &p),
                    "pdf" => self.start_pdf(&key, &p),
                    _ => self.start_inspect(&key, &p),
                });
                match started {
                    Ok((tab, op, kind)) => {
                        if matches!(kind, CallKind::Screenshot { .. } | CallKind::Pdf { .. }) {
                            self.begin_capture(tab);
                        }
                        let why = Pending::Call {
                            inc,
                            kind,
                            tab: tab.0,
                        };
                        self.agent(tab, op, Some(why));
                        return Task::none();
                    }
                    Err(e) => Err(e),
                }
            }
            other => Err(format!("design has no method {other}")),
        };
        match out {
            Ok(v) => inc.reply.ok(v),
            Err(e) => inc.reply.err(e),
        }
        Task::none()
    }

    /// An agent capture (screenshot / PDF) of `tab` starts. It sees the
    /// page as the page is, whatever the human is doing on the canvas: the
    /// board is held whole (no detail render from the camera), and the
    /// human's pointer leaves it (no `:hover` in the picture). It glows.
    fn begin_capture(&mut self, tab: TabId) {
        if let Some(ask) = self.details.hold(tab.0) {
            self.ask(ask);
        }
        self.send(Cmd::BoardInput {
            id: tab,
            input: InputEvent::PointerLeave {
                x: -1,
                y: -1,
                modifiers: 0,
            },
        });
        self.glows.board(tab.0, Instant::now());
    }

    pub(super) fn finish_call(&mut self, inc: Incoming, kind: CallKind, tab: u64, reply: AgentReply) {
        let now = Instant::now();
        match &kind {
            CallKind::Screenshot { .. } | CallKind::Pdf { .. } => {
                self.details.release(tab);
                self.details.poke(now);
            }
            CallKind::Inspect { .. } if reply.ok => {
                let rects = inspected_rects(reply.data.as_ref());
                if rects.is_empty() {
                    self.glows.board(tab, now);
                } else {
                    self.glows.elements(tab, rects, now);
                }
            }
            CallKind::Inspect { .. } => {}
        }
        if !reply.ok {
            inc.reply
                .err(reply.error.unwrap_or_else(|| "board call failed".into()));
            return;
        }
        match kind {
            CallKind::Screenshot {
                artboard,
                path,
                width,
                height,
                scale,
            } => inc.reply.ok(json!({
                "artboard": artboard,
                "path": reply.path.map(PathBuf::from).unwrap_or(path),
                "width": width,
                "height": height,
                "scale": scale,
            })),
            CallKind::Pdf { artboard, path } => inc.reply.ok(json!({
                "artboard": artboard,
                "path": reply.path.map(PathBuf::from).unwrap_or(path),
            })),
            CallKind::Inspect { artboard } => {
                let mut v = reply.data.unwrap_or(Value::Null);
                if let Some(m) = v.as_object_mut() {
                    m.insert("artboard".into(), json!(artboard));
                }
                inc.reply.ok(v);
            }
        }
    }

    // ── read-only views ───────────────────────────────────────────

    fn projects_json(&self) -> Value {
        let projects: Vec<Value> = self
            .catalog
            .projects
            .iter()
            .map(|p| {
                let canvases: Vec<Value> = canvas::scan(&p.root)
                    .into_iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "title": c.title,
                            "path": canvas::canvas_dir(&p.root, &c.name),
                        })
                    })
                    .collect();
                json!({"id": p.id, "name": p.name, "root": p.root, "canvases": canvases})
            })
            .collect();
        json!({ "projects": projects })
    }

    /// `status` of live canvas `key`.
    pub(super) fn status_json(&mut self, key: &str) -> Value {
        let zoom = self.camera_of(key).map(|c| c.zoom);
        let Some(live) = self.live(key) else {
            return Value::Null;
        };
        let loading = self.loading_map();
        let shown = self.tab_urls();
        let artboards: Vec<Value> = live
            .boards
            .iter()
            .map(|b| {
                json!({
                    "id": b.art.id,
                    "title": b.art.title,
                    "src": b.art.url,
                    // What the tab is showing; differs from `src` while a
                    // navigation is pending or after an in-page link.
                    "url": shown.get(&b.tab.0),
                    "error": b.error,
                    "x": b.art.x,
                    "y": b.art.y,
                    "width": b.art.width,
                    "height": b.art.height,
                    "scale": b.scale,
                    // Part of the board re-rastered at screen scale, if any.
                    "detail": self.details.current(b.tab.0).map(|d| json!({
                        "x": d.x, "y": d.y, "scale": d.scale,
                    })),
                    "auto_height": b.art.auto_height,
                    "painted": self.frames.is_painted(b.tab.0),
                    "loading": loading.get(&b.tab.0).copied().unwrap_or(true),
                })
            })
            .collect();
        let canvas = json!({
            "project": live.at.project,
            "canvas": live.at.canvas,
            "path": live.dir,
            "error": live.error,
            // On the human's screen; agents' canvases stay off it.
            "shown": self.is_shown(key),
            // Live link for the web (while Design runs).
            "share": self.share.as_ref().and_then(|s| s.url(key)),
        });
        json!({
            "canvas": canvas,
            "artboards": artboards,
            // Files the boards loaded from outside design/; edits reload too.
            "linked": self.linked(key),
            "zoom": zoom,
            "selection": self.selection.as_ref().map(|s| s.to_json()),
        })
    }

    /// Helper tab snapshot: tab id → still loading.
    pub(super) fn loading_map(&self) -> HashMap<u64, bool> {
        self.engine
            .tabs_handle()
            .lock()
            .unwrap()
            .iter()
            .map(|t| (t.id.0, t.is_loading))
            .collect()
    }

    // ── changes ───────────────────────────────────────────────────

    fn call_project_add(&mut self, p: &Value) -> Result<Value, String> {
        let id = self.catalog.add(str_param(p, "path")?)?;
        self.persist();
        self.rescan();
        let project = self.catalog.project(&id).expect("just added");
        Ok(json!({"id": project.id, "name": project.name, "root": project.root}))
    }

    fn call_open(&mut self, p: &Value) -> Result<Value, String> {
        let raw = projects::expand_home(str_param(p, "path")?);
        let path = raw
            .canonicalize()
            .map_err(|e| format!("{}: {e}", raw.display()))?;
        let (root, name) = canvas::locate(&path).ok_or_else(|| {
            format!(
                "{} is not in a canvas (want <project>/design/<name>/canvas.toml)",
                path.display()
            )
        })?;
        let (project, added) = self.ensure_project(&root)?;
        Ok(self.show(p, project, name, added, None))
    }

    /// `<project>/design/<slug(name)>/canvas.toml`, then open it. An existing
    /// canvas of that name is opened, not replaced (`created: false`).
    fn call_canvas_new(&mut self, p: &Value) -> Result<Value, String> {
        let title = str_param(p, "name")?;
        let raw = projects::expand_home(str_param(p, "path")?);
        let root = raw
            .canonicalize()
            .map_err(|e| format!("{}: {e}", raw.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        let slug = projects::slug(title);
        let exists = canvas::canvas_dir(&root, &slug)
            .join(canvas::CANVAS_FILE)
            .is_file();
        let name = if exists {
            slug
        } else {
            canvas::create(&root, title)?
        };
        let (project, added) = self.ensure_project(&root)?;
        Ok(self.show(p, project, name, added, Some(!exists)))
    }

    /// Project id for `root`, registering it if Design does not know it.
    fn ensure_project(&mut self, root: &std::path::Path) -> Result<(String, bool), String> {
        if let Some(p) = self.catalog.projects.iter().find(|p| p.root == root) {
            return Ok((p.id.clone(), false));
        }
        let id = self.catalog.add(&root.to_string_lossy())?;
        tracing::info!(project = %id, root = %root.display(), "call registered design project");
        self.persist();
        Ok((id, true))
    }

    /// Make `canvas` of `project` live in the background — the human's
    /// screen stays as it is — or, with `--raise`, show it and raise the
    /// window. Reports its status.
    fn show(
        &mut self,
        p: &Value,
        project: String,
        canvas: String,
        added: bool,
        created: Option<bool>,
    ) -> Value {
        self.rescan();
        let at = CanvasRef { project, canvas };
        let Some(key) = self.ensure_live(&at) else {
            return Value::Null;
        };
        self.agent_touch(&key);
        if bool_param(p, "raise") {
            self.show_canvas(at);
            self.raise();
        }
        let mut v = self.status_json(&key);
        v["project_added"] = json!(added);
        if let Some(created) = created {
            v["created"] = json!(created);
        }
        v
    }

    /// Steer the camera of `key` — unless the human has, on that canvas:
    /// then the view is theirs and stays put (`moved: false`). An
    /// off-screen canvas keeps the view for when the human opens it.
    fn call_view(&mut self, key: &str, p: &Value) -> Result<Value, String> {
        if !self.is_shown(key) {
            return self.view_parked(key, p);
        }
        let view = self
            .plane_size
            .ok_or("the Design window has not laid out a canvas yet")?;
        let zoom = p.get("zoom").and_then(Value::as_f64).map(|z| z as f32);
        if self.human_camera {
            return Ok(json!({
                "zoom": self.camera.zoom,
                "moved": false,
                "reason": "the human is steering this canvas's view; it stays where they put it",
            }));
        }
        self.user_camera = true;
        if p.get("artboard").is_some() {
            let rect = self.target_board(key, p)?.rect();
            self.camera = match zoom {
                Some(z) => Camera::centered(rect, view, z),
                None => Camera::fit(rect, view, 1.0),
            };
        } else if bool_param(p, "fit") {
            self.fit();
        } else if let Some(z) = zoom {
            if let Some(c) = self.plane_center() {
                self.camera.set_zoom_at(c, z);
            }
        } else {
            return Err("give --fit, --artboard, or --zoom".into());
        }
        Ok(json!({"zoom": self.camera.zoom, "moved": true, "shown": true}))
    }

    /// `view` for an off-screen canvas: its parked camera.
    fn view_parked(&mut self, key: &str, p: &Value) -> Result<Value, String> {
        let view = self
            .plane_size
            .ok_or("the Design window has not laid out a canvas yet")?;
        let zoom = p.get("zoom").and_then(Value::as_f64).map(|z| z as f32);
        let mut camera = self.camera_of(key).ok_or(NO_CANVAS)?;
        let parked = self.live(key).and_then(|l| l.parked).ok_or(NO_CANVAS)?;
        if parked.human_camera {
            return Ok(json!({
                "zoom": camera.zoom,
                "moved": false,
                "shown": false,
                "reason": "the human framed this canvas; it stays where they put it",
            }));
        }
        if p.get("artboard").is_some() {
            let rect = self.target_board(key, p)?.rect();
            camera = match zoom {
                Some(z) => Camera::centered(rect, view, z),
                None => Camera::fit(rect, view, 1.0),
            };
        } else if bool_param(p, "fit") {
            let boards = &self.live(key).ok_or(NO_CANVAS)?.boards;
            if let Some(content) = crate::camera::bounds_of(boards.iter().map(Board::rect)) {
                camera = Camera::fit(content, view, 1.0);
            }
        } else if let Some(z) = zoom {
            camera.set_zoom_at(iced::Point::new(view.width / 2.0, view.height / 2.0), z);
        } else {
            return Err("give --fit, --artboard, or --zoom".into());
        }
        if let Some(p) = self.live_mut(key).and_then(|l| l.parked.as_mut()) {
            p.camera = camera;
            p.fit_pending = false;
            p.user_camera = true;
        }
        Ok(json!({"zoom": camera.zoom, "moved": true, "shown": false}))
    }

    // ── page calls ────────────────────────────────────────────────

    /// The live canvas a call is about: `--canvas` (a canvas folder, its
    /// `canvas.toml`, or any file in it — made live in the background if
    /// need be), else the one on screen.
    fn target(&mut self, p: &Value) -> Result<String, String> {
        let Some(raw) = opt_str(p, "canvas") else {
            let key = self.shown.clone().ok_or(NO_CANVAS)?;
            self.agent_touch(&key);
            return Ok(key);
        };
        let raw = projects::expand_home(raw);
        let path = raw
            .canonicalize()
            .map_err(|e| format!("{}: {e}", raw.display()))?;
        let (root, name) = canvas::locate(&path).ok_or_else(|| {
            format!(
                "{} is not in a canvas (want <project>/design/<name>/canvas.toml)",
                path.display()
            )
        })?;
        let (project, added) = self.ensure_project(&root)?;
        if added {
            self.rescan();
        }
        let key = self
            .ensure_live(&CanvasRef {
                project,
                canvas: name,
            })
            .ok_or_else(|| format!("cannot open {}", path.display()))?;
        self.agent_touch(&key);
        Ok(key)
    }

    /// `--artboard`, else the human's pick on that canvas, else the only
    /// board.
    fn target_board(&self, key: &str, p: &Value) -> Result<&Board, String> {
        let live = self.live(key).ok_or(NO_CANVAS)?;
        let ids = || {
            live.boards
                .iter()
                .map(|b| b.art.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        if let Some(id) = opt_str(p, "artboard") {
            return live.boards.iter().find(|b| b.art.id == id).ok_or_else(|| {
                format!("no artboard {id:?} on {} (have: {})", live.at.canvas, ids())
            });
        }
        let selected = self
            .selection
            .as_ref()
            .and_then(|s| live.boards.iter().find(|b| b.tab.0 == s.tab));
        if let Some(b) = selected {
            return Ok(b);
        }
        match live.boards.as_slice() {
            [only] => Ok(only),
            [] => Err(format!("{} has no artboards", live.at.canvas)),
            _ => Err(format!("--artboard is required (have: {})", ids())),
        }
    }

    /// `--out`, else `$XDG_RUNTIME_DIR/sola/design/shots/<canvas>-<artboard>-<ms>.<ext>`.
    fn out_path(&self, key: &str, p: &Value, b: &Board, ext: &str) -> Result<PathBuf, String> {
        let path = match opt_str(p, "out") {
            Some(out) => PathBuf::from(out),
            None => default_shot_path(&self.live(key).ok_or(NO_CANVAS)?.at, &b.art.id, ext),
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        Ok(path)
    }

    fn start_screenshot(&self, key: &str, p: &Value) -> Result<(TabId, AgentOp, CallKind), String> {
        let b = self.target_board(key, p)?;
        let path = self.out_path(key, p, b, "png")?;
        let kind = CallKind::Screenshot {
            artboard: b.art.id.clone(),
            path: path.clone(),
            width: (f64::from(b.art.width) * b.scale).round() as u32,
            height: (f64::from(b.art.height) * b.scale).round() as u32,
            scale: b.scale,
        };
        let op = AgentOp::Screenshot {
            path: path.display().to_string(),
        };
        Ok((b.tab, op, kind))
    }

    /// One page, the artboard's size, as it looks on screen.
    fn start_pdf(&self, key: &str, p: &Value) -> Result<(TabId, AgentOp, CallKind), String> {
        let b = self.target_board(key, p)?;
        let path = self.out_path(key, p, b, "pdf")?;
        let op = AgentOp::Pdf {
            path: path.display().to_string(),
            width_in: f64::from(b.art.width) / CSS_DPI,
            height_in: f64::from(b.art.height) / CSS_DPI,
        };
        let kind = CallKind::Pdf {
            artboard: b.art.id.clone(),
            path,
        };
        Ok((b.tab, op, kind))
    }

    fn start_inspect(&self, key: &str, p: &Value) -> Result<(TabId, AgentOp, CallKind), String> {
        let b = self.target_board(key, p)?;
        let selector = opt_str(p, "selector").map(str::to_string).or_else(|| {
            self.selection
                .as_ref()
                .filter(|s| s.tab == b.tab.0)
                .map(|s| s.picked.selector.clone())
        });
        let op = AgentOp::Eval {
            expression: inspect::script(selector.as_deref(), bool_param(p, "all")),
        };
        let kind = CallKind::Inspect {
            artboard: b.art.id.clone(),
        };
        Ok((b.tab, op, kind))
    }

    // ── wait / reload ─────────────────────────────────────────────

    fn begin_wait(&mut self, inc: Incoming, reload: bool) -> Task<Msg> {
        let p = inc.params.clone();
        let key = match self.target(&p) {
            Ok(key) => key,
            Err(e) => {
                inc.reply.err(e);
                return Task::none();
            }
        };
        let targets = if opt_str(&p, "artboard").is_some() {
            self.target_board(&key, &p).map(|b| vec![(b.tab.0, b.art.id.clone())])
        } else {
            Ok(self
                .live(&key)
                .map(|l| l.boards.iter().map(|b| (b.tab.0, b.art.id.clone())).collect())
                .unwrap_or_default())
        };
        let tabs = match targets {
            Ok(t) => t,
            Err(e) => {
                inc.reply.err(e);
                return Task::none();
            }
        };
        let secs = p
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(WAIT_DEFAULT_SECS)
            .max(1);
        let mut task = Task::none();
        // `wait` right after an edit: the watcher may not have acted yet.
        // Boards older than the files are reloaded first, so the wait means
        // "showing what is on disk".
        if reload || self.boards_stale(&key) {
            let ids: Vec<TabId> = tabs.iter().map(|(tab, _)| TabId(*tab)).collect();
            self.reload_tabs(&key, &ids);
            if self.is_shown(&key) {
                task = self.requery_later();
            }
        }
        let now = Instant::now();
        self.waits.push(Wait {
            inc,
            tabs,
            saw_loading: HashSet::new(),
            started: now,
            deadline: now + Duration::from_secs(secs),
        });
        self.check_waits();
        task
    }

    /// Answer every wait that is done or out of time. A board is done when
    /// it has painted, is idle, its `auto` height is measured, and it was
    /// seen loading (or has been idle past the grace).
    pub(super) fn check_waits(&mut self) {
        if self.waits.is_empty() {
            return;
        }
        let loading = self.loading_map();
        let now = Instant::now();
        let live: HashSet<u64> = self
            .live
            .iter()
            .flat_map(|l| l.boards.iter().map(|b| b.tab.0))
            .collect();
        let mut i = 0;
        while i < self.waits.len() {
            let w = &mut self.waits[i];
            for (tab, _) in &w.tabs {
                if loading.get(tab) == Some(&true) {
                    w.saw_loading.insert(*tab);
                }
            }
            let grace = now.duration_since(w.started) >= LOAD_GRACE;
            let ready = |tab: u64| {
                self.frames.is_painted(tab)
                    && loading.get(&tab) == Some(&false)
                    && self.loads.is_settled(tab)
                    && (w.saw_loading.contains(&tab) || self.loads.is_loaded(tab) || grace)
            };
            let gone: Vec<&str> = w
                .tabs
                .iter()
                .filter(|(tab, _)| !live.contains(tab))
                .map(|(_, id)| id.as_str())
                .collect();
            let waiting: Vec<&str> = w
                .tabs
                .iter()
                .filter(|(tab, _)| !ready(*tab))
                .map(|(_, id)| id.as_str())
                .collect();
            let answer = if !gone.is_empty() {
                Some(Err(format!("artboard closed: {}", gone.join(", "))))
            } else if waiting.is_empty() {
                let ids: Vec<&str> = w.tabs.iter().map(|(_, id)| id.as_str()).collect();
                Some(Ok(json!({
                    "artboards": ids,
                    "ms": now.duration_since(w.started).as_millis() as u64,
                })))
            } else if now >= w.deadline {
                Some(Err(format!("timed out waiting for: {}", waiting.join(", "))))
            } else {
                None
            };
            match answer {
                Some(result) => {
                    let w = self.waits.remove(i);
                    match result {
                        Ok(v) => w.inc.reply.ok(v),
                        Err(e) => w.inc.reply.err(e),
                    }
                }
                None => i += 1,
            }
        }
    }
}

fn default_shot_path(at: &CanvasRef, artboard: &str, ext: &str) -> PathBuf {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    sola_core::env::runtime_dir()
        .join("sola")
        .join("design")
        .join("shots")
        .join(format!("{}-{}-{ms}.{ext}", at.canvas, artboard))
}

fn opt_str<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(p, key).ok_or_else(|| format!("missing --{key}"))
}

fn bool_param(p: &Value, key: &str) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Element rects in an `inspect` reply: the one element, or every match
/// (`all`). Board-local CSS px.
fn inspected_rects(data: Option<&Value>) -> Vec<select::Rect> {
    let Some(data) = data else {
        return Vec::new();
    };
    let rect = |v: &Value| serde_json::from_value::<select::Rect>(v.get("rect")?.clone()).ok();
    match data.get("elements").and_then(Value::as_array) {
        Some(all) => all.iter().filter_map(rect).collect(),
        None => data.get("element").and_then(rect).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_rects_from_one_or_all() {
        let one = json!({"element": {"rect": {"x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0}}});
        assert_eq!(inspected_rects(Some(&one)).len(), 1);
        let all = json!({"elements": [
            {"rect": {"x": 0.0, "y": 0.0, "width": 5.0, "height": 5.0}},
            {"rect": {"x": 9.0, "y": 0.0, "width": 5.0, "height": 5.0}},
            {"no": "rect"}
        ]});
        assert_eq!(inspected_rects(Some(&all)).len(), 2);
        assert!(inspected_rects(Some(&json!({"element": null}))).is_empty());
        assert!(inspected_rects(None).is_empty());
    }
}
