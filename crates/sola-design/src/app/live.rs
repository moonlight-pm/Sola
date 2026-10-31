//! Board work for a live canvas (see [`super::canvases`]): re-layout,
//! reload, loads and settles, `auto` heights, and raster scales. Everything
//! takes the canvas key; on-screen or not makes no difference here.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;
use sola_browser::agent::{AgentOp, AgentReply};
use sola_browser::engine::{Cmd, Engine, NavCmd, TabId};

use super::budget::Admit;
use super::{App, Pending};
use crate::boards::{self, Op};
use crate::canvas;
use crate::inspect;
use crate::marks::Marks;
use crate::memory;
use crate::projects::CanvasRef;
use crate::watch;

impl App {
    /// Read `canvas.toml` and bring the boards of `key` in line. A bad
    /// file keeps the current boards and shows the error. New boards open
    /// as `admit` says; one refused stays discarded (no picture) until it
    /// fits.
    pub(super) fn relayout(&mut self, key: &str, admit: Admit) {
        let Some(dir) = self.live(key).map(|l| l.dir.clone()) else {
            return;
        };
        let loaded = canvas::load(&dir);
        let Some(live) = self.live_mut(key) else {
            return;
        };
        let canvas = match loaded {
            Ok(c) => {
                live.error = None;
                c
            }
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "load canvas");
                live.error = Some(e);
                return;
            }
        };
        // A new web root changes what the same URLs serve: reload.
        let root_moved = live.web_root != canvas.web_root && !live.boards.is_empty();
        live.web_root = canvas.web_root.clone();
        let pins_moved = live.annot.notes != canvas.annot.notes;
        live.annot = canvas.annot.clone();
        let at = live.at.clone();
        let current = std::mem::take(&mut live.boards);
        let mut canvas = canvas;
        let mut missing = HashMap::new();
        for art in &mut canvas.artboards {
            if let Some(file) = canvas::file_url_path(&art.url).filter(|f| !f.exists()) {
                missing.insert(art.id.clone(), file.display().to_string());
            }
            if let Some(url) = self.served_url(&at, &art.url, &art.id, canvas.web_root.as_deref()) {
                art.url = url;
            }
        }
        let engine = &self.engine;
        let (mut synced, ops) =
            boards::sync(current, &canvas.artboards, self.surface_cap, || engine.alloc_tab_id());
        for b in &mut synced {
            match missing.get(&b.art.id) {
                Some(path) => b.error = Some(format!("src not found: {path}")),
                None if b.error.as_deref().is_some_and(|e| e.starts_with("src not found")) => {
                    b.error = None;
                }
                None => {}
            }
        }
        let tabs: HashSet<u64> = synced.iter().map(|b| b.tab.0).collect();
        if let Some(live) = self.live_mut(key) {
            live.boards = synced;
        }
        for op in ops {
            self.apply(key, op, admit);
        }
        if self.is_shown(key) {
            self.check_tour();
            if self.selection.as_ref().is_some_and(|s| !tabs.contains(&s.tab)) {
                self.selection = None;
            }
            if self.focus.is_some_and(|f| !tabs.contains(&f)) {
                self.focus = None;
            }
            if self.fit_pending {
                self.fit();
            }
        } else if let Some(p) = self.live_mut(key).and_then(|l| l.parked.as_mut()) {
            // Another window draws it: a pick on a board that went goes.
            if p.selection.as_ref().is_some_and(|s| !tabs.contains(&s.tab)) {
                p.selection = None;
            }
        }
        self.settle_rasters(key);
        self.details.poke(Instant::now());
        if root_moved {
            let tabs: Vec<TabId> = tabs.into_iter().map(TabId).collect();
            self.reload_tabs(key, &tabs);
        } else if pins_moved {
            // Notes pinned to new elements: measure the pages again.
            self.remeasure(key);
        }
    }

    /// Read what each settled board of `key` declares again (no reload).
    fn remeasure(&mut self, key: &str) {
        let tabs: Vec<u64> = self
            .tabs_of(key)
            .into_iter()
            .filter(|&t| self.loads.is_settled(t) && self.frames.is_painted(t))
            .collect();
        for tab in tabs {
            self.remeasure_tab(tab);
        }
    }

    /// Read what board `tab`'s page declares again.
    fn remeasure_tab(&mut self, tab: u64) {
        let why = Pending::Marks {
            tab,
            generation: self.loads.generation(tab),
        };
        let expression = self.settle_script(tab);
        self.agent(TabId(tab), AgentOp::Eval { expression }, Some(why));
    }

    /// A re-read of board `tab`'s marks (same load only).
    pub(super) fn on_marks(&mut self, tab: u64, generation: u64, reply: AgentReply) {
        if self.loads.generation(tab) != generation {
            return;
        }
        let Some(data) = reply.ok.then_some(reply.data).flatten() else {
            return;
        };
        let key = self.canvas_of_tab(tab);
        let board = key
            .and_then(|k| self.live_mut(&k))
            .and_then(|l| l.boards.iter_mut().find(|b| b.tab.0 == tab));
        if let Some(b) = board {
            b.marks = Marks::from_settle(&data);
        }
    }

    /// The settle script for board `tab`, with the notes pinned into it.
    fn settle_script(&self, tab: u64) -> String {
        let pins = self
            .canvas_of_tab(tab)
            .and_then(|key| {
                let live = self.live(&key)?;
                let board = live.boards.iter().find(|b| b.tab.0 == tab)?;
                Some(live.annot.pins(&board.art.id))
            })
            .unwrap_or_default();
        inspect::settle(&pins)
    }

    fn apply(&mut self, key: &str, op: Op, admit: Admit) {
        let now = Instant::now();
        match op {
            Op::Open {
                tab,
                url,
                title,
                size,
            } => {
                let cost = memory::board_cost(size.surface);
                if admit == Admit::IfRoom {
                    let what = format!("new artboard `{title}` on `{key}`");
                    if let Err(e) = self.make_room(cost, &HashSet::from([tab.0]), &what) {
                        tracing::warn!(canvas = %key, artboard = %title, reason = %e, "board held: memory");
                        if let Some(b) = self.board_mut(tab.0) {
                            b.discarded = true;
                        }
                        return;
                    }
                }
                self.frames.open(tab.0);
                self.loads.begin(tab.0, now);
                self.seen.insert(tab.0, now);
                self.ledger.open(tab.0, cost, now);
                // Born `Live`.
                self.paces.remove(&tab.0);
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
    fn served_url(
        &self,
        at: &CanvasRef,
        url: &str,
        artboard: &str,
        web_root: Option<&std::path::Path>,
    ) -> Option<String> {
        let server = self.serve.as_ref()?;
        let root = &self.catalog.project(&at.project)?.root;
        let cut = url.find(['?', '#']).unwrap_or(url.len());
        let (base, suffix) = url.split_at(cut);
        let file = canvas::file_url_path(base)?;
        server.url(&at.project, root, &file, suffix, artboard, &at.canvas, web_root)
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
        self.board_of(tab.0).is_some_and(|b| b.art.auto_height)
    }

    /// The board is gone from its canvas: its tab, picture, and links.
    pub(super) fn close_board(&mut self, tab: TabId) {
        self.shut_tab(tab);
        self.pictures.remove(&tab.0);
        self.seen.remove(&tab.0);
        for l in &mut self.live {
            l.links.remove(&tab.0);
        }
    }

    /// Close board `tab`'s page and forget its page state (the board
    /// itself may stay, discarded).
    pub(super) fn shut_tab(&mut self, tab: TabId) {
        self.frames.close(tab.0);
        self.details.forget(tab.0);
        self.glows.forget(tab.0);
        self.listed.remove(&tab.0);
        self.loads.forget(tab.0);
        self.ledger.landed(tab.0);
        // A height watch never answers for a closed tab.
        self.pending
            .retain(|_, p| !matches!(p, Pending::Height { tab: t, .. } if *t == tab.0));
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
        // A discarded board's tab is gone on purpose.
        for b in self.live.iter().flat_map(|l| l.boards.iter()).filter(|b| !b.discarded) {
            if live.contains(&b.tab.0) {
                self.listed.insert(b.tab.0);
            } else if self.listed.contains(&b.tab.0) {
                lost = true;
            }
        }
        if !lost {
            return;
        }
        let keys: Vec<String> = self.live.iter().map(|l| l.at.key()).collect();
        tracing::warn!(canvases = keys.len(), "board tabs gone (helper restarted) — reopening");
        for l in &mut self.live {
            for b in std::mem::take(&mut l.boards) {
                self.frames.close(b.tab.0);
            }
            l.links.clear();
        }
        self.pictures.clear();
        self.seen.clear();
        self.listed.clear();
        self.loads.clear();
        // The old helper's height watches never answer.
        self.pending.retain(|_, p| !matches!(p, Pending::Height { .. }));
        self.details.clear();
        self.glows = Default::default();
        self.focus = None;
        for key in keys {
            self.relayout(&key, Admit::Always);
        }
    }

    /// A watcher burst: re-layout for `canvas.toml`, reload for any other
    /// file — unless the boards already show it (a `reload` / `wait` beat
    /// the watcher to it).
    pub(super) fn on_canvas_change(&mut self, key: &str, change: watch::Change) -> iced::Task<super::Msg> {
        if change.layout {
            // New boards on the human's screen always open; elsewhere they
            // wait for room.
            let admit = if self.displayed(key).is_some() { Admit::Always } else { Admit::IfRoom };
            self.relayout(key, admit);
        }
        if change.content && self.boards_stale(key) {
            return self.reload_boards(key);
        }
        if change.layout && self.is_shown(key) {
            return self.requery_later();
        }
        iced::Task::none()
    }

    pub(super) fn reload_boards(&mut self, key: &str) -> iced::Task<super::Msg> {
        let tabs: Vec<TabId> = self.tabs_of(key).into_iter().map(TabId).collect();
        self.reload_tabs(key, &tabs);
        if self.is_shown(key) { self.requery_later() } else { iced::Task::none() }
    }

    /// Files the boards of `key` loaded from outside `design/`, sorted.
    pub(super) fn linked(&self, key: &str) -> Vec<PathBuf> {
        let Some(live) = self.live(key) else {
            return Vec::new();
        };
        let all: BTreeSet<&PathBuf> = live.links.values().flatten().collect();
        all.into_iter().cloned().collect()
    }

    /// Some input file of `key` is newer than what its boards loaded.
    pub(super) fn boards_stale(&self, key: &str) -> bool {
        let Some(live) = self.live(key) else {
            return false;
        };
        let newest = watch::newest_input(&live.dir, &self.linked(key));
        newest.is_some() && newest > live.shown_at
    }

    /// The boards of `key` are about to read the files as they are now.
    pub(super) fn mark_shown(&mut self, key: &str) {
        let Some(dir) = self.live(key).map(|l| l.dir.clone()) else {
            return;
        };
        let newest = watch::newest_input(&dir, &self.linked(key));
        if let Some(l) = self.live_mut(key) {
            l.shown_at = newest;
        }
    }

    /// Hard-reload `tabs` (bypass the cache: the files just changed).
    /// A tab still on its intended URL hard-reloads; one showing anything
    /// else (a navigation that was cancelled, a click in the page) loads the
    /// intended URL — so a reload also repairs drift. A discarded board is
    /// skipped: it loads the files fresh when it wakes.
    pub(super) fn reload_tabs(&mut self, key: &str, tabs: &[TabId]) {
        let now = Instant::now();
        self.mark_shown(key);
        let shown = self.tab_urls();
        for &tab in tabs {
            let Some(want) = self
                .board_of(tab.0)
                .filter(|b| !b.discarded)
                .map(|b| b.art.url.clone())
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
        let tabs: Vec<u64> = self.live.iter().flat_map(|l| l.boards.iter().map(|b| b.tab.0)).collect();
        self.loads.observe(&loading, tabs);
        let frames = &self.frames;
        let idle = |tab: u64| frames.is_painted(tab) && loading.get(&tab) == Some(&false);
        let now = Instant::now();
        for (tab, generation) in self.loads.take_settles(now, idle) {
            let op = AgentOp::Eval {
                expression: self.settle_script(tab),
            };
            self.agent(TabId(tab), op, Some(Pending::Settle { tab, generation }));
        }
    }

    /// A board settled: flag an HTTP error page, record the files it loaded
    /// from outside `design/`, and resize an `auto` board to its content —
    /// then settle once more (bounded: [`crate::loads::MAX_CONFIRMS`]), since
    /// the new viewport can reflow the page. An `auto` board's page then
    /// watches its height for the rest of the load ([`Self::on_height`]), for
    /// content that arrives late. A reply for an earlier load of the tab is
    /// ignored.
    pub(super) fn on_settled(&mut self, tab: u64, generation: u64, reply: AgentReply) {
        if !self.loads.settled(tab, generation) {
            return;
        }
        let data = reply.ok.then_some(reply.data).flatten();
        let Some(data) = data else {
            tracing::warn!(tab, error = ?reply.error, "settle board");
            return;
        };
        let Some(key) = self.canvas_of_tab(tab) else {
            return;
        };
        self.note_links(&key, tab, &data);
        let status = data.get("status").and_then(Value::as_u64).unwrap_or(0);
        let Some(live) = self.live_mut(&key) else {
            return;
        };
        if let Some(b) = live.boards.iter_mut().find(|b| b.tab.0 == tab) {
            b.marks = Marks::from_settle(&data);
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
        if self.is_auto(TabId(tab)) && self.loads.watch(tab) {
            self.watch_height(tab, generation, height);
        }
        self.set_auto_height(&key, tab, height);
    }

    /// Ask the page of `auto` board `tab` for its next content-height change
    /// from `known` (what it measured last).
    fn watch_height(&mut self, tab: u64, generation: u64, known: f64) {
        let expression = format!("{}({})", inspect::WATCH_HEIGHT, known.ceil() as u64);
        self.agent(
            TabId(tab),
            AgentOp::Eval { expression },
            Some(Pending::Height { tab, generation }),
        );
    }

    /// The page of `auto` board `tab` changed height: watch for the next
    /// change, and resize the board — unless the page only follows its
    /// viewport (`vh`) and has grown with the board too often in a row.
    pub(super) fn on_height(&mut self, tab: u64, generation: u64, reply: AgentReply) {
        if self.loads.generation(tab) != generation || !self.is_auto(TabId(tab)) {
            return;
        }
        let data = reply.ok.then_some(reply.data).flatten();
        let Some(height) = data
            .as_ref()
            .and_then(|d| d.get("height"))
            .and_then(Value::as_f64)
        else {
            // Navigated or closed; the next load watches again.
            tracing::debug!(tab, error = ?reply.error, "height watch ended");
            self.loads.unwatch(tab);
            return;
        };
        self.watch_height(tab, generation, height);
        // The page moved: its exits and pinned elements may have too.
        self.remeasure_tab(tab);
        let reflow = data
            .as_ref()
            .and_then(|d| d.get("reflow"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !self.loads.reflowed(tab, reflow) {
            tracing::debug!(tab, height, "auto board follows its viewport; not resized");
            return;
        }
        if let Some(key) = self.canvas_of_tab(tab) {
            self.set_auto_height(&key, tab, height);
        }
    }

    /// Size `auto` board `tab` of canvas `key` to its content `height`.
    fn set_auto_height(&mut self, key: &str, tab: u64, height: f64) {
        let height = (height.ceil() as u32).clamp(1, canvas::MAX_SIDE);
        let Some(b) = self.live_mut(key).and_then(|live| {
            live.boards
                .iter_mut()
                .find(|b| b.tab.0 == tab && b.art.auto_height)
        }) else {
            return;
        };
        if b.art.height == height {
            return;
        }
        b.art.height = height;
        tracing::debug!(artboard = %b.art.id, height, "auto artboard height");
        let artboard = b.art.id.clone();
        // The web share lays the canvas out with Design's heights.
        if let Some(share) = &self.share {
            share.set_height(key, &artboard, height);
        }
        self.resize_boards(key, Some(tab));
        self.details.poke(Instant::now());
        self.loads.confirm(tab, Instant::now());
        if self.is_shown(key) {
            if !self.user_camera {
                self.fit();
            }
        } else {
            self.refit_parked(key);
        }
    }

    fn note_links(&mut self, key: &str, tab: u64, data: &Value) {
        let Some(design) = self.live(key).and_then(|l| l.dir.parent().map(PathBuf::from)) else {
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
        let Some(live) = self.live_mut(key) else {
            return;
        };
        if live.links.get(&tab) != Some(&links) {
            tracing::debug!(tab, links = links.len(), "board linked files");
            live.links.insert(tab, links);
            // The share serves exactly these outside files for the canvas.
            if let Some(share) = &self.share {
                share.set_links(key, self.linked(key).into_iter().collect());
            }
        }
    }

    // ── raster scale ──────────────────────────────────────────────

    /// Raster scale per board of `key` (see [`boards::host_scales`]).
    fn board_scales(&self, key: &str) -> Vec<f64> {
        let Some(live) = self.live(key) else {
            return Vec::new();
        };
        let sizes: Vec<(&str, u32, u32)> = live
            .boards
            .iter()
            .map(|b| (boards::url_host(&b.art.url), b.art.width, b.art.height))
            .collect();
        boards::host_scales(&sizes)
    }

    /// Bring every board of `key` to its scale (after a layout change).
    pub(super) fn settle_rasters(&mut self, key: &str) {
        self.resize_boards(key, None);
    }

    /// Push each board's geometry (scale, surface), plus a `SetBoard` for
    /// `also` (a board whose size changed) even when it looks unchanged.
    fn resize_boards(&mut self, key: &str, also: Option<u64>) {
        let scales = self.board_scales(key);
        let cap = self.surface_cap;
        let Some(live) = self.live_mut(key) else {
            return;
        };
        let mut cmds = Vec::new();
        for (b, scale) in live.boards.iter_mut().zip(scales) {
            let size = boards::board_size(&b.art, scale, b.cap(cap));
            if size == b.size() && also != Some(b.tab.0) {
                continue;
            }
            if b.discarded {
                // No tab: it opens at this geometry when woken.
                b.scale = size.scale;
                b.surface = size.surface;
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
