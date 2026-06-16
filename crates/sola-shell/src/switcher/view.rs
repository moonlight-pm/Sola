//! Switcher overlay view — alt-tab equivalent.
//!
//! Renders a centered grid of app tiles over a transparent full-screen backdrop.
//! The grid wraps so the switcher grows to fit any number of open apps.
//! When `switcher.active` is false, returns an invisible placeholder so the
//! surface stays alive without rendering content.

use iced::widget::{column, container, mouse_area, row, text};
use iced::{Alignment, Element, Length, Padding};
use sola_kit::components::icon_colored;

use crate::app::{Msg, Shell};

/// Render the switcher overlay for `shell`.
///
/// Layout:
///   Full-screen invisible mouse_area (click-outside-to-cancel)
///   └─ Centered backplate card with shell-switcher-pad padding.
///      Background: shell-switcher-bg (translucent accent fill by default).
///      Inside: a balanced grid of uniform-width tiles, one per open app.
///      The grid wraps to extra rows so the switcher grows to fit any number
///      of open apps instead of overflowing (and clipping at) the screen width.
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.switcher.active {
        // Invisible placeholder — keeps iced from getting an empty view.
        return container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let switcher = &shell.switcher;

    // Uniform tile width so the grid wraps predictably; each tile holds a
    // 52px icon over a centered, wrapping label.
    const TILE_W: f32 = 128.0;

    // --- app tiles (uniform width) ---
    let cards: Vec<Element<'_, Msg>> = switcher
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            // Look up display label and icon from the application catalog.
            let catalog_entry = shell.applications.get(&app.app_id);
            let label_str = catalog_entry
                .map(|a| a.label.as_str())
                .unwrap_or(app.app_id.as_str());
            let icon_name = catalog_entry
                .map(|a| a.icon.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("lucide/box");

            let is_selected = i == switcher.selected;

            // Glyph + label foreground differs by state: the highlighted tile
            // uses shell-switcher-icon-fg-sel, the rest shell-switcher-icon-fg.
            let icon_fg = if is_selected {
                shell.style.switcher_icon_fg_sel
            } else {
                shell.style.switcher_icon_fg
            };

            let icon_el: Element<'_, Msg> = icon_colored(icon_name, 52, icon_fg);
            // Label fills the tile width and centers, wrapping if it's long —
            // this is what keeps every tile a uniform TILE_W.
            let label_el: Element<'_, Msg> = text(label_str)
                .size(13)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .into();

            let card_content: Element<'_, Msg> = column![icon_el, label_el]
                .spacing(8)
                .align_x(Alignment::Center)
                .width(Length::Fill)
                .into();

            // Fixed-width tile: selected fills with shell-switcher-icon-bg,
            // unselected is transparent; shell-switcher-icon-fg tints the
            // glyph + label in both states (RADIUS_MD=6). Tile padding knob:
            // vertical = shell-switcher-tile-pad, horizontal = vertical + 4
            // (preserves the original 16/20).
            let tp = shell.style.switcher_tile_pad;
            let card_container: Element<'_, Msg> = container(card_content)
                .width(Length::Fixed(TILE_W))
                .padding(Padding { top: tp, bottom: tp, left: tp + 4.0, right: tp + 4.0 })
                .style(sola_kit::components::card::list_tile_style_colored(
                    is_selected,
                    shell.style.switcher_icon_bg,
                    icon_fg,
                ))
                .into();

            mouse_area(card_container)
                .on_enter(Msg::SwitcherHover { index: i })
                .into()
        })
        .collect();

    // --- wrap the tiles into a balanced grid ---
    // The grid grows to fit every open app: tiles flow across as many columns
    // as fit the screen width, then wrap to new rows. We balance the column
    // count so the last row isn't left with a single stranded tile.
    let gap = 12.0_f32;
    let n = cards.len();
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    // Width available to the grid: screen minus backplate padding (both sides)
    // and a small margin so the panel never reaches the screen edges.
    let margin = 24.0_f32;
    let avail = (output_w - 2.0 * (shell.style.switcher_pad + margin)).max(TILE_W);
    let cols_cap = (((avail + gap) / (TILE_W + gap)).floor() as usize).max(1);
    let cols = grid_columns(n, cols_cap);

    let mut grid_rows: Vec<Element<'_, Msg>> = Vec::new();
    let mut current: Vec<Element<'_, Msg>> = Vec::new();
    for card in cards {
        current.push(card);
        if current.len() == cols {
            grid_rows.push(
                row(std::mem::take(&mut current))
                    .spacing(gap)
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
    }
    if !current.is_empty() {
        grid_rows.push(row(current).spacing(gap).align_y(Alignment::Center).into());
    }
    let grid: Element<'_, Msg> = column(grid_rows)
        .spacing(gap)
        .align_x(Alignment::Center)
        .into();

    // Backplate fill/border come from the shell-* tokens (alpha-capable);
    // padding from shell-switcher-pad. Seed values match the old
    // accent-derived look exactly.
    let backplate: Element<'_, Msg> = sola_kit::components::backplate(
        grid,
        shell.style.switcher_bg,
        shell.style.switcher_border,
    )
    .padding(Padding::new(shell.style.switcher_pad))
    .into();

    // Center the backplate on screen.
    let centered: Element<'_, Msg> = container(backplate)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    // Full-screen invisible click-catcher dismisses the switcher. The
    // backplate sits inside its own region and absorbs hover/clicks first,
    // so clicking outside the cards is what reaches this layer.
    mouse_area(centered)
        .on_press(Msg::SwitcherCancel)
        .into()
}

/// Balanced column count for `n` tiles when at most `cap` fit per row.
///
/// Spreads tiles as evenly as possible across the rows actually needed, so the
/// last row isn't left with a single stranded tile. Example: 15 tiles with a
/// cap of 13 wraps to 8 columns over 2 rows (8 + 7), not 13 + 2.
fn grid_columns(n: usize, cap: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let cap = cap.max(1);
    let rows_needed = n.div_ceil(cap);
    n.div_ceil(rows_needed).max(1)
}

#[cfg(test)]
mod tests {
    use super::grid_columns;

    #[test]
    fn columns_empty_or_single_is_one() {
        assert_eq!(grid_columns(0, 13), 1);
        assert_eq!(grid_columns(1, 13), 1);
    }

    #[test]
    fn columns_fit_in_one_row() {
        assert_eq!(grid_columns(5, 13), 5);
        assert_eq!(grid_columns(13, 13), 13);
    }

    #[test]
    fn columns_balance_across_two_rows() {
        // 14 over a cap of 13 → 2 rows, balanced to 7 each (not 13 + 1).
        assert_eq!(grid_columns(14, 13), 7);
        // 15 → 2 rows balanced to 8 (8 + 7), not 13 + 2.
        assert_eq!(grid_columns(15, 13), 8);
        // Exactly two full rows.
        assert_eq!(grid_columns(26, 13), 13);
    }

    #[test]
    fn columns_balance_across_three_rows() {
        // 27 over a cap of 13 → 3 rows, balanced to 9 each.
        assert_eq!(grid_columns(27, 13), 9);
    }

    #[test]
    fn columns_cap_of_one_is_single_column() {
        // Degenerate narrow screen: one tile per row.
        assert_eq!(grid_columns(3, 1), 1);
        assert_eq!(grid_columns(3, 0), 1);
    }
}
