//! Small view helpers: icons, hairlines, the list cursor wrapper.

use std::collections::HashMap;
use std::sync::OnceLock;

use iced::widget::{Space, container, svg};
use iced::{Background, Border, Element, Length, Theme};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::toolbar::toolbar_icon_tip;

use super::Msg;
use super::style;

const ICONS: &[&str] = &[
    "lucide/square-pen",
    "lucide/paperclip",
    "lucide/file",
    "lucide/file-pen",
    "lucide/reply",
    "lucide/reply-all",
    "lucide/forward",
    "lucide/archive",
    "lucide/inbox",
    "lucide/trash-2",
    "lucide/ban",
    "lucide/mail",
    "lucide/mail-open",
    "lucide/flag",
    "lucide/copy",
    "lucide/undo-2",
    "lucide/x",
];

pub fn icon(name: &'static str) -> svg::Handle {
    static HANDLES: OnceLock<HashMap<&'static str, svg::Handle>> = OnceLock::new();
    HANDLES
        .get_or_init(|| ICONS.iter().map(|n| (*n, icon_handle(n))).collect())
        .get(name)
        .cloned()
        .unwrap_or_else(|| icon_handle(name))
}

/// Toolbar icon with tooltip; muted (no press) when `on_press` is `None`.
pub fn tool(name: &'static str, tip: &'static str, on_press: Option<Msg>) -> Element<'static, Msg> {
    toolbar_icon_tip(icon(name), tip, on_press)
}

/// Secondary-ink icon (list row marks).
pub fn muted_icon(name: &'static str, size: f32) -> Element<'static, Msg> {
    svg(icon(name))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(|theme: &Theme, _status| svg::Style {
            color: Some(theme.extended_palette().secondary.base.text),
        })
        .into()
}

/// Accent-ink icon (flagged rows).
pub fn accent_icon(name: &'static str, size: f32) -> Element<'static, Msg> {
    svg(icon(name))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(|theme: &Theme, _status| svg::Style {
            color: Some(theme.extended_palette().warning.base.color),
        })
        .into()
}

pub fn v_hairline() -> Element<'static, Msg> {
    container(Space::new().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(style::hairline)
        .into()
}

pub fn h_hairline() -> Element<'static, Msg> {
    container(Space::new().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(1)
        .style(style::hairline)
        .into()
}

pub fn status_dot(failed: bool) -> Element<'static, Msg> {
    let inner = container(Space::new().width(6).height(6)).style(move |theme: &Theme| {
        let p = theme.extended_palette();
        let color = if failed {
            p.danger.base.color
        } else {
            p.background.strongest.color
        };
        container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color,
                width: 0.0,
                radius: 999.0.into(),
            },
            ..container::Style::default()
        }
    });
    container(inner)
        .width(8)
        .height(8)
        .center_x(8)
        .center_y(8)
        .into()
}

/// Default arrow over the message list: no I-bar, no hand, no text copy.
///
/// `list_item` buttons would otherwise advertise `Pointer`, and iced's
/// row/column take the max child interaction (`Text` from the letter
/// outranks everything). Presses are captured so a drag here cannot
/// start a letter selection.
pub fn default_cursor<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::overlay;
    use iced::advanced::renderer;
    use iced::advanced::widget::tree::{self, Tree};
    use iced::advanced::widget::{Operation, Widget};
    use iced::advanced::{Clipboard, Shell};
    use iced::mouse;
    use iced::{Event, Rectangle, Size, Vector};

    struct DefaultCursor<'a> {
        content: Element<'a, Msg>,
    }

    impl Widget<Msg, Theme, iced::Renderer> for DefaultCursor<'_> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }

        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }

        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }

        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }

        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn size_hint(&self) -> Size<Length> {
            self.content.as_widget().size_hint()
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &iced::Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content.as_widget_mut().layout(tree, renderer, limits)
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut iced::Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }

        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &iced::Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Msg>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
                && cursor.is_over(layout.bounds())
            {
                shell.capture_event();
            }
        }

        fn mouse_interaction(
            &self,
            _tree: &Tree,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
            _renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            mouse::Interaction::None
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &iced::Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Msg, Theme, iced::Renderer>> {
            self.content
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)
        }
    }

    Element::new(DefaultCursor {
        content: content.into(),
    })
}
