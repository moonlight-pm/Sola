//! Plane overlay: artboard frames, titles, annotations (see
//! [`crate::annotate`]), the selection outline, and all pointer / key input
//! over the canvas.
//!
//! Pages are always live: pointer and keys go to the artboard under the
//! pointer. Super+click picks an element instead (Super+click on empty
//! canvas clears the pick). Right-drag pans and the wheel zooms. A click on
//! an exit (or a plain link to another board) is the app's: the camera
//! goes to that board. A click on a note marker keeps its card open. With
//! no page focused, → ← Esc 1–9 drive a tour and N opens every note.

use std::collections::HashSet;
use std::time::Instant;

use iced::alignment::Vertical;
use iced::mouse::{self, Button, Cursor, Interaction};
use iced::widget::canvas::{self, Action, Frame, Geometry, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector, keyboard};
use sola_browser::CursorKind;
use sola_browser::cef::engine::InputEvent;
use sola_browser::cef::input;
use sola_kit::fonts;

use crate::annotate::{self, Focus};
use crate::boards::{self, Board};
use crate::camera::{self, Camera};
use crate::glow;
use crate::scene::Scene;
use crate::select;
use crate::tour::Nav;

#[derive(Debug, Clone)]
pub enum Event {
    /// Widget bounds changed (needed to fit the camera).
    Bounds(Rectangle),
    Pan(Vector),
    Zoom { at: Point, factor: f32 },
    /// Super+click at board-local CSS px.
    Pick { tab: u64, x: f32, y: f32 },
    ClearSelection,
    /// Events for board tabs, in order.
    Input(Vec<(u64, InputEvent)>),
    /// Click on empty canvas: no board has the keyboard.
    Blur,
    /// A click on an exit / board link in board `tab`: go to board `to`.
    Follow { tab: u64, to: String },
    /// A click on note `n`'s marker.
    ToggleNote(usize),
    /// A tour / notes key while no page has the keyboard.
    Nav(Nav),
}

pub struct Overlay<'a> {
    pub camera: Camera,
    pub boards: &'a [Board],
    /// Parallel to `boards`: the board has painted at least once.
    pub painted: Vec<bool>,
    /// Outline: board tab + element rect in that board's viewport.
    pub selection: Option<(u64, select::Rect)>,
    pub focus: Option<u64>,
    /// Engine cursor (`CursorKind` as u32) over a page.
    pub cursor: u32,
    /// Off while a dialog is open: no input reaches the plane.
    pub live: bool,
    /// What agents are touching (see [`crate::glow`]).
    pub glows: Vec<glow::Lit>,
    /// The canvas's annotations (see [`crate::scene`]).
    pub scene: Scene,
    /// The tour's step board, while a tour is on.
    pub step: Option<&'a str>,
    /// Every note card open (the notes toggle).
    pub notes_open: bool,
    /// Note cards clicked open.
    pub open_notes: &'a HashSet<usize>,
}

/// What a hover over the plane would act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Note(usize),
    Hot(usize),
}

#[derive(Default)]
pub struct State {
    bounds: Option<Rectangle>,
    pan_last: Option<Point>,
    hover: Option<u64>,
    /// Board that got the button press; keeps moves until release.
    capture: Option<u64>,
    held: u32,
    last_press: Option<(u32, i32, i32, Instant, u32)>,
    /// An IME preedit is live in the focused page: typed characters belong
    /// to it, not to the page as keys.
    composing: bool,
    /// The note marker / clickable region under the pointer (redraw when
    /// it changes).
    mark: Option<Mark>,
}

/// Super is held. River eats a bare Super press (it reaches us as a bus
/// chord, folded in by the app), so iced's modifiers alone miss it.
fn super_held() -> bool {
    sola_browser::input::stored_modifiers().logo()
}

fn page_mods() -> u32 {
    input::modifiers_to_cef_mouse(sola_browser::input::stored_modifiers())
}

impl Overlay<'_> {
    fn board(&self, tab: u64) -> Option<&Board> {
        self.boards.iter().find(|b| b.tab.0 == tab)
    }

    /// Topmost board under a widget-local point, with board-local CSS px.
    fn hit(&self, local: Point) -> Option<(u64, f32, f32)> {
        let (b, (x, y)) = boards::hit(self.boards, self.camera.to_canvas(local))?;
        Some((b.tab.0, x, y))
    }

    /// The note marker, else the clickable region, under a widget-local
    /// point. Super+click picks, so then there is none.
    fn mark(&self, local: Point) -> Option<Mark> {
        if super_held() {
            return None;
        }
        if let Some(n) = annotate::note_at(&self.scene, self.camera, local) {
            return Some(Mark::Note(n));
        }
        let (tab, x, y) = self.hit(local)?;
        self.scene.hot_at(tab, x, y).map(Mark::Hot)
    }

    /// Board-local CSS px for any widget-local point (may be outside).
    fn board_point(&self, tab: u64, local: Point) -> Option<(i32, i32)> {
        let b = self.board(tab)?;
        let c = self.camera.to_canvas(local);
        Some(((c.x - b.art.x).round() as i32, (c.y - b.art.y).round() as i32))
    }

    fn mouse(
        &self,
        st: &mut State,
        event: mouse::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Event>> {
        // While dragging, points outside the widget still count.
        let local = cursor.position().map(|p| p - Vector::new(bounds.x, bounds.y));
        match event {
            mouse::Event::ButtonPressed(Button::Right) => {
                st.pan_last = Some(cursor.position_in(bounds)?);
                Some(Action::request_redraw().and_capture())
            }
            mouse::Event::ButtonReleased(Button::Right) if st.pan_last.is_some() => {
                st.pan_last = None;
                Some(Action::request_redraw().and_capture())
            }
            mouse::Event::CursorMoved { .. } => {
                if let Some(last) = st.pan_last {
                    let p = local?;
                    st.pan_last = Some(p);
                    return Some(Action::publish(Event::Pan(p - last)).and_capture());
                }
                let mark = cursor.position_in(bounds).and_then(|p| self.mark(p));
                let moved_mark = std::mem::replace(&mut st.mark, mark) != mark;
                match self.page_move(st, local?, bounds, cursor) {
                    Some(action) => Some(action),
                    None if moved_mark => Some(Action::request_redraw()),
                    None => None,
                }
            }
            mouse::Event::WheelScrolled { delta } => {
                let at = cursor.position_in(bounds)?;
                let factor = camera::wheel_factor(delta);
                Some(Action::publish(Event::Zoom { at, factor }).and_capture())
            }
            mouse::Event::ButtonPressed(Button::Left) if super_held() => {
                let p = cursor.position_in(bounds)?;
                let ev = match self.hit(p) {
                    Some((tab, x, y)) => Event::Pick { tab, x, y },
                    None => Event::ClearSelection,
                };
                Some(Action::publish(ev).and_capture())
            }
            mouse::Event::ButtonPressed(Button::Left) if self.mark(cursor.position_in(bounds)?).is_some() => {
                let p = cursor.position_in(bounds)?;
                let ev = match self.mark(p)? {
                    Mark::Note(n) => Event::ToggleNote(n),
                    Mark::Hot(i) => {
                        let hot = &self.scene.hots[i];
                        Event::Follow {
                            tab: hot.tab,
                            to: hot.to.clone(),
                        }
                    }
                };
                Some(Action::publish(ev).and_capture())
            }
            mouse::Event::ButtonPressed(b @ (Button::Left | Button::Middle)) => {
                let p = cursor.position_in(bounds)?;
                let Some((tab, x, y)) = self.hit(p) else {
                    return Some(Action::publish(Event::Blur).and_capture());
                };
                let button = input::button_number(b)?;
                let (xi, yi) = (x.round() as i32, y.round() as i32);
                let now = Instant::now();
                let prev = st
                    .last_press
                    .map(|(pb, px, py, t, c)| (pb, px, py, now.duration_since(t).as_millis(), c));
                let count = input::next_click_count(prev, button, xi, yi);
                st.last_press = Some((button, xi, yi, now, count));
                st.held |= input::button_to_modifier(button);
                st.capture = Some(tab);
                let ev = input::pointer_button(true, button, xi, yi, st.held, page_mods(), count);
                Some(Action::publish(Event::Input(vec![(tab, ev)])).and_capture())
            }
            mouse::Event::ButtonReleased(b @ (Button::Left | Button::Middle)) => {
                let tab = st.capture?;
                let button = input::button_number(b)?;
                let (x, y) = self.board_point(tab, local?)?;
                st.held &= !input::button_to_modifier(button);
                if st.held == 0 {
                    st.capture = None;
                }
                let count = st.last_press.map_or(1, |p| p.4);
                let ev = input::pointer_button(false, button, x, y, st.held, page_mods(), count);
                Some(Action::publish(Event::Input(vec![(tab, ev)])).and_capture())
            }
            mouse::Event::CursorLeft => {
                let tab = st.hover.take()?;
                let (x, y) = local.and_then(|p| self.board_point(tab, p)).unwrap_or((-1, -1));
                let ev = input::pointer_leave(x, y, st.held, page_mods());
                Some(Action::publish(Event::Input(vec![(tab, ev)])))
            }
            _ => None,
        }
    }

    fn page_move(
        &self,
        st: &mut State,
        local: Point,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Event>> {
        let mods = page_mods();
        if let Some(tab) = st.capture {
            let (x, y) = self.board_point(tab, local)?;
            let ev = input::pointer_move(x, y, st.held, mods);
            return Some(Action::publish(Event::Input(vec![(tab, ev)])).and_capture());
        }
        let hit = cursor.position_in(bounds).and_then(|p| self.hit(p));
        let mut out = Vec::new();
        if let Some(old) = st.hover.filter(|old| Some(*old) != hit.map(|h| h.0)) {
            let (x, y) = self.board_point(old, local).unwrap_or((-1, -1));
            out.push((old, input::pointer_leave(x, y, st.held, mods)));
        }
        st.hover = hit.map(|h| h.0);
        if let Some((tab, x, y)) = hit {
            let ev = input::pointer_move(x.round() as i32, y.round() as i32, st.held, mods);
            out.push((tab, ev));
        }
        (!out.is_empty()).then(|| Action::publish(Event::Input(out)))
    }

    /// Keys go to the focused page. ⌘ chords are app / shell shortcuts.
    /// With no page focused, tour and notes keys are the plane's.
    fn keyboard(&self, event: &keyboard::Event) -> Option<Action<Event>> {
        let Some(tab) = self.focus else {
            return nav_key(event).map(|n| Action::publish(Event::Nav(n)).and_capture());
        };
        let ev = match event {
            keyboard::Event::KeyPressed {
                key,
                modified_key,
                location,
                modifiers,
                text,
                ..
            } if !modifiers.logo() && !super_held() => input::translate_key(
                true,
                key,
                modified_key,
                text.as_ref().and_then(|t| t.chars().next()),
                *modifiers,
                *location,
            )?,
            keyboard::Event::KeyReleased {
                key,
                modified_key,
                location,
                modifiers,
                ..
            } if !modifiers.logo() => {
                input::translate_key(false, key, modified_key, None, *modifiers, *location)?
            }
            _ => return None,
        };
        Some(Action::publish(Event::Input(vec![(tab, ev)])).and_capture())
    }
}

impl canvas::Program<Event> for Overlay<'_> {
    type State = State;

    fn update(
        &self,
        st: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Event>> {
        match event {
            iced::Event::Window(_) => {
                if st.bounds == Some(bounds) {
                    return None;
                }
                st.bounds = Some(bounds);
                Some(Action::publish(Event::Bounds(bounds)))
            }
            iced::Event::Mouse(m) if self.live => self.mouse(st, *m, bounds, cursor),
            iced::Event::Keyboard(k) => {
                track_modifiers(k);
                if !self.live {
                    return None;
                }
                if st.composing {
                    if let keyboard::Event::KeyPressed { key, .. } = k {
                        // Escape cancels the composition; characters are
                        // the IME's (a CHAR would double-insert).
                        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                            st.composing = false;
                            let tab = self.focus?;
                            return Some(
                                Action::publish(Event::Input(vec![(tab, input::ime_cancel())]))
                                    .and_capture(),
                            );
                        }
                        if matches!(key, keyboard::Key::Character(_)) {
                            return Some(Action::capture());
                        }
                    }
                }
                self.keyboard(k)
            }
            iced::Event::InputMethod(ime) if self.live => {
                use iced::advanced::input_method::Event as Ime;
                let tab = self.focus?;
                let ev = match ime {
                    Ime::Opened => return None,
                    Ime::Preedit(text, sel) => {
                        st.composing = !text.is_empty();
                        input::ime_set_composition(text.clone(), sel.clone())
                    }
                    Ime::Commit(text) => {
                        st.composing = false;
                        input::ime_commit(text.clone())
                    }
                    Ime::Closed => {
                        let was = std::mem::take(&mut st.composing);
                        if !was {
                            return None;
                        }
                        input::ime_cancel()
                    }
                };
                Some(Action::publish(Event::Input(vec![(tab, ev)])).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _st: &State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Vec<Geometry> {
        let mark = cursor.position_in(bounds).and_then(|p| self.mark(p));
        let focus = Focus {
            step: self.step,
            hover: match mark {
                Some(Mark::Hot(i)) => Some(i),
                _ => None,
            },
        };
        let p = theme.extended_palette();
        let border = p.background.strong.color;
        let placeholder = p.background.weak.color;
        let label = Color {
            a: 0.75,
            ..p.background.base.text
        };
        let accent = p.primary.base.color;
        let danger = p.danger.base.color;

        let mut frame = Frame::new(renderer, bounds.size());
        for (board, painted) in self.boards.iter().zip(&self.painted) {
            let r = self.camera.rect_to_screen(board.rect());
            if !painted {
                frame.fill_rectangle(r.position(), r.size(), placeholder);
            }
            let focused = self.focus == Some(board.tab.0);
            let (color, width) = if focused { (accent, 2.0) } else { (border, 1.0) };
            frame.stroke_rectangle(
                r.position(),
                r.size(),
                Stroke::default().with_color(color).with_width(width),
            );
            let title_box = Rectangle::new(
                Point::new(r.x, r.y - 22.0),
                Size::new(r.width.max(1.0), 20.0),
            );
            let (title, color) = match &board.error {
                Some(e) => (format!("{} — {e}", board.art.title), danger),
                None => (board.art.title.clone(), label),
            };
            // iced 0.14's `with_clip` clips but does not move the origin.
            frame.with_clip(title_box, |f| {
                f.fill_text(Text {
                    content: title,
                    position: Point::new(title_box.x, title_box.y + 16.0),
                    color,
                    size: 12.0.into(),
                    font: fonts::ui(),
                    align_y: Vertical::Bottom,
                    ..Text::default()
                });
            });
        }
        let heads = annotate::section_titles(&mut frame, &self.scene, self.camera, theme);
        annotate::arrows(&mut frame, &self.scene, self.boards, self.camera, focus, theme);
        annotate::hots(&mut frame, &self.scene, self.boards, self.camera, focus, theme);
        if let Some((tab, rect)) = self.selection {
            if let Some(b) = self.board(tab) {
                let canvas_rect = Rectangle::new(
                    Point::new(b.art.x + rect.x, b.art.y + rect.y),
                    Size::new(rect.width.max(1.0), rect.height.max(1.0)),
                );
                let s = self.camera.rect_to_screen(canvas_rect);
                frame.fill_rectangle(s.position(), s.size(), Color { a: 0.10, ..accent });
                frame.stroke_rectangle(
                    s.position(),
                    s.size(),
                    Stroke::default().with_color(accent).with_width(2.0),
                );
            }
        }
        for lit in &self.glows {
            let Some(b) = self.board(lit.tab) else {
                continue;
            };
            let canvas_rect = match lit.rect {
                // Clip an element to its board: the board shows no more.
                Some(r) => {
                    let el = Rectangle::new(
                        Point::new(b.art.x + r.x, b.art.y + r.y),
                        Size::new(r.width, r.height),
                    );
                    let Some(shown) = el.intersection(&b.rect()) else {
                        continue;
                    };
                    shown
                }
                None => b.rect(),
            };
            glow::draw(&mut frame, self.camera.rect_to_screen(canvas_rect), lit, accent);
        }
        // Notes on top of everything. On a tour step, its notes are open.
        let step_notes: HashSet<usize> = self
            .scene
            .notes
            .iter()
            .filter(|n| self.step.is_some() && n.board.as_deref() == self.step)
            .map(|n| n.n)
            .collect();
        let open = |n: usize| self.notes_open || self.open_notes.contains(&n) || step_notes.contains(&n);
        let hover = match mark {
            Some(Mark::Note(n)) => Some(n),
            _ => None,
        };
        let around = annotate::Around {
            boards: self.boards,
            heads: &heads,
            plane: bounds.size(),
        };
        annotate::notes(&mut frame, &self.scene, self.camera, around, open, hover, theme);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, st: &State, bounds: Rectangle, cursor: Cursor) -> Interaction {
        if st.pan_last.is_some() {
            return Interaction::Grabbing;
        }
        let Some(p) = cursor.position_in(bounds) else {
            return Interaction::default();
        };
        if self.mark(p).is_some() {
            return Interaction::Pointer;
        }
        match self.hit(p) {
            Some(_) if super_held() => Interaction::Crosshair,
            Some(_) => CursorKind::from_u32(self.cursor).to_iced(),
            None => Interaction::default(),
        }
    }
}

/// → ← Esc 1–9 N, without ⌘ / Ctrl / Alt.
fn nav_key(event: &keyboard::Event) -> Option<Nav> {
    use keyboard::Key;
    use keyboard::key::Named;
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    if modifiers.logo() || modifiers.control() || modifiers.alt() || super_held() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowRight) => Some(Nav::Next),
        Key::Named(Named::ArrowLeft) => Some(Nav::Back),
        Key::Named(Named::Escape) => Some(Nav::End),
        Key::Character(c) if c.eq_ignore_ascii_case("n") => Some(Nav::Notes),
        Key::Character(c) => match c.parse::<usize>() {
            Ok(d @ 1..=9) => Some(Nav::Pick(d)),
            _ => None,
        },
        _ => None,
    }
}

/// Fold iced modifier / Super key events into the shared modifier state
/// (`sola_browser::input`), alongside the bus chords the app applies.
fn track_modifiers(event: &keyboard::Event) {
    match event {
        keyboard::Event::ModifiersChanged(m) => input_store(*m),
        keyboard::Event::KeyPressed { key, modifiers, .. } => {
            input_store(*modifiers);
            if sola_browser::input::is_super_key(key) {
                sola_browser::input::note_super_key(true);
            }
        }
        keyboard::Event::KeyReleased { key, modifiers, .. } => {
            input_store(*modifiers);
            if sola_browser::input::is_super_key(key) {
                sola_browser::input::note_super_key(false);
            }
        }
    }
}

fn input_store(m: keyboard::Modifiers) {
    sola_browser::input::store_modifiers(m);
}
