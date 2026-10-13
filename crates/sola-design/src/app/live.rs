//! Live boards: open / re-layout / reload a canvas, track loads, measure
//! `auto`-height artboards, and keep the canvas raster scale in force.

use std::collections::HashSet;
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
        });
        self.catalog.selected = Some(at);
        self.persist();
        self.camera = Camera::default();
        self.fit_pending = true;
        self.user_camera = false;
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
        let engine = &self.engine;
        let (boards, ops) = boards::sync(
            std::mem::take(&mut self.boards),
            &canvas.artboards,
            || engine.alloc_tab_id(),
        );
        self.boards = boards;
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
    }

    fn apply(&mut self, op: Op) {
        let now = Instant::now();
        match op {
            Op::Open {
                tab,
                url,
                title,
                width,
                height,
                scale,
            } => {
                self.frames.open(tab.0);
                self.loads.begin(tab.0, self.is_auto(tab), now);
                self.send(Cmd::OpenBoard {
                    id: tab,
                    url,
                    title,
                    width,
                    height,
                    scale,
                });
            }
            Op::View {
                tab,
                width,
                height,
                scale,
            } => {
                // An `auto` board only resizes here when its width changed:
                // its content height is stale.
                if self.is_auto(tab) {
                    self.loads.begin(tab.0, true, now);
                }
                self.send(Cmd::SetBoard {
                    id: tab,
                    width,
                    height,
                    scale,
                });
            }
            Op::Load { tab, url } => {
                self.loads.begin(tab.0, self.is_auto(tab), now);
                self.agent(tab, AgentOp::Nav(NavCmd::LoadUrl(url)), None);
            }
            Op::Close { tab } => self.close_board(tab),
        }
    }

    fn is_auto(&self, tab: TabId) -> bool {
        self.boards
            .iter()
            .any(|b| b.tab == tab && b.art.auto_height)
    }

    fn close_board(&mut self, tab: TabId) {
        self.frames.close(tab.0);
        self.listed.remove(&tab.0);
        self.loads.forget(tab.0);
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
        self.focus = None;
        self.relayout();
    }

    pub(super) fn on_canvas_change(&mut self, change: watch::Change) -> iced::Task<super::Msg> {
        match change {
            watch::Change::Layout => {
                self.relayout();
                self.requery_later()
            }
            watch::Change::Content => self.reload_boards(),
        }
    }

    pub(super) fn reload_boards(&mut self) -> iced::Task<super::Msg> {
        let tabs: Vec<TabId> = self.boards.iter().map(|b| b.tab).collect();
        self.reload_tabs(&tabs);
        self.requery_later()
    }

    /// Hard-reload `tabs` (bypass the cache: the files just changed).
    pub(super) fn reload_tabs(&mut self, tabs: &[TabId]) {
        let now = Instant::now();
        for &tab in tabs {
            self.loads.begin(tab.0, self.is_auto(tab), now);
            self.agent(tab, AgentOp::Nav(NavCmd::ReloadIgnoreCache), None);
        }
    }

    // ── loads + auto height ───────────────────────────────────────

    /// Fold the helper's tab snapshot into [`crate::loads::Loads`] and
    /// measure `auto` boards whose load has settled.
    pub(super) fn observe_loads(&mut self) {
        let loading = self.loading_map();
        self.loads
            .observe(&loading, self.boards.iter().map(|b| b.tab.0));
        let frames = &self.frames;
        let idle = |tab: u64| frames.is_painted(tab) && loading.get(&tab) == Some(&false);
        for tab in self.loads.take_measures(Instant::now(), idle) {
            let op = AgentOp::Eval {
                expression: inspect::MEASURE_HEIGHT.to_string(),
            };
            self.agent(TabId(tab), op, Some(Pending::Measure { tab }));
        }
    }

    /// Content height back from an `auto` board: resize its viewport.
    /// Measured once per load, so `vh`-based content cannot run away.
    pub(super) fn on_measured(&mut self, tab: u64, reply: AgentReply) {
        self.loads.measured(tab);
        let height = reply
            .ok
            .then_some(reply.data.as_ref())
            .flatten()
            .and_then(Value::as_f64);
        let Some(height) = height else {
            tracing::warn!(tab, error = ?reply.error, "measure auto artboard");
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
        if !self.user_camera {
            self.fit();
        }
    }

    // ── raster scale ──────────────────────────────────────────────

    /// The canvas raster scale (2×, or 1× if some board cannot fit 2×).
    fn canvas_scale(&self) -> f64 {
        boards::canvas_scale(self.boards.iter().map(|b| (b.art.width, b.art.height)))
    }

    /// Bring every board to the canvas scale (after a layout change).
    pub(super) fn settle_rasters(&mut self) {
        self.resize_boards(None);
    }

    /// Push the canvas scale to every board, plus a `SetBoard` for `also`
    /// (a board whose size changed) even when its scale did not.
    fn resize_boards(&mut self, also: Option<u64>) {
        let scale = self.canvas_scale();
        let mut cmds = Vec::new();
        for b in &mut self.boards {
            if b.scale == scale && also != Some(b.tab.0) {
                continue;
            }
            if b.scale != scale {
                tracing::debug!(artboard = %b.art.id, from = b.scale, to = scale, "re-raster");
            }
            b.scale = scale;
            cmds.push(Cmd::SetBoard {
                id: b.tab,
                width: b.art.width,
                height: b.art.height,
                scale,
            });
        }
        for cmd in cmds {
            self.send(cmd);
        }
    }
}
