//! IME for pages: wraps the plane overlay so iced turns the compositor's
//! input method on while a board has the keyboard. A canvas `Program`
//! cannot ask for it itself; this widget forwards everything to the overlay
//! and asks, at the page's caret. The overlay turns the IME's preedit and
//! commit into the page's composition (`InputEvent::Ime*`).

use iced::advanced::input_method::{InputMethod, Purpose};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::tree::Tree;
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::{Clipboard, Shell, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

/// `content` with the input method on at `caret` (relative to the widget),
/// or off when `None`.
pub fn target<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    caret: Option<Rectangle>,
) -> Element<'a, M> {
    Element::new(Target {
        content: content.into(),
        caret,
    })
}

struct Target<'a, M> {
    content: Element<'a, M>,
    caret: Option<Rectangle>,
}

impl<M> Widget<M, Theme, iced::Renderer> for Target<'_, M> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if let Some(caret) = self.caret {
            let at = layout.bounds().position();
            shell.request_input_method(&InputMethod::<String>::Enabled {
                cursor: caret + Vector::new(at.x, at.y),
                purpose: Purpose::Normal,
                preedit: None,
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
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
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}
