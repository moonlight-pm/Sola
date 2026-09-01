use iced::{Element, Theme};
use sola_bus::topics::TerminalConfig;
use sola_kit::components::sidebar;
use sola_kit::components::{
    DividerColors, SidebarDensity, SidebarItem, SidebarPanel, SidebarSection,
};

pub use sola_kit::components::SidebarState;

use sola_terminal::state::Tabs;

use crate::Msg;

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
pub fn view<'a>(
    gestures: &'a SidebarState,
    tabs: &'a Tabs,
    active: Option<&str>,
    config: &TerminalConfig,
    theme: &Theme,
    term_bg: iced::Color,
) -> Element<'a, Msg> {
    let ordered = tabs.tab_strip();

    let items: Vec<SidebarItem<Msg>> = ordered
        .iter()
        .enumerate()
        .map(|(i, meta)| {
            let is_active = active == Some(meta.id.as_str());
            let item = SidebarItem::new(tab_label(&meta.cwd), Msg::Noop)
                .id(meta.id.clone())
                .active(is_active);
            if i < 9 {
                item.shortcut((i + 1) as u8)
            } else {
                item
            }
        })
        .collect();

    let sections = vec![SidebarSection::unlabeled(items)];

    let p = theme.extended_palette();
    let divider = DividerColors {
        a: p.background.weaker.color,
        line: p.background.stronger.color,
        b: term_bg,
    };

    SidebarPanel::new(sections)
        .density(SidebarDensity::Large)
        .controller(gestures, Msg::Sidebar)
        .resizable_with(config.sidebar_width as f32, divider)
        .reorderable()
        .build()
}

pub fn apply_drop(tabs: &mut Tabs, drop: sidebar::Drop) {
    use sola_kit::components::Dest;
    let ids = tabs.tab_ids_in_order();
    let dragged = drop.id;
    if !ids.iter().any(|id| id == &dragged) {
        return;
    }
    let mut new_order: Vec<String> = ids.into_iter().filter(|id| id != &dragged).collect();
    let insert = match drop.dest {
        Dest::Loose { before: Some(next) }
        | Dest::Join {
            before: Some(next), ..
        } => new_order
            .iter()
            .position(|id| id == &next)
            .unwrap_or(new_order.len()),
        Dest::Loose { before: None }
        | Dest::Join { before: None, .. }
        | Dest::BeforeGroup { .. }
        | Dest::BlockBefore { .. }
        | Dest::Sections(_) => new_order.len(),
    };
    new_order.insert(insert.min(new_order.len()), dragged);

    for (k, id) in new_order.iter().enumerate() {
        if let Some(tab) = tabs.get_tab_mut(id) {
            tab.ordinal = k as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_label() {
        assert_eq!(
            tab_label(&Some("/home/joshua/Workspace".into())),
            "Workspace"
        );
        assert_eq!(tab_label(&Some("/".into())), "/");
        assert_eq!(tab_label(&None), "shell");
    }
}
