//! The zone picker: the green light's sheet, hanging under it inside the
//! window. A little drawing of the screen for each zone of the layout, the
//! zone filled where it sits, then Full, Cinema and Float under a rule. A
//! chosen one goes to [`crate::menu::choose`] as its Window menu item
//! would, and the shell moves the focused window there.

use std::marker::PhantomData;

use iced::advanced::layout::{self, Layout};
use iced::advanced::{Clipboard, Shell, mouse, overlay, renderer};
use iced::border::Radius;
use iced::keyboard::{self, key::Named};
use iced::{Color, Event, Point, Rectangle, Size, Theme, Vector};
use sola_bus::topics::{GridZone, Zone, ZoneLayout};

use super::bar::{line, measure};
use super::frame::State;
use super::paint::{self, Colors};
use crate::components::menu_sheet::{self, PAD, RADIUS, ROW_PAD, ROW_RADIUS, SEP_H};
use crate::components::style::alpha;
use crate::fonts;
use crate::menu::{ACTION_CINEMA, ACTION_FLOAT, ACTION_FULLSCREEN, grid_action_id};

/// A drawing of the screen, 16:10.
const TILE_W: f32 = 88.0;
const TILE_H: f32 = 55.0;
/// Around a drawing and its name, inside its highlight.
const CELL_PAD: f32 = 6.0;
const NAME_H: f32 = 18.0;
const NAME_SIZE: f32 = 12.0;
const CELL_W: f32 = TILE_W + 2.0 * CELL_PAD;
const CELL_H: f32 = CELL_PAD + TILE_H + NAME_H + CELL_PAD;
/// Drawings in a row, at most.
const COLUMNS: usize = 4;
/// The tab bar's share of the screen's width (240 of 1920).
const BAR_SHARE: f32 = 0.125;
/// Room kept between the sheet and the window's edges.
const MARGIN: f32 = 4.0;

/// One zone to pick.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Choice {
    pub zone: Zone,
    pub name: String,
    /// Its cell (drawing and name), relative to the sheet.
    pub cell: Rectangle,
}

impl Choice {
    /// The Window menu item that moves a window to this zone.
    fn action(&self) -> String {
        match self.zone {
            Zone::Grid(id) => grid_action_id(id),
            Zone::Fullscreen => ACTION_FULLSCREEN.into(),
            Zone::Cinema => ACTION_CINEMA.into(),
            Zone::Float => ACTION_FLOAT.into(),
        }
    }
}

/// The laid-out sheet.
#[derive(Debug)]
pub(super) struct Sheet {
    pub choices: Vec<Choice>,
    pub size: Size,
    /// Where the rule between the grid's zones and the others sits.
    pub rule: Option<f32>,
}

impl Sheet {
    /// The grid's zones, [`COLUMNS`] to a row, then Full, Cinema and Float
    /// on a row of their own.
    pub fn new(layout: &ZoneLayout) -> Self {
        let grid: Vec<(Zone, String)> = layout
            .zones
            .iter()
            .map(|z| (Zone::Grid(z.id), layout.label(z.id)))
            .collect();
        let others = [
            (Zone::Fullscreen, "Full"),
            (Zone::Cinema, "Cinema"),
            (Zone::Float, "Float"),
        ]
        .map(|(z, name)| (z, name.to_string()));
        let mut choices = Vec::new();
        let mut rule = None;
        let mut columns = 0;
        let mut y = PAD;
        for group in [grid, others.to_vec()] {
            if group.is_empty() {
                continue;
            }
            if !choices.is_empty() {
                rule = Some(y + SEP_H / 2.0);
                y += SEP_H;
            }
            let rows = group.len().div_ceil(COLUMNS);
            for (i, (zone, name)) in group.into_iter().enumerate() {
                let (r, c) = (i / COLUMNS, i % COLUMNS);
                columns = columns.max(c + 1);
                choices.push(Choice {
                    zone,
                    name,
                    cell: Rectangle::new(
                        Point::new(PAD + c as f32 * CELL_W, y + r as f32 * CELL_H),
                        Size::new(CELL_W, CELL_H),
                    ),
                });
            }
            y += rows as f32 * CELL_H;
        }
        Self {
            choices,
            size: Size::new(2.0 * PAD + columns as f32 * CELL_W, y + PAD),
            rule,
        }
    }

    /// The choice under `p` (relative to the sheet).
    pub fn at(&self, p: Point) -> Option<usize> {
        self.choices.iter().position(|c| c.cell.contains(p))
    }

    /// The nearest choice in the row above or below `from`'s.
    fn vertical(&self, from: usize, down: bool) -> Option<usize> {
        let at = self.choices.get(from)?.cell;
        let key = |c: &Choice| ((c.cell.y - at.y).abs(), (c.cell.center_x() - at.center_x()).abs());
        self.choices
            .iter()
            .enumerate()
            .filter(|(_, c)| if down { c.cell.y > at.y } else { c.cell.y < at.y })
            .min_by(|(_, a), (_, b)| {
                let (a, b) = (key(a), key(b));
                a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
            })
            .map(|(i, _)| i)
    }

    /// The choice an arrow key moves the highlight to.
    pub fn step(&self, from: Option<usize>, key: Named) -> Option<usize> {
        let n = self.choices.len();
        if n == 0 {
            return None;
        }
        let Some(from) = from else {
            return Some(0);
        };
        match key {
            Named::ArrowRight => Some((from + 1) % n),
            Named::ArrowLeft => Some((from + n - 1) % n),
            Named::ArrowDown => self.vertical(from, true).or(Some(from)),
            Named::ArrowUp => self.vertical(from, false).or(Some(from)),
            _ => Some(from),
        }
    }
}

pub(super) struct ZonePicker<'b, Message> {
    state: &'b mut State,
    app_id: &'b str,
    layout: ZoneLayout,
    sheet: Sheet,
    /// The sheet's top-left: under the green light.
    anchor: Point,
    _message: PhantomData<Message>,
}

impl<'b, Message> ZonePicker<'b, Message> {
    pub fn new(state: &'b mut State, app_id: &'b str, anchor: Point) -> Self {
        let layout = crate::menu::zone_layout();
        let sheet = Sheet::new(&layout);
        Self {
            state,
            app_id,
            layout,
            sheet,
            anchor,
            _message: PhantomData,
        }
    }

    fn activate(&mut self, index: usize) {
        if let Some(choice) = self.sheet.choices.get(index) {
            crate::menu::choose(self.app_id, &choice.action());
        }
        self.state.close_menu();
    }
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for ZonePicker<'_, Message> {
    fn layout(&mut self, _renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let size = self.sheet.size;
        let x = (self.anchor.x - PAD - CELL_PAD)
            .min(bounds.width - size.width - MARGIN)
            .max(MARGIN);
        layout::Node::new(size).translate(Vector::new(x, self.anchor.y))
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
        let accent = theme.extended_palette().primary.base.color;
        let sheet = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds: sheet,
                border: iced::Border {
                    color: colors.sheet_edge,
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                shadow: menu_sheet::shadow(),
                snap: true,
            },
            colors.sheet,
        );
        if let Some(y) = self.sheet.rule {
            paint::fill(
                renderer,
                Rectangle::new(
                    Point::new(sheet.x + PAD + ROW_PAD, sheet.y + y),
                    Size::new(sheet.width - 2.0 * (PAD + ROW_PAD), 1.0),
                ),
                colors.sep,
                Radius::default(),
            );
        }
        for (i, choice) in self.sheet.choices.iter().enumerate() {
            let cell = Rectangle::new(
                Point::new(sheet.x + choice.cell.x, sheet.y + choice.cell.y),
                choice.cell.size(),
            );
            let hot = self.state.hot == Some(i);
            if hot {
                paint::fill(renderer, cell, colors.hot, ROW_RADIUS.into());
            }
            let tile = Rectangle::new(
                Point::new(cell.x + CELL_PAD, cell.y + CELL_PAD),
                Size::new(TILE_W, TILE_H),
            );
            draw_tile(
                renderer,
                tile,
                &self.layout,
                choice.zone,
                &colors,
                alpha(accent, if hot { 1.0 } else { 0.7 }),
            );
            let room = CELL_W - 2.0 * CELL_PAD;
            let name = fonts::fit_line(&choice.name, NAME_SIZE, fonts::chrome(), room);
            let w = measure(&name, NAME_SIZE, fonts::chrome());
            line(
                renderer,
                &name,
                NAME_SIZE,
                fonts::chrome(),
                Point::new(cell.center_x() - w / 2.0, tile.y + TILE_H),
                NAME_H,
                colors.ink,
                sheet,
            );
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
                let hot = over.and_then(|p| self.sheet.at(p));
                // Leaving the sheet keeps the keyboard's choice.
                if over.is_some() && hot != self.state.hot {
                    self.state.hot = hot;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) if over.is_some() => {
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(i) = over.and_then(|p| self.sheet.at(p)) {
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
                    Named::Enter => match self.state.hot {
                        Some(i) => self.activate(i),
                        None => return,
                    },
                    Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown => {
                        self.state.hot = self.sheet.step(self.state.hot, *named);
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
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::None
        }
    }
}

/// A little screen with `zone` filled in `fill`: the tab bar a sliver at
/// its left, the content area right of it (Cinema covers both).
fn draw_tile(
    renderer: &mut iced::Renderer,
    tile: Rectangle,
    layout: &ZoneLayout,
    zone: Zone,
    colors: &Colors,
    fill: Color,
) {
    use iced::advanced::Renderer as _;
    renderer.fill_quad(
        renderer::Quad {
            bounds: tile,
            border: iced::Border {
                color: alpha(colors.ink, 0.22),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..renderer::Quad::default()
        },
        alpha(colors.ink, 0.05),
    );
    let inner = tile.shrink(3.0);
    if zone == Zone::Cinema {
        paint::fill(renderer, inner, fill, 2.0.into());
        return;
    }
    let bar = Rectangle::new(inner.position(), Size::new(inner.width * BAR_SHARE, inner.height));
    paint::fill(renderer, bar, alpha(colors.ink, 0.12), 1.5.into());
    let content = Rectangle::new(
        Point::new(bar.x + bar.width + 2.0, inner.y),
        Size::new(inner.width - bar.width - 2.0, inner.height),
    );
    let rect = match zone {
        Zone::Grid(id) => match layout.grid(id) {
            Some(z) => cells(content, layout, z),
            None => return,
        },
        Zone::Float => Rectangle::new(
            Point::new(
                content.x + content.width * 0.25,
                content.y + content.height * 0.25,
            ),
            Size::new(content.width * 0.5, content.height * 0.5),
        ),
        Zone::Fullscreen | Zone::Cinema => content,
    };
    paint::fill(renderer, rect, fill, 1.5.into());
}

/// A grid zone's cells in `content`, a pixel kept clear around it so
/// neighbours stand apart.
fn cells(content: Rectangle, layout: &ZoneLayout, z: &GridZone) -> Rectangle {
    let edge = |n: u8, count: u8, from: f32, len: f32| {
        from + len * f32::from(n) / f32::from(count.max(1))
    };
    let x0 = edge(z.column, layout.columns, content.x, content.width);
    let x1 = edge(z.column + z.columns, layout.columns, content.x, content.width);
    let y0 = edge(z.row, layout.rows, content.y, content.height);
    let y1 = edge(z.row + z.rows, layout.rows, content.y, content.height);
    Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0)).shrink(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones(n: u32) -> ZoneLayout {
        ZoneLayout {
            rows: 1,
            columns: n as u8,
            zones: (0..n)
                .map(|i| GridZone {
                    id: i + 1,
                    row: 0,
                    column: i as u8,
                    rows: 1,
                    columns: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn the_grid_comes_first_then_the_others_on_their_own_row() {
        let sheet = Sheet::new(&ZoneLayout::default());
        let picked: Vec<Zone> = sheet.choices.iter().map(|c| c.zone).collect();
        assert_eq!(
            picked,
            [
                Zone::Grid(1),
                Zone::Grid(2),
                Zone::Fullscreen,
                Zone::Cinema,
                Zone::Float
            ]
        );
        assert_eq!(sheet.choices[0].name, "Column 1");
        assert_eq!(sheet.choices[2].cell.x, sheet.choices[0].cell.x);
        assert!(sheet.choices[2].cell.y > sheet.choices[1].cell.y);
        assert_eq!(sheet.size.width, 2.0 * PAD + 3.0 * CELL_W);
        assert!(sheet.rule.is_some());
    }

    #[test]
    fn a_long_grid_wraps() {
        let sheet = Sheet::new(&zones(6));
        assert_eq!(sheet.size.width, 2.0 * PAD + 4.0 * CELL_W);
        assert_eq!(sheet.choices[4].cell.x, PAD);
        assert_eq!(sheet.choices[4].cell.y, PAD + CELL_H);
        // Two rows of the grid, a rule, a row of the others.
        assert_eq!(sheet.size.height, PAD + 3.0 * CELL_H + SEP_H + PAD);
    }

    #[test]
    fn no_grid_no_rule() {
        let sheet = Sheet::new(&zones(0));
        assert_eq!(sheet.choices.len(), 3);
        assert_eq!(sheet.rule, None);
    }

    #[test]
    fn a_point_lands_on_its_cell() {
        let sheet = Sheet::new(&ZoneLayout::default());
        let second = sheet.choices[1].cell.center();
        assert_eq!(sheet.at(second), Some(1));
        assert_eq!(sheet.at(Point::new(1.0, 1.0)), None);
    }

    #[test]
    fn arrows_walk_the_sheet() {
        let sheet = Sheet::new(&ZoneLayout::default());
        assert_eq!(sheet.step(None, Named::ArrowDown), Some(0));
        assert_eq!(sheet.step(Some(0), Named::ArrowRight), Some(1));
        assert_eq!(sheet.step(Some(0), Named::ArrowLeft), Some(4));
        // Down from Column 2 lands under it: Cinema.
        assert_eq!(sheet.step(Some(1), Named::ArrowDown), Some(3));
        assert_eq!(sheet.step(Some(4), Named::ArrowUp), Some(1));
        assert_eq!(sheet.step(Some(4), Named::ArrowDown), Some(4));
    }

    #[test]
    fn each_choice_is_its_window_menu_item() {
        let sheet = Sheet::new(&ZoneLayout::default());
        let actions: Vec<String> = sheet.choices.iter().map(Choice::action).collect();
        assert_eq!(
            actions,
            [
                "window.zone.1",
                "window.zone.2",
                "window.fullscreen",
                "window.cinema",
                "window.float"
            ]
        );
    }
}
