//! A menu's dropdown: a sheet hanging under its title, inside the window.
//! Rows are drawn here (no child widgets), so the sheet needs nothing from
//! the app; a chosen row goes to [`crate::menu::choose`].

use std::marker::PhantomData;

use iced::advanced::layout::{self, Layout};
use iced::advanced::{Clipboard, Shell, mouse, overlay, renderer};
use iced::border::Radius;
use iced::keyboard::{self, key::Named};
use iced::{Event, Point, Rectangle, Shadow, Size, Theme, Vector};
use sola_bus::topics::{MenuDefinition, MenuItem};

use super::bar::{line, measure};
use super::frame::State;
use super::paint::{self, Colors};
use crate::fonts;

const PAD: f32 = 5.0;
const ROW_H: f32 = 26.0;
const ROW_PAD: f32 = 10.0;
const ROW_RADIUS: f32 = 7.0;
const SEP_H: f32 = 11.0;
const CHECK_W: f32 = 16.0;
const KEY_GAP: f32 = 28.0;
const MIN_W: f32 = 200.0;
const RADIUS: f32 = 12.0;
const LABEL_SIZE: f32 = 13.0;
const KEY_SIZE: f32 = 12.0;
/// Room kept between the sheet and the window's edges.
const MARGIN: f32 = 4.0;

pub(super) struct Dropdown<'b, Message> {
    state: &'b mut State,
    app_id: &'b str,
    menu: &'b MenuDefinition,
    /// How many menus the titlebar shows (Left / Right wrap over them).
    count: usize,
    /// Where the first row's text starts: under the title's text.
    anchor: Point,
    _message: PhantomData<Message>,
}

impl<'b, Message> Dropdown<'b, Message> {
    pub fn new(
        state: &'b mut State,
        app_id: &'b str,
        menu: &'b MenuDefinition,
        count: usize,
        anchor: Point,
    ) -> Self {
        Self {
            state,
            app_id,
            menu,
            count,
            anchor,
            _message: PhantomData,
        }
    }

    fn checks(&self) -> bool {
        self.menu
            .items
            .iter()
            .any(|i| matches!(i, MenuItem::Action { checked: true, .. }))
    }

    fn text_x(&self) -> f32 {
        PAD + ROW_PAD + if self.checks() { CHECK_W } else { 0.0 }
    }

    /// Rows' tops, relative to the sheet.
    fn rows(&self) -> Vec<(f32, &MenuItem)> {
        let mut y = PAD;
        self.menu
            .items
            .iter()
            .map(|item| {
                let top = y;
                y += match item {
                    MenuItem::Divider => SEP_H,
                    MenuItem::Action { .. } => ROW_H,
                };
                (top, item)
            })
            .collect()
    }

    fn size(&self) -> Size {
        let mut width: f32 = MIN_W;
        for item in &self.menu.items {
            if let MenuItem::Action {
                label, shortcut, ..
            } = item
            {
                let key = shortcut
                    .map(|k| KEY_GAP + measure(&k.display(), KEY_SIZE, fonts::chrome()))
                    .unwrap_or(0.0);
                let w = self.text_x() + measure(label, LABEL_SIZE, fonts::chrome()) + key + ROW_PAD + PAD;
                width = width.max(w.ceil());
            }
        }
        let height: f32 = self
            .menu
            .items
            .iter()
            .map(|i| match i {
                MenuItem::Divider => SEP_H,
                MenuItem::Action { .. } => ROW_H,
            })
            .sum();
        Size::new(width, height + 2.0 * PAD)
    }

    /// The enabled row under `p` (relative to the sheet).
    fn row_at(&self, p: Point) -> Option<usize> {
        if p.x < PAD || p.x > self.size().width - PAD {
            return None;
        }
        self.rows().iter().position(|(top, item)| {
            enabled(item) && p.y >= *top && p.y < top + ROW_H
        })
    }

    fn step(&self, from: Option<usize>, down: bool) -> Option<usize> {
        let n = self.menu.items.len();
        if n == 0 {
            return None;
        }
        let mut i = from.unwrap_or(if down { n - 1 } else { 0 });
        for _ in 0..n {
            i = if down { (i + 1) % n } else { (i + n - 1) % n };
            if enabled(&self.menu.items[i]) {
                return Some(i);
            }
        }
        None
    }

    fn activate(&mut self, index: usize) {
        if let Some(MenuItem::Action { id, disabled: false, .. }) = self.menu.items.get(index) {
            crate::menu::choose(self.app_id, id);
        }
        self.state.close_menu();
    }
}

fn enabled(item: &MenuItem) -> bool {
    matches!(item, MenuItem::Action { disabled: false, .. })
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for Dropdown<'_, Message> {
    fn layout(&mut self, _renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let size = self.size();
        let x = (self.anchor.x - self.text_x())
            .min(bounds.width - size.width - MARGIN)
            .max(MARGIN);
        let y = self.anchor.y;
        layout::Node::new(size).translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        use iced::advanced::Renderer as _;
        let colors: Colors = paint::colors(theme, true);
        let sheet = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds: sheet,
                border: iced::Border {
                    color: colors.sheet_edge,
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                shadow: Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.28),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 10.0,
                },
                snap: true,
            },
            colors.sheet,
        );
        let text_x = sheet.x + self.text_x();
        for (i, (top, item)) in self.rows().into_iter().enumerate() {
            let y = sheet.y + top;
            match item {
                MenuItem::Divider => paint::fill(
                    renderer,
                    Rectangle::new(
                        Point::new(sheet.x + PAD + ROW_PAD, y + SEP_H / 2.0),
                        Size::new(sheet.width - 2.0 * (PAD + ROW_PAD), 1.0),
                    ),
                    colors.sep,
                    Radius::default(),
                ),
                MenuItem::Action {
                    label,
                    shortcut,
                    disabled,
                    checked,
                    ..
                } => {
                    if self.state.hot == Some(i) {
                        paint::fill(
                            renderer,
                            Rectangle::new(
                                Point::new(sheet.x + PAD, y),
                                Size::new(sheet.width - 2.0 * PAD, ROW_H),
                            ),
                            colors.hot,
                            ROW_RADIUS.into(),
                        );
                    }
                    let ink = if *disabled { colors.ink_faint } else { colors.ink };
                    if *checked {
                        line(
                            renderer,
                            "✓",
                            LABEL_SIZE,
                            fonts::chrome(),
                            Point::new(sheet.x + PAD + ROW_PAD, y),
                            ROW_H,
                            ink,
                            sheet,
                        );
                    }
                    line(
                        renderer,
                        label,
                        LABEL_SIZE,
                        fonts::chrome(),
                        Point::new(text_x, y),
                        ROW_H,
                        ink,
                        sheet,
                    );
                    if let Some(key) = shortcut {
                        let key = key.display();
                        let w = measure(&key, KEY_SIZE, fonts::chrome());
                        line(
                            renderer,
                            &key,
                            KEY_SIZE,
                            fonts::chrome(),
                            Point::new(sheet.x + sheet.width - PAD - ROW_PAD - w, y),
                            ROW_H,
                            colors.ink_dim,
                            sheet,
                        );
                    }
                }
            }
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let sheet = layout.bounds();
        let over = cursor
            .position_over(sheet)
            .map(|p| Point::new(p.x - sheet.x, p.y - sheet.y));
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let hot = over.and_then(|p| self.row_at(p));
                // Leaving the sheet keeps the keyboard's row.
                if over.is_some() && hot != self.state.hot {
                    self.state.hot = hot;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) if over.is_some() => {
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(i) = over.and_then(|p| self.row_at(p)) {
                    self.activate(i);
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(named),
                ..
            }) => {
                match named {
                    Named::Escape => self.state.close_menu(),
                    Named::ArrowDown => self.state.hot = self.step(self.state.hot, true),
                    Named::ArrowUp => self.state.hot = self.step(self.state.hot, false),
                    Named::Enter => match self.state.hot {
                        Some(i) => self.activate(i),
                        None => return,
                    },
                    Named::ArrowLeft | Named::ArrowRight if self.count > 0 => {
                        let open = self.state.open.unwrap_or(0);
                        let next = if *named == Named::ArrowRight {
                            (open + 1) % self.count
                        } else {
                            (open + self.count - 1) % self.count
                        };
                        self.state.open = Some(next);
                        self.state.hot = None;
                    }
                    _ => return,
                }
                shell.request_redraw();
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        // Off the sheet the frame underneath keeps the cursor, so hovering
        // another title can switch menus.
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::None
        }
    }
}
