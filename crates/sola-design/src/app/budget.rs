//! Boards within the memory budget ([`crate::memory`]). When the machine
//! runs short, idle boards are discarded — their tab closes, the plane keeps
//! their picture — least recently used first. A discarded board reopens when
//! it comes into view or an agent's call needs it. An agent is refused only
//! when the boards in use still do not fit, and is told why.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sola_browser::engine::{Cmd, TabId};

use super::App;
use super::canvases::{AGENT_RECENT, Live};
use crate::boards;
use crate::canvas;
use crate::memory::{self, Candidate, MemInfo};
use crate::projects::CanvasRef;

/// How often the machine's free memory is checked while boards are open.
pub(super) const BUDGET_TICK: Duration = Duration::from_secs(2);
/// A board is idle once nobody has looked at it or worked on it this long.
const IDLE_AFTER: Duration = Duration::from_secs(30);

/// Whether boards may open regardless of the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Admit {
    /// The human's view (or already admitted): make room if possible, open
    /// anyway.
    Always,
    /// An agent's: open only if there is room after discarding idle boards.
    IfRoom,
}

impl App {
    /// Short on memory: discard idle boards until the reserve (and some
    /// slack) is free again.
    pub(super) fn budget_tick(&mut self) {
        let now = Instant::now();
        self.settle_ledger(now);
        let Some(info) = memory::read() else {
            return;
        };
        let free = self.ledger.project(info.available);
        let keep = memory::reserve(info.total);
        if free >= keep {
            self.budget_short = false;
            return;
        }
        let need = keep + memory::SLACK - free;
        let freed = self.discard_idle(need, &HashSet::new(), now);
        if freed < need && !self.budget_short {
            tracing::warn!(
                free = %memory::size(free),
                keep_free = %memory::size(keep),
                open = %memory::size(self.open_cost()),
                "memory short and no idle boards left to discard"
            );
        }
        self.budget_short = freed < need;
    }

    /// Opens whose boards have painted and settled show in `MemAvailable`.
    fn settle_ledger(&mut self, now: Instant) {
        self.ledger.expire(now);
        let landed: Vec<u64> = self
            .ledger
            .opening()
            .filter(|tab| self.frames.is_painted(*tab) && self.loads.is_settled(*tab))
            .collect();
        for tab in landed {
            self.ledger.landed(tab);
        }
    }

    /// Room for `cost` more bytes of boards, discarding idle boards (never
    /// `keep`) for it. `Err` explains to an agent why there is none; `what`
    /// names what it asked for.
    pub(super) fn make_room(
        &mut self,
        cost: u64,
        keep: &HashSet<u64>,
        what: &str,
    ) -> Result<(), String> {
        let Some(info) = memory::read() else {
            return Ok(());
        };
        let now = Instant::now();
        self.settle_ledger(now);
        let reserve = memory::reserve(info.total);
        let free = self.ledger.project(info.available);
        if free >= reserve + cost {
            return Ok(());
        }
        self.discard_idle(reserve + cost - free, keep, now);
        let free = self.ledger.project(info.available);
        if free >= reserve + cost {
            return Ok(());
        }
        let message = self.short_message(what, cost, info, free, reserve);
        tracing::warn!(%what, need = %memory::size(cost), free = %memory::size(free), "refused: memory");
        Err(message)
    }

    /// Discard idle boards (never `keep`), least recently used first, for
    /// `need` bytes. The bytes freed (estimated).
    fn discard_idle(&mut self, need: u64, keep: &HashSet<u64>, now: Instant) -> u64 {
        let picked = memory::pick(self.idle_boards(keep, now), need);
        let mut freed = 0;
        for c in picked {
            freed += c.cost;
            self.discard_board(TabId(c.tab), now);
        }
        freed
    }

    /// Open boards nobody is using: not in view on the human's screen, not
    /// working (loading, settling, a call, a wait), not on a canvas an agent
    /// called lately, not the human's focus or pick, and unused for
    /// [`IDLE_AFTER`].
    fn idle_boards(&self, keep: &HashSet<u64>, now: Instant) -> Vec<Candidate> {
        let busy = self.busy_tabs();
        let waiting: HashSet<u64> = self.waits.iter().flat_map(|w| w.tabs()).collect();
        let agents = self.agent_busy();
        let held: HashSet<u64> = self
            .focus
            .into_iter()
            .chain(self.selection.as_ref().map(|s| s.tab))
            .collect();
        let mut out = Vec::new();
        for l in self.live.iter().filter(|l| !agents.contains(&l.at.key())) {
            let shown = self.is_shown(&l.at.key());
            for b in l.boards.iter().filter(|b| !b.discarded) {
                let tab = b.tab.0;
                let working = busy.contains(&tab) || waiting.contains(&tab) || held.contains(&tab);
                if keep.contains(&tab) || working || (shown && self.in_view(b)) {
                    continue;
                }
                let used = self.used_at(l, tab, now);
                if now.duration_since(used) >= IDLE_AFTER {
                    out.push(Candidate {
                        tab,
                        cost: b.cost(),
                        used,
                    });
                }
            }
        }
        out
    }

    /// When board `tab` of `live` was last looked at or worked on.
    fn used_at(&self, live: &Live, tab: u64, now: Instant) -> Instant {
        let seen = self.seen.get(&tab).copied().unwrap_or(now);
        live.agent_at.map_or(seen, |a| a.max(seen))
    }

    /// Close board `tab`'s page to free its memory. The plane keeps its
    /// last picture; its layout, links, and share stay.
    pub(super) fn discard_board(&mut self, tab: TabId, now: Instant) {
        let picture = self.frames.is_painted(tab.0);
        let Some((key, b)) = self.live.iter_mut().find_map(|l| {
            let key = l.at.key();
            l.boards.iter_mut().find(|b| b.tab == tab).map(|b| (key, b))
        }) else {
            return;
        };
        let cost = b.cost();
        let artboard = b.art.id.clone();
        b.discarded = true;
        // It reopens at its own size; a detail render grows it again.
        b.grown = false;
        b.surface = b.size().pixels();
        self.shut_tab(tab);
        self.paces.remove(&tab.0);
        self.seen.remove(&tab.0);
        if picture {
            self.pictures.insert(tab.0);
        }
        self.ledger.landed(tab.0);
        self.ledger.freed(cost, now);
        tracing::info!(canvas = %key, %artboard, cost = %memory::size(cost), "board discarded");
    }

    /// Reopen the discarded boards among `tabs`. With [`Admit::IfRoom`] an
    /// agent is refused (nothing reopens) when they do not fit; `what` names
    /// them in the reason.
    pub(super) fn wake_boards(
        &mut self,
        tabs: &[u64],
        admit: Admit,
        what: &str,
    ) -> Result<(), String> {
        let sleeping: Vec<(TabId, u64)> = tabs
            .iter()
            .filter_map(|t| self.board_of(*t))
            .filter(|b| b.discarded)
            .map(|b| (b.tab, b.cost()))
            .collect();
        if sleeping.is_empty() {
            return Ok(());
        }
        let cost = sleeping.iter().map(|(_, c)| c).sum();
        let keep: HashSet<u64> = tabs.iter().copied().collect();
        if let Err(e) = self.make_room(cost, &keep, what) {
            if admit == Admit::IfRoom {
                return Err(e);
            }
        }
        for (tab, _) in sleeping {
            self.reopen_board(tab, admit == Admit::IfRoom);
        }
        Ok(())
    }

    /// Open a discarded board's page again (its picture shows until it
    /// paints). An agent's wake glows like a reload.
    fn reopen_board(&mut self, tab: TabId, glow: bool) {
        let now = Instant::now();
        let Some(b) = self.board_mut(tab.0) else {
            return;
        };
        b.discarded = false;
        let (url, title, size, cost) = (b.art.url.clone(), b.art.title.clone(), b.size(), b.cost());
        let artboard = b.art.id.clone();
        self.frames.open(tab.0);
        self.loads.begin(tab.0, now);
        // Born `Live`.
        self.paces.remove(&tab.0);
        self.seen.insert(tab.0, now);
        self.ledger.open(tab.0, cost, now);
        if glow {
            self.glows.reloading(tab.0, now);
        }
        self.send(Cmd::OpenBoard {
            id: tab,
            url,
            title,
            size,
        });
        tracing::info!(%artboard, cost = %memory::size(cost), "board woken");
    }

    /// Estimated bytes of every open board.
    pub(super) fn open_cost(&self) -> u64 {
        self.live
            .iter()
            .flat_map(|l| l.boards.iter())
            .filter(|b| !b.discarded)
            .map(boards::Board::cost)
            .sum()
    }

    /// Estimated bytes `at`'s boards take when it opens (its layout now).
    fn canvas_cost(&self, at: &CanvasRef) -> (u64, usize) {
        let Some(root) = self.catalog.project(&at.project).map(|p| &p.root) else {
            return (0, 0);
        };
        let Ok(c) = canvas::load(&canvas::canvas_dir(root, &at.canvas)) else {
            return (0, 0);
        };
        let cost = c
            .artboards
            .iter()
            .map(|a| {
                let scale = boards::raster_scale(a.width, a.height);
                memory::board_cost(boards::board_size(a, scale, (0, 0)).surface)
            })
            .sum();
        (cost, c.artboards.len())
    }

    /// `at` live for an agent — refused, with the reason, when its boards
    /// do not fit after discarding idle ones.
    pub(super) fn ensure_live_for_agent(&mut self, at: &CanvasRef) -> Result<String, String> {
        if self.live(&at.key()).is_none() {
            let (cost, n) = self.canvas_cost(at);
            let what = format!("opening canvas `{}` ({n} artboards)", at.canvas);
            self.make_room(cost, &HashSet::new(), &what)?;
        }
        self.ensure_live(at)
            .ok_or_else(|| format!("no project {} for canvas {}", at.project, at.canvas))
    }

    /// The machine's memory and Design's share of it, for `status`.
    pub(super) fn memory_json(&self) -> Value {
        let boards: Vec<&boards::Board> = self.live.iter().flat_map(|l| l.boards.iter()).collect();
        let discarded = boards.iter().filter(|b| b.discarded).count();
        let mut v = json!({
            "boards_mib": memory::mib(self.open_cost()),
            "open_boards": boards.len() - discarded,
            "discarded_boards": discarded,
        });
        if let Some(info) = memory::read() {
            v["free_mib"] = json!(memory::mib(info.available));
            v["total_mib"] = json!(memory::mib(info.total));
            v["keep_free_mib"] = json!(memory::mib(memory::reserve(info.total)));
        }
        v
    }

    /// Why an agent cannot have `what` (`cost` bytes): the numbers, what is
    /// open and why it is kept, and the way out.
    fn short_message(
        &self,
        what: &str,
        cost: u64,
        info: MemInfo,
        free: u64,
        reserve: u64,
    ) -> String {
        let now = Instant::now();
        let mut lines = vec![format!(
            "Design is short on memory: {what} needs about {}, and the machine has {} free of {} — \
             Design keeps {} free for everything else. Idle boards are already discarded.",
            memory::size(cost),
            memory::size(free),
            memory::size(info.total),
            memory::size(reserve),
        )];
        lines.push("Open now:".into());
        let agents = self.agent_busy();
        for l in &self.live {
            let key = l.at.key();
            let open: Vec<&boards::Board> = l.boards.iter().filter(|b| !b.discarded).collect();
            if open.is_empty() {
                continue;
            }
            let bytes: u64 = open.iter().map(|b| b.cost()).sum();
            let why = if self.is_shown(&key) {
                "on the human's screen".to_string()
            } else if agents.contains(&key) {
                let ago = l.agent_at.map_or(0, |t| now.duration_since(t).as_secs());
                format!("an agent called {ago} s ago")
            } else {
                "loading, answering a call, or used in the last minute".to_string()
            };
            let project = self
                .catalog
                .project(&l.at.project)
                .map_or(l.at.project.as_str(), |p| p.name.as_str());
            let put_away = l.boards.len() - open.len();
            let away = if put_away > 0 {
                format!(" ({put_away} discarded)")
            } else {
                String::new()
            };
            lines.push(format!(
                "  {project} / {} — {} boards{away}, ~{}, {why} ({})",
                l.at.canvas,
                open.len(),
                memory::size(bytes),
                l.dir.display(),
            ));
        }
        lines.push(format!(
            "Close a canvas you are done with (solactl design canvas.close --canvas <dir>; MCP \
             design_canvas_close), wait for another agent to finish (boards idle {}s are discarded \
             on their own), or ask the human.",
            IDLE_AFTER.as_secs() + AGENT_RECENT.as_secs()
        ));
        lines.join("\n")
    }
}
