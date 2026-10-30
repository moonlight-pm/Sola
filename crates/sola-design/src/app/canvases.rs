//! Live canvases: the one in each project window, plus any an agent is
//! working on in the background. Each keeps its own boards, files, and
//! camera, so several agents can work at once and none of them moves what
//! the human sees. The active window's canvas is *shown*: its view lives
//! in the app. Every other canvas keeps its view parked — and a canvas
//! another window shows draws from it. Agents never bring a canvas to the
//! front (`--raise` is the explicit exception). Boards nobody looks at
//! sleep ([`crate::pace`]); when memory runs short, idle ones are discarded
//! ([`super::budget`]). A canvas no window shows that nobody has used for
//! [`IDLE`] closes; the next call reopens it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use iced::{Size, window};
use sola_browser::engine::{BoardPace, Cmd, TabId};

use super::budget::Admit;
use super::{App, Pending};
use crate::annot::Annotations;
use crate::boards::Board;
use crate::camera::{self, Camera};
use crate::canvas;
use crate::pace;
use crate::projects::CanvasRef;
use crate::select::Selection;
use crate::tour::{Notes, Tour};

/// An off-screen canvas closes after this long without a call.
pub(super) const IDLE: Duration = Duration::from_secs(10 * 60);
/// The top bar marks a canvas an agent called on within this.
pub(super) const AGENT_RECENT: Duration = Duration::from_secs(30);

pub(super) struct Live {
    pub at: CanvasRef,
    pub dir: PathBuf,
    /// Last `canvas.toml` read failure; the previous layout stays up.
    pub error: Option<String>,
    /// The canvas web root in force (`root` in `canvas.toml`).
    pub web_root: Option<PathBuf>,
    pub boards: Vec<Board>,
    /// Sections, notes, board links, and flows (last good `canvas.toml`).
    pub annot: Annotations,
    /// Files each board loaded from outside `design/` (watched too).
    pub links: HashMap<u64, BTreeSet<PathBuf>>,
    /// Newest input file time the boards were (re)loaded at.
    pub shown_at: Option<SystemTime>,
    /// Its view while off screen; `None` until first framed (fit on show).
    pub parked: Option<Parked>,
    /// Last call or look at it.
    pub touched: Instant,
    /// Last agent call on it.
    pub agent_at: Option<Instant>,
}

/// A canvas's view while another is shown: what its window draws, and
/// what comes back when it is shown again.
#[derive(Debug, Clone)]
pub(super) struct Parked {
    pub camera: Camera,
    pub fit_pending: bool,
    pub user_camera: bool,
    pub human_camera: bool,
    /// The human's pick, tour and open notes there (kept only while the
    /// canvas stays on screen in its window).
    pub selection: Option<Selection>,
    pub tour: Option<Tour>,
    pub notes: Notes,
    pub flows_open: bool,
}

impl Parked {
    fn fresh() -> Self {
        Self {
            camera: Camera::default(),
            fit_pending: true,
            user_camera: false,
            human_camera: false,
            selection: None,
            tour: None,
            notes: Notes::default(),
            flows_open: false,
        }
    }

    /// The canvas left the screen: its pick, tour and notes go.
    fn leave(&mut self) {
        self.selection = None;
        self.tour = None;
        self.notes = Notes::default();
        self.flows_open = false;
    }
}

impl Live {
    /// The camera that shows every board of this canvas in `view` (with
    /// room above for section titles when it has sections).
    pub fn fit_camera(&self, view: iced::Size) -> Option<Camera> {
        let content = camera::bounds_of(self.boards.iter().map(Board::rect))?;
        let top = if self.annot.sections.is_empty() { 0.0 } else { crate::routes::HEADER_ROOM };
        Some(Camera::fit_below(content, view, 1.0, top))
    }
}

impl App {
    pub(super) fn live(&self, key: &str) -> Option<&Live> {
        self.live.iter().find(|l| l.at.key() == key)
    }

    pub(super) fn live_mut(&mut self, key: &str) -> Option<&mut Live> {
        self.live.iter_mut().find(|l| l.at.key() == key)
    }

    pub(super) fn shown_live(&self) -> Option<&Live> {
        self.live(self.shown.as_deref()?)
    }

    /// Boards of the canvas on screen.
    pub(super) fn shown_boards(&self) -> &[Board] {
        self.shown_live().map_or(&[], |l| l.boards.as_slice())
    }

    pub(super) fn is_shown(&self, key: &str) -> bool {
        self.shown.as_deref() == Some(key)
    }

    /// Key of the canvas holding board `tab`.
    pub(super) fn canvas_of_tab(&self, tab: u64) -> Option<String> {
        self.live
            .iter()
            .find(|l| l.boards.iter().any(|b| b.tab.0 == tab))
            .map(|l| l.at.key())
    }

    pub(super) fn board_of(&self, tab: u64) -> Option<&Board> {
        self.live
            .iter()
            .flat_map(|l| l.boards.iter())
            .find(|b| b.tab.0 == tab)
    }

    pub(super) fn board_mut(&mut self, tab: u64) -> Option<&mut Board> {
        self.live
            .iter_mut()
            .flat_map(|l| l.boards.iter_mut())
            .find(|b| b.tab.0 == tab)
    }

    pub(super) fn tabs_of(&self, key: &str) -> Vec<u64> {
        self.live(key)
            .map(|l| l.boards.iter().map(|b| b.tab.0).collect())
            .unwrap_or_default()
    }

    /// `at` with live boards — opened in the background if need be.
    pub(super) fn ensure_live(&mut self, at: &CanvasRef) -> Option<String> {
        let key = at.key();
        if self.live(&key).is_none() {
            let root = self.catalog.project(&at.project)?.root.clone();
            let dir = canvas::canvas_dir(&root, &at.canvas);
            tracing::info!(canvas = %key, dir = %dir.display(), "canvas live");
            self.live.push(Live {
                at: at.clone(),
                dir,
                error: None,
                web_root: None,
                boards: Vec::new(),
                annot: Annotations::default(),
                links: HashMap::new(),
                shown_at: None,
                parked: None,
                touched: Instant::now(),
                agent_at: None,
            });
            self.mark_shown(&key);
            // Agents are admitted before this ([`App::ensure_live_for_agent`]).
            self.relayout(&key, Admit::Always);
        }
        self.touch(&key);
        Some(key)
    }

    pub(super) fn touch(&mut self, key: &str) {
        if let Some(l) = self.live_mut(key) {
            l.touched = Instant::now();
        }
    }

    /// An agent call on `key` (the rail shows it working there).
    pub(super) fn agent_touch(&mut self, key: &str) {
        let now = Instant::now();
        if let Some(l) = self.live_mut(key) {
            l.touched = now;
            l.agent_at = Some(now);
        }
    }

    /// Canvases an agent has called on lately.
    pub(super) fn agent_busy(&self) -> HashSet<String> {
        let now = Instant::now();
        self.live
            .iter()
            .filter(|l| {
                l.agent_at
                    .is_some_and(|t| now.duration_since(t) < AGENT_RECENT)
            })
            .map(|l| l.at.key())
            .collect()
    }

    /// The canvas project window `window` shows: its project's selected
    /// canvas while it is on disk, else the first. `None` for the
    /// no-project window or a project without canvases.
    pub(super) fn canvas_of(&self, window: window::Id) -> Option<CanvasRef> {
        let project = self.windows.project_of(window)?;
        let list = self.canvases.get(project)?;
        let selected = self.catalog.project(project)?.selected.as_deref();
        let name = selected
            .filter(|s| list.iter().any(|c| c.name == *s))
            .or_else(|| list.first().map(|c| c.name.as_str()))?;
        Some(CanvasRef {
            project: project.to_string(),
            canvas: name.to_string(),
        })
    }

    /// The window showing canvas `key`.
    pub(super) fn displayed(&self, key: &str) -> Option<window::Id> {
        self.windows
            .iter()
            .map(|(w, _)| w)
            .find(|w| self.canvas_of(*w).is_some_and(|at| at.key() == key))
    }

    /// The plane of the window showing `key`, else the active one's.
    pub(super) fn plane_of(&self, key: &str) -> Option<Size> {
        self.displayed(key)
            .and_then(|w| self.windows.get(w))
            .and_then(|w| w.plane)
            .or(self.plane_size)
    }

    /// The camera `key` is drawn with: the app's when shown, else its
    /// parked one.
    pub(super) fn view_camera(&self, key: &str) -> Option<Camera> {
        if self.is_shown(key) {
            return Some(self.camera);
        }
        self.live(key)?.parked.as_ref().map(|p| p.camera)
    }

    /// Put `at` on screen in its project's window, which becomes the active
    /// one: the human picked it (or asked an agent to show it). The canvas
    /// that was in that window leaves the screen, keeping its camera.
    pub(super) fn show_canvas(&mut self, at: CanvasRef) {
        let window = self.windows.window_of(&at.project);
        let before = window.and_then(|w| self.canvas_of(w)).map(|c| c.key());
        if self.catalog.selected.as_ref() != Some(&at) || before.as_deref() != Some(&at.key()) {
            self.catalog.select(&at);
            self.persist();
        }
        if let Some(old) = before.filter(|old| *old != at.key()) {
            self.leave_screen(&old);
        }
        // A closed window shows it when it opens.
        if let Some(window) = window {
            self.activate(window);
        }
    }

    /// `window` takes input from now on: its canvas comes off its parked
    /// view into the app. The canvas that was shown stays on screen in its
    /// own window, parked with its view, pick and tour.
    pub(super) fn activate(&mut self, window: window::Id) {
        self.windows.active = Some(window);
        if let Some(w) = self.windows.get(window) {
            self.plane_size = w.plane;
            self.scale_factor = w.scale;
        }
        let key = self.canvas_of(window).and_then(|at| self.ensure_live(&at));
        if key == self.shown {
            return;
        }
        self.park_shown(false);
        if let Some(key) = key {
            self.unpark(&key);
        }
        self.grow_surfaces();
    }

    /// Show `key`: its parked view (a fresh one the first time) comes into
    /// the app.
    fn unpark(&mut self, key: &str) {
        let p = self
            .live_mut(key)
            .and_then(|l| l.parked.take())
            .unwrap_or_else(Parked::fresh);
        self.camera = p.camera;
        self.fit_pending = p.fit_pending;
        self.user_camera = p.user_camera;
        self.human_camera = p.human_camera;
        self.selection = p.selection;
        self.tour = p.tour;
        self.tween = None;
        self.notes = p.notes;
        self.flows_open = p.flows_open;
        tracing::info!(canvas = %key, "show canvas");
        self.shown = Some(key.to_string());
        self.check_tour();
        if self.fit_pending {
            self.fit();
        }
        self.details.poke(Instant::now());
    }

    /// Park the shown canvas: its view, and — unless it is `leaving` the
    /// screen — the human's pick, tour and notes there, and its detail
    /// renders (its window still draws it). Keyboard focus on its boards
    /// goes either way.
    pub(super) fn park_shown(&mut self, leaving: bool) {
        let Some(key) = self.shown.take() else {
            return;
        };
        self.focus_board(None);
        self.tween = None;
        let mut parked = Parked {
            camera: self.camera,
            fit_pending: self.fit_pending,
            user_camera: self.user_camera,
            human_camera: self.human_camera,
            selection: self.selection.take(),
            tour: self.tour.take(),
            notes: std::mem::take(&mut self.notes),
            flows_open: std::mem::take(&mut self.flows_open),
        };
        if let Some(l) = self.live_mut(&key) {
            l.touched = Instant::now();
        }
        if leaving {
            parked.leave();
            self.retire_details(&key);
        }
        if let Some(l) = self.live_mut(&key) {
            l.parked = Some(parked);
        }
    }

    /// Canvas `key` is no longer in any window: it drops the human's pick,
    /// tour and notes there, and its detail renders.
    pub(super) fn leave_screen(&mut self, key: &str) {
        if self.is_shown(key) {
            self.park_shown(true);
            return;
        }
        if let Some(p) = self.live_mut(key).and_then(|l| l.parked.as_mut()) {
            p.leave();
        }
        self.retire_details(key);
    }

    fn retire_details(&mut self, key: &str) {
        for tab in self.tabs_of(key) {
            if let Some(ask) = self.details.retire(tab) {
                self.ask(ask);
            }
        }
    }

    /// Close a live canvas and its boards.
    pub(super) fn close_live(&mut self, key: &str) {
        if self.is_shown(key) {
            self.park_shown(true);
        }
        let Some(i) = self.live.iter().position(|l| l.at.key() == key) else {
            return;
        };
        let live = self.live.remove(i);
        tracing::info!(canvas = %key, boards = live.boards.len(), "canvas closed");
        let now = Instant::now();
        for b in live.boards {
            if !b.discarded {
                self.ledger.freed(b.cost(), now);
            }
            self.close_board(b.tab);
        }
    }

    /// Close off-screen canvases idle past [`IDLE`] with no call in flight.
    pub(super) fn evict_idle(&mut self, now: Instant) {
        let mut busy: HashSet<u64> = self
            .pending
            .values()
            .filter_map(|p| match p {
                Pending::Call { tab, .. } => Some(*tab),
                _ => None,
            })
            .collect();
        busy.extend(self.waits.iter().flat_map(|w| w.tabs()));
        let idle: Vec<String> = self
            .live
            .iter()
            .filter(|l| self.displayed(&l.at.key()).is_none())
            .filter(|l| now.duration_since(l.touched) >= IDLE)
            .filter(|l| !l.boards.iter().any(|b| busy.contains(&b.tab.0)))
            .map(|l| l.at.key())
            .collect();
        for key in idle {
            self.close_live(&key);
        }
    }

    /// Some canvas no window shows is live (the eviction tick runs).
    pub(super) fn has_background(&self) -> bool {
        self.live
            .iter()
            .any(|l| self.displayed(&l.at.key()).is_none())
    }

    /// Where canvas `key` is on the human's screen: the camera and plane
    /// of the window showing it, while the shell shows that window.
    pub(super) fn on_screen(&self, key: &str) -> Option<(Camera, Option<Size>)> {
        let window = self.displayed(key).filter(|w| self.windows.shown(*w))?;
        if self.is_shown(key) {
            return Some((self.camera, self.plane_size));
        }
        let plane = self.windows.get(window).and_then(|w| w.plane);
        Some((self.view_camera(key)?, plane))
    }

    /// Board `b` is in view of `screen` (see [`Self::on_screen`]; a plane
    /// not laid out yet counts as in view).
    pub(super) fn board_in_view(screen: Option<(Camera, Option<Size>)>, b: &Board) -> bool {
        screen.is_some_and(|(camera, plane)| {
            plane.is_none_or(|p| pace::in_view(b.rect(), &camera, p))
        })
    }

    /// Send each board its pace ([`crate::pace`]) where that changed. A
    /// discarded board that comes into view on the human's screen reopens;
    /// boards in view or working are marked seen ([`super::budget`]).
    pub(super) fn pace_boards(&mut self) {
        let busy = self.busy_tabs();
        let agents = self.agent_busy();
        let mut want = Vec::new();
        let mut seen = Vec::new();
        let mut wake = Vec::new();
        let mut visible = HashSet::new();
        let mut fastest = 0;
        for l in &self.live {
            let key = l.at.key();
            let shown = self.is_shown(&key);
            let screen = self.on_screen(&key);
            let readable = self
                .view_camera(&key)
                .is_some_and(|c| c.zoom >= pace::READABLE_ZOOM);
            for b in &l.boards {
                let in_view = Self::board_in_view(screen, b);
                if b.discarded {
                    if in_view {
                        wake.push(b.tab.0);
                    }
                    continue;
                }
                if in_view || busy.contains(&b.tab.0) {
                    seen.push(b.tab.0);
                }
                let look = pace::Look {
                    busy: busy.contains(&b.tab.0),
                    in_view,
                    focused: self.focused && shown,
                    readable,
                    agent: agents.contains(&key),
                };
                let pace = pace::pace(look);
                if in_view {
                    visible.insert(b.tab.0);
                    fastest = fastest.max(pace::hz(pace));
                }
                want.push((b.tab, pace));
            }
        }
        let now = Instant::now();
        for tab in seen {
            self.seen.insert(tab, now);
        }
        let open: HashSet<u64> = want.iter().map(|(tab, _)| tab.0).collect();
        self.paces.retain(|tab, _| open.contains(tab));
        for (tab, pace) in want {
            self.set_pace(tab, pace);
        }
        // Only boards in view wake the window, no faster than they paint.
        self.frames.set_visible(visible, fastest);
        if !wake.is_empty() {
            // The human's view: never refused.
            let _ = self.wake_boards(&wake, Admit::Always, "boards in view");
        }
    }

    /// Board `tab` paints at `pace` (sent only when that changes).
    pub(super) fn set_pace(&mut self, tab: TabId, pace: BoardPace) {
        let was = self.paces.insert(tab.0, pace).unwrap_or_default();
        if was != pace {
            self.send(Cmd::BoardPace { id: tab, pace });
        }
    }

    /// Boards working for someone: loading or settling, or answering a
    /// call or a pick. A page's height watch is not work.
    pub(super) fn busy_tabs(&self) -> HashSet<u64> {
        let mut busy: HashSet<u64> = self
            .pending
            .values()
            .filter_map(|p| match p {
                Pending::Pick { tab, .. }
                | Pending::Requery { tab }
                | Pending::Call { tab, .. }
                | Pending::Settle { tab, .. }
                // A re-read waits on page frames: at 1 fps it would crawl.
                | Pending::Marks { tab, .. } => Some(*tab),
                Pending::Height { .. } => None,
            })
            .collect();
        let unsettled = self
            .live
            .iter()
            .flat_map(|l| l.boards.iter())
            .filter(|b| !b.discarded)
            .map(|b| b.tab.0)
            .filter(|tab| !self.loads.is_settled(*tab));
        busy.extend(unsettled);
        busy
    }

    /// The camera of `key`, shown or parked; a never-framed parked canvas
    /// is fit to its window's plane first.
    pub(super) fn camera_of(&mut self, key: &str) -> Option<Camera> {
        if self.is_shown(key) {
            return Some(self.camera);
        }
        let plane = self.plane_of(key);
        let l = self.live_mut(key)?;
        if l.parked.is_none() {
            let mut p = Parked::fresh();
            if let Some(cam) = plane.and_then(|view| l.fit_camera(view)) {
                p.camera = cam;
                p.fit_pending = false;
            }
            l.parked = Some(p);
        }
        l.parked.as_ref().map(|p| p.camera)
    }

    /// Re-fit a parked canvas the human has not framed (its `auto` boards
    /// grew, or its window laid out).
    pub(super) fn refit_parked(&mut self, key: &str) {
        let plane = self.plane_of(key);
        let Some(l) = self.live_mut(key) else {
            return;
        };
        let cam = plane.and_then(|view| l.fit_camera(view));
        if let (Some(p), Some(cam)) = (l.parked.as_mut(), cam) {
            if !p.user_camera {
                p.camera = cam;
                p.fit_pending = false;
            }
        }
    }
}
