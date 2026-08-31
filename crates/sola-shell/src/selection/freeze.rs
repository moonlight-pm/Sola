//! Pixel-identical freeze layer for the selection overlay.
//!
//! Forces a synchronous GPU upload of the RGBA still *before* the overlay
//! joins composition, so the first visible frame is the captured desktop
//! rather than an empty/transparent flash.

use iced::advanced::image::Renderer as ImageRenderer;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::widget::image::{self, FilterMethod, Handle};
use iced::window;
use iced::{ContentFit, Element, Event, Length, Rectangle, Rotation, Size, mouse};

use crate::app::Msg;

pub struct FreezeLayer {
    handle: Handle,
}

impl FreezeLayer {
    pub fn new(handle: Handle) -> Self {
        Self { handle }
    }
}

#[derive(Default)]
struct State {
    notified: bool,
    handle: Option<Handle>,
}

impl Widget<Msg, iced::Theme, iced::Renderer> for FreezeLayer {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let _ = ImageRenderer::load_image(renderer, &self.handle);
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if state.handle.as_ref() != Some(&self.handle) {
            state.handle = Some(self.handle.clone());
            state.notified = false;
        }
        if !matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            return;
        }
        let _ = ImageRenderer::load_image(renderer, &self.handle);
        if state.notified {
            return;
        }
        let size = layout.bounds().size();
        if !crate::zoning::overlay_geometry_is_live(size.width as i32, size.height as i32) {
            return;
        }
        state.notified = true;
        shell.publish(Msg::SelectionTextureReady);
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let _ = ImageRenderer::load_image(renderer, &self.handle);
        image::draw(
            renderer,
            layout,
            &self.handle,
            None,
            Default::default(),
            ContentFit::None,
            FilterMethod::Nearest,
            Rotation::default(),
            1.0,
            1.0,
        );
    }
}

impl<'a> From<FreezeLayer> for Element<'a, Msg> {
    fn from(layer: FreezeLayer) -> Self {
        Self::new(layer)
    }
}
