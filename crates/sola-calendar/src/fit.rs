//! Month-cell event stack: lay out as many chips as the cell can hold.
//! Only then emit “+N more”.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::text::{self, Text};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::{
    Element, Event, Length, Pixels, Point, Rectangle, Size, Theme, Vector, alignment,
};
use sola_kit::fonts;

const SPACING: f32 = 1.0;
const MORE_H: f32 = 14.0;

/// How many leading items fit in `budget`, leaving a “+N more” line when
/// the rest would overflow.
pub fn visible_count(heights: &[f32], spacing: f32, more_h: f32, budget: f32) -> (usize, usize) {
    let n = heights.len();
    if n == 0 {
        return (0, 0);
    }
    if pack(heights, spacing, None) <= budget + 0.5 {
        return (n, 0);
    }
    let mut shown = 0;
    while shown < n {
        let next = shown + 1;
        let more = (n > next).then_some(more_h);
        if pack(&heights[..next], spacing, more) > budget + 0.5 {
            break;
        }
        shown = next;
    }
    if shown == 0 {
        shown = 1;
    }
    (shown, n - shown)
}

fn pack(heights: &[f32], spacing: f32, more: Option<f32>) -> f32 {
    let mut h = 0.0;
    for (i, &ch) in heights.iter().enumerate() {
        if i > 0 {
            h += spacing;
        }
        h += ch;
    }
    if let Some(mh) = more {
        if !heights.is_empty() {
            h += spacing;
        }
        h += mh;
    }
    h
}

pub fn stack<'a, Message: 'a>(children: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(FitStack { children })
}

struct FitStack<'a, Message> {
    children: Vec<Element<'a, Message>>,
}

#[derive(Default)]
struct State {
    extra: usize,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for FitStack<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
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
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let child_limits = layout::Limits::new(Size::ZERO, Size::new(size.width, f32::INFINITY));
        let n = self.children.len();
        let mut measured: Vec<layout::Node> = Vec::with_capacity(n);
        let mut heights = Vec::with_capacity(n);
        for (i, child) in self.children.iter_mut().enumerate() {
            let node = child
                .as_widget_mut()
                .layout(&mut tree.children[i], renderer, &child_limits);
            heights.push(node.size().height);
            measured.push(node);
        }
        let (shown, extra) = visible_count(&heights, SPACING, MORE_H, size.height);
        tree.state.downcast_mut::<State>().extra = extra;
        let mut y = 0.0;
        let mut nodes = Vec::with_capacity(shown);
        for (i, node) in measured.into_iter().take(shown).enumerate() {
            let h = node.size().height;
            nodes.push(node.move_to(Point::new(0.0, y)));
            y += h;
            if i + 1 < shown || extra > 0 {
                y += SPACING;
            }
        }
        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((child, state), layout)| {
                    child
                        .as_widget_mut()
                        .operate(state, layout, renderer, operation);
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((child, state), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                state, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
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
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
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
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        for ((child, state), child_layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(state, renderer, theme, style, child_layout, cursor, &visible);
        }
        let extra = tree.state.downcast_ref::<State>().extra;
        if extra == 0 {
            return;
        }
        let y = layout
            .children()
            .last()
            .map(|c| c.bounds().y + c.bounds().height + SPACING)
            .unwrap_or(bounds.y);
        let color = theme.extended_palette().secondary.base.text;
        use iced::advanced::text::Renderer as _;
        renderer.fill_text(
            Text {
                content: format!("+{extra} more"),
                bounds: Size::new(bounds.width, MORE_H),
                size: Pixels(11.0),
                line_height: text::LineHeight::default(),
                font: fonts::ui(),
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            },
            Point::new(bounds.x, y),
            color,
            visible,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::visible_count;

    #[test]
    fn all_fit_no_more() {
        assert_eq!(visible_count(&[18.0, 18.0, 18.0], 1.0, 14.0, 200.0), (3, 0));
    }

    #[test]
    fn overflow_keeps_more_line() {
        // 5 × 18 + spacing; budget of ~70 holds three chips + more.
        let heights = [18.0, 18.0, 18.0, 18.0, 18.0];
        let (shown, extra) = visible_count(&heights, 1.0, 14.0, 70.0);
        assert!(shown >= 2 && shown < 5, "shown={shown}");
        assert_eq!(extra, 5 - shown);
    }

    #[test]
    fn empty() {
        assert_eq!(visible_count(&[], 1.0, 14.0, 80.0), (0, 0));
    }
}
