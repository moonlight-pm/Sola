//! Popover — visual chrome for a floating panel.
//!
//! v0 ships *only* the chrome (raised bg, hairline border, drop
//! shadow, padding). Show/hide and anchoring are the caller's
//! responsibility — iced 0.14's `widget::Stack` + `widget::float` (or
//! a plain `Stack` with conditional rendering) handles that better
//! than the kit could prescribe.
//!
//! Once we have a kit consumer that wants the full
//! trigger+anchor+dismiss pattern boxed up, we'll grow this into a
//! stateful widget. Today the consumer composes the chrome with its
//! own positioning logic.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::widget::{Container, container};
use iced::{
    Background, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme, Vector,
};

use crate::components::style::{hairline, RADIUS_MD, SPACE_SM};

/// Wrap `content` in a popover-styled container. Default padding is
/// 4px (menu-bar dropdown density); override with `.padding(...)` if needed.
pub fn popover<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Container<'a, Message, Theme> {
    container(content).style(style).padding(SPACE_SM)
}

/// Floating-panel chrome tuned for macOS menu-bar dropdown calm:
/// raised bg, hairline at `RADIUS_MD`, tight soft shadow (not a marketing
/// card). Escape hatch: iced's palette has no shadow token, so the drop
/// shadow is a fixed translucent black (see convention note in `mod.rs`).
pub fn style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_MD),
        shadow: Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.28),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 10.0,
        },
        ..container::Style::default()
    }
}

// ---------------------------------------------------------------------
// Anchored popover — trigger widget + floating overlay + outside-click
// dismiss. Unlike the bare `popover` chrome above, this *positions*
// itself: it renders `base` (a swatch, a button, …) in place and, while
// constructed, floats `content` beside it as a true iced overlay so the
// panel sits next to the thing that opened it and tracks it through
// scrolling. A left-click anywhere outside the panel publishes
// `on_dismiss`.
//
// Build it only while the popover should be open; render `base` alone
// when closed (the caller owns the open/closed bit — usually
// "is this the atom being edited?"). The drag/focus state of whatever
// lives in `content` persists across frames because the widget keeps
// the same tree node while it stays constructed.
// ---------------------------------------------------------------------

/// Gap in logical px between the anchor and the floated panel (End).
const ANCHOR_GAP_END: f32 = 12.0;
/// Tighter gap for a hanging select menu (Below).
const ANCHOR_GAP_BELOW: f32 = 6.0;

/// Where the floated panel sits relative to `base`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    /// Prefer the right of the trigger (swatches, overflow menus).
    #[default]
    End,
    /// Hang under the trigger, start-aligned (select / combo).
    Below,
}

/// Choose the panel's top-left so it sits next to `anchor`.
fn anchor_offset(
    anchor: Rectangle,
    panel: Size,
    viewport: Size,
    placement: Placement,
) -> Point {
    match placement {
        Placement::End => {
            let gap = ANCHOR_GAP_END;
            let right_x = anchor.x + anchor.width + gap;
            let left_x = anchor.x - panel.width - gap;
            let x = if right_x + panel.width <= viewport.width {
                right_x
            } else if left_x >= 0.0 {
                left_x
            } else {
                (viewport.width - panel.width).max(0.0)
            };

            let mut y = anchor.y;
            if y + panel.height > viewport.height {
                y = viewport.height - panel.height;
            }
            if y < 0.0 {
                y = 0.0;
            }
            Point::new(x, y)
        }
        Placement::Below => {
            let gap = ANCHOR_GAP_BELOW;
            // Center a narrower panel under the trigger so the list
            // behind peeks on both sides (overlay, not another row).
            let mut x = anchor.x + (anchor.width - panel.width) / 2.0;
            if x + panel.width > viewport.width {
                x = (viewport.width - panel.width).max(0.0);
            }
            if x < 0.0 {
                x = 0.0;
            }
            let below = anchor.y + anchor.height + gap;
            let above = anchor.y - panel.height - gap;
            let y = if below + panel.height <= viewport.height {
                below
            } else if above >= 0.0 {
                above
            } else {
                (viewport.height - panel.height).max(0.0)
            };
            Point::new(x, y)
        }
    }
}

/// An anchored popover. See the module-level note above the constructor.
pub struct Anchored<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    base: Element<'a, Message, Theme, Renderer>,
    content: Element<'a, Message, Theme, Renderer>,
    on_dismiss: Message,
    placement: Placement,
}

/// Render `base` in place and float `content` beside it; a left-click
/// outside `content` publishes `on_dismiss`. Construct only while open.
pub fn popover_anchored<'a, Message, Theme, Renderer>(
    base: impl Into<Element<'a, Message, Theme, Renderer>>,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    on_dismiss: Message,
) -> Anchored<'a, Message, Theme, Renderer> {
    Anchored {
        base: base.into(),
        content: content.into(),
        on_dismiss,
        placement: Placement::End,
    }
}

impl<'a, Message, Theme, Renderer> Anchored<'a, Message, Theme, Renderer> {
    /// Pin the panel relative to the trigger. Default is [`Placement::End`].
    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Anchored<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.base), Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.base.as_widget(), self.content.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.base.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
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
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.base.as_widget().draw(
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
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.base
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let b = layout.bounds();
        let anchor = Rectangle {
            x: b.x + translation.x,
            y: b.y + translation.y,
            ..b
        };
        Some(overlay::Element::new(Box::new(AnchoredOverlay {
            content: &mut self.content,
            tree: &mut tree.children[1],
            anchor,
            on_dismiss: self.on_dismiss.clone(),
            placement: self.placement,
        })))
    }
}

impl<'a, Message, Theme, Renderer> From<Anchored<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(w: Anchored<'a, Message, Theme, Renderer>) -> Self {
        Element::new(w)
    }
}

struct AnchoredOverlay<'a, 'b, Message, Theme, Renderer> {
    content: &'b mut Element<'a, Message, Theme, Renderer>,
    tree: &'b mut Tree,
    anchor: Rectangle,
    on_dismiss: Message,
    placement: Placement,
}

impl<Message, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer>
    for AnchoredOverlay<'_, '_, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.content.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let size = node.size();
        let offset = anchor_offset(self.anchor, size, bounds, self.placement);
        layout::Node::with_children(size, vec![node])
            .translate(Vector::new(offset.x, offset.y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
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
        renderer: &Renderer,
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
        // A left-press that lands outside the panel dismisses it. We do
        // NOT capture it, so the same click can also retarget another
        // swatch underneath — its trigger publishes its own EditAtom and
        // the messages apply in order (close, then open the new one).
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if cursor.position_over(bounds).is_none() {
                shell.publish(self.on_dismiss.clone());
            }
        }
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
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
        renderer: &Renderer,
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
    use iced::{Rectangle, Size};

    fn anchor(x: f32, y: f32) -> Rectangle {
        Rectangle { x, y, width: 56.0, height: 56.0 }
    }

    #[test]
    fn placed_to_the_right_when_there_is_room() {
        let offset = anchor_offset(
            anchor(40.0, 100.0),
            Size::new(480.0, 260.0),
            Size::new(1600.0, 900.0),
            Placement::End,
        );
        // right of the swatch: anchor.x + width + gap
        assert_eq!(offset.x, 40.0 + 56.0 + 12.0);
        // top aligned with the swatch
        assert_eq!(offset.y, 100.0);
    }

    #[test]
    fn flips_to_the_left_when_the_right_would_overflow() {
        // Swatch hard against the right edge; popover can't fit to the right.
        let offset = anchor_offset(
            anchor(1500.0, 100.0),
            Size::new(480.0, 260.0),
            Size::new(1600.0, 900.0),
            Placement::End,
        );
        assert_eq!(offset.x, 1500.0 - 480.0 - 12.0);
    }

    #[test]
    fn clamps_bottom_into_the_viewport() {
        // Swatch low on screen: a 260-tall popover would overflow the 900 viewport.
        let offset = anchor_offset(
            anchor(40.0, 800.0),
            Size::new(480.0, 260.0),
            Size::new(1600.0, 900.0),
            Placement::End,
        );
        assert_eq!(offset.y, 900.0 - 260.0);
    }

    #[test]
    fn never_positions_above_the_viewport() {
        // Popover taller than the viewport — clamp to the top edge, not negative.
        let offset = anchor_offset(
            anchor(40.0, 10.0),
            Size::new(480.0, 1000.0),
            Size::new(1600.0, 900.0),
            Placement::End,
        );
        assert_eq!(offset.y, 0.0);
    }

    #[test]
    fn below_hangs_under_the_trigger() {
        let offset = anchor_offset(
            anchor(40.0, 100.0),
            Size::new(200.0, 120.0),
            Size::new(1600.0, 900.0),
            Placement::Below,
        );
        // 200-wide panel under a 56-wide swatch: centered, then clamped.
        assert_eq!(offset.x, 0.0);
        assert_eq!(offset.y, 100.0 + 56.0 + 6.0);
    }

    #[test]
    fn below_centers_a_narrower_panel() {
        let trigger = Rectangle {
            x: 40.0,
            y: 100.0,
            width: 200.0,
            height: 28.0,
        };
        let offset = anchor_offset(
            trigger,
            Size::new(184.0, 120.0),
            Size::new(1600.0, 900.0),
            Placement::Below,
        );
        assert_eq!(offset.x, 40.0 + 8.0);
        assert_eq!(offset.y, 100.0 + 28.0 + 6.0);
    }

    #[test]
    fn below_flips_above_when_the_bottom_would_overflow() {
        let offset = anchor_offset(
            anchor(40.0, 800.0),
            Size::new(200.0, 160.0),
            Size::new(1600.0, 900.0),
            Placement::Below,
        );
        assert_eq!(offset.y, 800.0 - 160.0 - 6.0);
    }
}
