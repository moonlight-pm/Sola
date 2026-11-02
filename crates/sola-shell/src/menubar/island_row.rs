//! The workspace islands' row: side by side in the top row. A press and a
//! release is a click; a press and a move is a drag: the island follows the
//! pointer and the others make room as it passes their middles. A widget so
//! the drag holds when the pointer leaves the row, and so a click knows
//! where its island really is. A switch inside an island takes its own
//! press (it starts no drag).

use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

/// How far a press moves before it is a drag.
const DRAG_FROM: f32 = 4.0;

#[derive(Default)]
struct State {
    press: Option<Press>,
}

struct Press {
    /// The island's place in the row (it follows the drag).
    index: usize,
    start_x: f32,
    /// The pointer's distance from the island's left edge.
    grab: f32,
    pointer: f32,
    dragging: bool,
    /// A move was asked for and the row has not been rebuilt with it: the
    /// layout still has the old order.
    moving: bool,
}

pub struct IslandRow<'a, M> {
    islands: Vec<Element<'a, M>>,
    gap: f32,
    /// A click on an island: its place, and its left edge in the window.
    on_click: fn(usize, f32) -> M,
    /// A dragged island passed another: from one place to another.
    on_move: fn(usize, usize) -> M,
}

impl<'a, M> IslandRow<'a, M> {
    pub fn new(
        islands: Vec<Element<'a, M>>,
        gap: f32,
        on_click: fn(usize, f32) -> M,
        on_move: fn(usize, usize) -> M,
    ) -> Self {
        Self {
            islands,
            gap,
            on_click,
            on_move,
        }
    }
}

/// The place a dragged island takes: past every other whose middle is left
/// of its own. `spans` are the islands' (left, width) in row order.
fn slot_at(spans: &[(f32, f32)], index: usize, middle: f32) -> usize {
    spans
        .iter()
        .enumerate()
        .filter(|(i, (left, width))| *i != index && left + width / 2.0 < middle)
        .count()
}

impl<'a, M: 'a> Widget<M, iced::Theme, iced::Renderer> for IslandRow<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.islands.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.islands);
        if let Some(press) = &mut tree.state.downcast_mut::<State>().press {
            press.moving = false;
        }
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if tree.children.len() != self.islands.len() {
            tree.diff_children(&self.islands);
        }
        let limits = limits.loose();
        let mut x = 0.0_f32;
        let mut height = 0.0_f32;
        let mut nodes = Vec::with_capacity(self.islands.len());
        for (island, state) in self.islands.iter_mut().zip(&mut tree.children) {
            let node = island
                .as_widget_mut()
                .layout(state, renderer, &limits)
                .move_to(Point::new(x, 0.0));
            x += node.size().width + self.gap;
            height = height.max(node.size().height);
            nodes.push(node);
        }
        let width = (x - self.gap).max(0.0);
        layout::Node::with_children(Size::new(width, height), nodes)
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
        for ((island, state), child) in self
            .islands
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            island
                .as_widget_mut()
                .update(state, event, child, cursor, renderer, clipboard, shell, viewport);
        }
        if shell.is_event_captured() {
            return;
        }
        let st = tree.state.downcast_mut::<State>();
        let spans = || -> Vec<(f32, f32)> {
            layout
                .children()
                .map(|c| (c.bounds().x, c.bounds().width))
                .collect()
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(at) = cursor.land().position() else {
                    return;
                };
                let hit = layout.children().position(|c| c.bounds().contains(at));
                if let Some(index) = hit {
                    st.press = Some(Press {
                        index,
                        start_x: at.x,
                        grab: at.x - spans()[index].0,
                        pointer: at.x,
                        dragging: false,
                        moving: false,
                    });
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(press) = &mut st.press else {
                    return;
                };
                let Some(at) = cursor.land().position() else {
                    return;
                };
                press.pointer = at.x;
                if !press.dragging && (at.x - press.start_x).abs() < DRAG_FROM {
                    return;
                }
                press.dragging = true;
                shell.request_redraw();
                if press.moving {
                    return;
                }
                let spans = spans();
                let Some((_, width)) = spans.get(press.index) else {
                    return;
                };
                let middle = at.x - press.grab + width / 2.0;
                let slot = slot_at(&spans, press.index, middle);
                if slot != press.index {
                    shell.publish((self.on_move)(press.index, slot));
                    press.index = slot;
                    press.moving = true;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let Some(press) = st.press.take() else {
                    return;
                };
                if press.dragging {
                    shell.request_redraw();
                } else if let Some((left, _)) = spans().get(press.index) {
                    shell.publish((self.on_click)(press.index, *left));
                }
                shell.capture_event();
            }
            _ => {}
        }
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
        let press = tree.state.downcast_ref::<State>().press.as_ref();
        let held = press.filter(|p| p.dragging);
        for (i, ((island, state), child)) in self
            .islands
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
        {
            if held.is_none_or(|p| p.index != i) {
                island
                    .as_widget()
                    .draw(state, renderer, theme, style, child, cursor, viewport);
            }
        }
        // The held island last and over the rest, under the pointer, kept
        // to the row.
        let Some(p) = held else {
            return;
        };
        let (Some(island), Some(state), Some(child)) = (
            self.islands.get(p.index),
            tree.children.get(p.index),
            layout.children().nth(p.index),
        ) else {
            return;
        };
        let row = layout.bounds();
        let at = child.bounds();
        let left = (p.pointer - p.grab).clamp(row.x, (row.x + row.width - at.width).max(row.x));
        renderer.with_layer(*viewport, |renderer| {
            renderer.with_translation(Vector::new(left - at.x, 0.0), |renderer| {
                island
                    .as_widget()
                    .draw(state, renderer, theme, style, child, cursor, viewport);
            });
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let press = tree.state.downcast_ref::<State>().press.as_ref();
        if press.is_some_and(|p| p.dragging) {
            return mouse::Interaction::Grabbing;
        }
        let inner = self
            .islands
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((island, state), child)| {
                island
                    .as_widget()
                    .mouse_interaction(state, child, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default();
        if inner == mouse::Interaction::default() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            inner
        }
    }
}

impl<'a, M: 'a> From<IslandRow<'a, M>> for Element<'a, M> {
    fn from(row: IslandRow<'a, M>) -> Self {
        Element::new(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three islands: 0–100, 106–146, 152–252.
    const SPANS: [(f32, f32); 3] = [(0.0, 100.0), (106.0, 40.0), (152.0, 100.0)];

    #[test]
    fn a_held_island_takes_the_place_past_the_middles_it_passed() {
        // Island 0 held: its middle before the next one's stays first.
        assert_eq!(slot_at(&SPANS, 0, 125.0), 0);
        // Past island 1's middle (126), not island 2's (202).
        assert_eq!(slot_at(&SPANS, 0, 127.0), 1);
        assert_eq!(slot_at(&SPANS, 0, 203.0), 2);
        // Island 2 held, back to the start.
        assert_eq!(slot_at(&SPANS, 2, 40.0), 0);
        assert_eq!(slot_at(&SPANS, 2, 60.0), 1);
    }

    #[test]
    fn a_held_island_does_not_flip_back_after_the_others_made_room() {
        // A wide island passes a narrow one's middle, and the row re-lays:
        // the narrow one is now well left of the held island's middle.
        let after = [(0.0, 40.0), (46.0, 100.0), (152.0, 100.0)];
        assert_eq!(slot_at(&after, 1, 127.0), 1);
    }
}
