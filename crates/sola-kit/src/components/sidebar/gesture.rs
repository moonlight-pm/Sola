//! Kit-owned sidebar gesture: hover, resize, click-vs-drag, live preview.
//!
//! The consumer stores a [`State`] blob and maps [`Msg`] into
//! [`State::update`]. It never sees cursor samples, thresholds, or
//! animation. Product meaning arrives as [`Event`].

use iced::Subscription;
use iced::time::Instant;
use iced::window;

use super::{
    PANEL_REORDER_THRESHOLD, PANEL_ROW_H, PANEL_W_MAX, PANEL_W_MIN, PanelDropBias, ReorderAnim,
    panel_drop_bias, panel_drop_index_visual, panel_row_rest_ys, panel_section_at_y,
    panel_shift_skip_header,
};

/// Opaque gesture / hover / animation state. Hold one per sidebar.
#[derive(Debug, Default)]
pub struct State {
    hover: Option<String>,
    press: Option<Press>,
    divider: Option<DividerDrag>,
    anim: ReorderAnim,
}

#[derive(Debug, Clone)]
struct Press {
    from: usize,
    start_y: f32,
    cursor_y: f32,
    dragging: bool,
    snapshot: StripSnapshot,
}

#[derive(Debug, Clone, Copy)]
struct DividerDrag {
    anchor_x: f32,
    anchor_w: f32,
}

/// Visible strip at the moment a row was pressed.
#[derive(Debug, Clone, PartialEq)]
pub struct StripSnapshot {
    pub rows: Vec<Row>,
    /// `(grouped, visible_len)` per section, same order as the panel.
    pub lens: Vec<(bool, usize)>,
    pub item_spacing: f32,
    pub row_h: f32,
}

/// One painted row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header { id: String },
    Item { id: String, section: Option<String> },
}

impl Row {
    pub fn id(&self) -> &str {
        match self {
            Self::Header { id } | Self::Item { id, .. } => id,
        }
    }
}

/// Messages the panel emits. Forward every one into [`State::update`].
#[derive(Debug, Clone)]
pub enum Msg {
    PressRow {
        index: usize,
        snapshot: StripSnapshot,
    },
    PressDivider {
        width: f32,
    },
    Pointer {
        x: f32,
        y: f32,
    },
    Release,
    Tick,
    Hover(Option<String>),
}

/// Semantic outcome after [`State::update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Activate { id: String },
    ToggleSection { id: String },
    Drop(Drop),
    Resize { width: f32 },
}

/// A finished drag of one visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drop {
    pub id: String,
    pub dest: Dest,
}

/// Where the dragged row should land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// Join `section`. `before` is the member to sit in front of (`None` = append).
    Join {
        section: String,
        before: Option<String>,
    },
    /// Become ungrouped. `before` is the loose item to sit in front of (`None` = append).
    Loose { before: Option<String> },
    /// Header drag: grouped section ids in the new order.
    Sections(Vec<String>),
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hover(&self) -> Option<&str> {
        self.hover.as_deref()
    }

    pub fn resizing(&self) -> bool {
        self.divider.is_some()
    }

    pub fn reordering(&self) -> bool {
        self.press.as_ref().is_some_and(|p| p.dragging)
    }

    /// True while a press is live (including the pre-threshold click).
    pub fn capturing(&self) -> bool {
        self.press.is_some() || self.divider.is_some()
    }

    /// Live-reorder preview — `None` until movement crosses the threshold.
    pub fn preview_active(&self) -> Option<(usize, f32)> {
        self.press
            .as_ref()
            .filter(|p| p.dragging)
            .map(|p| (p.from, p.start_y))
    }

    pub fn cursor_y(&self) -> f32 {
        self.press.as_ref().map(|p| p.cursor_y).unwrap_or(0.0)
    }

    pub fn preview_anim(&self) -> Option<&ReorderAnim> {
        self.press
            .as_ref()
            .filter(|p| p.dragging)
            .map(|_| &self.anim)
    }

    pub fn animating(&self) -> bool {
        self.reordering() || self.anim.is_animating(Instant::now())
    }

    /// Frames while a drag is live so sibling glides keep painting.
    pub fn subscription(&self) -> Subscription<Msg> {
        if self.animating() {
            window::frames().map(|_| Msg::Tick)
        } else {
            Subscription::none()
        }
    }

    pub fn update(&mut self, msg: Msg) -> Option<Event> {
        match msg {
            Msg::Hover(id) => {
                self.hover = id;
                None
            }
            Msg::PressRow { index, snapshot } => {
                self.anim.clear();
                self.press = Some(Press {
                    from: index,
                    start_y: 0.0,
                    cursor_y: 0.0,
                    dragging: false,
                    snapshot,
                });
                None
            }
            Msg::PressDivider { width } => {
                self.divider = Some(DividerDrag {
                    anchor_x: f32::NAN,
                    anchor_w: width,
                });
                None
            }
            Msg::Pointer { x, y } => self.on_pointer(x, y),
            Msg::Tick => {
                if self.press.as_ref().is_some_and(|p| p.dragging) {
                    self.sync_preview(Instant::now());
                }
                None
            }
            Msg::Release => self.on_release(),
        }
    }

    fn on_pointer(&mut self, x: f32, y: f32) -> Option<Event> {
        if let Some(d) = &mut self.divider {
            if d.anchor_x.is_nan() {
                d.anchor_x = x;
            }
            let desired = d.anchor_w + (x - d.anchor_x);
            let width = desired.clamp(PANEL_W_MIN, PANEL_W_MAX);
            return Some(Event::Resize { width });
        }
        let Some(p) = self.press.as_mut() else {
            return None;
        };
        if p.start_y == 0.0 {
            p.start_y = y;
        }
        p.cursor_y = y;
        if (y - p.start_y).abs() >= PANEL_REORDER_THRESHOLD {
            p.dragging = true;
        }
        if p.dragging {
            self.sync_preview(Instant::now());
        }
        None
    }

    fn on_release(&mut self) -> Option<Event> {
        if self.divider.take().is_some() {
            return None;
        }
        let Some(p) = self.press.take() else {
            return None;
        };
        self.anim.clear();
        if !p.dragging || p.start_y == 0.0 {
            return match p.snapshot.rows.get(p.from) {
                Some(Row::Item { id, .. }) => Some(Event::Activate { id: id.clone() }),
                Some(Row::Header { id }) => Some(Event::ToggleSection { id: id.clone() }),
                None => None,
            };
        }
        resolve_drop(&p.snapshot, p.from, p.start_y, p.cursor_y).map(Event::Drop)
    }

    fn sync_preview(&mut self, now: Instant) {
        let Some(p) = self.press.as_ref() else {
            return;
        };
        if !p.dragging {
            return;
        }
        let n = p.snapshot.rows.len();
        if n == 0 {
            return;
        }
        let lens = &p.snapshot.lens;
        let ys = panel_row_rest_ys(lens, p.snapshot.item_spacing);
        let to = panel_drop_index_visual(p.from, p.start_y, p.cursor_y, &ys, PANEL_ROW_H);
        let from_si = section_index(lens, p.from);
        let to_si = section_index(lens, to);
        let ghost_mid =
            ys.get(p.from).copied().unwrap_or(0.0) + (p.cursor_y - p.start_y) + PANEL_ROW_H * 0.5;
        let hover_si = panel_section_at_y(lens, p.snapshot.item_spacing, ghost_mid, PANEL_ROW_H);
        let dragging_header = matches!(p.snapshot.rows.get(p.from), Some(Row::Header { .. }));
        let over_foreign_well = !dragging_header
            && lens.get(hover_si).is_some_and(|(grouped, _)| *grouped)
            && hover_si != from_si;
        let (a, b) = member_range(lens, hover_si);
        let to_in_hover_members = to >= a && to < b;
        let header_i = section_start(lens, hover_si);
        let over_title = lens.get(hover_si).is_some_and(|(grouped, _)| *grouped)
            && ys
                .get(header_i)
                .is_some_and(|y| ghost_mid >= *y && ghost_mid < *y + PANEL_ROW_H);
        let row_h = p.snapshot.row_h;
        let pitch = row_h + p.snapshot.item_spacing;
        let (extra, extra_si) = if over_foreign_well && !over_title {
            (row_h, Some(hover_si))
        } else {
            (0.0, None)
        };
        let shift_to = if dragging_header {
            to
        } else if over_foreign_well && !to_in_hover_members {
            p.from
        } else {
            panel_shift_skip_header(lens, p.from, to)
        };
        let dest = if dragging_header {
            None
        } else if over_foreign_well && !to_in_hover_members {
            Some((0, 0))
        } else {
            let si = if over_foreign_well { hover_si } else { to_si };
            Some(member_range(lens, si))
        };
        let origin_hole = !over_title && (from_si == to_si || extra_si.is_some());
        self.anim.sync_well(
            p.from,
            shift_to,
            n,
            extra,
            extra_si,
            dest,
            lens.len(),
            pitch,
            origin_hole,
            now,
        );
    }
}

pub fn resolve_drop(
    snap: &StripSnapshot,
    from: usize,
    start_y: f32,
    cursor_y: f32,
) -> Option<Drop> {
    let n = snap.rows.len();
    if from >= n {
        return None;
    }
    let ys = panel_row_rest_ys(&snap.lens, snap.item_spacing);
    let to = panel_drop_index_visual(from, start_y, cursor_y, &ys, PANEL_ROW_H);
    if from == to {
        return None;
    }
    let bias = panel_drop_bias(from, start_y, cursor_y, PANEL_ROW_H, to);
    match snap.rows[from].clone() {
        Row::Header { id } => {
            let order = header_reorder(snap, &id, to)?;
            Some(Drop {
                id,
                dest: Dest::Sections(order),
            })
        }
        Row::Item { id, .. } => {
            let dest = item_dest(snap, from, to, bias)?;
            Some(Drop { id, dest })
        }
    }
}

fn header_reorder(snap: &StripSnapshot, gid: &str, to: usize) -> Option<Vec<String>> {
    let mut ids: Vec<String> = snap
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Header { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let from_g = ids.iter().position(|id| id == gid)?;
    let mut dest = 0usize;
    let last = ids.len().saturating_sub(1);
    for (i, row) in snap.rows.iter().enumerate() {
        match row {
            Row::Header { id } => {
                if let Some(gi) = ids.iter().position(|g| g == id) {
                    dest = gi;
                }
            }
            Row::Item { section, .. } => {
                if let Some(sid) = section {
                    if let Some(gi) = ids.iter().position(|g| g == sid) {
                        dest = gi;
                    }
                } else {
                    dest = last;
                }
            }
        }
        if i >= to {
            break;
        }
    }
    if dest == from_g {
        return None;
    }
    let g = ids.remove(from_g);
    let dest = dest.min(ids.len());
    ids.insert(dest, g);
    Some(ids)
}

fn item_dest(snap: &StripSnapshot, from: usize, to: usize, bias: PanelDropBias) -> Option<Dest> {
    let mut rest = snap.rows.clone();
    rest.remove(from);
    let insert_at = to.min(rest.len());
    let after_row = rest.get(insert_at);
    let before_row = if insert_at == 0 {
        None
    } else {
        rest.get(insert_at - 1)
    };
    dest_for_drop(before_row, after_row, bias)
}

fn dest_for_drop(
    before_row: Option<&Row>,
    after_row: Option<&Row>,
    bias: PanelDropBias,
) -> Option<Dest> {
    if bias == PanelDropBias::PocketAbove {
        match (before_row, after_row) {
            (Some(Row::Item { section, .. }), Some(Row::Header { id: next_gid })) => {
                if let Some(gid) = section {
                    if gid != next_gid {
                        return Some(Dest::Join {
                            section: gid.clone(),
                            before: None,
                        });
                    }
                }
                return Some(Dest::Join {
                    section: next_gid.clone(),
                    before: None,
                });
            }
            (Some(Row::Header { id: prev_gid }), Some(Row::Header { id: next_gid }))
                if prev_gid != next_gid =>
            {
                return Some(Dest::Join {
                    section: prev_gid.clone(),
                    before: None,
                });
            }
            (
                Some(Row::Item { section, .. }),
                Some(Row::Item {
                    section: next_sec, ..
                }),
            ) if next_sec.is_none() => {
                if let Some(gid) = section {
                    return Some(Dest::Join {
                        section: gid.clone(),
                        before: None,
                    });
                }
            }
            _ => {}
        }
    }
    dest_on_slot(after_row, before_row)
}

fn dest_on_slot(after_row: Option<&Row>, before_row: Option<&Row>) -> Option<Dest> {
    match after_row {
        Some(Row::Header { .. }) => None,
        Some(Row::Item {
            id: next,
            section: next_sec,
        }) => {
            if let Some(gid) = next_sec {
                Some(Dest::Join {
                    section: gid.clone(),
                    before: Some(next.clone()),
                })
            } else {
                Some(Dest::Loose {
                    before: Some(next.clone()),
                })
            }
        }
        None => match before_row {
            Some(Row::Header { id }) => Some(Dest::Join {
                section: id.clone(),
                before: None,
            }),
            Some(Row::Item { section, .. }) => {
                if let Some(gid) = section {
                    Some(Dest::Join {
                        section: gid.clone(),
                        before: None,
                    })
                } else {
                    Some(Dest::Loose { before: None })
                }
            }
            None => Some(Dest::Loose { before: None }),
        },
    }
}

fn section_index(lens: &[(bool, usize)], row: usize) -> usize {
    let mut start = 0usize;
    for (i, (_, len)) in lens.iter().enumerate() {
        if row < start + *len {
            return i;
        }
        start += *len;
    }
    lens.len().saturating_sub(1)
}

fn section_start(lens: &[(bool, usize)], si: usize) -> usize {
    lens.iter().take(si).map(|(_, len)| *len).sum()
}

fn member_range(lens: &[(bool, usize)], si: usize) -> (usize, usize) {
    let mut start = 0usize;
    for (i, (grouped, len)) in lens.iter().enumerate() {
        if i == si {
            let end = start + *len;
            let first = if *grouped {
                (start + 1).min(end)
            } else {
                start
            };
            return (first, end);
        }
        start += *len;
    }
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, section: Option<&str>) -> Row {
        Row::Item {
            id: id.into(),
            section: section.map(str::to_string),
        }
    }
    fn header(id: &str) -> Row {
        Row::Header { id: id.into() }
    }

    fn setup() -> StripSnapshot {
        // H work, 1, 2, H research, 3, 4, 5  (4 and 5 loose)
        StripSnapshot {
            rows: vec![
                header("work"),
                item("1", Some("work")),
                item("2", Some("work")),
                header("research"),
                item("3", Some("research")),
                item("4", None),
                item("5", None),
            ],
            lens: vec![(true, 3), (true, 2), (false, 2)],
            item_spacing: 3.0,
            row_h: 32.0,
        }
    }

    fn pointer_for(snap: &StripSnapshot, from: usize, to: usize, pocket: bool) -> (f32, f32) {
        let ys = super::super::panel_row_rest_ys(&snap.lens, snap.item_spacing);
        let start_y = ys[from];
        let cursor_y = if pocket {
            ys[to] + snap.row_h * 0.1
        } else {
            ys[to] + snap.row_h * 0.6
        };
        (start_y, cursor_y)
    }

    #[test]
    fn click_activates() {
        let mut s = State::new();
        let snap = setup();
        assert!(
            s.update(Msg::PressRow {
                index: 5,
                snapshot: snap
            })
            .is_none()
        );
        match s.update(Msg::Release) {
            Some(Event::Activate { id }) => assert_eq!(id, "4"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn click_header_toggles() {
        let mut s = State::new();
        assert!(
            s.update(Msg::PressRow {
                index: 0,
                snapshot: setup()
            })
            .is_none()
        );
        match s.update(Msg::Release) {
            Some(Event::ToggleSection { id }) => assert_eq!(id, "work"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dest_on_member_joins_before() {
        let dest = dest_on_slot(
            Some(&item("2", Some("work"))),
            Some(&item("1", Some("work"))),
        )
        .expect("dest");
        assert_eq!(
            dest,
            Dest::Join {
                section: "work".into(),
                before: Some("2".into()),
            }
        );
    }

    #[test]
    fn dest_on_header_is_noop() {
        assert!(dest_on_slot(Some(&header("research")), Some(&item("2", Some("work")))).is_none());
    }

    #[test]
    fn drag_to_floor_of_group_stays_in_group() {
        let snap = setup();
        let (start, cursor) = pointer_for(&snap, 6, 3, true);
        let d = resolve_drop(&snap, 6, start, cursor).expect("drop");
        assert_eq!(d.id, "5");
        match d.dest {
            Dest::Join { section, before } => {
                assert_eq!(section, "work");
                assert!(before.is_none() || before.as_deref() == Some("3"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn header_drag_reorders_blocks() {
        let snap = setup();
        let (start, cursor) = pointer_for(&snap, 3, 0, false);
        let d = resolve_drop(&snap, 3, start, cursor).expect("drop");
        assert_eq!(d.id, "research");
        assert_eq!(
            d.dest,
            Dest::Sections(vec!["research".into(), "work".into()])
        );
    }

    #[test]
    fn member_to_loose_ungroups() {
        let snap = setup();
        let (start, cursor) = pointer_for(&snap, 1, 5, false);
        let d = resolve_drop(&snap, 1, start, cursor).expect("drop");
        assert_eq!(d.id, "1");
        match d.dest {
            Dest::Loose { .. } => {}
            other => panic!("{other:?}"),
        }
    }
}
