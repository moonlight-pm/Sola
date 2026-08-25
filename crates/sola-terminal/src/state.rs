use std::collections::{BTreeMap, HashMap, HashSet};

use iced::widget::canvas;

use crate::emulator::Emulator;
use crate::pty::PtyBackend;

pub use sola_bus::topics::{PaneLayout, SplitDir};

/// Divider thickness in logical pixels — must match the kit `split`
/// hit strip (`sola_kit::components::DIVIDER_HIT_PX`) so pane rects
/// line up with what's drawn.
pub const DIVIDER_PX: f32 = sola_kit::components::DIVIDER_HIT_PX;

/// A plain rectangle in window-logical pixels. Kept iced-free so the
/// layout helpers stay unit-testable headlessly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Live per-PANE runtime: the alacritty emulator + the PTY backend
/// handle + that pane's canvas geometry cache. (Was `TabRuntime`,
/// keyed by tab id; now keyed by `PaneId` since a tab can host many.)
///
/// The MAIN side reads `emulator` for the renderer and calls
/// `emulator.resize()`; the reader thread drives a cloned term handle.
/// The runtime is dropped when the pane is removed — but the explicit
/// close path calls `backend.close()` FIRST, because a plain drop
/// deliberately preserves the tmux session (so a crash doesn't nuke it).
pub struct PaneRuntime {
    pub emulator: Emulator,
    pub backend: PtyBackend,
    /// Cached canvas geometry for this pane's grid. Cleared on this
    /// pane's PtyOutput so the next `view` re-renders from the live Term.
    pub cache: canvas::Cache,
}

/// Per-pane metadata: the PTY identity persisted (indirectly) on the bus.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneMeta {
    pub id: String,
    pub tmux_session: String,
    pub cwd: Option<String>,
}

/// Binary split tree of panes within one tab.
///
/// `Leaf(PaneId)` is a single shell; `Split` carries a stable
/// `SplitId` (so a divider keeps its identity across rebuilds), the
/// orientation, and pane `a`'s fraction of the main axis (`ratio`).
#[derive(Clone, Debug, PartialEq)]
pub enum PaneNode {
    Leaf(String),
    Split {
        id: String,
        dir: SplitDir,
        ratio: f32,
        a: Box<PaneNode>,
        b: Box<PaneNode>,
    },
}

/// One tab in the strip: a pane tree, the focused pane, and an ordinal.
#[derive(Clone, Debug)]
pub struct Tab {
    pub id: String,
    pub layout: PaneNode,
    pub active_pane: String,
    pub ordinal: u32,
}

/// Lightweight per-tab view for the sidebar + reorder gesture (no
/// runtime). `cwd` is the active pane's cwd (the tab label follows it).
#[derive(Clone, Debug, PartialEq)]
pub struct TabView {
    pub id: String,
    pub cwd: Option<String>,
    pub ordinal: u32,
}

/// Runtime model. Tabs keyed by tab id (ordered by ordinal); panes
/// (runtime + meta) keyed by pane id.
#[derive(Default)]
pub struct Tabs {
    tabs: BTreeMap<String, Tab>,
    panes: HashMap<String, PaneRuntime>,
    pane_meta: HashMap<String, PaneMeta>,
}

impl Tabs {
    // --- tabs ---

    pub fn upsert_tab(&mut self, t: Tab) {
        self.tabs.insert(t.id.clone(), t);
    }

    pub fn get_tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.get(id)
    }

    pub fn get_tab_mut(&mut self, id: &str) -> Option<&mut Tab> {
        self.tabs.get_mut(id)
    }

    /// Remove a tab (its panes must be removed separately via
    /// `remove_pane`, which is where the runtime drop / tmux policy lives).
    pub fn remove_tab(&mut self, id: &str) {
        self.tabs.remove(id);
    }

    pub fn tab_ids_in_order(&self) -> Vec<String> {
        let mut v: Vec<&Tab> = self.tabs.values().collect();
        v.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then(a.id.cmp(&b.id)));
        v.into_iter().map(|t| t.id.clone()).collect()
    }

    /// Per-tab strip rows in display order. `cwd` is the active pane's cwd.
    pub fn tab_strip(&self) -> Vec<TabView> {
        self.tab_ids_in_order()
            .into_iter()
            .filter_map(|id| {
                let t = self.tabs.get(&id)?;
                let cwd = self
                    .pane_meta
                    .get(&t.active_pane)
                    .and_then(|m| m.cwd.clone());
                Some(TabView {
                    id: t.id.clone(),
                    cwd,
                    ordinal: t.ordinal,
                })
            })
            .collect()
    }

    /// The tab id that owns `pane_id`, if any.
    pub fn tab_of_pane(&self, pane_id: &str) -> Option<String> {
        self.tabs
            .values()
            .find(|t| leaves_of(&t.layout).iter().any(|p| p == pane_id))
            .map(|t| t.id.clone())
    }

    // --- panes ---

    pub fn insert_pane_runtime(&mut self, pane_id: String, rt: PaneRuntime) {
        self.panes.insert(pane_id, rt);
    }

    pub fn pane_runtime(&self, pane_id: &str) -> Option<&PaneRuntime> {
        self.panes.get(pane_id)
    }

    pub fn has_pane_runtime(&self, pane_id: &str) -> bool {
        self.panes.contains_key(pane_id)
    }

    /// Drop a pane's runtime + meta. The caller must call
    /// `backend.close()` first if it wants the tmux session GONE (a
    /// plain drop preserves it).
    pub fn remove_pane(&mut self, pane_id: &str) {
        self.panes.remove(pane_id);
        self.pane_meta.remove(pane_id);
    }

    pub fn upsert_pane_meta(&mut self, m: PaneMeta) {
        self.pane_meta.insert(m.id.clone(), m);
    }

    pub fn pane_meta(&self, pane_id: &str) -> Option<&PaneMeta> {
        self.pane_meta.get(pane_id)
    }

    pub fn pane_cwd(&self, pane_id: &str) -> Option<String> {
        self.pane_meta.get(pane_id).and_then(|m| m.cwd.clone())
    }

    /// Clear one pane's geometry cache (after its PtyOutput).
    pub fn clear_pane_cache(&self, pane_id: &str) {
        if let Some(rt) = self.panes.get(pane_id) {
            rt.cache.clear();
        }
    }

    /// Clear every pane's geometry cache (e.g. blink tick, theme change).
    pub fn clear_all_caches(&self) {
        for rt in self.panes.values() {
            rt.cache.clear();
        }
    }

    /// Build the persistable layout for a tab (regenerating its
    /// `PaneLayout` from the live tree + pane metas).
    pub fn layout_of(&self, tab_id: &str) -> Option<PaneLayout> {
        let t = self.tabs.get(tab_id)?;
        Some(to_layout(&t.layout, &self.pane_meta))
    }
}

// ---------------------------------------------------------------------------
// Pure tree helpers (no I/O; unit-tested below).
// ---------------------------------------------------------------------------

/// All pane ids in left-to-right / top-to-bottom order.
pub fn leaves_of(node: &PaneNode) -> Vec<String> {
    let mut out = Vec::new();
    collect_leaves(node, &mut out);
    out
}

fn collect_leaves(node: &PaneNode, out: &mut Vec<String>) {
    match node {
        PaneNode::Leaf(id) => out.push(id.clone()),
        PaneNode::Split { a, b, .. } => {
            collect_leaves(a, out);
            collect_leaves(b, out);
        }
    }
}

/// The first (top-left-most) leaf of a subtree.
pub fn first_leaf(node: &PaneNode) -> String {
    match node {
        PaneNode::Leaf(id) => id.clone(),
        PaneNode::Split { a, .. } => first_leaf(a),
    }
}

/// Replace `Leaf(target)` with a 50/50 `Split` of `(target, new_pane)`.
/// Returns true when the target was found and replaced.
pub fn split_leaf(
    node: &mut PaneNode,
    target: &str,
    split_id: &str,
    dir: SplitDir,
    new_pane: &str,
) -> bool {
    match node {
        PaneNode::Leaf(id) => {
            if id == target {
                *node = PaneNode::Split {
                    id: split_id.to_string(),
                    dir,
                    ratio: 0.5,
                    a: Box::new(PaneNode::Leaf(id.clone())),
                    b: Box::new(PaneNode::Leaf(new_pane.to_string())),
                };
                true
            } else {
                false
            }
        }
        PaneNode::Split { a, b, .. } => {
            split_leaf(a, target, split_id, dir, new_pane)
                || split_leaf(b, target, split_id, dir, new_pane)
        }
    }
}

/// Drop `Leaf(target)`, promoting its sibling subtree into the parent's
/// place. Returns the rebuilt tree, or `None` when `target` was the only
/// leaf (caller then closes the tab).
pub fn close_leaf(node: PaneNode, target: &str) -> Option<PaneNode> {
    match node {
        PaneNode::Leaf(id) => {
            if id == target {
                None
            } else {
                Some(PaneNode::Leaf(id))
            }
        }
        PaneNode::Split {
            id,
            dir,
            ratio,
            a,
            b,
        } => {
            let new_a = close_leaf(*a, target);
            let new_b = close_leaf(*b, target);
            match (new_a, new_b) {
                (Some(a), Some(b)) => Some(PaneNode::Split {
                    id,
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                // The subtree that still has both children is unchanged;
                // when one side collapses to None, promote the other.
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            }
        }
    }
}

/// First leaf of the sibling of `target` — the pane to focus after
/// `target` is closed. Finds the parent split where `target` is a direct
/// leaf child and returns the other child's first leaf.
pub fn sibling_first_leaf(node: &PaneNode, target: &str) -> Option<String> {
    if let PaneNode::Split { a, b, .. } = node {
        if matches!(&**a, PaneNode::Leaf(x) if x == target) {
            return Some(first_leaf(b));
        }
        if matches!(&**b, PaneNode::Leaf(x) if x == target) {
            return Some(first_leaf(a));
        }
        return sibling_first_leaf(a, target).or_else(|| sibling_first_leaf(b, target));
    }
    None
}

/// Set the `ratio` of the split identified by `split_id`. Returns true
/// when found.
pub fn set_ratio(node: &mut PaneNode, split_id: &str, value: f32) -> bool {
    match node {
        PaneNode::Leaf(_) => false,
        PaneNode::Split {
            id, ratio, a, b, ..
        } => {
            if id == split_id {
                *ratio = value;
                return true;
            }
            set_ratio(a, split_id, value) || set_ratio(b, split_id, value)
        }
    }
}

fn split_area(area: Rect, dir: SplitDir, ratio: f32) -> (Rect, Rect) {
    let r = ratio.clamp(0.0, 1.0);
    match dir {
        SplitDir::Vertical => {
            let avail = (area.w - DIVIDER_PX).max(0.0);
            let aw = (avail * r).round();
            let bw = (avail - aw).max(0.0);
            (
                Rect {
                    x: area.x,
                    y: area.y,
                    w: aw,
                    h: area.h,
                },
                Rect {
                    x: area.x + aw + DIVIDER_PX,
                    y: area.y,
                    w: bw,
                    h: area.h,
                },
            )
        }
        SplitDir::Horizontal => {
            let avail = (area.h - DIVIDER_PX).max(0.0);
            let ah = (avail * r).round();
            let bh = (avail - ah).max(0.0);
            (
                Rect {
                    x: area.x,
                    y: area.y,
                    w: area.w,
                    h: ah,
                },
                Rect {
                    x: area.x,
                    y: area.y + ah + DIVIDER_PX,
                    w: area.w,
                    h: bh,
                },
            )
        }
    }
}

/// Per-leaf rectangles for a content `area`, partitioning by each
/// split's `dir`/`ratio` minus divider thickness.
pub fn pane_rects(node: &PaneNode, area: Rect) -> Vec<(String, Rect)> {
    let mut out = Vec::new();
    collect_pane_rects(node, area, &mut out);
    out
}

fn collect_pane_rects(node: &PaneNode, area: Rect, out: &mut Vec<(String, Rect)>) {
    match node {
        PaneNode::Leaf(id) => out.push((id.clone(), area)),
        PaneNode::Split {
            dir, ratio, a, b, ..
        } => {
            let (ra, rb) = split_area(area, *dir, *ratio);
            collect_pane_rects(a, ra, out);
            collect_pane_rects(b, rb, out);
        }
    }
}

/// Per-split rectangles (+ orientation), used to turn a divider drag
/// into a ratio. Same partition as `pane_rects`.
pub fn split_rects(node: &PaneNode, area: Rect) -> Vec<(String, Rect, SplitDir)> {
    let mut out = Vec::new();
    collect_split_rects(node, area, &mut out);
    out
}

fn collect_split_rects(node: &PaneNode, area: Rect, out: &mut Vec<(String, Rect, SplitDir)>) {
    if let PaneNode::Split {
        id,
        dir,
        ratio,
        a,
        b,
    } = node
    {
        out.push((id.clone(), area, *dir));
        let (ra, rb) = split_area(area, *dir, *ratio);
        collect_split_rects(a, ra, out);
        collect_split_rects(b, rb, out);
    }
}

/// Turn a cursor position over split `area` into a clamped ratio,
/// keeping each side at least `min_px` along the main axis.
pub fn ratio_for_drag(area: Rect, dir: SplitDir, cx: f32, cy: f32, min_px: f32) -> f32 {
    let (pos, len) = match dir {
        SplitDir::Vertical => (cx - area.x, area.w),
        SplitDir::Horizontal => (cy - area.y, area.h),
    };
    if len <= 0.0 {
        return 0.5;
    }
    let min_frac = (min_px / len).clamp(0.02, 0.48);
    (pos / len).clamp(min_frac, 1.0 - min_frac)
}

/// Serialize a live tree + pane metas into a persistable [`PaneLayout`].
pub fn to_layout(node: &PaneNode, meta: &HashMap<String, PaneMeta>) -> PaneLayout {
    match node {
        PaneNode::Leaf(id) => {
            let m = meta.get(id);
            PaneLayout::Leaf {
                tmux_session: m
                    .map(|m| m.tmux_session.clone())
                    .unwrap_or_else(|| crate::tmux::session_name(id)),
                cwd: m.and_then(|m| m.cwd.clone()),
            }
        }
        PaneNode::Split {
            dir, ratio, a, b, ..
        } => PaneLayout::Split {
            dir: *dir,
            ratio: *ratio,
            a: Box::new(to_layout(a, meta)),
            b: Box::new(to_layout(b, meta)),
        },
    }
}

fn fresh_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Rebuild a tree from a persisted [`PaneLayout`], minting fresh
/// pane/split ids and collecting one [`PaneMeta`] per leaf.
pub fn from_layout(layout: &PaneLayout, metas: &mut Vec<PaneMeta>) -> PaneNode {
    match layout {
        PaneLayout::Leaf { tmux_session, cwd } => {
            let id = fresh_id();
            metas.push(PaneMeta {
                id: id.clone(),
                tmux_session: tmux_session.clone(),
                cwd: cwd.clone(),
            });
            PaneNode::Leaf(id)
        }
        PaneLayout::Split { dir, ratio, a, b } => PaneNode::Split {
            id: fresh_id(),
            dir: *dir,
            ratio: *ratio,
            a: Box::new(from_layout(a, metas)),
            b: Box::new(from_layout(b, metas)),
        },
    }
}

/// Prune leaves whose tmux session isn't in `live` (when the boot
/// snapshot is known). Returns the surviving layout, or `None` when
/// every leaf is dead (the whole tab is retracted).
pub fn reconcile_layout(layout: PaneLayout, live: &Option<HashSet<String>>) -> Option<PaneLayout> {
    match live {
        None => Some(layout), // tmux query failed → admit everything
        Some(live) => prune_dead(layout, live),
    }
}

fn prune_dead(layout: PaneLayout, live: &HashSet<String>) -> Option<PaneLayout> {
    match layout {
        PaneLayout::Leaf { tmux_session, cwd } => {
            if live.contains(&tmux_session) {
                Some(PaneLayout::Leaf { tmux_session, cwd })
            } else {
                None
            }
        }
        PaneLayout::Split { dir, ratio, a, b } => {
            let na = prune_dead(*a, live);
            let nb = prune_dead(*b, live);
            match (na, nb) {
                (Some(a), Some(b)) => Some(PaneLayout::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            }
        }
    }
}

/// Next ordinal for a new tab: `max(existing) + 1`, or 0 when empty.
pub fn next_ordinal(existing: &[u32]) -> u32 {
    existing.iter().copied().max().map(|m| m + 1).unwrap_or(0)
}

/// The cwd a new pane/tab should start in, given the source's cwd.
pub fn inherit_cwd(active_cwd: Option<&str>) -> Option<String> {
    active_cwd.map(|s| s.to_string())
}

/// Pick the tab to focus after `closing` is removed, given the display
/// order BEFORE removal: the previous tab, or the next if `closing` was
/// first, or `None` if it was the only tab.
pub fn next_active_after_close(order: &[String], closing: &str) -> Option<String> {
    let idx = order.iter().position(|id| id == closing)?;
    if idx > 0 {
        order.get(idx - 1).cloned()
    } else {
        order.get(idx + 1).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w: 808.0,
            h: 600.0,
        }
    }

    #[test]
    fn split_leaf_inserts_balanced_split() {
        let mut tree = PaneNode::Leaf("a".into());
        assert!(split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b"));
        match &tree {
            PaneNode::Split {
                id,
                dir,
                ratio,
                a,
                b,
            } => {
                assert_eq!(id, "s1");
                assert_eq!(*dir, SplitDir::Vertical);
                assert_eq!(*ratio, 0.5);
                assert_eq!(**a, PaneNode::Leaf("a".into()));
                assert_eq!(**b, PaneNode::Leaf("b".into()));
            }
            _ => panic!("expected a split"),
        }
        assert_eq!(leaves_of(&tree), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_leaf_targets_nested_leaf() {
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b");
        // Split the right pane horizontally.
        assert!(split_leaf(&mut tree, "b", "s2", SplitDir::Horizontal, "c"));
        assert_eq!(
            leaves_of(&tree),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(!split_leaf(&mut tree, "zzz", "s3", SplitDir::Vertical, "d"));
    }

    #[test]
    fn close_leaf_promotes_sibling() {
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b");
        let pruned = close_leaf(tree, "a").expect("sibling remains");
        assert_eq!(pruned, PaneNode::Leaf("b".into()));
    }

    #[test]
    fn close_leaf_on_only_leaf_is_none() {
        let tree = PaneNode::Leaf("a".into());
        assert!(close_leaf(tree, "a").is_none());
    }

    #[test]
    fn close_leaf_promotes_nested_subtree() {
        // a | (b / c) — closing a promotes the whole (b/c) subtree.
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b");
        split_leaf(&mut tree, "b", "s2", SplitDir::Horizontal, "c");
        let pruned = close_leaf(tree, "a").expect("subtree remains");
        assert_eq!(leaves_of(&pruned), vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn sibling_first_leaf_picks_neighbor() {
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b");
        assert_eq!(sibling_first_leaf(&tree, "a"), Some("b".to_string()));
        assert_eq!(sibling_first_leaf(&tree, "b"), Some("a".to_string()));
    }

    #[test]
    fn single_leaf_fills_box() {
        let tree = PaneNode::Leaf("a".into());
        let rects = pane_rects(&tree, area());
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, "a");
        assert_eq!(rects[0].1, area());
    }

    #[test]
    fn vertical_split_partitions_width_minus_divider() {
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b");
        let rects = pane_rects(&tree, area());
        let a = rects.iter().find(|(id, _)| id == "a").unwrap().1;
        let b = rects.iter().find(|(id, _)| id == "b").unwrap().1;
        // (808 - 8) / 2 = 400 each; b starts after a + divider.
        assert_eq!(a.w, 400.0);
        assert_eq!(b.w, 400.0);
        assert_eq!(b.x, 408.0);
        assert_eq!(a.h, 600.0);
        assert_eq!(b.h, 600.0);
    }

    #[test]
    fn horizontal_split_partitions_height_minus_divider() {
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Horizontal, "b");
        let rects = pane_rects(
            &tree,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 608.0,
            },
        );
        let a = rects.iter().find(|(id, _)| id == "a").unwrap().1;
        let b = rects.iter().find(|(id, _)| id == "b").unwrap().1;
        assert_eq!(a.h, 300.0);
        assert_eq!(b.h, 300.0);
        assert_eq!(b.y, 308.0);
        assert_eq!(a.w, 800.0);
    }

    #[test]
    fn ratio_for_drag_clamps_to_min() {
        let area = Rect {
            x: 100.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        };
        // Cursor far left → clamp to min fraction, not 0.
        let r = ratio_for_drag(area, SplitDir::Vertical, 100.0, 0.0, 80.0);
        assert!(r > 0.0 && r < 0.2);
        // Cursor at center.
        let mid = ratio_for_drag(area, SplitDir::Vertical, 500.0, 0.0, 80.0);
        assert!((mid - 0.5).abs() < 0.01);
    }

    #[test]
    fn layout_round_trips_structure() {
        let mut tree = PaneNode::Leaf("a".into());
        split_leaf(&mut tree, "a", "s1", SplitDir::Vertical, "b");
        let mut meta = HashMap::new();
        meta.insert(
            "a".to_string(),
            PaneMeta {
                id: "a".into(),
                tmux_session: "sola-a".into(),
                cwd: Some("/tmp".into()),
            },
        );
        meta.insert(
            "b".to_string(),
            PaneMeta {
                id: "b".into(),
                tmux_session: "sola-b".into(),
                cwd: None,
            },
        );
        let layout = to_layout(&tree, &meta);
        match &layout {
            PaneLayout::Split { dir, a, b, .. } => {
                assert_eq!(*dir, SplitDir::Vertical);
                assert!(
                    matches!(&**a, PaneLayout::Leaf { tmux_session, .. } if tmux_session == "sola-a")
                );
                assert!(
                    matches!(&**b, PaneLayout::Leaf { tmux_session, .. } if tmux_session == "sola-b")
                );
            }
            _ => panic!("expected split layout"),
        }
        // Rebuild: fresh ids, two leaves, sessions preserved.
        let mut metas = Vec::new();
        let rebuilt = from_layout(&layout, &mut metas);
        assert_eq!(leaves_of(&rebuilt).len(), 2);
        assert_eq!(metas.len(), 2);
        assert!(metas.iter().any(|m| m.tmux_session == "sola-a"));
        assert!(metas.iter().any(|m| m.tmux_session == "sola-b"));
    }

    #[test]
    fn reconcile_prunes_dead_leaves() {
        let layout = PaneLayout::Split {
            dir: SplitDir::Vertical,
            ratio: 0.5,
            a: Box::new(PaneLayout::Leaf {
                tmux_session: "sola-a".into(),
                cwd: None,
            }),
            b: Box::new(PaneLayout::Leaf {
                tmux_session: "sola-b".into(),
                cwd: None,
            }),
        };
        let mut live = HashSet::new();
        live.insert("sola-a".to_string());
        // Only a is live → b pruned, a promoted.
        let pruned = reconcile_layout(layout.clone(), &Some(live)).unwrap();
        assert!(
            matches!(pruned, PaneLayout::Leaf { tmux_session, .. } if tmux_session == "sola-a")
        );
        // None live → whole tab retracted.
        let empty = HashSet::new();
        assert!(reconcile_layout(layout.clone(), &Some(empty)).is_none());
        // Unknown snapshot → admit everything unchanged.
        assert_eq!(reconcile_layout(layout.clone(), &None), Some(layout));
    }

    #[test]
    fn next_ordinal_is_max_plus_one() {
        assert_eq!(next_ordinal(&[]), 0);
        assert_eq!(next_ordinal(&[0, 2, 1]), 3);
        assert_eq!(next_ordinal(&[5, 1]), 6);
    }

    #[test]
    fn inherit_cwd_passes_through() {
        assert_eq!(inherit_cwd(Some("/home/x")), Some("/home/x".to_string()));
        assert_eq!(inherit_cwd(None), None);
    }

    #[test]
    fn close_active_picks_previous_then_next() {
        let order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(next_active_after_close(&order, "b"), Some("a".to_string()));
        assert_eq!(next_active_after_close(&order, "a"), Some("b".to_string()));
        assert_eq!(next_active_after_close(&["a".to_string()], "a"), None);
    }
}
