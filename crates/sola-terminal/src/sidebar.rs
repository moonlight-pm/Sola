use iced::widget::{button, column, container, scrollable, text};
use iced::{Background, Border, Color, Element, Length, Theme};
use sola_bus::topics::TerminalConfig;

use crate::state::Tabs;
use crate::Msg;

#[derive(Default)]
pub struct SidebarState {
    pub dragging_divider: bool,
    pub drag_anchor: Option<(f32, f32)>,
    pub reorder: Option<(usize, f32)>,
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
            let id = meta.id.clone();
            button(text(label))
                .on_press(Msg::SelectTab(id))
                .style(move |theme: &Theme, status| tab_button_style(theme, status, is_active))
                .width(Length::Fill)
                .into()
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

    let _ = state; // drag state is held on App; sidebar just renders

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
/// The sidebar grows as the cursor moves left (toward the sidebar) and shrinks
/// as it moves right. Uses an anchor-relative formula so there is no drift when
/// the cursor re-enters the clamped range after having exceeded it:
///
///   `new_width = anchor_width + (anchor_x - cursor_x)`
///
/// Result is clamped to `[SIDEBAR_W_MIN, SIDEBAR_W_MAX]`.
///
/// This is a pure function so it can be unit-tested without an iced runtime.
pub fn dragged_width(anchor_x: f32, anchor_w: f32, cursor_x: f32) -> f32 {
    let desired = anchor_w + (anchor_x - cursor_x);
    desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_label() {
        assert_eq!(tab_label(&Some("/home/joshua/Workspace".into())), "Workspace");
        assert_eq!(tab_label(&Some("/".into())), "/");
        assert_eq!(tab_label(&None), "shell");
    }

    // --- dragged_width ---

    #[test]
    fn dragged_width_widens_on_left_drag() {
        // Anchor at x=200, width=120. Cursor moves left to x=150 → delta=-50 → new=170.
        let w = dragged_width(200.0, 120.0, 150.0);
        assert_eq!(w, 170.0);
    }

    #[test]
    fn dragged_width_narrows_on_right_drag() {
        // Anchor at x=200, width=160. Cursor moves right to x=240 → delta=+40 → new=120.
        let w = dragged_width(200.0, 160.0, 240.0);
        assert_eq!(w, 120.0);
    }

    #[test]
    fn dragged_width_clamps_min() {
        // A very far right drag would give negative width — clamp to MIN.
        let w = dragged_width(200.0, 100.0, 600.0);
        assert_eq!(w, SIDEBAR_W_MIN);
    }

    #[test]
    fn dragged_width_clamps_max() {
        // A very far left drag would exceed 250 — clamp to MAX.
        let w = dragged_width(200.0, 200.0, 0.0);
        assert_eq!(w, SIDEBAR_W_MAX);
    }

    #[test]
    fn dragged_width_no_movement() {
        // Cursor stays at anchor — width unchanged (clamped to valid range).
        let w = dragged_width(200.0, 160.0, 200.0);
        assert_eq!(w, 160.0);
    }
}
