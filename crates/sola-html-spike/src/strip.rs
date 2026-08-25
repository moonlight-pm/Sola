//! Reorder-strip math — port of `/home/joshua/Workspace/Scratch/strip.js`.
//! Law (Scratch comment): sola-kit sidebar strip. No JS.

pub const THRESHOLD: i32 = 2;
pub const ANIM_MS: u64 = 180;
pub const ROW_H: i32 = 32;
pub const WELL_PAD_V: i32 = 3;
pub const ROW_GAP: i32 = 3;
pub const SPAN_END_GAP: i32 = 5;
pub const STRIP_PAD_TOP: i32 = 6;
pub const FIRST_SPAN_START_EXTRA: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Header,
    Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub y: i32,
    pub h: i32,
    pub kind: Kind,
}

/// Runtime meta uses owned ids; strip math only needs kind + group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeafMeta {
    pub id: String,
    pub kind: Kind,
    pub group: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Origin,
    End,
    Before(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropDest {
    Join {
        section: String,
        before: Option<String>,
    },
    Loose {
        before: Option<String>,
    },
    BeforeGroup {
        id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drop {
    pub id: String,
    pub dest: DropDest,
}

pub fn slot_eq(a: Slot, b: Slot) -> bool {
    a == b
}

pub fn px(v: f32) -> i32 {
    v.round() as i32
}

pub fn hit_leaf(rects: &[Rect], y: i32) -> Option<usize> {
    rects.iter().position(|r| y >= r.y && y < r.y + r.h.max(1))
}

fn canonicalize(origin: usize, to: usize, n: usize) -> Slot {
    if to == origin || to == origin + 1 {
        return Slot::Origin;
    }
    if to >= n {
        return if origin + 1 >= n {
            Slot::Origin
        } else {
            Slot::End
        };
    }
    Slot::Before(to)
}

pub fn dest_from_pointer(pointer_y: i32, origin: usize, rects: &[Rect]) -> Slot {
    if rects.is_empty() {
        return Slot::Origin;
    }
    if let Some(origin_rect) = rects.get(origin) {
        if pointer_y >= origin_rect.y && pointer_y < origin_rect.y + origin_rect.h {
            return Slot::Origin;
        }
    }
    if let Some(i) = hit_leaf(rects, pointer_y) {
        if rects[i].kind == Kind::Header {
            return Slot::Origin;
        }
        let to = if i < origin { i } else { i + 1 };
        return canonicalize(origin, to, rects.len());
    }
    let to = rects
        .iter()
        .position(|r| pointer_y < r.y)
        .unwrap_or(rects.len());
    canonicalize(origin, to, rects.len())
}

pub fn dest_index(origin: usize, dest: Slot, n: usize) -> usize {
    match dest {
        Slot::Origin => origin + 1,
        Slot::End => n,
        Slot::Before(i) => i,
    }
}

pub fn row_extra(i: usize, origin: usize, t: f32, h: i32) -> f32 {
    if i == origin {
        return 0.0;
    }
    let ii = i as f32;
    let o = origin as f32;
    let h = h as f32;
    if t > o {
        if ii <= o || ii >= t {
            return 0.0;
        }
        if ii + 1.0 <= t {
            return -h;
        }
        return -h * (t - ii);
    }
    if ii >= o {
        return 0.0;
    }
    if ii + 1.0 <= t {
        return 0.0;
    }
    if t <= ii {
        return h;
    }
    h * (ii + 1.0 - t)
}

pub fn extra_at(i: usize, origin: usize, t: f32, h: i32) -> i32 {
    px(row_extra(i, origin, t, h))
}

pub fn group_well_bottom(group: &str, meta: &[LeafMeta], rects: &[Rect]) -> Option<i32> {
    let mut last = None;
    let mut header = None;
    for (i, m) in meta.iter().enumerate() {
        if m.group.as_deref() != Some(group) {
            continue;
        }
        if m.kind == Kind::Header {
            header = rects.get(i).copied();
        }
        if m.kind == Kind::Item {
            last = rects.get(i).copied();
        }
    }
    let r = last.or(header)?;
    Some(r.y + r.h + WELL_PAD_V)
}

pub fn pointer_in_origin_group(
    group: &str,
    pointer_y: i32,
    meta: &[LeafMeta],
    rects: &[Rect],
) -> bool {
    group_well_bottom(group, meta, rects).is_some_and(|bottom| pointer_y < bottom)
}

pub fn drop_from_slot(
    origin: usize,
    dest: Slot,
    meta: &[LeafMeta],
    pointer_y: i32,
    rects: &[Rect],
) -> Option<Drop> {
    let leaf = meta.get(origin)?;
    if leaf.kind == Kind::Header {
        return None;
    }
    let id = leaf.id.clone();
    match dest {
        Slot::Origin => None,
        Slot::End => {
            if leaf.group.is_none() && origin + 1 == meta.len() {
                return None;
            }
            Some(Drop {
                id,
                dest: DropDest::Loose { before: None },
            })
        }
        Slot::Before(i) => {
            let target = meta.get(i)?;
            if target.kind == Kind::Header {
                return Some(Drop {
                    id,
                    dest: DropDest::BeforeGroup {
                        id: target.id.clone(),
                    },
                });
            }
            if let Some(section) = target.group.clone() {
                return Some(Drop {
                    id,
                    dest: DropDest::Join {
                        section,
                        before: Some(target.id.clone()),
                    },
                });
            }
            if let Some(group) = leaf.group.as_deref() {
                if pointer_in_origin_group(group, pointer_y, meta, rects) {
                    return Some(Drop {
                        id,
                        dest: DropDest::Join {
                            section: group.to_string(),
                            before: None,
                        },
                    });
                }
                return Some(Drop {
                    id,
                    dest: DropDest::Loose {
                        before: Some(target.id.clone()),
                    },
                });
            }
            Some(Drop {
                id,
                dest: DropDest::Loose {
                    before: Some(target.id.clone()),
                },
            })
        }
    }
}

/// Rest Y/H for each leaf from the Scratch CSS (fixed Large density).
pub fn rest_rects(kinds: &[Kind], span_start: &[bool], span_end: &[bool]) -> Vec<Rect> {
    let n = kinds.len();
    let mut out = Vec::with_capacity(n);
    let mut y = STRIP_PAD_TOP;
    if n > 0 && span_start.first().copied().unwrap_or(false) {
        y += FIRST_SPAN_START_EXTRA;
    }
    for i in 0..n {
        out.push(Rect {
            y,
            h: ROW_H,
            kind: kinds[i],
        });
        let gap = if span_end.get(i).copied().unwrap_or(false) {
            SPAN_END_GAP
        } else {
            ROW_GAP
        };
        y += ROW_H + gap;
    }
    out
}

pub fn ease_out(p: f32) -> f32 {
    let x = p.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(y: i32, h: i32) -> Rect {
        Rect {
            y,
            h,
            kind: Kind::Item,
        }
    }
    fn header(y: i32, h: i32) -> Rect {
        Rect {
            y,
            h,
            kind: Kind::Header,
        }
    }
    fn item32(y: i32) -> Rect {
        item(y, 32)
    }
    fn header32(y: i32) -> Rect {
        header(y, 32)
    }

    fn meta(id: &str, kind: Kind, group: Option<&str>) -> LeafMeta {
        LeafMeta {
            id: id.into(),
            kind,
            group: group.map(str::to_string),
        }
    }

    #[test]
    fn pointer_on_origin_is_origin() {
        let rects = [item32(0), item32(32), item32(64)];
        assert_eq!(dest_from_pointer(40, 1, &rects), Slot::Origin);
    }

    #[test]
    fn pointer_on_header_is_origin() {
        let rects = [header32(0), item32(32), item32(80)];
        assert_eq!(dest_from_pointer(10, 2, &rects), Slot::Origin);
    }

    #[test]
    fn before_next_sibling_is_origin() {
        let rects = [header32(0), item32(32), item32(64), item32(112)];
        assert_eq!(dest_from_pointer(50, 1, &rects), Slot::Origin);
        assert_eq!(dest_from_pointer(63, 1, &rects), Slot::Origin);
    }

    #[test]
    fn over_next_member_inserts_after_it() {
        let rects = [header32(0), item32(32), item32(64), item32(112)];
        assert_eq!(dest_from_pointer(70, 1, &rects), Slot::Before(3));
    }

    #[test]
    fn over_previous_member_inserts_before_it() {
        let rects = [header32(0), item32(32), item32(64), item32(96)];
        assert_eq!(dest_from_pointer(40, 2, &rects), Slot::Before(1));
        assert_eq!(dest_from_pointer(55, 2, &rects), Slot::Before(1));
    }

    #[test]
    fn rest_gap_under_group_is_before_next() {
        let rects = [header32(0), item32(32), item32(64), item32(112)];
        assert_eq!(dest_from_pointer(104, 1, &rects), Slot::Before(3));
    }

    #[test]
    fn row_extra_first_hop_moves_c2() {
        let h = 32;
        assert_eq!(row_extra(2, 1, 3.0, h), -h as f32);
        assert_eq!(row_extra(3, 1, 3.0, h), 0.0);
    }

    #[test]
    fn row_extra_second_hop_eases_c3() {
        let h = 32;
        assert_eq!(row_extra(2, 1, 3.5, h), -h as f32);
        assert_eq!(row_extra(3, 1, 3.5, h), -h as f32 * 0.5);
        assert_eq!(row_extra(3, 1, 4.0, h), -h as f32);
        assert_eq!(row_extra(4, 1, 4.0, h), 0.0);
    }

    #[test]
    fn dest_index_origin_is_same_place() {
        assert_eq!(dest_index(1, Slot::Origin, 8), 2);
        assert_eq!(dest_index(1, Slot::Before(3), 8), 3);
    }

    #[test]
    fn drop_on_origin_is_none() {
        let meta = [meta("c4", Kind::Item, Some("c"))];
        assert!(drop_from_slot(0, Slot::Origin, &meta, 0, &[]).is_none());
    }

    #[test]
    fn drop_before_header_is_before_group() {
        let meta = [
            meta("c4", Kind::Item, Some("c")),
            meta("u1", Kind::Item, None),
            meta("b", Kind::Header, Some("b")),
        ];
        let d = drop_from_slot(0, Slot::Before(2), &meta, 0, &[]).unwrap();
        assert_eq!(d.dest, DropDest::BeforeGroup { id: "b".into() });
    }

    #[test]
    fn drop_on_last_member_appends_in_group() {
        let meta = [
            meta("c4", Kind::Item, Some("c")),
            meta("c5", Kind::Item, Some("c")),
            meta("u1", Kind::Item, None),
        ];
        let rects = [item32(0), item32(32), item32(80)];
        let d = drop_from_slot(0, Slot::Before(2), &meta, 40, &rects).unwrap();
        assert_eq!(
            d.dest,
            DropDest::Join {
                section: "c".into(),
                before: None,
            }
        );
    }

    #[test]
    fn drop_below_last_member_is_loose() {
        let meta = [
            meta("c4", Kind::Item, Some("c")),
            meta("c5", Kind::Item, Some("c")),
            meta("u1", Kind::Item, None),
        ];
        let rects = [item32(0), item32(32), item32(80)];
        let d = drop_from_slot(0, Slot::Before(2), &meta, 70, &rects).unwrap();
        assert_eq!(
            d.dest,
            DropDest::Loose {
                before: Some("u1".into()),
            }
        );
    }

    #[test]
    fn drop_in_well_pad_under_last_member_still_appends() {
        let meta = [
            meta("c3", Kind::Item, Some("c")),
            meta("c5", Kind::Item, Some("c")),
            meta("u1", Kind::Item, None),
        ];
        let rects = [item32(0), item32(32), item32(80)];
        let bottom = 32 + 32;
        assert!(pointer_in_origin_group(
            "c",
            bottom + WELL_PAD_V - 1,
            &meta,
            &rects
        ));
        assert!(!pointer_in_origin_group(
            "c",
            bottom + WELL_PAD_V,
            &meta,
            &rects
        ));
        let d = drop_from_slot(0, Slot::Before(2), &meta, bottom + WELL_PAD_V - 1, &rects).unwrap();
        assert_eq!(
            d.dest,
            DropDest::Join {
                section: "c".into(),
                before: None,
            }
        );
    }

    #[test]
    fn c3_over_last_member_dest_is_before_u1() {
        let rects = [
            header32(0),
            item32(32),
            item32(64),
            item32(96),
            item32(128),
            item32(160),
            item32(200),
        ];
        assert_eq!(dest_from_pointer(170, 3, &rects), Slot::Before(6));
    }

    #[test]
    fn slot_eq_cases() {
        assert!(slot_eq(Slot::Origin, Slot::Origin));
        assert!(slot_eq(Slot::Before(3), Slot::Before(3)));
        assert!(!slot_eq(Slot::Before(3), Slot::Before(4)));
    }
}
