//! Drawing a window's text on a canvas, and the pointer on it.
//!
//! Three layers, so most frames redraw little: rectangles under the text
//! (selection, matches, the current line) and the point over it are drawn
//! each frame; the text and line numbers in between are cached until a
//! [`DrawKey`] says they changed.

use iced::advanced::mouse::click;
use iced::mouse::{self, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, keyboard, window};
use sola_kit::components::style::mix;
use sola_kit::fonts;

use crate::Msg;
use crate::buffer::Pos;
use crate::grid;
use crate::overlay::MiniKind;
use crate::search;
use crate::syntax::{Role, Span};
use crate::win::{DrawKey, Grid, Win};

/// The width kept at the right for the scrollbar.
pub const SCROLLBAR: f32 = 10.0;
/// Lines a wheel notch scrolls.
const WHEEL_LINES: f32 = 3.0;

/// The colors of a theme, as this view uses them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub background: Color,
    pub text: Color,
    pub muted: Color,
    pub gutter: Color,
    pub current: Color,
    pub selection: Color,
    pub find: Color,
    keyword: Color,
    string: Color,
    constant: Color,
    kind: Color,
    function: Color,
    invalid: Color,
}

impl Colors {
    pub fn of(theme: &Theme) -> Self {
        let p = theme.palette();
        Self {
            background: p.background,
            text: p.text,
            muted: mix(p.background, p.text, 0.45),
            gutter: mix(p.background, p.text, 0.6),
            current: mix(p.background, p.text, 0.94),
            selection: sola_kit::theme::selection(),
            find: Color { a: 0.35, ..p.warning },
            keyword: p.primary,
            string: p.success,
            constant: p.warning,
            kind: mix(p.primary, p.text, 0.45),
            function: mix(p.warning, p.text, 0.5),
            invalid: p.danger,
        }
    }

    fn of_role(&self, role: Role) -> Color {
        match role {
            Role::Plain => self.text,
            Role::Comment => self.muted,
            Role::String => self.string,
            Role::Keyword => self.keyword,
            Role::Constant => self.constant,
            Role::Type => self.kind,
            Role::Function => self.function,
            Role::Invalid => self.invalid,
        }
    }
}

/// How many clicks a press was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clicks {
    One,
    Two,
    Three,
}

#[derive(Default)]
pub struct Gesture {
    selecting: bool,
    on_scrollbar: bool,
    last_click: Option<click::Click>,
    modifiers: keyboard::Modifiers,
}

pub struct Surface<'a> {
    pub win: &'a Win,
}

impl Surface<'_> {
    fn pos_at(&self, point: Point) -> Pos {
        let win = self.win;
        let gutter = win.grid.gutter(win.ed.buf.line_count());
        let row = (point.y / win.grid.cell_h).floor().max(0.0) as usize;
        let line = (win.top + row).min(win.ed.buf.line_count() - 1);
        // The nearer edge of the cell.
        let cell = ((point.x - gutter) / win.grid.cell_w).round().max(0.0) as usize;
        let text = win.ed.buf.line(line);
        Pos::new(line, grid::col_at_display(text, win.left + cell, win.ed.tab_size))
    }

    /// Where the scrollbar's thumb is, as (top, height) in pixels.
    fn thumb(&self, height: f32) -> Option<(f32, f32)> {
        let lines = self.win.ed.buf.line_count() as f32;
        let rows = self.win.rows() as f32;
        if lines <= rows {
            return None;
        }
        let size = (rows / lines * height).max(24.0);
        let last = (lines - 1.0).max(1.0);
        Some((self.win.top as f32 / last * (height - size), size))
    }
}

impl canvas::Program<Msg> for Surface<'_> {
    type State = Gesture;

    fn update(
        &self,
        state: &mut Gesture,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Msg>> {
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                None
            }
            // A drawn frame found lines it had no colors for.
            Event::Window(window::Event::RedrawRequested(_)) if self.win.hl_behind.get() => {
                self.win.hl_behind.set(false);
                Some(Action::publish(Msg::HighlightMore))
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                if at.x >= bounds.width - SCROLLBAR {
                    state.on_scrollbar = true;
                    return Some(Action::publish(Msg::ScrollTo(at.y / bounds.height)).and_capture());
                }
                let made = click::Click::new(at, mouse::Button::Left, state.last_click.take());
                let clicks = match made.kind() {
                    click::Kind::Single => Clicks::One,
                    click::Kind::Double => Clicks::Two,
                    click::Kind::Triple => Clicks::Three,
                };
                state.last_click = Some(made);
                state.selecting = true;
                Some(
                    Action::publish(Msg::Press {
                        at: self.pos_at(at),
                        clicks,
                        shift: state.modifiers.shift(),
                    })
                    .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let at = Point::new(position.x - bounds.x, position.y - bounds.y);
                if state.on_scrollbar {
                    Some(Action::publish(Msg::ScrollTo(at.y / bounds.height)))
                } else if state.selecting {
                    Some(Action::publish(Msg::DragTo(self.pos_at(at))))
                } else {
                    None
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.selecting = false;
                state.on_scrollbar = false;
                None
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let (lines, columns) = match delta {
                    ScrollDelta::Lines { x, y } => (-y * WHEEL_LINES, -x * WHEEL_LINES),
                    ScrollDelta::Pixels { x, y } => {
                        (-y / self.win.grid.cell_h, -x / self.win.grid.cell_w)
                    }
                };
                Some(Action::publish(Msg::Wheel { lines, columns }).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Gesture,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let win = self.win;
        let size = bounds.size();
        win.viewport.set(size);
        let colors = Colors::of(theme);
        let rows = (size.height / win.grid.cell_h).ceil() as usize + 1;
        let end = (win.top + rows).min(win.ed.buf.line_count());
        if !win.hl.covers(end, win.ed.buf.line_count()) {
            win.hl_behind.set(true);
        }
        let key = DrawKey {
            revision: win.ed.buf.revision(),
            top: win.top,
            left: win.left,
            size,
            highlight: win.hl.generation(),
            colors,
            grid: win.grid,
        };
        if win.drawn.get() != Some(key) {
            win.text.clear();
            win.drawn.set(Some(key));
        }

        let mut under = Frame::new(renderer, size);
        draw_under(&mut under, win, &colors, size, end);
        let text = win.text.draw(renderer, size, |frame| draw_text(frame, win, &colors, size, end));
        let mut over = Frame::new(renderer, size);
        draw_over(&mut over, win, &colors, end);
        if let Some((y, height)) = self.thumb(size.height) {
            over.fill_rectangle(
                Point::new(size.width - SCROLLBAR + 2.0, y),
                Size::new(SCROLLBAR - 4.0, height),
                Color { a: 0.25, ..colors.text },
            );
        }
        vec![under.into_geometry(), text, over.into_geometry()]
    }

    fn mouse_interaction(&self, state: &Gesture, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(at) if at.x >= bounds.width - SCROLLBAR => mouse::Interaction::default(),
            Some(_) => mouse::Interaction::Text,
            None if state.selecting => mouse::Interaction::Text,
            None => mouse::Interaction::default(),
        }
    }
}

fn x_of(win: &Win, gutter: f32, column: usize) -> f32 {
    gutter + (column as f32 - win.left as f32) * win.grid.cell_w
}

fn y_of(win: &Win, line: usize) -> f32 {
    (line - win.top) as f32 * win.grid.cell_h
}

fn draw_under(frame: &mut Frame<Renderer>, win: &Win, colors: &Colors, size: Size, end: usize) {
    let ed = &win.ed;
    let grid = win.grid;
    let gutter = grid.gutter(ed.buf.line_count());
    frame.fill_rectangle(Point::ORIGIN, size, colors.background);

    let region = ed.region();
    if region.is_none() && (win.top..end).contains(&ed.point.line) {
        frame.fill_rectangle(
            Point::new(0.0, y_of(win, ed.point.line)),
            Size::new(size.width, grid.cell_h),
            colors.current,
        );
    }

    let mut fill = |line: usize, start: usize, stop: usize, color: Color| {
        let x = x_of(win, gutter, start).max(gutter).round();
        let right = x_of(win, gutter, stop).round();
        if right > x {
            frame.fill_rectangle(
                Point::new(x, y_of(win, line)),
                Size::new(right - x, grid.cell_h),
                color,
            );
        }
    };

    if let Some(mini) = win.mini.as_ref().filter(|m| m.kind == MiniKind::Find && !m.text.is_empty()) {
        for line in win.top..end {
            let text = ed.buf.line(line);
            for found in search::in_line(text, &mini.text) {
                fill(
                    line,
                    grid::display_col(text, found.start, ed.tab_size),
                    grid::display_col(text, found.end, ed.tab_size),
                    colors.find,
                );
            }
        }
    }

    if let Some((a, b)) = region {
        for line in a.line.max(win.top)..=b.line.min(end.saturating_sub(1)) {
            let text = ed.buf.line(line);
            let start = if line == a.line {
                grid::display_col(text, a.col, ed.tab_size)
            } else {
                0
            };
            // Past the end of a line that continues: its newline is selected.
            let stop = if line == b.line {
                grid::display_col(text, b.col, ed.tab_size)
            } else {
                grid::display_col(text, text.len(), ed.tab_size) + 1
            };
            fill(line, start, stop, colors.selection);
        }
    }
}

fn draw_text(frame: &mut Frame<Renderer>, win: &Win, colors: &Colors, size: Size, end: usize) {
    let ed = &win.ed;
    let grid = win.grid;
    let gutter = grid.gutter(ed.buf.line_count());
    let columns = (((size.width - gutter) / grid.cell_w).ceil() as usize).max(1);
    let font = fonts::mono();
    // Text scrolled sideways stops at the gutter. It can't be painted over:
    // a tiled window's background is clear, and only its frame paints.
    let area = Rectangle::new(Point::new(gutter, 0.0), Size::new((size.width - gutter).max(0.0), size.height));
    frame.with_clip(area, |frame| {
        for line in win.top..end {
            let text = ed.buf.line(line);
            let y = y_of(win, line);
            let spans = win.hl.spans(line).filter(|s| !s.is_empty());
            for run in grid::visible_runs(text, win.left, columns, ed.tab_size) {
                let x = x_of(win, gutter, run.column);
                draw_run(frame, run, x, y, spans, colors, grid, font);
            }
        }
    });
    for line in win.top..end {
        draw_number(frame, line, y_of(win, line), gutter, colors.gutter, grid);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_run(
    frame: &mut Frame<Renderer>,
    run: grid::Run<'_>,
    x: f32,
    y: f32,
    spans: Option<&[Span]>,
    colors: &Colors,
    grid: Grid,
    font: iced::Font,
) {
    let shaping = if run.ascii { Shaping::Basic } else { Shaping::Advanced };
    let mut put = |content: &str, x: f32, color: Color| {
        frame.fill_text(Text {
            content: content.to_string(),
            position: Point::new(x, y.round()),
            color,
            size: grid.font_size.into(),
            font,
            line_height: LineHeight::Absolute(grid.cell_h.into()),
            shaping,
            ..Text::default()
        });
    };
    let Some(spans) = spans else {
        put(run.text, x, colors.text);
        return;
    };
    // Cut the run where the colors change. ASCII: a byte is a cell.
    let stop = run.byte + run.text.len();
    let mut at = run.byte;
    let mut index = spans.partition_point(|s| (s.0 as usize) <= at);
    while at < stop {
        let (end, role) = spans.get(index).map_or((stop, Role::Plain), |s| (s.0 as usize, s.1));
        let until = end.min(stop);
        if until > at {
            let piece = &run.text[at - run.byte..until - run.byte];
            put(piece, x + (at - run.byte) as f32 * grid.cell_w, colors.of_role(role));
            at = until;
        }
        index += 1;
    }
}

fn draw_number(frame: &mut Frame<Renderer>, line: usize, y: f32, gutter: f32, color: Color, grid: Grid) {
    let number = (line + 1).to_string();
    frame.fill_text(Text {
        position: Point::new(gutter - (number.len() as f32 + 1.0) * grid.cell_w, y.round()),
        content: number,
        color,
        size: grid.font_size.into(),
        font: fonts::mono(),
        line_height: LineHeight::Absolute(grid.cell_h.into()),
        shaping: Shaping::Basic,
        ..Text::default()
    });
}

fn draw_over(frame: &mut Frame<Renderer>, win: &Win, colors: &Colors, end: usize) {
    let ed = &win.ed;
    let grid = win.grid;
    if !(win.top..end).contains(&ed.point.line) {
        return;
    }
    let gutter = grid.gutter(ed.buf.line_count());
    let y = y_of(win, ed.point.line);
    draw_number(frame, ed.point.line, y, gutter, colors.text, grid);
    let x = x_of(win, gutter, ed.point_column()).round();
    if x < gutter {
        return;
    }
    if win.focused {
        frame.fill_rectangle(Point::new(x, y), Size::new(2.0, grid.cell_h), colors.text);
    } else {
        frame.stroke(
            &Path::rectangle(Point::new(x + 0.5, y + 0.5), Size::new(grid.cell_w - 1.0, grid.cell_h - 1.0)),
            Stroke::default().with_color(colors.muted).with_width(1.0),
        );
    }
}
