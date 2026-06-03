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
    _state: &SidebarState,
    tabs: &'a Tabs,
    active: Option<&str>,
    config: &TerminalConfig,
) -> Element<'a, Msg> {
    let width = if config.sidebar_collapsed {
        36.0_f32
    } else {
        config.sidebar_width as f32
    };

    let ordered = tabs.ordered_meta();
    let mut col_items: Vec<Element<'a, Msg>> = ordered
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
