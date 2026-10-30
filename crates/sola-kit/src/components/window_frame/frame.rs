//! The frame widget. It keeps its own state (focus, the open menu, the
//! hovered row), so an app only wraps its view; menu choices leave through
//! [`crate::menu::choose`], not the app's message type.

use std::sync::atomic::{AtomicBool, Ordering};

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, renderer};
use iced::window::Direction;
use iced::{Element, Event, Length, Point, Rectangle, Size, Theme, Vector, window};
use sola_bus::topics::{AppMenuPayload, MenuDefinition};

use super::bar::{self, Bar, Hit};
use super::dropdown::Dropdown;
use super::{Chrome, HEIGHT, grips, paint};

/// The last focus change any frame in this process saw. A frame whose
/// widget tree was just rebuilt (its window's view changed shape, e.g. a
/// stack added around it) has seen none yet and starts from this. Exact
/// for a single-window app; a window of a multi-window app gets its own
/// event as soon as focus moves.
static PROCESS_FOCUSED: AtomicBool = AtomicBool::new(false);

/// What the frame remembers between frames.
#[derive(Debug, Default)]
pub(super) struct State {
    /// `None` until this frame's window reports focus.
    pub focused: Option<bool>,
    /// The menu whose dropdown is showing.
    pub open: Option<usize>,
    /// The highlighted row of the open dropdown.
    pub hot: Option<usize>,
    pub close_hover: bool,
    pub bar: Bar,
}

impl State {
    fn is_focused(&self) -> bool {
        self.focused
            .unwrap_or_else(|| PROCESS_FOCUSED.load(Ordering::Relaxed))
    }

    pub fn close_menu(&mut self) {
        self.open = None;
        self.hot = None;
    }
}

/// A window's frame around its content. Usually built by
/// [`super::window_frame`]; the builder is for previews that pin the menus
/// or the focus.
pub struct WindowFrame<'a, Message> {
    content: Element<'a, Message, Theme>,
    chrome: Chrome,
    title: String,
    menus: Option<AppMenuPayload>,
    on_drag: Option<Message>,
    on_close: Option<Message>,
    on_resize: Option<Box<dyn Fn(Direction) -> Message + 'a>>,
    focused: Option<bool>,
}

impl<'a, Message> WindowFrame<'a, Message> {
    /// Frame `content`, showing this app's published menus.
    pub fn new(chrome: impl Into<Chrome>, content: impl Into<Element<'a, Message, Theme>>) -> Self {
        Self {
            content: content.into(),
            chrome: chrome.into(),
            title: String::new(),
            menus: crate::menu::current(),
            on_drag: None,
            on_close: None,
            on_resize: None,
            focused: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Show these menus instead of the app's published ones.
    pub fn menus(mut self, menus: AppMenuPayload) -> Self {
        self.menus = Some(crate::menu::ensure_window_menu(menus));
        self
    }

    pub fn on_drag(mut self, message: Message) -> Self {
        self.on_drag = Some(message);
        self
    }

    pub fn on_close(mut self, message: Message) -> Self {
        self.on_close = Some(message);
        self
    }

    pub fn on_resize(mut self, f: impl Fn(Direction) -> Message + 'a) -> Self {
        self.on_resize = Some(Box::new(f));
        self
    }

    /// Draw as focused (or not) whatever the window's focus is.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    fn definitions(&self) -> &[MenuDefinition] {
        self.menus.as_ref().map_or(&[], |m| m.menus.as_slice())
    }

    fn labels(&self) -> Vec<String> {
        self.definitions().iter().map(|m| m.label.clone()).collect()
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for WindowFrame<'_, Message>
where
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let top = if self.chrome == Chrome::Bare {
            0.0
        } else {
            HEIGHT
        };
        let inner = Size::new(size.width, (size.height - top).max(0.0));
        let child = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, inner),
        );
        if self.chrome != Chrome::Bare {
            let labels = self.labels();
            let state = tree.state.downcast_mut::<State>();
            state.bar.lay_out(size.width, &labels, &self.title);
            if state.open.is_some_and(|i| i >= state.bar.menus.len()) {
                state.close_menu();
            }
        }
        layout::Node::with_children(size, vec![child.move_to(Point::new(0.0, top))])
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
        let child = layout.children().next().expect("frame content");
        if self.chrome == Chrome::Bare {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
            );
            return;
        }
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<State>();
        let colors = paint::colors(theme, self.focused.unwrap_or_else(|| state.is_focused()));
        let bounds = layout.bounds();
        paint::under(renderer, bounds, colors.face);
        bar::draw(
            renderer,
            bar::DrawBar {
                bar: &state.bar,
                origin: bounds.position(),
                width: bounds.width,
                colors: &colors,
                open: state.open,
                close_hover: state.close_hover,
            },
            bounds,
        );
        if let Some(clip) = child.bounds().intersection(viewport) {
            renderer.with_layer(clip, |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child,
                    cursor,
                    &clip,
                );
            });
        }
        paint::over(renderer, bounds, colors.rim);
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
        let bounds = layout.bounds();
        if self.chrome != Chrome::Bare {
            let state = tree.state.downcast_mut::<State>();
            if self.handle(state, event, bounds, cursor, shell) {
                return;
            }
        }
        let child = layout.children().next().expect("frame content");
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
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
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        if self.chrome == Chrome::Floating && self.on_resize.is_some() {
            if let Some(dir) = cursor.position().and_then(|p| grips::zone(bounds, p)) {
                return grips::interaction(dir);
            }
        }
        if self.chrome != Chrome::Bare && on_bar(bounds, cursor).is_some() {
            return mouse::Interaction::Idle;
        }
        let child = layout.children().next().expect("frame content");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let child = layout.children().next().expect("frame content");
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], child, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let open = tree.state.downcast_ref::<State>().open;
        if let (Some(i), true) = (open, self.chrome != Chrome::Bare) {
            let this: &'b Self = self;
            let menus = this.menus.as_ref()?;
            let menu = menus.menus.get(i)?;
            let state = tree.state.downcast_mut::<State>();
            let pill = *state.bar.menus.get(i)?;
            let bounds = layout.bounds();
            let anchor = Point::new(
                bounds.x + pill.x + bar::PILL_PAD + translation.x,
                bounds.y + HEIGHT + 4.0 + translation.y,
            );
            let count = state.bar.menus.len();
            return Some(overlay::Element::new(Box::new(Dropdown::new(
                state,
                &menus.app_id,
                menu,
                count,
                anchor,
            ))));
        }
        let child = layout.children().next().expect("frame content");
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<Message: Clone> WindowFrame<'_, Message> {
    /// The frame's own handling of `event`. `true` when it took the event
    /// and the content must not see it.
    fn handle(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        match event {
            Event::Window(window::Event::Focused) => {
                state.focused = Some(true);
                PROCESS_FOCUSED.store(true, Ordering::Relaxed);
                shell.request_redraw();
                return false;
            }
            Event::Window(window::Event::Unfocused) => {
                state.focused = Some(false);
                PROCESS_FOCUSED.store(false, Ordering::Relaxed);
                state.close_menu();
                shell.request_redraw();
                return false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Mouse(mouse::Event::CursorLeft) => {
                let hit = on_bar(bounds, cursor).and_then(|p| state.bar.hit(p));
                let close_hover = hit == Some(Hit::Close);
                let mut changed = close_hover != state.close_hover;
                state.close_hover = close_hover;
                // Hovering another title while a menu is open switches to it.
                if let (Some(open), Some(Hit::Menu(i))) = (state.open, hit) {
                    if open != i {
                        state.open = Some(i);
                        state.hot = None;
                        changed = true;
                    }
                }
                if changed {
                    shell.request_redraw();
                }
                return false;
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let Some(p) = cursor.position() else {
                    return false;
                };
                if *button == mouse::Button::Left && self.chrome == Chrome::Floating {
                    if let (Some(on_resize), Some(dir)) = (&self.on_resize, grips::zone(bounds, p))
                    {
                        state.close_menu();
                        shell.publish(on_resize(dir));
                        shell.capture_event();
                        return true;
                    }
                }
                match on_bar(bounds, cursor).and_then(|p| state.bar.hit(p)) {
                    Some(hit) if *button == mouse::Button::Left => {
                        match hit {
                            Hit::Close => {
                                state.close_menu();
                                if let Some(m) = &self.on_close {
                                    shell.publish(m.clone());
                                }
                            }
                            Hit::Menu(i) => {
                                state.open = if state.open == Some(i) { None } else { Some(i) };
                                state.hot = None;
                            }
                            Hit::Bar => {
                                state.close_menu();
                                if self.chrome == Chrome::Floating {
                                    if let Some(m) = &self.on_drag {
                                        shell.publish(m.clone());
                                    }
                                }
                            }
                        }
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                    Some(_) => {
                        shell.capture_event();
                        return true;
                    }
                    None if state.open.is_some() => {
                        // A press outside an open menu only closes it.
                        state.close_menu();
                        shell.request_redraw();
                        shell.capture_event();
                        return true;
                    }
                    None => {}
                }
            }
            _ => {}
        }
        false
    }
}

/// The cursor relative to the frame, while it is on the titlebar row.
fn on_bar(bounds: Rectangle, cursor: mouse::Cursor) -> Option<Point> {
    let p = cursor.position()?;
    let rel = Point::new(p.x - bounds.x, p.y - bounds.y);
    (rel.x >= 0.0 && rel.x < bounds.width && rel.y >= 0.0 && rel.y < HEIGHT).then_some(rel)
}

impl<'a, Message: Clone + 'a> From<WindowFrame<'a, Message>> for Element<'a, Message, Theme> {
    fn from(frame: WindowFrame<'a, Message>) -> Self {
        Element::new(frame)
    }
}
