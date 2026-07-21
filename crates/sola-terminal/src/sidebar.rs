use iced::Element;
use sola_bus::topics::TerminalConfig;
use sola_kit::components::{ReorderCfg, SidebarItem, SidebarPanel, SidebarSection};

use crate::state::Tabs;
use crate::Msg;

#[derive(Default)]
pub struct SidebarState {
    pub dragging_divider: bool,
    pub drag_anchor: Option<(f32, f32)>,
    /// Active tab-reorder gesture.
    ///
    /// `Some((from_index, start_y))` from press until release.
    /// `from_index` is the position of the pressed tab in `ids_in_order()`.
    /// `start_y` is the cursor-y captured on the first move after the press
    /// (mirrors the divider's anchor-on-first-move pattern; `0.0` sentinel
    /// until that first sample).
    /// `None` means no reorder gesture is active.
    pub reorder: Option<(usize, f32)>,
    /// Current cursor-y during a reorder gesture; used to render the drop
    /// target highlight and to compute the drop slot on release.
    pub reorder_cursor_y: f32,
    /// True once the gesture has moved past [`PANEL_REORDER_THRESHOLD`].
    /// Gates drag chrome so a plain click never flashes the lifted/drop look.
    pub reorder_dragging: bool,
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

/// Render the terminal's tab sidebar via the shared kit [`SidebarPanel`].
///
/// Each tab becomes one [`SidebarItem`] in a single unlabeled section. The
/// item label carries the tab number + cwd basename; collapse/resize/reorder
/// are all delegated to the kit (which owns the divider mouse_area, the drag
/// overlay, and the live-reorder preview). The terminal still owns the cursor
/// subscriptions, `SidebarState`, and the release click-vs-drag logic.
///
/// The per-item `message` is **never fired** in this app: reorder is always
/// enabled, so the kit wraps each row in a `mouse_area` whose press emits
/// `ReorderStart`; selection is decided by `ReorderEnd`'s click threshold. We
/// pass `Msg::Noop` (a no-op-on-receive variant) to satisfy the API.
pub fn view<'a>(
    state: &'a SidebarState,
    tabs: &'a Tabs,
    active: Option<&str>,
    config: &TerminalConfig,
) -> Element<'a, Msg> {
    let ordered = tabs.tab_strip();

    let items: Vec<SidebarItem<Msg>> = ordered
        .iter()
        .enumerate()
        .map(|(i, meta)| {
            let is_active = active == Some(meta.id.as_str());
            // Label is just the cwd basename; the tab's ordinal shows as the
            // right-aligned dimmed shortcut hint (Cmd/Ctrl+1..9), not in the label.
            let item = SidebarItem::new(tab_label(&meta.cwd), Msg::Noop).active(is_active);
            if i < 9 {
                item.shortcut((i + 1) as u8)
            } else {
                item
            }
        })
        .collect();

    let sections = vec![SidebarSection::unlabeled(items)];

    SidebarPanel::new(sections)
        .resizable(
            config.sidebar_width as f32,
            state.dragging_divider,
            Msg::SidebarDragStart,
        )
        .reorderable(ReorderCfg {
            on_press: Box::new(Msg::ReorderStart),
            // Expose the gesture as "active" only once it's a real drag, so the
            // panel shows no drag chrome on a plain (un-moved) press. Matches
            // the kit storybook and the ReorderCfg contract.
            active: if state.reorder_dragging {
                state.reorder
            } else {
                None
            },
            cursor_y: state.reorder_cursor_y,
        })
        .build()
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
}
