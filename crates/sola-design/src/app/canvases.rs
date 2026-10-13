//! Live canvases: the one on screen, plus any an agent is working on in
//! the background. Each keeps its own boards, files, and camera, so several
//! agents can work at once and none of them moves what the human sees.
//! Agents never bring a canvas to the front (`--raise` is the explicit
//! exception). Boards nobody looks at sleep ([`crate::pace`]). An
//! off-screen canvas nobody has used for [`IDLE`] closes; the next call
//! reopens it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use sola_browser::engine::{BoardPace, Cmd, TabId};

use super::{App, Pending};
use crate::boards::Board;
use crate::camera::{self, Camera};
use crate::canvas;
use crate::pace;
use crate::projects::CanvasRef;

/// An off-screen canvas closes after this long without a call.
pub(super) const IDLE: Duration = Duration::from_secs(10 * 60);
/// The rail marks a canvas an agent called on within this.
const AGENT_RECENT: Duration = Duration::from_secs(30);

pub(super) struct Live {
    pub at: CanvasRef,
    pub dir: PathBuf,
    /// Last `canvas.toml` read failure; the previous layout stays up.
    pub error: Option<String>,
    /// The canvas web root in force (`root` in `canvas.toml`).
    pub web_root: Option<PathBuf>,
    pub boards: Vec<Board>,
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

/// A canvas's view while another is on screen.
#[derive(Debug, Clone, Copy)]
pub(super) struct Parked {
    pub camera: Camera,
    pub fit_pending: bool,
    pub user_camera: bool,
    pub human_camera: bool,
}

impl Parked {
    fn fresh() -> Self {
        Self {
            camera: Camera::default(),
            fit_pending: true,
            user_camera: false,
            human_camera: false,
        }
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
                links: HashMap::new(),
                shown_at: None,
                parked: None,
                touched: Instant::now(),
                agent_at: None,
            });
            self.mark_shown(&key);
            self.relayout(&key);
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

    /// Put `at` on screen: the human picked it (or asked an agent to show
    /// it). The canvas that was there keeps its view for later.
    pub(super) fn show_canvas(&mut self, at: CanvasRef) {
        let Some(key) = self.ensure_live(&at) else {
            return;
        };
        if self.is_shown(&key) {
            return;
        }
        self.park_shown();
        let p = self
            .live_mut(&key)
            .and_then(|l| l.parked.take())
            .unwrap_or_else(Parked::fresh);
        self.camera = p.camera;
        self.fit_pending = p.fit_pending;
        self.user_camera = p.user_camera;
        self.human_camera = p.human_camera;
        tracing::info!(canvas = %key, "show canvas");
        self.shown = Some(key);
        self.catalog.selected = Some(at);
        self.persist();
        if self.fit_pending {
            self.fit();
        }
        self.details.poke(Instant::now());
    }

    /// Take the canvas on screen off it: keep its view, drop the human's
    /// focus and pick there, and its detail renders.
    pub(super) fn park_shown(&mut self) {
        let Some(key) = self.shown.take() else {
            return;
        };
        self.focus_board(None);
        self.selection = None;
        let parked = Parked {
            camera: self.camera,
            fit_pending: self.fit_pending,
            user_camera: self.user_camera,
            human_camera: self.human_camera,
        };
        for tab in self.tabs_of(&key) {
            if let Some(ask) = self.details.retire(tab) {
                self.ask(ask);
            }
        }
        if let Some(l) = self.live_mut(&key) {
            l.parked = Some(parked);
            l.touched = Instant::now();
        }
    }

    /// Close a live canvas and its boards.
    pub(super) fn close_live(&mut self, key: &str) {
        if self.is_shown(key) {
            self.park_shown();
        }
        let Some(i) = self.live.iter().position(|l| l.at.key() == key) else {
            return;
        };
        let live = self.live.remove(i);
        tracing::info!(canvas = %key, boards = live.boards.len(), "canvas closed");
        for b in live.boards {
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
            .filter(|l| !self.is_shown(&l.at.key()))
            .filter(|l| now.duration_since(l.touched) >= IDLE)
            .filter(|l| !l.boards.iter().any(|b| busy.contains(&b.tab.0)))
            .map(|l| l.at.key())
            .collect();
        for key in idle {
            self.close_live(&key);
        }
    }

    /// Some canvas is live off screen (the eviction tick runs).
    pub(super) fn has_background(&self) -> bool {
        self.live.iter().any(|l| !self.is_shown(&l.at.key()))
    }

    /// Send each board its pace ([`crate::pace`]) where that changed.
    pub(super) fn pace_boards(&mut self) {
        let busy = self.busy_tabs();
        let agents = self.agent_busy();
        let mut want = Vec::new();
        for l in &self.live {
            let key = l.at.key();
            let shown = self.is_shown(&key);
            for b in &l.boards {
                let in_view = shown
                    && self
                        .plane_size
                        .is_none_or(|p| pace::in_view(b.rect(), &self.camera, p));
                let look = pace::Look {
                    busy: busy.contains(&b.tab.0),
                    in_view,
                    focused: self.focused,
                    agent: agents.contains(&key),
                };
                want.push((b.tab, pace::pace(look)));
            }
        }
        let open: HashSet<u64> = want.iter().map(|(tab, _)| tab.0).collect();
        self.paces.retain(|tab, _| open.contains(tab));
        for (tab, pace) in want {
            self.set_pace(tab, pace);
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
    fn busy_tabs(&self) -> HashSet<u64> {
        let mut busy: HashSet<u64> = self
            .pending
            .values()
            .filter_map(|p| match p {
                Pending::Pick { tab, .. }
                | Pending::Requery { tab }
                | Pending::Call { tab, .. }
                | Pending::Settle { tab, .. } => Some(*tab),
                Pending::Height { .. } => None,
            })
            .collect();
        let unsettled = self
            .live
            .iter()
            .flat_map(|l| l.boards.iter())
            .map(|b| b.tab.0)
            .filter(|tab| !self.loads.is_settled(*tab));
        busy.extend(unsettled);
        busy
    }

    /// The camera of `key`, on screen or parked; a never-framed off-screen
    /// canvas is fit to the plane first.
    pub(super) fn camera_of(&mut self, key: &str) -> Option<Camera> {
        if self.is_shown(key) {
            return Some(self.camera);
        }
        let plane = self.plane_size;
        let l = self.live_mut(key)?;
        if l.parked.is_none() {
            let mut p = Parked::fresh();
            if let (Some(view), Some(content)) =
                (plane, camera::bounds_of(l.boards.iter().map(Board::rect)))
            {
                p.camera = Camera::fit(content, view, 1.0);
                p.fit_pending = false;
            }
            l.parked = Some(p);
        }
        l.parked.map(|p| p.camera)
    }

    /// Re-fit an off-screen canvas the human has not framed (its `auto`
    /// boards grew).
    pub(super) fn refit_parked(&mut self, key: &str) {
        let plane = self.plane_size;
        let Some(l) = self.live_mut(key) else {
            return;
        };
        let content = camera::bounds_of(l.boards.iter().map(Board::rect));
        if let (Some(p), Some(view), Some(content)) = (l.parked.as_mut(), plane, content) {
            if !p.user_camera {
                p.camera = Camera::fit(content, view, 1.0);
                p.fit_pending = false;
            }
        }
    }
}
