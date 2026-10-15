//! Drawing a canvas's annotations, in screen space. Section regions go
//! under the boards (their own canvas layer, [`Sections`]); section
//! titles, arrows, exit outlines, and notes go over them (called from the
//! overlay). Sizes on screen are fixed: zooming moves them, never scales
//! them. Nothing here is in a page, so nothing here is in a screenshot.

use iced::advanced::text::Paragraph as _;
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, mouse};
use sola_kit::fonts;

use crate::boards::Board;
use crate::camera::Camera;
use crate::routes::{self, Arrow};
use crate::scene::Scene;

const NOTE_RADIUS: f32 = 9.0;
const CARD_TEXT: f32 = 12.0;
const CARD_WIDTH: f32 = 280.0;
const CARD_PAD: [f32; 2] = [9.0, 11.0];
const LABEL_TEXT: f32 = 11.0;
const SECTION_TITLE: f32 = 15.0;
const SECTION_NOTE: f32 = 12.0;

type Para = <Renderer as iced::advanced::text::Renderer>::Paragraph;

/// Laid-out size of `text` wrapped at `max_width`.
fn measure(text: &str, size: f32, font: iced::Font, max_width: f32) -> Size {
    let p = Para::with_text(iced::advanced::Text {
        content: text,
        bounds: Size::new(max_width, f32::INFINITY),
        size: Pixels(size),
        line_height: LineHeight::default(),
        font,
        align_x: iced::advanced::text::Alignment::Default,
        align_y: Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::WordOrGlyph,
    });
    p.min_bounds()
}

fn label(content: &str, at: Point, size: f32, font: iced::Font, color: Color, max_width: f32) -> Text {
    Text {
        content: content.to_string(),
        position: at,
        max_width,
        color,
        size: Pixels(size),
        font,
        shaping: Shaping::Advanced,
        ..Text::default()
    }
}

fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

// ── sections (under the boards) ──────────────────────────────────

/// Section regions: a canvas layer below the board textures.
pub struct Sections {
    pub camera: Camera,
    pub scene: Scene,
}

impl<M> canvas::Program<M> for Sections {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, theme: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let p = theme.extended_palette();
        let fill = alpha(p.background.weak.color, 0.5);
        let line = alpha(p.background.strong.color, 0.9);
        let mut frame = Frame::new(renderer, bounds.size());
        for s in &self.scene.sections {
            let Some(b) = section_box(self.camera, s) else { continue };
            let path = Path::rounded_rectangle(b.position(), b.size(), 14.0.into());
            frame.fill(&path, fill);
            frame.stroke(&path, Stroke::default().with_color(line).with_width(1.0));
        }
        vec![frame.into_geometry()]
    }
}

fn section_box(camera: Camera, s: &crate::scene::SectionMark) -> Option<Rectangle> {
    routes::section_box(s.boards.iter().map(|r| camera.rect_to_screen(*r)), camera.zoom)
}

/// Section titles (and notes) above their regions, over the boards — on a
/// plate, so they read where a zoomed-out row runs under them.
pub fn section_titles(frame: &mut Frame, scene: &Scene, camera: Camera, theme: &Theme) {
    let p = theme.extended_palette();
    let ink = p.background.base.text;
    let plate = alpha(p.background.base.color, 0.88);
    for s in &scene.sections {
        let Some(b) = section_box(camera, s) else { continue };
        let width = b.width.clamp(160.0, 640.0);
        let title = measure(&s.title, SECTION_TITLE, fonts::ui_medium(), width);
        let note = s.note.as_ref().map(|n| (n, measure(n, SECTION_NOTE, fonts::ui(), width)));
        let gap = if note.is_some() { 3.0 } else { 0.0 };
        let note_h = note.as_ref().map_or(0.0, |(_, m)| m.height);
        let height = title.height + gap + note_h;
        let wide = note.as_ref().map_or(title.width, |(_, m)| m.width.max(title.width));
        let top = b.y - 8.0 - height;
        let back = Rectangle::new(Point::new(b.x - 4.0, top - 3.0), Size::new(wide + 12.0, height + 6.0));
        frame.fill(&Path::rounded_rectangle(back.position(), back.size(), 6.0.into()), plate);
        frame.fill_text(label(&s.title, Point::new(b.x + 2.0, top), SECTION_TITLE, fonts::ui_medium(), alpha(ink, 0.92), width));
        if let Some((n, _)) = note {
            let at = Point::new(b.x + 2.0, top + title.height + gap);
            frame.fill_text(label(n, at, SECTION_NOTE, fonts::ui(), alpha(ink, 0.62), width));
        }
    }
}

// ── arrows ───────────────────────────────────────────────────────

/// How the arrows are emphasised: the tour's board (its arrows bright,
/// the rest dim) and the clickable region under the pointer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Focus<'a> {
    pub step: Option<&'a str>,
    pub hover: Option<usize>,
}

pub fn arrows(frame: &mut Frame, scene: &Scene, boards: &[Board], camera: Camera, focus: Focus, theme: &Theme) {
    let p = theme.extended_palette();
    let accent = p.primary.base.color;
    let bg = p.background.base.color;
    let routes_in: Vec<Arrow> = scene
        .arrows
        .iter()
        .map(|a| Arrow {
            from: camera.rect_to_screen(a.from),
            to: camera.rect_to_screen(boards[a.target].rect()),
            target: a.target,
        })
        .collect();
    let curves = routes::route(&routes_in);
    let pills: Vec<Option<Size>> = scene
        .arrows
        .iter()
        .map(|a| {
            let t = measure(a.label.as_deref()?, LABEL_TEXT, fonts::ui(), 220.0);
            Some(Size::new(t.width + 14.0, t.height + 6.0))
        })
        .collect();
    let spots = routes::label_spots(&curves, &pills);
    // Dim ones first, so bright ones cross over them.
    let mut order: Vec<usize> = (0..curves.len()).collect();
    let weight = |i: usize| -> (f32, f32) {
        let a = &scene.arrows[i];
        let hovered = a.hot.is_some() && a.hot == focus.hover;
        match focus.step {
            _ if hovered => (1.0, 2.5),
            Some(step) if a.from_board == step => (1.0, 2.0),
            Some(_) => (0.22, 1.5),
            None => (0.8, 1.5),
        }
    };
    order.sort_by(|&a, &b| weight(a).0.total_cmp(&weight(b).0));
    for i in order {
        let (a, width) = weight(i);
        let c = &curves[i];
        let color = alpha(accent, a);
        let path = Path::new(|b| {
            b.move_to(c.from);
            b.bezier_curve_to(c.c1, c.c2, c.to);
        });
        frame.stroke(&path, Stroke::default().with_color(color).with_width(width));
        frame.fill(&Path::circle(c.from, 2.5 + width / 2.0), color);
        let [tip, l, r] = c.head(9.0 + width, 4.0 + width / 2.0);
        frame.fill(
            &Path::new(|b| {
                b.move_to(tip);
                b.line_to(l);
                b.line_to(r);
                b.close();
            }),
            color,
        );
        if let (Some(text), Some(mid), Some(size)) = (&scene.arrows[i].label, spots[i], pills[i]) {
            let pill = Rectangle::new(
                Point::new(mid.x - size.width / 2.0, mid.y - size.height / 2.0),
                size,
            );
            let shape = Path::rounded_rectangle(pill.position(), pill.size(), 7.0.into());
            frame.fill(&shape, alpha(bg, a.max(0.6)));
            frame.stroke(&shape, Stroke::default().with_color(color).with_width(1.0));
            frame.fill_text(label(text, Point::new(pill.x + 7.0, pill.y + 3.0), LABEL_TEXT, fonts::ui(), alpha(p.background.base.text, a.max(0.4)), 220.0));
        }
    }
}

/// Outline clickable regions: the one under the pointer, and the exits of
/// the tour's board (where it can go from here).
pub fn hots(frame: &mut Frame, scene: &Scene, boards: &[Board], camera: Camera, focus: Focus, theme: &Theme) {
    let accent = theme.extended_palette().primary.base.color;
    for (i, h) in scene.hots.iter().enumerate() {
        let hovered = focus.hover == Some(i);
        let step = h.exit && focus.step == Some(h.board.as_str());
        if !hovered && !step {
            continue;
        }
        let Some(b) = boards.iter().find(|b| b.tab.0 == h.tab) else { continue };
        let canvas = Rectangle::new(b.rect().position() + iced::Vector::new(h.local.x, h.local.y), h.local.size());
        let r = camera.rect_to_screen(canvas).expand(2.0);
        let shape = Path::rounded_rectangle(r.position(), r.size(), 4.0.into());
        if hovered {
            frame.fill(&shape, alpha(accent, 0.1));
        }
        let (a, w) = if hovered { (1.0, 1.5) } else { (0.55, 1.0) };
        frame.stroke(&shape, Stroke::default().with_color(alpha(accent, a)).with_width(w));
    }
}

// ── notes ────────────────────────────────────────────────────────

/// The note marker under screen point `p` (its number).
pub fn note_at(scene: &Scene, camera: Camera, p: Point) -> Option<usize> {
    scene
        .notes
        .iter()
        .rev()
        .find(|n| camera.to_screen(n.at).distance(p) <= NOTE_RADIUS + 2.0)
        .map(|n| n.n)
}

/// Markers for every note; cards for the open ones (`open`) and the one
/// under the pointer (drawn last, on top).
pub fn notes(frame: &mut Frame, scene: &Scene, camera: Camera, plane: Size, open: impl Fn(usize) -> bool, hover: Option<usize>, theme: &Theme) {
    let p = theme.extended_palette();
    let mark = p.warning.base.color;
    let ring = p.background.base.color;
    for n in &scene.notes {
        let at = camera.to_screen(n.at);
        let dot = Path::circle(at, NOTE_RADIUS);
        frame.fill(&dot, mark);
        frame.stroke(&dot, Stroke::default().with_color(ring).with_width(1.5));
        frame.fill_text(Text {
            content: n.n.to_string(),
            position: at,
            color: p.warning.base.text,
            size: Pixels(11.0),
            font: fonts::ui_medium(),
            align_x: iced::widget::text::Alignment::Center,
            align_y: Vertical::Center,
            ..Text::default()
        });
    }
    let cards = scene
        .notes
        .iter()
        .filter(|n| open(n.n) && hover != Some(n.n))
        .chain(scene.notes.iter().filter(|n| hover == Some(n.n)));
    for n in cards {
        let at = camera.to_screen(n.at);
        let text = measure(&n.text, CARD_TEXT, fonts::ui(), CARD_WIDTH);
        let size = Size::new(text.width + 2.0 * CARD_PAD[1], text.height + 2.0 * CARD_PAD[0]);
        let tl = routes::card_at(at, size, plane);
        let shape = Path::rounded_rectangle(tl, size, 8.0.into());
        frame.fill(&shape, p.background.weak.color);
        frame.stroke(&shape, Stroke::default().with_color(alpha(mark, 0.75)).with_width(1.0));
        frame.fill_text(label(
            &n.text,
            Point::new(tl.x + CARD_PAD[1], tl.y + CARD_PAD[0]),
            CARD_TEXT,
            fonts::ui(),
            p.background.weak.text,
            CARD_WIDTH,
        ));
    }
}
