//! Hyprland-style dwindle BSP for one screen.
//!
//! A tree of window ids. Insert splits the focused leaf along its longer
//! edge; the new window is the right/bottom child. Close replaces a split
//! with the surviving sibling. Split direction is preserved after insert
//! so Super+J is sticky.

use sola_bus::topics::{FrameUpdate, ScreenTileDir, ScreenTileNode};

/// Side-by-side (`Row`) or stacked (`Col`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Row,
    Col,
}

impl Dir {
    pub fn toggle(self) -> Self {
        match self {
            Dir::Row => Dir::Col,
            Dir::Col => Dir::Row,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf {
        window_id: u32,
        app_id: String,
    },
    Split {
        dir: Dir,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }

    pub fn center(self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

pub const GAP_OUTER: i32 = 12;
pub const GAP_INNER: i32 = 8;
pub const RATIO_MIN: f32 = 0.15;
pub const RATIO_MAX: f32 = 0.85;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub root: Option<Node>,
}

impl Tree {
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn contains(&self, window_id: u32) -> bool {
        self.root
            .as_ref()
            .is_some_and(|n| n.contains_window(window_id))
    }

    pub fn windows(&self) -> Vec<u32> {
        let mut out = Vec::new();
        if let Some(n) = &self.root {
            n.collect_windows(&mut out);
        }
        out
    }

    /// Insert `new` by splitting `focused` (or the whole tree if focused is
    /// missing). First window becomes the sole leaf.
    pub fn insert(&mut self, focused: Option<u32>, new_id: u32, new_app: &str, leaf_rect: Option<Rect>) {
        if self.contains(new_id) {
            return;
        }
        let leaf = Node::Leaf {
            window_id: new_id,
            app_id: new_app.to_string(),
        };
        match self.root.take() {
            None => self.root = Some(leaf),
            Some(root) => {
                let target = focused.filter(|id| root.contains_window(*id));
                self.root = Some(root.insert_split(target, leaf, leaf_rect));
            }
        }
    }

    pub fn remove(&mut self, window_id: u32) {
        let Some(root) = self.root.take() else {
            return;
        };
        self.root = root.remove(window_id);
    }

    pub fn swap(&mut self, a: u32, b: u32) {
        if let Some(root) = self.root.as_mut() {
            root.swap_ids(a, b);
        }
    }

    pub fn toggle_split(&mut self, focused: u32) {
        if let Some(root) = self.root.as_mut() {
            root.toggle_parent(focused);
        }
    }

    pub fn set_ratio(&mut self, window_id: u32, ratio: f32) {
        if let Some(root) = self.root.as_mut() {
            root.set_parent_ratio(window_id, ratio.clamp(RATIO_MIN, RATIO_MAX));
        }
    }

    /// Parent split dir + whether `window_id` is the first child.
    pub fn parent_split(&self, window_id: u32) -> Option<(Dir, bool)> {
        self.root.as_ref()?.parent_split(window_id)
    }

    pub fn attach_window(&mut self, app_id: &str, window_id: u32) -> bool {
        self.root
            .as_mut()
            .is_some_and(|n| n.attach_window(app_id, window_id))
    }

    pub fn layout(&self, area: Rect) -> Vec<(u32, Rect)> {
        let mut out = Vec::new();
        if let Some(n) = &self.root {
            n.layout(area, &mut out);
        }
        out
    }

    pub fn neighbor(&self, focused: u32, dir: Compass, laid: &[(u32, Rect)]) -> Option<u32> {
        let focus_rect = laid.iter().find(|(id, _)| *id == focused)?.1;
        let (fx, fy) = focus_rect.center();
        let mut best: Option<(u32, i32)> = None;
        for &(id, r) in laid {
            if id == focused {
                continue;
            }
            let (cx, cy) = r.center();
            let (ok, dist) = match dir {
                Compass::Left => (cx < fx && overlap_1d(focus_rect.y, focus_rect.h, r.y, r.h), fx - cx),
                Compass::Right => (cx > fx && overlap_1d(focus_rect.y, focus_rect.h, r.y, r.h), cx - fx),
                Compass::Up => (cy < fy && overlap_1d(focus_rect.x, focus_rect.w, r.x, r.w), fy - cy),
                Compass::Down => (cy > fy && overlap_1d(focus_rect.x, focus_rect.w, r.x, r.w), cy - fy),
            };
            if !ok || dist <= 0 {
                continue;
            }
            if best.is_none_or(|(_, d)| dist < d) {
                best = Some((id, dist));
            }
        }
        best.map(|(id, _)| id)
    }

    pub fn persist_nodes(&self) -> Option<ScreenTileNode> {
        self.root.as_ref().map(Node::to_persist)
    }

    pub fn from_persist(node: Option<ScreenTileNode>) -> Self {
        Self {
            root: node.map(Node::from_persist),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compass {
    Left,
    Right,
    Up,
    Down,
}

fn overlap_1d(a: i32, ah: i32, b: i32, bh: i32) -> bool {
    let a2 = a + ah;
    let b2 = b + bh;
    a < b2 && b < a2
}

impl Node {
    fn contains_window(&self, window_id: u32) -> bool {
        match self {
            Node::Leaf { window_id: id, .. } => *id == window_id,
            Node::Split { a, b, .. } => a.contains_window(window_id) || b.contains_window(window_id),
        }
    }

    fn collect_windows(&self, out: &mut Vec<u32>) {
        match self {
            Node::Leaf { window_id, .. } => out.push(*window_id),
            Node::Split { a, b, .. } => {
                a.collect_windows(out);
                b.collect_windows(out);
            }
        }
    }

    fn insert_split(self, target: Option<u32>, new_leaf: Node, leaf_rect: Option<Rect>) -> Node {
        match self {
            Node::Leaf { window_id, .. } if target.is_none_or(|t| t == window_id) => {
                let dir = match leaf_rect {
                    Some(r) if r.h > r.w => Dir::Col,
                    _ => Dir::Row,
                };
                Node::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(self),
                    b: Box::new(new_leaf),
                }
            }
            Node::Leaf { .. } => self,
            Node::Split { dir, ratio, a, b } => {
                if target.is_some_and(|t| a.contains_window(t)) {
                    Node::Split {
                        dir,
                        ratio,
                        a: Box::new(a.insert_split(target, new_leaf, leaf_rect)),
                        b,
                    }
                } else if target.is_some_and(|t| b.contains_window(t)) {
                    Node::Split {
                        dir,
                        ratio,
                        a,
                        b: Box::new(b.insert_split(target, new_leaf, leaf_rect)),
                    }
                } else {
                    // Focused leaf not in this tree: split at the root.
                    let dir = match leaf_rect {
                        Some(r) if r.h > r.w => Dir::Col,
                        _ => Dir::Row,
                    };
                    Node::Split {
                        dir,
                        ratio: 0.5,
                        a: Box::new(Node::Split {
                            dir,
                            ratio,
                            a,
                            b,
                        }),
                        b: Box::new(new_leaf),
                    }
                }
            }
        }
    }

    fn remove(self, window_id: u32) -> Option<Node> {
        match self {
            Node::Leaf { window_id: id, .. } if id == window_id => None,
            Node::Leaf { .. } => Some(self),
            Node::Split { dir, ratio, a, b } => match (a.remove(window_id), b.remove(window_id)) {
                (None, None) => None,
                (Some(x), None) | (None, Some(x)) => Some(x),
                (Some(a), Some(b)) => Some(Node::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
            },
        }
    }

    fn swap_ids(&mut self, a_id: u32, b_id: u32) {
        let mut a_app: Option<String> = None;
        let mut b_app: Option<String> = None;
        self.each_leaf(|id, app| {
            if *id == a_id {
                a_app = Some(app.clone());
            } else if *id == b_id {
                b_app = Some(app.clone());
            }
        });
        let (Some(a_app), Some(b_app)) = (a_app, b_app) else {
            return;
        };
        self.each_leaf_mut(|id, app| {
            if *id == a_id {
                *id = b_id;
                *app = b_app.clone();
            } else if *id == b_id {
                *id = a_id;
                *app = a_app.clone();
            }
        });
    }

    fn each_leaf(&self, mut f: impl FnMut(&u32, &String)) {
        fn walk(n: &Node, f: &mut dyn FnMut(&u32, &String)) {
            match n {
                Node::Leaf { window_id, app_id } => f(window_id, app_id),
                Node::Split { a, b, .. } => {
                    walk(a, f);
                    walk(b, f);
                }
            }
        }
        walk(self, &mut f);
    }

    fn each_leaf_mut(&mut self, mut f: impl FnMut(&mut u32, &mut String)) {
        fn walk(n: &mut Node, f: &mut dyn FnMut(&mut u32, &mut String)) {
            match n {
                Node::Leaf { window_id, app_id } => f(window_id, app_id),
                Node::Split { a, b, .. } => {
                    walk(a, f);
                    walk(b, f);
                }
            }
        }
        walk(self, &mut f);
    }

    fn toggle_parent(&mut self, focused: u32) -> bool {
        match self {
            Node::Leaf { .. } => false,
            Node::Split { dir, a, b, .. } => {
                if matches!(a.as_ref(), Node::Leaf { window_id, .. } if *window_id == focused)
                    || matches!(b.as_ref(), Node::Leaf { window_id, .. } if *window_id == focused)
                {
                    *dir = dir.toggle();
                    true
                } else {
                    a.toggle_parent(focused) || b.toggle_parent(focused)
                }
            }
        }
    }

    fn set_parent_ratio(&mut self, window_id: u32, ratio: f32) -> bool {
        match self {
            Node::Leaf { .. } => false,
            Node::Split {
                dir: _,
                ratio: r,
                a,
                b,
            } => {
                let in_a = matches!(a.as_ref(), Node::Leaf { window_id: id, .. } if *id == window_id);
                let in_b = matches!(b.as_ref(), Node::Leaf { window_id: id, .. } if *id == window_id);
                if in_a {
                    *r = ratio;
                    true
                } else if in_b {
                    *r = 1.0 - ratio;
                    true
                } else {
                    a.set_parent_ratio(window_id, ratio) || b.set_parent_ratio(window_id, ratio)
                }
            }
        }
    }

    fn parent_split(&self, window_id: u32) -> Option<(Dir, bool)> {
        match self {
            Node::Leaf { .. } => None,
            Node::Split { dir, a, b, .. } => {
                if matches!(a.as_ref(), Node::Leaf { window_id: id, .. } if *id == window_id) {
                    Some((*dir, true))
                } else if matches!(b.as_ref(), Node::Leaf { window_id: id, .. } if *id == window_id)
                {
                    Some((*dir, false))
                } else {
                    a.parent_split(window_id)
                        .or_else(|| b.parent_split(window_id))
                }
            }
        }
    }

    fn attach_window(&mut self, app_id: &str, window_id: u32) -> bool {
        match self {
            Node::Leaf {
                window_id: id,
                app_id: aid,
            } if aid == app_id && *id == 0 => {
                *id = window_id;
                true
            }
            Node::Leaf { .. } => false,
            Node::Split { a, b, .. } => {
                a.attach_window(app_id, window_id) || b.attach_window(app_id, window_id)
            }
        }
    }

    fn layout(&self, area: Rect, out: &mut Vec<(u32, Rect)>) {
        match self {
            Node::Leaf { window_id, .. } => {
                if *window_id != 0 {
                    out.push((*window_id, area));
                }
            }
            Node::Split { dir, ratio, a, b } => {
                let gap = GAP_INNER;
                match dir {
                    Dir::Row => {
                        let inner = (area.w - gap).max(1);
                        let aw = ((*ratio as f64) * inner as f64).round() as i32;
                        let aw = aw.clamp(1, inner.saturating_sub(1).max(1));
                        let bw = (inner - aw).max(1);
                        a.layout(
                            Rect {
                                x: area.x,
                                y: area.y,
                                w: aw,
                                h: area.h,
                            },
                            out,
                        );
                        b.layout(
                            Rect {
                                x: area.x + aw + gap,
                                y: area.y,
                                w: bw,
                                h: area.h,
                            },
                            out,
                        );
                    }
                    Dir::Col => {
                        let inner = (area.h - gap).max(1);
                        let ah = ((*ratio as f64) * inner as f64).round() as i32;
                        let ah = ah.clamp(1, inner.saturating_sub(1).max(1));
                        let bh = (inner - ah).max(1);
                        a.layout(
                            Rect {
                                x: area.x,
                                y: area.y,
                                w: area.w,
                                h: ah,
                            },
                            out,
                        );
                        b.layout(
                            Rect {
                                x: area.x,
                                y: area.y + ah + gap,
                                w: area.w,
                                h: bh,
                            },
                            out,
                        );
                    }
                }
            }
        }
    }

    fn to_persist(&self) -> ScreenTileNode {
        match self {
            Node::Leaf { app_id, .. } => ScreenTileNode::Leaf {
                app_id: app_id.clone(),
            },
            Node::Split {
                dir,
                ratio,
                a,
                b,
            } => ScreenTileNode::Split {
                dir: match dir {
                    Dir::Row => ScreenTileDir::Row,
                    Dir::Col => ScreenTileDir::Col,
                },
                ratio: *ratio,
                a: Box::new(a.to_persist()),
                b: Box::new(b.to_persist()),
            },
        }
    }

    fn from_persist(p: ScreenTileNode) -> Node {
        match p {
            ScreenTileNode::Leaf { app_id } => Node::Leaf {
                window_id: 0,
                app_id,
            },
            ScreenTileNode::Split {
                dir,
                ratio,
                a,
                b,
            } => Node::Split {
                dir: match dir {
                    ScreenTileDir::Row => Dir::Row,
                    ScreenTileDir::Col => Dir::Col,
                },
                ratio,
                a: Box::new(Node::from_persist(*a)),
                b: Box::new(Node::from_persist(*b)),
            },
        }
    }
}

pub fn usable_area(output_w: i32, output_h: i32, menubar: i32) -> Rect {
    let y = menubar + GAP_OUTER;
    let h = (output_h - menubar - 2 * GAP_OUTER).max(1);
    let w = (output_w - 2 * GAP_OUTER).max(1);
    Rect {
        x: GAP_OUTER,
        y,
        w,
        h,
    }
}

pub fn fullscreen_area(output_w: i32, output_h: i32, menubar: i32) -> Rect {
    Rect {
        x: 0,
        y: menubar,
        w: output_w.max(1),
        h: (output_h - menubar).max(1),
    }
}

pub fn cinema_area(output_w: i32, output_h: i32) -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: output_w.max(1),
        h: output_h.max(1),
    }
}

pub fn frame_from_rect(window_id: u32, r: Rect, fullscreen: bool) -> FrameUpdate {
    FrameUpdate {
        window_id,
        x: r.x,
        y: r.y,
        width: r.w.max(1),
        height: r.h.max(1),
        fullscreen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_two() -> Tree {
        let mut t = Tree::default();
        t.insert(None, 1, "a", None);
        t.insert(Some(1), 2, "b", Some(Rect { x: 0, y: 0, w: 800, h: 400 }));
        t
    }

    #[test]
    fn first_insert_is_leaf() {
        let mut t = Tree::default();
        t.insert(None, 1, "a", None);
        assert_eq!(t.windows(), vec![1]);
    }

    #[test]
    fn insert_splits_focused_on_long_edge() {
        let t = tree_two();
        match t.root.as_ref().unwrap() {
            Node::Split { dir, ratio, a, b } => {
                assert_eq!(*dir, Dir::Row);
                assert_eq!(*ratio, 0.5);
                assert!(matches!(a.as_ref(), Node::Leaf { window_id: 1, .. }));
                assert!(matches!(b.as_ref(), Node::Leaf { window_id: 2, .. }));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remove_collapses_to_sibling() {
        let mut t = tree_two();
        t.remove(1);
        assert_eq!(t.windows(), vec![2]);
    }

    #[test]
    fn swap_exchanges_leaves() {
        let mut t = tree_two();
        t.swap(1, 2);
        assert_eq!(t.windows(), vec![2, 1]);
    }

    #[test]
    fn toggle_split_flips_parent() {
        let mut t = tree_two();
        t.toggle_split(1);
        match t.root.as_ref().unwrap() {
            Node::Split { dir, .. } => assert_eq!(*dir, Dir::Col),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn layout_row_has_inner_gap() {
        let t = tree_two();
        let laid = t.layout(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 40,
        });
        assert_eq!(laid.len(), 2);
        let (_, a) = laid[0];
        let (_, b) = laid[1];
        assert_eq!(b.x, a.x + a.w + GAP_INNER);
        assert_eq!(a.w + GAP_INNER + b.w, 100);
    }

    #[test]
    fn neighbor_right() {
        let t = tree_two();
        let laid = t.layout(Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 40,
        });
        assert_eq!(t.neighbor(1, Compass::Right, &laid), Some(2));
        assert_eq!(t.neighbor(2, Compass::Left, &laid), Some(1));
    }

    #[test]
    fn persist_roundtrip_keeps_app_ids() {
        let t = tree_two();
        let p = t.persist_nodes();
        let restored = Tree::from_persist(p);
        restored.root.as_ref().unwrap();
        let mut r = restored;
        assert!(r.attach_window("a", 10));
        assert!(r.attach_window("b", 20));
        assert_eq!(r.windows(), vec![10, 20]);
    }
}
