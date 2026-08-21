//! Pointer-anchored context menu.
//!
//! Flat actions, separators, and disabled rows. The caller owns whether
//! a menu is open (`Option<MenuState>`); construct [`menu_at`] only while
//! it should show. Outside click and Escape dismiss.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Shell, Widget};
use iced::widget::{Space, button, column, container, text};
use iced::{
    Background, Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme, Vector,
    keyboard, mouse,
};

use crate::components::popover;
use crate::components::style::mix_white;
use crate::fonts;

/// One row in a context menu.
#[derive(Clone, Debug)]
pub enum MenuItem<Message> {
    Action { label: String, message: Message },
    Disabled { label: String },
    Separator,
}

impl<Message> MenuItem<Message> {
    pub fn action(label: impl Into<String>, message: Message) -> Self {
        Self::Action {
            label: label.into(),
            message,
        }
    }

    pub fn disabled(label: impl Into<String>) -> Self {
        Self::Disabled {
            label: label.into(),
        }
    }

    pub fn separator() -> Self {
        Self::Separator
    }
}

/// Wide enough that labels read; `Fill` rows inside a `Shrink` column
/// collapse to 0 width (a thin empty strip).
pub const MENU_MIN_W: f32 = 200.0;

/// Menu chrome only — no positioning. Prefer [`menu_at`] for a real menu.
pub fn menu<'a, Message: Clone + 'a>(
    items: impl IntoIterator<Item = MenuItem<Message>>,
) -> iced::widget::Container<'a, Message, Theme> {
    let rows = column(items.into_iter().map(menu_row))
        .spacing(1)
        .width(Length::Fixed(MENU_MIN_W));
    popover::popover(rows)
        .padding(Padding::from([4, 4]))
        .width(Length::Fixed(MENU_MIN_W + 8.0))
}

/// Float the menu at `position` (window-logical). Build only while open.
pub fn menu_at<'a, Message: Clone + 'a>(
    position: Point,
    items: impl IntoIterator<Item = MenuItem<Message>>,
    on_dismiss: Message,
) -> Element<'a, Message, Theme> {
    Element::new(At {
        content: menu(items).into(),
        position,
        on_dismiss,
    })
}

fn menu_row<'a, Message: Clone + 'a>(item: MenuItem<Message>) -> Element<'a, Message, Theme> {
    match item {
        MenuItem::Separator => container(Space::new().height(1.0).width(Length::Fill))
            .width(Length::Fill)
            .padding(Padding::from([3, 6]))
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: Some(Background::Color(Color {
                        a: 0.18,
                        ..p.background.strong.color
                    })),
                    ..container::Style::default()
                }
            })
            .into(),
        MenuItem::Disabled { label } => container(text(label).font(fonts::ui()).size(13).style(
            |theme: &Theme| {
                let c = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(Color { a: 0.38, ..c }),
                }
            },
        ))
        .padding(Padding::from([5, 10]))
        .width(Length::Fill)
        .into(),
        MenuItem::Action { label, message } => {
            button(text(label).font(fonts::ui()).size(13).width(Length::Fill))
                .style(item_style)
                .padding(Padding::from([5, 10]))
                .width(Length::Fill)
                .on_press(message)
                .into()
        }
    }
}

fn item_style(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let idle = button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: p.background.base.text,
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: crate::components::style::RADIUS_SM.into(),
        },
        shadow: iced::Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(mix_white(
                p.background.strong.color,
                0.08,
            ))),
            ..idle
        },
        _ => idle,
    }
}

struct At<'a, Message> {
    content: Element<'a, Message, Theme>,
    position: Point,
    on_dismiss: Message,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for At<'_, Message>
where
    Message: Clone,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::ZERO)
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let anchor = Point::new(
            self.position.x + translation.x,
            self.position.y + translation.y,
        );
        Some(overlay::Element::new(Box::new(AtOverlay {
            content: &mut self.content,
            tree: &mut tree.children[0],
            anchor,
            on_dismiss: self.on_dismiss.clone(),
        })))
    }
}

impl<'a, Message: Clone + 'a> From<At<'a, Message>> for Element<'a, Message, Theme> {
    fn from(w: At<'a, Message>) -> Self {
        Element::new(w)
    }
}

struct AtOverlay<'a, 'b, Message> {
    content: &'b mut Element<'a, Message, Theme>,
    tree: &'b mut Tree,
    anchor: Point,
    on_dismiss: Message,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for AtOverlay<'_, '_, Message>
where
    Message: Clone,
{
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let node = self.content.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let size = node.size();
        let mut x = self.anchor.x;
        let mut y = self.anchor.y;
        if x + size.width > bounds.width {
            x = (self.anchor.x - size.width).max(0.0);
        }
        if y + size.height > bounds.height {
            y = (self.anchor.y - size.height).max(0.0);
        }
        layout::Node::with_children(size, vec![node]).translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let content_layout = layout.children().next().unwrap();
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            content_layout,
            cursor,
            &layout.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();
        let content_layout = layout.children().next().unwrap();
        self.content.as_widget_mut().update(
            self.tree,
            event,
            content_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &bounds,
        );
        if shell.is_event_captured() {
            return;
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if cursor.position_over(bounds).is_none() {
                    shell.publish(self.on_dismiss.clone());
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                shell.publish(self.on_dismiss.clone());
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let content_layout = layout.children().next().unwrap();
        self.content
            .as_widget_mut()
            .operate(self.tree, content_layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let content_layout = layout.children().next().unwrap();
        self.content.as_widget().mouse_interaction(
            self.tree,
            content_layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_constructors() {
        let a = MenuItem::action("New group", 1u8);
        let d = MenuItem::<u8>::disabled("Add to…");
        assert!(matches!(a, MenuItem::Action { .. }));
        assert!(matches!(d, MenuItem::Disabled { .. }));
        let _ = MenuItem::<u8>::separator();
    }
}
