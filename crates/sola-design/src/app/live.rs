//! Live boards: open / re-layout / reload a canvas, track loads, measure
//! `auto`-height artboards, and keep the canvas raster scale in force.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;
use sola_browser::agent::{AgentOp, AgentReply};
use sola_browser::engine::{Cmd, Engine, NavCmd, TabId};

use super::{App, OpenCanvas, Pending};
use crate::boards::{self, Op};
use crate::camera::Camera;
use crate::canvas;
use crate::inspect;
use crate::projects::CanvasRef;
use crate::watch;

impl App {
    pub(super) fn open_canvas(&mut self, at: CanvasRef) {
        if self.open.as_ref().is_some_and(|o| o.at == at) {
            return;
        }
        let Some(project) = self.catalog.project(&at.project) else {
            return;
        };
        let dir = canvas::canvas_dir(&project.root, &at.canvas);
        self.close_canvas();
        tracing::info!(canvas = %at.key(), dir = %dir.display(), "open canvas");
        self.open = Some(OpenCanvas {
            at: at.clone(),
            dir,
            error: None,
            web_root: None,
        });
        self.catalog.selected = Some(at);
        self.persist();
        self.camera = Camera::default();
        self.fit_pending = true;
        self.user_camera = false;
        self.human_camera = false;
        self.board_links.clear();
        self.mark_shown();
        self.relayout();
    }

    pub(super) fn close_canvas(&mut self) {
        for b in std::mem::take(&mut self.boards) {
            self.close_board(b.tab);
        }
        self.open = None;
        self.selection = None;
        self.focus = None;
    }

    /// Read `canvas.toml` and bring the boards in line. A bad file keeps
    /// the current boards and shows the error.
    pub(super) fn relayout(&mut self) {
        let Some(dir) = self.open.as_ref().map(|o| o.dir.clone()) else {
            return;
        };
        let loaded = canvas::load(&dir);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let canvas = match loaded {
            Ok(c) => {
                open.error = None;
                c
            }
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "load canvas");
                open.error = Some(e);
                return;
            }
        };
        // A new web root changes what the same URLs serve: reload.
        let root_moved = open.web_root != canvas.web_root && !self.boards.is_empty();
        open.web_root = canvas.web_root.clone();
        let mut canvas = canvas;
        let mut missing = HashMap::new();
        for art in &mut canvas.artboards {
            if let Some(file) = canvas::file_url_path(&art.url).filter(|f| !f.exists()) {
                missing.insert(art.id.clone(), file.display().to_string());
            }
            if let Some(url) = self.served_url(&art.url, &art.id, canvas.web_root.as_deref()) {
                art.url = url;
            }
        }
        let engine = &self.engine;
        let (boards, ops) = boards::sync(
            std::mem::take(&mut self.boards),
            &canvas.artboards,
            self.surface_cap,
            || engine.alloc_tab_id(),
        );
        self.boards = boards;
        for b in &mut self.boards {
            match missing.get(&b.art.id) {
                Some(path) => b.error = Some(format!("src not found: {path}")),
                None if b.error.as_deref().is_some_and(|e| e.starts_with("src not found")) => {
                    b.error = None;
                }
                None => {}
            }
        }
        for op in ops {
            self.apply(op);
        }
        let live = |tab: u64| self.boards.iter().any(|b| b.tab.0 == tab);
        if self.selection.as_ref().is_some_and(|s| !live(s.tab)) {
            self.selection = None;
        }
        if self.focus.is_some_and(|f| !live(f)) {
            self.focus = None;
        }
        if self.fit_pending {
            self.fit();
        }
        self.settle_rasters();
        self.details.poke(Instant::now());
        if root_moved {
            let tabs: Vec<TabId> = self.boards.iter().map(|b| b.tab).collect();
            self.reload_tabs(&tabs);
        }
    }

    fn apply(&mut self, op: Op) {
        let now = Instant::now();
        match op {
            Op::Open {
                tab,
                url,
                title,
                size,
            } => {
                self.frames.open(tab.0);
                self.loads.begin(tab.0, now);
                self.send(Cmd::OpenBoard {
                    id: tab,
                    url,
                    title,
                    size,
                });
            }
            Op::View { tab, size, resized } => {
                // An `auto` board only resizes here when its width changed:
                // its content height is stale.
                if resized && self.is_auto(tab) {
                    self.loads.begin(tab.0, now);
                }
                self.send(Cmd::SetBoard { id: tab, size });
            }
            Op::Load { tab, url } => {
                self.loads.begin(tab.0, now);
                self.agent(tab, AgentOp::Nav(NavCmd::LoadUrl(url)), None);
            }
            Op::Close { tab } => self.close_board(tab),
        }
    }

    /// A `file://` board URL as this project's served origin (its own
    /// host, looking in the canvas `web_root` first). `None` keeps the URL
    /// (web `src`, no server, or outside the project).
    fn served_url(&self, url: &str, artboard: &str, web_root: Option<&std::path::Path>) -> Option<String> {
        let server = self.serve.as_ref()?;
        let open = self.open.as_ref()?;
        let root = &self.catalog.project(&open.at.project)?.root;
        let cut = url.find(['?', '#']).unwrap_or(url.len());
        let (base, suffix) = url.split_at(cut);
        let file = canvas::file_url_path(base)?;
        server.url(&open.at.project, root, &file, suffix, artboard, &open.at.canvas, web_root)
    }

    /// The file a board resource URL names: a `file://` URL or this
    /// app's served origin.
    fn resource_path(&self, url: &str) -> Option<PathBuf> {
        canvas::file_url_path(url).or_else(|| self.serve.as_ref()?.local_path(url))
    }

    /// URL each board tab is showing (helper snapshot).
    pub(super) fn tab_urls(&self) -> HashMap<u64, String> {
        self.engine
            .tabs_handle()
            .lock()
            .unwrap()
            .iter()
            .map(|t| (t.id.0, t.url.clone()))
            .collect()
    }

    fn is_auto(&self, tab: TabId) -> bool {
        self.boards
            .iter()
            .any(|b| b.tab == tab && b.art.auto_height)
    }

    fn close_board(&mut self, tab: TabId) {
        self.frames.close(tab.0);
        self.details.forget(tab.0);
        self.glows.forget(tab.0);
        self.listed.remove(&tab.0);
        self.loads.forget(tab.0);
        self.board_links.remove(&tab.0);
        self.send(Cmd::CloseTab(tab));
    }

    /// The helper came back empty: drop the dead boards and reopen the
    /// layout on fresh tabs.
    pub(super) fn check_helper_restart(&mut self) {
        let live: HashSet<u64> = self
            .engine
            .tabs_handle()
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.id.0)
            .collect();
        let mut lost = false;
        for b in &self.boards {
            if live.contains(&b.tab.0) {
                self.listed.insert(b.tab.0);
            } else if self.listed.contains(&b.tab.0) {
                lost = true;
            }
        }
        if !lost {
            return;
        }
        tracing::warn!(boards = self.boards.len(), "board tabs gone (helper restarted) — reopening");
        for b in std::mem::take(&mut self.boards) {
            self.frames.close(b.tab.0);
        }
        self.listed.clear();
        self.loads.clear();
        self.details.clear();
        self.glows = Default::default();
        self.focus = None;
        self.relayout();
    }

    /// A watcher burst: re-layout for `canvas.toml`, reload for any other
    /// file — unless the boards already show it (a `reload` / `wait` beat
    /// the watcher to it).
    pub(super) fn on_canvas_change(&mut self, change: watch::Change) -> iced::Task<super::Msg> {
        if change.layout {
            self.relayout();
        }
        if change.content && self.boards_stale() {
            return self.reload_boards();
        }
        if change.layout {
            return self.requery_later();
        }
        iced::Task::none()
    }

    pub(super) fn reload_boards(&mut self) -> iced::Task<super::Msg> {
        let tabs: Vec<TabId> = self.boards.iter().map(|b| b.tab).collect();
        self.reload_tabs(&tabs);
        self.requery_later()
    }

    /// Files the boards loaded from outside `design/`, sorted.
    pub(super) fn linked(&self) -> Vec<PathBuf> {
        let all: BTreeSet<&PathBuf> = self.board_links.values().flatten().collect();
        all.into_iter().cloned().collect()
    }

    /// Some input file is newer than what the boards loaded.
    pub(super) fn boards_stale(&self) -> bool {
        let Some(dir) = self.open.as_ref().map(|o| o.dir.clone()) else {
            return false;
        };
        let newest = watch::newest_input(&dir, &self.linked());
        newest.is_some() && newest > self.shown_at
    }

    /// The boards are about to read the files as they are now.
    fn mark_shown(&mut self) {
        self.shown_at = self
            .open
            .as_ref()
            .and_then(|o| watch::newest_input(&o.dir, &self.linked()));
    }

    /// Hard-reload `tabs` (bypass the cache: the files just changed).
    /// A tab still on its intended URL hard-reloads; one showing anything
    /// else (a navigation that was cancelled, a click in the page) loads the
    /// intended URL — so a reload also repairs drift.
    pub(super) fn reload_tabs(&mut self, tabs: &[TabId]) {
        let now = Instant::now();
        self.mark_shown();
        let shown = self.tab_urls();
        for &tab in tabs {
            let Some(want) = self.boards.iter().find(|b| b.tab == tab).map(|b| b.art.url.clone())
            else {
                continue;
            };
            self.loads.begin(tab.0, now);
            self.glows.reloading(tab.0, now);
            let nav = if shown.get(&tab.0) == Some(&want) {
                NavCmd::ReloadIgnoreCache
            } else {
                NavCmd::LoadUrl(want)
            };
            self.agent(tab, AgentOp::Nav(nav), None);
        }
    }

    // ── loads + settle ────────────────────────────────────────────

    /// Fold the helper's tab snapshot into [`crate::loads::Loads`] and
    /// settle boards whose load has finished (fonts ready, then height and
    /// loaded files read back).
    pub(super) fn observe_loads(&mut self) {
        let loading = self.loading_map();
        self.loads
            .observe(&loading, self.boards.iter().map(|b| b.tab.0));
        let frames = &self.frames;
        let idle = |tab: u64| frames.is_painted(tab) && loading.get(&tab) == Some(&false);
        let now = Instant::now();
        let settles = self.loads.take_settles(now, idle);
        let follow_ups = self.loads.take_follow_ups(now);
        let settles = settles.into_iter().map(|(t, g)| (t, g, false));
        let follow_ups = follow_ups
            .into_iter()
            .filter(|&(t, _)| self.is_auto(TabId(t)))
            .map(|(t, g)| (t, g, true));
        for (tab, generation, follow_up) in settles.chain(follow_ups).collect::<Vec<_>>() {
            let op = AgentOp::Eval {
                expression: inspect::SETTLE.to_string(),
            };
            let why = Pending::Settle {
                tab,
                generation,
                follow_up,
            };
            self.agent(TabId(tab), op, Some(why));
        }
    }

    /// A board settled: flag an HTTP error page, record the files it loaded
    /// from outside `design/`, and resize an `auto` board to its content —
    /// then settle once more (bounded: [`crate::loads::MAX_CONFIRMS`]), since
    /// the new viewport can reflow the page. An `auto` board is measured
    /// again a few times after its first settle (`follow_up` replies), for
    /// content that arrives late. A reply for an earlier load of the tab is
    /// ignored.
    pub(super) fn on_settled(&mut self, tab: u64, generation: u64, follow_up: bool, reply: AgentReply) {
        if follow_up {
            if self.loads.generation(tab) != generation {
                return;
            }
        } else if !self.loads.settled(tab, generation) {
            return;
        }
        if !follow_up && self.is_auto(TabId(tab)) {
            self.loads.follow_up(tab, Instant::now());
        }
        let data = reply.ok.then_some(reply.data).flatten();
        let Some(data) = data else {
            tracing::warn!(tab, error = ?reply.error, "settle board");
            return;
        };
        self.note_links(tab, &data);
        let status = data.get("status").and_then(Value::as_u64).unwrap_or(0);
        if let Some(b) = self.boards.iter_mut().find(|b| b.tab.0 == tab) {
            if status >= 400 {
                // A missing `src` (found at layout) says more than its 404.
                if b.error.is_none() {
                    let url = data.get("url").and_then(Value::as_str).unwrap_or_default();
                    b.error = Some(format!("HTTP {status} for {url}"));
                }
                return;
            }
            if b.error.as_deref().is_some_and(|e| e.starts_with("HTTP ")) {
                b.error = None;
            }
        }
        let Some(height) = data.get("height").and_then(Value::as_f64) else {
            return;
        };
        let height = (height.ceil() as u32).clamp(1, canvas::MAX_SIDE);
        let Some(i) = self
            .boards
            .iter()
            .position(|b| b.tab.0 == tab && b.art.auto_height)
        else {
            return;
        };
        if self.boards[i].art.height == height {
            return;
        }
        self.boards[i].art.height = height;
        tracing::debug!(artboard = %self.boards[i].art.id, height, "auto artboard height");
        self.resize_boards(Some(tab));
        self.details.poke(Instant::now());
        self.loads.confirm(tab, Instant::now());
        if !self.user_camera {
            self.fit();
        }
    }

    fn note_links(&mut self, tab: u64, data: &Value) {
        let Some(design) = self.open.as_ref().and_then(|o| o.dir.parent().map(PathBuf::from)) else {
            return;
        };
        let links: BTreeSet<PathBuf> = data
            .get("resources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|u| self.resource_path(u))
            .filter(|p| !p.starts_with(&design))
            .collect();
        if self.board_links.get(&tab) != Some(&links) {
            tracing::debug!(tab, links = links.len(), "board linked files");
            self.board_links.insert(tab, links);
        }
    }

    // ── raster scale ──────────────────────────────────────────────

    /// Raster scale per board (see [`boards::host_scales`]).
    fn board_scales(&self) -> Vec<f64> {
        let sizes: Vec<(&str, u32, u32)> = self
            .boards
            .iter()
            .map(|b| (boards::url_host(&b.art.url), b.art.width, b.art.height))
            .collect();
        boards::host_scales(&sizes)
    }

    /// Bring every board to its scale (after a layout change).
    pub(super) fn settle_rasters(&mut self) {
        self.resize_boards(None);
    }

    /// Push each board's geometry (scale, surface), plus a `SetBoard` for
    /// `also` (a board whose size changed) even when it looks unchanged.
    fn resize_boards(&mut self, also: Option<u64>) {
        let scales = self.board_scales();
        let cap = self.surface_cap;
        let mut cmds = Vec::new();
        for (b, scale) in self.boards.iter_mut().zip(scales) {
            let size = boards::board_size(&b.art, scale, b.cap(cap));
            if size == b.size() && also != Some(b.tab.0) {
                continue;
            }
            if b.scale != scale {
                tracing::debug!(artboard = %b.art.id, from = b.scale, to = scale, "re-raster");
            }
            b.scale = size.scale;
            b.surface = size.surface;
            cmds.push(Cmd::SetBoard { id: b.tab, size });
        }
        for cmd in cmds {
            self.send(cmd);
        }
    }
}
