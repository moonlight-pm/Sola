//! Wrap the OSR shader so iced enables the compositor IME on the page.
//!
//! The shader `Program` cannot call `Shell::request_input_method`. This
//! widget forwards everything to the shader and, while the page owns
//! keys, asks winit to allow IME at the last CEF caret (or click).

use std::sync::Arc;

use iced::advanced::input_method::{InputMethod, Purpose};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::tree::Tree;
use iced::advanced::widget::Widget;
use iced::advanced::{Clipboard, Shell, mouse, renderer};
use iced::{Element, Event, Length, Rectangle, Size};

use crate::engine::{Engine, FrameSlot};

pub fn page_ime<'a, E: Engine>(
    content: impl Into<Element<'a, crate::app::Msg>>,
    slot: Arc<FrameSlot<E>>,
    page_owns_keys: bool,
) -> Element<'a, crate::app::Msg> {
    Element::new(PageIme {
        content: content.into(),
        slot,
        page_owns_keys,
    })
}

struct PageIme<'a, E: Engine> {
    content: Element<'a, crate::app::Msg>,
    slot: Arc<FrameSlot<E>>,
    page_owns_keys: bool,
}

impl<E: Engine> Widget<crate::app::Msg, iced::Theme, iced::Renderer> for PageIme<'_, E> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
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
        shell: &mut Shell<'_, crate::app::Msg>,
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
        if self.page_owns_keys {
            let (req_w, _) = *self.slot.last_size.lock().unwrap();
            let scale = crate::input::scale_from_last_size(layout.bounds(), req_w, 1.0);
            let caret = *self.slot.ime.lock().unwrap();
            shell.request_input_method(&InputMethod::<String>::Enabled {
                cursor: caret.logical_rect(layout.bounds(), scale),
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
        theme: &iced::Theme,
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
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a, E: Engine> From<PageIme<'a, E>> for Element<'a, crate::app::Msg> {
    fn from(value: PageIme<'a, E>) -> Self {
        Element::new(value)
    }
}
