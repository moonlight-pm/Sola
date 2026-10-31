//! Where the desk menu's sheets go: the first at the click, each submenu
//! beside its row (to the left where the output ends), all inside the
//! output. The overlay window is their union with room for the shadow.

use sola_kit::components::menu_sheet::{
    KEY_GAP, KEY_SIZE, LABEL_SIZE, MIN_W, PAD, ROW_H, ROW_PAD, SEP_H,
};

use super::rows::Row;

/// Room around the sheets for their drop shadow.
pub const SHADOW: f32 = 12.0;
/// A submenu overlaps its parent's edge by this much.
const OVERLAP: f32 = 4.0;
/// Kept clear at the output's edges.
const EDGE: f32 = crate::zoning::GAP as f32;
/// The widest a sheet grows for a long name.
pub const MAX_W: f32 = 340.0;
/// The search column's width, and its field's row above the results.
pub const SEARCH_W: f32 = 280.0;
pub const FIELD_H: f32 = 36.0;
/// A submenu row's chevron, after its hint.
pub const CHEVRON_W: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The overlay window, in output pixels (whole).
    pub window: Rect,
    /// Each sheet, in the window's pixels.
    pub sheets: Vec<Rect>,
}

fn row_h(row: &Row) -> f32 {
    match row {
        Row::Sep => SEP_H,
        Row::Item(_) => ROW_H,
    }
}

/// A sheet's height: its rows (under the search field when it has one).
pub fn height(rows: &[Row], search: bool) -> f32 {
    row_top(rows, rows.len(), search) + PAD
}

/// The top of row `index`, from its sheet's top.
pub fn row_top(rows: &[Row], index: usize, search: bool) -> f32 {
    let field = if search { FIELD_H } else { 0.0 };
    PAD + field + rows[..index.min(rows.len())].iter().map(row_h).sum::<f32>()
}

/// The search column's height: its field and room for every result it
/// shows, so typing never resizes the overlay.
pub fn search_height() -> f32 {
    2.0 * PAD + FIELD_H + super::vault::FOUND_MAX as f32 * ROW_H
}

/// A sheet's width: its widest row, at least [`MIN_W`], at most
/// [`MAX_W`]; the search column is fixed. `measure` is a line's width at
/// a size.
pub fn width(rows: &[Row], search: bool, measure: impl Fn(&str, f32) -> f32) -> f32 {
    if search {
        return SEARCH_W;
    }
    let widest = rows
        .iter()
        .filter_map(Row::item)
        .map(|item| {
            let hint = item
                .hint
                .as_deref()
                .map_or(0.0, |h| KEY_GAP + measure(h, KEY_SIZE));
            let chevron = if matches!(item.does, super::rows::Does::Sub(_)) {
                KEY_GAP / 2.0 + CHEVRON_W
            } else {
                0.0
            };
            2.0 * (PAD + ROW_PAD) + measure(&item.label, LABEL_SIZE) + hint + chevron
        })
        .fold(MIN_W, f32::max);
    widest.ceil().min(MAX_W)
}

/// Place sheets of `sizes` (width, height): the first at `anchor`, each
/// next one beside the open row of the one before (`open_tops[i]`, that
/// row's top in sheet `i`). Everything stays inside `output`.
pub fn place(output: (f32, f32), anchor: (f32, f32), sizes: &[(f32, f32)], open_tops: &[f32]) -> Placed {
    let (ow, oh) = output;
    let fit_x = |x: f32, w: f32| x.min(ow - EDGE - w).max(EDGE);
    let fit_y = |y: f32, h: f32| y.min(oh - EDGE - h).max(EDGE);
    let mut sheets: Vec<Rect> = Vec::with_capacity(sizes.len());
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let (x, y) = match sheets.last() {
            None => {
                // Past the right or bottom, the menu opens the other way
                // from the pointer.
                let (ax, ay) = anchor;
                let x = if ax + w > ow - EDGE { ax - w } else { ax };
                let y = if ay + h > oh - EDGE { ay - h } else { ay };
                (x, y)
            }
            Some(parent) => {
                let top = parent.y + open_tops.get(i - 1).copied().unwrap_or(PAD) - PAD;
                let right = parent.x + parent.w - OVERLAP;
                let x = if right + w > ow - EDGE {
                    parent.x - w + OVERLAP
                } else {
                    right
                };
                (x, top)
            }
        };
        sheets.push(Rect {
            x: fit_x(x, w),
            y: fit_y(y, h),
            w,
            h,
        });
    }
    let x0 = sheets.iter().map(|s| s.x).fold(f32::INFINITY, f32::min) - SHADOW;
    let y0 = sheets.iter().map(|s| s.y).fold(f32::INFINITY, f32::min) - SHADOW;
    let x1 = sheets.iter().map(|s| s.x + s.w).fold(0.0, f32::max) + SHADOW;
    let y1 = sheets.iter().map(|s| s.y + s.h).fold(0.0, f32::max) + SHADOW;
    let (x0, y0) = (x0.max(0.0).floor(), y0.max(0.0).floor());
    let (x1, y1) = (x1.min(ow).ceil(), y1.min(oh).ceil());
    Placed {
        window: Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        },
        sheets: sheets
            .into_iter()
            .map(|s| Rect {
                x: s.x - x0,
                y: s.y - y0,
                ..s
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: (f32, f32) = (1920.0, 1080.0);

    #[test]
    fn the_menu_opens_at_the_click() {
        let p = place(OUT, (500.0, 300.0), &[(200.0, 100.0)], &[]);
        assert_eq!(p.window.x, 500.0 - SHADOW);
        assert_eq!(p.window.y, 300.0 - SHADOW);
        assert_eq!(p.sheets[0].x, SHADOW);
        assert_eq!(p.sheets[0].y, SHADOW);
        assert_eq!(p.window.w, 200.0 + 2.0 * SHADOW);
    }

    #[test]
    fn near_the_right_and_bottom_it_opens_the_other_way() {
        let p = place(OUT, (1900.0, 1070.0), &[(200.0, 100.0)], &[]);
        let s = p.sheets[0];
        assert_eq!(p.window.x + s.x, 1700.0);
        assert_eq!(p.window.y + s.y, 970.0);
    }

    #[test]
    fn a_submenu_hangs_beside_its_row() {
        let p = place(OUT, (500.0, 300.0), &[(200.0, 100.0), (220.0, 80.0)], &[36.0]);
        let (main, sub) = (p.sheets[0], p.sheets[1]);
        assert_eq!(sub.x, main.x + main.w - OVERLAP);
        // Its first row lines up with the row that opened it.
        assert_eq!(sub.y + PAD, main.y + 36.0);
        assert!(p.window.w >= 200.0 + 220.0 - OVERLAP + 2.0 * SHADOW);
    }

    #[test]
    fn a_submenu_with_no_room_on_the_right_goes_left() {
        let p = place(OUT, (1600.0, 300.0), &[(200.0, 100.0), (220.0, 80.0)], &[10.0]);
        let (main, sub) = (p.sheets[0], p.sheets[1]);
        assert_eq!(sub.x + sub.w - OVERLAP, main.x);
    }

    #[test]
    fn everything_stays_on_the_output() {
        let p = place(OUT, (10.0, 1075.0), &[(200.0, 400.0), (300.0, 900.0)], &[380.0]);
        for s in &p.sheets {
            assert!(p.window.x + s.x >= EDGE);
            assert!(p.window.y + s.y >= EDGE);
            assert!(p.window.y + s.y + s.h <= OUT.1 - EDGE);
        }
        assert!(p.window.x >= 0.0 && p.window.x + p.window.w <= OUT.0);
        assert!(p.window.y + p.window.h <= OUT.1);
    }

    #[test]
    fn heights_and_row_tops_count_separators() {
        use super::super::rows::{Does, Item};
        let item = || {
            Row::Item(Item {
                label: "Copy".into(),
                hint: None,
                enabled: true,
                does: Does::Nothing,
            })
        };
        let rows = vec![item(), Row::Sep, item()];
        assert_eq!(row_top(&rows, 2, false), PAD + ROW_H + SEP_H);
        assert_eq!(height(&rows, false), 2.0 * PAD + 2.0 * ROW_H + SEP_H);
        assert_eq!(row_top(&rows, 0, true), PAD + FIELD_H);
        let w = width(&rows, false, |s, _| s.len() as f32 * 7.0);
        assert_eq!(w, MIN_W);
        assert_eq!(width(&rows, true, |_, _| 0.0), SEARCH_W);
    }
}
