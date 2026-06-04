use iced::widget::{button, column, container, mouse_area, scrollable, text};
use iced::{Background, Border, Color, Element, Length, Theme};
use sola_bus::topics::TerminalConfig;

use crate::state::Tabs;
use crate::Msg;

#[derive(Default)]
pub struct SidebarState {
    pub dragging_divider: bool,
    pub drag_anchor: Option<(f32, f32)>,
    /// Active tab-reorder gesture.
    ///
    /// `Some((from_index, start_y))` while a drag is in progress.
    /// `from_index` is the position of the pressed tab in `ids_in_order()`.
    /// `start_y` is the cursor-y captured on the first `ReorderMove` after
    /// the press (mirrors the divider's anchor-on-first-move pattern).
    /// `None` means no reorder gesture is active.
    pub reorder: Option<(usize, f32)>,
    /// Current cursor-y during a reorder gesture; used to render the drop
    /// target highlight and to compute the drop slot on `ReorderEnd`.
    pub reorder_cursor_y: f32,
}

/// cwd basename → tab label, falling back to "shell".
pub fn tab_label(cwd: &Option<String>) -> String {
    match cwd.as_deref() {
        Some("/") => "/".into(),
        Some(p) if !p.is_empty() => p
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("shell")
            .to_string(),
        _ => "shell".into(),
    }
}

pub fn view<'a>(
    state: &'a SidebarState,
    tabs: &'a Tabs,
    active: Option<&str>,
    config: &TerminalConfig,
) -> Element<'a, Msg> {
    let width = if config.sidebar_collapsed {
        36.0_f32
    } else {
        config.sidebar_width as f32
    };

    // Collapse/expand toggle button at the top.
    let toggle_label = if config.sidebar_collapsed { "»" } else { "«" };
    let toggle_btn = button(text(toggle_label))
        .on_press(Msg::ToggleCollapse)
        .style(|theme: &Theme, status| tab_button_style(theme, status, false))
        .width(Length::Fill)
        .into();

    let mut col_items: Vec<Element<'a, Msg>> = vec![toggle_btn];

    let ordered = tabs.ordered_meta();

    // Compute the drop-target index for highlight rendering.
    let drop_target: Option<usize> = if state.reorder.is_some() && !ordered.is_empty() {
        Some(drop_index(
            state.reorder_cursor_y,
            SIDEBAR_HEADER_H,
            TAB_ROW_H,
            ordered.len(),
        ))
    } else {
        None
    };

    let tab_items: Vec<Element<'a, Msg>> = ordered
        .iter()
        .enumerate()
        .map(|(i, meta)| {
            let label = if config.sidebar_collapsed {
                format!("{}", i + 1)
            } else {
                format!("{}  {}", i + 1, tab_label(&meta.cwd))
            };
            let is_active = active == Some(meta.id.as_str());
            // Highlight this row if it is the current drop target (and a
            // reorder gesture is active, but the item is not being dragged
            // from this slot — we show where it will land).
            let is_drop_target = drop_target == Some(i)
                && state.reorder.map(|(from, _)| from) != Some(i);
            // The tab row is a *non-pressable* container wrapped in a
            // mouse_area.  An inner `button` with its own `on_press` would
            // `shell.capture_event()` on ButtonPressed, and mouse_area bails
            // out early on a captured event — so its `on_press(ReorderStart)`
            // would never fire (the press would only ever SelectTab).  Using a
            // plain container guarantees the press reaches the mouse_area.
            //
            // Selection is therefore driven entirely by the reorder gesture:
            // ReorderEnd treats a sub-threshold (or zero-move) gesture as a
            // click → select_tab.  See main.rs Msg::ReorderEnd.
            let row = container(text(label).width(Length::Fill))
                .width(Length::Fill)
                .padding([4, 8])
                .style(move |theme: &Theme| {
                    tab_row_style(theme, is_active, is_drop_target)
                });
            mouse_area(row).on_press(Msg::ReorderStart(i)).into()
        })
        .collect();
    col_items.extend(tab_items);

    // "+ New Tab" button (hidden when collapsed to save space)
    if !config.sidebar_collapsed {
        col_items.push(
            button(text("+ New Tab"))
                .on_press(Msg::NewTab)
                .style(|theme: &Theme, status| tab_button_style(theme, status, false))
                .width(Length::Fill)
                .into(),
        );
    }

    container(scrollable(column(col_items)))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .into()
}

pub fn tab_button_style(
    theme: &Theme,
    _status: button::Status,
    active: bool,
) -> button::Style {
    let p = theme.extended_palette();
    let bg = if active {
        Some(Background::Color(p.background.weak.color))
    } else {
        Some(Background::Color(Color::TRANSPARENT))
    };
    button::Style {
        background: bg,
        text_color: p.background.base.text,
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    }
}

/// Minimum sidebar width in logical pixels (drag clamp).
pub const SIDEBAR_W_MIN: f32 = 80.0;
/// Maximum sidebar width in logical pixels (drag clamp).
pub const SIDEBAR_W_MAX: f32 = 250.0;

/// Compute the new sidebar width from a drag gesture.
///
/// The sidebar is on the LEFT, so it grows as the cursor moves right (away from
/// the sidebar) and shrinks as it moves left. Uses an anchor-relative formula so
/// there is no drift when the cursor re-enters the clamped range after exceeding
/// it:
///
///   `new_width = anchor_width + (cursor_x - anchor_x)`
///
/// Result is clamped to `[SIDEBAR_W_MIN, SIDEBAR_W_MAX]`.
///
/// This is a pure function so it can be unit-tested without an iced runtime.
pub fn dragged_width(anchor_x: f32, anchor_w: f32, cursor_x: f32) -> f32 {
    // The sidebar is on the LEFT, so the divider is its right edge: dragging
    // the cursor RIGHT (cursor_x > anchor_x) widens it, dragging LEFT narrows.
    // (sola-monitor's sidebar is on the right, so its formula uses the opposite
    // sign — don't copy it verbatim.)
    let desired = anchor_w + (cursor_x - anchor_x);
    desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX)
}

// ---------------------------------------------------------------------------
// Reorder geometry constants
// ---------------------------------------------------------------------------

/// Height of the toggle-button row at the top of the sidebar (px).
///
/// This matches the button height that iced renders for a single-line button
/// with default padding (approximately `line-height + 2×padding`). Iced 0.14
/// default padding is 5px top/bottom on a ~20px line, giving ~30px total.
/// We use 32px as a conservative round number; if the real height differs by a
/// few pixels the drop-slot calc rounds down (floor), so it may land one row
/// off at the very top — an acceptable approximation that can be tightened
/// once exact pixel geometry is measured from the rendered layout.
pub const SIDEBAR_HEADER_H: f32 = 32.0;

/// Height of each tab row in the sidebar (px).
///
/// Same reasoning as SIDEBAR_HEADER_H: iced default single-line button.
/// Both constants are shared here so `drop_index` and the view rendering
/// cannot drift apart.
pub const TAB_ROW_H: f32 = 32.0;

/// Movement threshold (px) below which a press-then-release is treated as a
/// click (→ SelectTab) rather than a completed reorder drag.
pub const REORDER_THRESHOLD: f32 = 5.0;

// ---------------------------------------------------------------------------
// Pure reorder helpers
// ---------------------------------------------------------------------------

/// Return the slot index (0-based, "insert before slot k") that the cursor
/// is hovering over, given the top-y of the tab list and the row height.
///
/// Formula: `floor((cursor_y - list_top) / row_h)`, clamped to `0..=(n-1)`.
///
/// - `list_top` — y of the top edge of the first tab row (= `SIDEBAR_HEADER_H`).
/// - `row_h`    — height of each tab row (= `TAB_ROW_H`).
/// - `n`        — number of tabs (must be > 0; returns 0 for n == 0).
///
/// The result indexes into the current `ids_in_order()` list and indicates
/// which existing slot the dragged tab will replace (push-down model): the
/// dragged item is removed from `from`, then inserted at `to`, shifting
/// everything between them by one. This is the same model used by `reordered`.
///
/// NOTE: the drop position is approximate. It uses an absolute `cursor_y` and
/// fixed row metrics, and does NOT account for the `scrollable` offset. For a
/// handful of tabs (no scroll) this is exact; for a long, scrolled tab list the
/// computed slot will be off by the hidden scroll distance. Known limitation —
/// not worth a full geometry rework until many-tab reordering is a real need.
pub fn drop_index(cursor_y: f32, list_top: f32, row_h: f32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let rel = cursor_y - list_top;
    if rel < 0.0 {
        return 0;
    }
    let slot = (rel / row_h).floor() as usize;
    slot.min(n - 1)
}

/// Move the item at position `from` to position `to` in `order`, returning the
/// new ordering.
///
/// `to` is clamped into `0..=(n-1)` so out-of-range values are safe.  When
/// `from == to` (after clamping) the original order is returned unchanged.
///
/// This is a pure "remove then insert" operation consistent with
/// `drop_index`'s slot model.
pub fn reordered(order: &[String], from: usize, to: usize) -> Vec<String> {
    if order.is_empty() {
        return Vec::new();
    }
    let from = from.min(order.len() - 1);
    let to = to.min(order.len() - 1);
    let mut v: Vec<String> = order.to_vec();
    let item = v.remove(from);
    v.insert(to, item);
    v
}

/// Given a new ordering of tab ids, return the `(id, new_ordinal)` pairs for
/// tabs whose ordinal has changed.
///
/// The ordinal assigned is simply the 0-based position index in `new_order`.
/// `meta_ordinals` maps each id to its current ordinal; pairs are only
/// returned when `new_ordinal != current_ordinal`.
pub fn renumber_changed(
    meta_ordinals: &std::collections::HashMap<String, u32>,
    new_order: &[String],
) -> Vec<(String, u32)> {
    new_order
        .iter()
        .enumerate()
        .filter_map(|(k, id)| {
            let new_ordinal = k as u32;
            let cur = meta_ordinals.get(id).copied().unwrap_or(u32::MAX);
            if cur != new_ordinal {
                Some((id.clone(), new_ordinal))
            } else {
                None
            }
        })
        .collect()
}




/// Style for a tab *row* rendered as a non-pressable `container` (so the
/// wrapping `mouse_area` receives the press for the reorder gesture).
/// Drop-target highlight wins over the active-tab highlight, otherwise
/// transparent.
pub fn tab_row_style(theme: &Theme, active: bool, drop_target: bool) -> container::Style {
    let p = theme.extended_palette();
    let bg = if drop_target {
        Some(Background::Color(p.primary.weak.color))
    } else if active {
        Some(Background::Color(p.background.weak.color))
    } else {
        Some(Background::Color(Color::TRANSPARENT))
    };
    container::Style {
        background: bg,
        text_color: Some(p.background.base.text),
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn basename_label() {
        assert_eq!(tab_label(&Some("/home/joshua/Workspace".into())), "Workspace");
        assert_eq!(tab_label(&Some("/".into())), "/");
        assert_eq!(tab_label(&None), "shell");
    }

    // --- dragged_width ---

    #[test]
    fn dragged_width_widens_on_right_drag() {
        // Anchor at x=200, width=120. Cursor moves RIGHT to x=250 → delta=+50 → new=170.
        let w = dragged_width(200.0, 120.0, 250.0);
        assert_eq!(w, 170.0);
    }

    #[test]
    fn dragged_width_narrows_on_left_drag() {
        // Anchor at x=200, width=160. Cursor moves LEFT to x=160 → delta=-40 → new=120.
        let w = dragged_width(200.0, 160.0, 160.0);
        assert_eq!(w, 120.0);
    }

    #[test]
    fn dragged_width_clamps_min() {
        // A very far LEFT drag would give negative width — clamp to MIN.
        let w = dragged_width(200.0, 100.0, 0.0);
        assert_eq!(w, SIDEBAR_W_MIN);
    }

    #[test]
    fn dragged_width_clamps_max() {
        // A very far RIGHT drag would exceed 250 — clamp to MAX.
        let w = dragged_width(200.0, 200.0, 600.0);
        assert_eq!(w, SIDEBAR_W_MAX);
    }

    #[test]
    fn dragged_width_no_movement() {
        // Cursor stays at anchor — width unchanged (clamped to valid range).
        let w = dragged_width(200.0, 160.0, 200.0);
        assert_eq!(w, 160.0);
    }

    // --- reordered ---

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reordered_move_down() {
        // Move index 0 ("a") to index 2 → ["b","c","a"]
        let result = reordered(&sv(&["a", "b", "c"]), 0, 2);
        assert_eq!(result, sv(&["b", "c", "a"]));
    }

    #[test]
    fn reordered_move_up() {
        // Move index 2 ("c") to index 0 → ["c","a","b"]
        let result = reordered(&sv(&["a", "b", "c"]), 2, 0);
        assert_eq!(result, sv(&["c", "a", "b"]));
    }

    #[test]
    fn reordered_noop_same_index() {
        // from == to → unchanged
        let result = reordered(&sv(&["a", "b", "c"]), 1, 1);
        assert_eq!(result, sv(&["a", "b", "c"]));
    }

    #[test]
    fn reordered_clamps_to_out_of_range() {
        // to = 999 → clamped to n-1 = 2 → same as moving to index 2
        let result = reordered(&sv(&["a", "b", "c"]), 0, 999);
        assert_eq!(result, sv(&["b", "c", "a"]));
    }

    #[test]
    fn reordered_from_clamps_out_of_range() {
        // from = 999 → clamped to n-1 = 2 ("c")
        let result = reordered(&sv(&["a", "b", "c"]), 999, 0);
        assert_eq!(result, sv(&["c", "a", "b"]));
    }

    #[test]
    fn reordered_empty_slice() {
        let result = reordered(&[], 0, 0);
        assert!(result.is_empty());
    }

    // --- drop_index ---

    #[test]
    fn drop_index_slot_zero() {
        // cursor_y == list_top → slot 0
        let idx = drop_index(SIDEBAR_HEADER_H, SIDEBAR_HEADER_H, TAB_ROW_H, 3);
        assert_eq!(idx, 0);
    }

    #[test]
    fn drop_index_middle_slot() {
        // cursor at list_top + 1.5 × row_h → floor(1.5) = 1 → slot 1
        let idx = drop_index(SIDEBAR_HEADER_H + TAB_ROW_H * 1.5, SIDEBAR_HEADER_H, TAB_ROW_H, 3);
        assert_eq!(idx, 1);
    }

    #[test]
    fn drop_index_past_end_clamps() {
        // cursor way below last row → clamps to n-1 = 2
        let idx = drop_index(SIDEBAR_HEADER_H + TAB_ROW_H * 100.0, SIDEBAR_HEADER_H, TAB_ROW_H, 3);
        assert_eq!(idx, 2);
    }

    #[test]
    fn drop_index_above_list_clamps_to_zero() {
        // cursor above list_top → clamps to 0
        let idx = drop_index(0.0, SIDEBAR_HEADER_H, TAB_ROW_H, 3);
        assert_eq!(idx, 0);
    }

    // --- renumber_changed ---

    fn ordinal_map(pairs: &[(&str, u32)]) -> HashMap<String, u32> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn renumber_changed_detects_changed_pairs() {
        // Original ordinals: a=0, b=1, c=2
        // New order: [b, c, a] → b gets 0 (was 1), c gets 1 (was 2), a gets 2 (was 0)
        // All three changed.
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["b", "c", "a"]);
        let changed = renumber_changed(&ordinals, &new_order);
        // All three differ.
        assert_eq!(changed.len(), 3);
        // b → 0
        assert!(changed.contains(&("b".to_string(), 0)));
        // c → 1
        assert!(changed.contains(&("c".to_string(), 1)));
        // a → 2
        assert!(changed.contains(&("a".to_string(), 2)));
    }

    #[test]
    fn renumber_changed_no_changes_when_same_order() {
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["a", "b", "c"]);
        let changed = renumber_changed(&ordinals, &new_order);
        assert!(changed.is_empty());
    }

    #[test]
    fn renumber_changed_single_swap() {
        // Move a to end: [b, c, a]; only a and b differ from new positions.
        // b: was 1 → now 0 (changed)
        // c: was 2 → now 1 (changed)
        // a: was 0 → now 2 (changed)
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["b", "c", "a"]);
        let changed = renumber_changed(&ordinals, &new_order);
        // All three changed in this rotation.
        assert_eq!(changed.len(), 3);
    }

    #[test]
    fn renumber_changed_adjacent_swap_only_two_changed() {
        // Swap b and c: [a, c, b]
        // a: 0 → 0 (no change)
        // c: 2 → 1 (changed)
        // b: 1 → 2 (changed)
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["a", "c", "b"]);
        let changed = renumber_changed(&ordinals, &new_order);
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&("c".to_string(), 1)));
        assert!(changed.contains(&("b".to_string(), 2)));
    }
}
