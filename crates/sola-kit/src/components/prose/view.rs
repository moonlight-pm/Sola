//! Selectable letter renderer. Drag-select copies visible text; links
//! still click. Selection lives in the widget tree; the parent is told
//! the selected string (and can force Select All via a generation).

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Highlight, Paragraph as _, Span, Text};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::mouse;
use iced::widget::text::{LineHeight, Wrapping};
use iced::{
    Background, Border, Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Theme,
};

use super::{
    LayoutLine, ProseBlock, ProseRun, iter_lines, selected_visible, snap_byte, visible_text,
    word_at,
};
use crate::components::style::{HAIRLINE_A, SPACE_LG, SPACE_MD, mix_white};
use crate::components::text::PROSE_SIZE;
use crate::fonts;

const DRAG_SLOP: f32 = 3.0;
const MULTI_CLICK: Duration = Duration::from_millis(400);
const LINE_HEIGHT: LineHeight = LineHeight::Relative(1.45);

/// Letter column: paragraphs, quotes, inline links. Selection is local
/// unless [`prose_selectable`] is used.
pub fn prose<'a, Message: Clone + 'a>(
    blocks: impl IntoIterator<Item = ProseBlock>,
    theme: &Theme,
    on_link: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message, Theme> {
    ProseView::new(blocks, theme, on_link).into()
}

/// Same as [`prose`], plus selection callbacks.
///
/// `select_all` is a generation: bump it to select the whole letter
/// (Edit → Select All). `on_select` receives the visible selected text,
/// or `None` when the caret collapses.
pub fn prose_selectable<'a, Message: Clone + 'a>(
    blocks: impl IntoIterator<Item = ProseBlock>,
    theme: &Theme,
    select_all: u64,
    on_link: impl Fn(String) -> Message + 'a,
    on_select: impl Fn(Option<String>) -> Message + 'a,
) -> Element<'a, Message, Theme> {
    ProseView::new(blocks, theme, on_link)
        .select_all(select_all)
        .on_select(on_select)
        .into()
}

struct ProseView<'a, Message> {
    blocks: Vec<ProseBlock>,
    select_all: u64,
    on_link: Box<dyn Fn(String) -> Message + 'a>,
    on_select: Option<Box<dyn Fn(Option<String>) -> Message + 'a>>,
    link: Color,
    quote_ink: Color,
    sel_bg: Color,
    ink: Color,
}

impl<'a, Message> ProseView<'a, Message> {
    fn new(
        blocks: impl IntoIterator<Item = ProseBlock>,
        theme: &Theme,
        on_link: impl Fn(String) -> Message + 'a,
    ) -> Self {
        let p = theme.extended_palette();
        Self {
            blocks: blocks.into_iter().collect(),
            select_all: 0,
            on_link: Box::new(on_link),
            on_select: None,
            link: p.primary.base.color,
            quote_ink: p.secondary.base.text,
            sel_bg: crate::theme::selection(),
            ink: p.background.base.text,
        }
    }

    fn select_all(mut self, generation: u64) -> Self {
        self.select_all = generation;
        self
    }

    fn on_select(mut self, f: impl Fn(Option<String>) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }
}

struct LineState<P: text::Paragraph> {
    paragraph: P,
    spans: Vec<Span<'static, String, P::Font>>,
    start: usize,
    text: String,
    quote: bool,
}

struct State<P: text::Paragraph> {
    lines: Vec<LineState<P>>,
    doc: String,
    sel: Option<(usize, usize)>,
    dragging: bool,
    drag_origin: Option<Point>,
    anchor: usize,
    select_all_seen: u64,
    last_click: Option<(Instant, usize)>,
    click_count: u8,
    hovered_link: Option<String>,
    link_pressed: Option<String>,
}

impl<P: text::Paragraph> Default for State<P> {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            doc: String::new(),
            sel: None,
            dragging: false,
            drag_origin: None,
            anchor: 0,
            select_all_seen: 0,
            last_click: None,
            click_count: 0,
            hovered_link: None,
            link_pressed: None,
        }
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for ProseView<'_, Message>
where
    Message: Clone,
    Renderer: text::Renderer<Font = iced::Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();
        let width = limits.max().width;
        let plan = iter_lines(&self.blocks);
        let doc = visible_text(&self.blocks);
        if state.doc != doc {
            state.doc = doc;
            state.sel = None;
            state.dragging = false;
            state.link_pressed = None;
            state.click_count = 0;
        }
        if self.select_all != 0 && self.select_all != state.select_all_seen {
            state.select_all_seen = self.select_all;
            if !state.doc.is_empty() {
                state.sel = Some((0, state.doc.len()));
            }
        }

        let mut y = 0.0;
        let mut children = Vec::with_capacity(plan.len());
        state.lines.clear();
        for (i, line) in plan.iter().enumerate() {
            let x = if line.quote { 1.0 + SPACE_MD } else { 0.0 };
            let line_w = (width - x).max(0.0);
            let spans = spans_for_line(
                line,
                state.sel,
                self.ink,
                self.quote_ink,
                self.link,
                self.sel_bg,
            );
            let para = layout_paragraph(renderer, line_w, &spans);
            let size = para.min_bounds();
            state.lines.push(LineState {
                paragraph: para,
                spans,
                start: line.start,
                text: line.text.clone(),
                quote: line.quote,
            });
            children.push(layout::Node::new(size).move_to(Point::new(x, y)));
            y += size.height;
            if i + 1 < plan.len() && line.gap >= 2 {
                y += SPACE_LG;
            }
        }
        layout::Node::with_children(Size::new(width, y), children)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();

        let cursor_pos = cursor.position().or_else(|| {
            if state.dragging {
                cursor.land().position()
            } else {
                None
            }
        });

        let was_link = state.hovered_link.is_some();
        state.hovered_link = None;
        if let Some(pos) = cursor.position_in(bounds) {
            let abs = Point::new(bounds.x + pos.x, bounds.y + pos.y);
            if let Some((i, local)) = line_at(layout, abs) {
                if let Some(line) = state.lines.get(i) {
                    if let Some(span_i) = line.paragraph.hit_span(local) {
                        if let Some(url) = line.spans.get(span_i).and_then(|s| s.link.clone()) {
                            state.hovered_link = Some(url);
                        }
                    }
                }
            }
        }
        if was_link != state.hovered_link.is_some() {
            shell.request_redraw();
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor
                    .position_over(bounds)
                    .or_else(|| cursor.position().filter(|p| bounds.contains(*p)))
                else {
                    return;
                };
                let offset = hit_offset(state, layout, point);
                let now = Instant::now();
                let count = match state.last_click {
                    Some((t, o))
                        if now.duration_since(t) < MULTI_CLICK && o.abs_diff(offset) <= 2 =>
                    {
                        (state.click_count % 3) + 1
                    }
                    _ => 1,
                };
                state.last_click = Some((now, offset));
                state.click_count = count;
                state.anchor = offset;
                state.dragging = true;
                state.drag_origin = Some(point);
                state.link_pressed = if count == 1 {
                    state.hovered_link.clone()
                } else {
                    None
                };
                match count {
                    2 => {
                        let w = word_at(&state.doc, offset);
                        state.sel = Some(w);
                        publish_sel(self, state, shell);
                    }
                    3 => {
                        if let Some(line) = line_covering(state, offset) {
                            state.sel = Some((line.0, line.1));
                            publish_sel(self, state, shell);
                        }
                    }
                    _ => {
                        state.sel = Some((offset, offset));
                    }
                }
                shell.request_redraw();
                shell.invalidate_layout();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    return;
                }
                let Some(point) = cursor_pos else {
                    return;
                };
                let Some(origin) = state.drag_origin else {
                    return;
                };
                if origin.distance(point) <= DRAG_SLOP && state.click_count == 1 {
                    return;
                }
                state.link_pressed = None;
                let focus = hit_offset(state, layout, point);
                state.sel = Some(match state.click_count {
                    2 => {
                        let (a1, a2) = word_at(&state.doc, state.anchor);
                        let (f1, f2) = word_at(&state.doc, focus);
                        (a1.min(f1), a2.max(f2))
                    }
                    3 => {
                        let a = line_covering(state, state.anchor)
                            .unwrap_or((state.anchor, state.anchor));
                        let f = line_covering(state, focus).unwrap_or((focus, focus));
                        (a.0.min(f.0), a.1.max(f.1))
                    }
                    _ => (state.anchor, focus),
                });
                publish_sel(self, state, shell);
                shell.request_redraw();
                shell.invalidate_layout();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return;
                }
                state.dragging = false;
                if let Some(url) = state.link_pressed.take() {
                    if !has_range(state.sel) {
                        state.sel = None;
                        shell.publish((self.on_link)(url));
                        shell.request_redraw();
                        return;
                    }
                }
                if !has_range(state.sel) {
                    state.sel = None;
                    publish_sel(self, state, shell);
                }
                shell.request_redraw();
                shell.invalidate_layout();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let children: Vec<_> = layout.children().collect();

        draw_quote_rules(renderer, state, &children, viewport, theme);

        for (i, child) in children.iter().enumerate() {
            let Some(line) = state.lines.get(i) else {
                continue;
            };
            if !child.bounds().intersects(viewport) {
                continue;
            }
            let translation = child.bounds().position() - Point::ORIGIN;
            for (index, span) in line.spans.iter().enumerate() {
                let regions = line.paragraph.span_bounds(index);
                if let Some(highlight) = span.highlight {
                    for bounds in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: *bounds + translation,
                                border: highlight.border,
                                ..Default::default()
                            },
                            highlight.background,
                        );
                    }
                }
                if span.underline {
                    let size = span.size.unwrap_or(Pixels(PROSE_SIZE));
                    let line_height = span.line_height.unwrap_or(LINE_HEIGHT).to_absolute(size);
                    let color = span.color.unwrap_or(self.link);
                    let baseline = translation
                        + iced::Vector::new(0.0, size.0 + (line_height.0 - size.0) / 2.0);
                    for bounds in &regions {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    bounds.position() + baseline
                                        - iced::Vector::new(0.0, size.0 * 0.08),
                                    Size::new(bounds.width, 1.0),
                                ),
                                ..Default::default()
                            },
                            color,
                        );
                    }
                }
            }
            let color = if line.quote {
                self.quote_ink
            } else {
                defaults.text_color
            };
            renderer.fill_paragraph(&line.paragraph, child.bounds().position(), color, *viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        // iced row/column take the *max* child interaction, and Text outranks
        // Pointer. Without a bounds check the I-bar leaks over sibling panes
        // (mail's message list, chrome, …).
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::None;
        }
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        if state.hovered_link.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Text
        }
    }
}

impl<'a, Message: Clone + 'a> From<ProseView<'a, Message>> for Element<'a, Message, Theme> {
    fn from(value: ProseView<'a, Message>) -> Self {
        Element::new(value)
    }
}

fn publish_sel<Message, P: text::Paragraph>(
    view: &ProseView<'_, Message>,
    state: &State<P>,
    shell: &mut Shell<'_, Message>,
) {
    let Some(on_select) = &view.on_select else {
        return;
    };
    let text = match state.sel {
        Some((a, b)) if a != b => Some(selected_visible(&view.blocks, a, b)),
        _ => None,
    };
    shell.publish(on_select(text));
}

fn has_range(sel: Option<(usize, usize)>) -> bool {
    matches!(sel, Some((a, b)) if a != b)
}

fn line_covering<P: text::Paragraph>(state: &State<P>, offset: usize) -> Option<(usize, usize)> {
    state.lines.iter().find_map(|l| {
        let end = l.start + l.text.len();
        if offset >= l.start && offset <= end {
            Some((l.start, end))
        } else {
            None
        }
    })
}

fn line_at(layout: Layout<'_>, point: Point) -> Option<(usize, Point)> {
    for (i, child) in layout.children().enumerate() {
        let b = child.bounds();
        if b.contains(point) {
            return Some((i, Point::new(point.x - b.x, point.y - b.y)));
        }
    }
    None
}

fn hit_offset<P: text::Paragraph>(state: &State<P>, layout: Layout<'_>, point: Point) -> usize {
    let children: Vec<_> = layout.children().collect();
    if children.is_empty() {
        return 0;
    }
    if point.y < children[0].bounds().y {
        return 0;
    }
    let last_b = children[children.len() - 1].bounds();
    if point.y > last_b.y + last_b.height {
        return state.doc.len();
    }
    for (i, child) in children.iter().enumerate() {
        let b = child.bounds();
        let next_y = children
            .get(i + 1)
            .map(|c| c.bounds().y)
            .unwrap_or(b.y + b.height);
        let in_line = point.y >= b.y && point.y < b.y + b.height;
        let in_gap = point.y >= b.y + b.height && point.y < next_y;
        if in_line {
            return offset_in_line(state, i, b, point);
        }
        if in_gap {
            let mid = (b.y + b.height + next_y) * 0.5;
            if point.y < mid {
                let line = &state.lines[i];
                return line.start + line.text.len();
            }
            if let Some(next) = state.lines.get(i + 1) {
                return next.start;
            }
        }
    }
    state.doc.len()
}

fn offset_in_line<P: text::Paragraph>(
    state: &State<P>,
    i: usize,
    bounds: Rectangle,
    point: Point,
) -> usize {
    let Some(line) = state.lines.get(i) else {
        return 0;
    };
    let local = Point::new(point.x - bounds.x, point.y - bounds.y);
    if let Some(hit) = line.paragraph.hit_test(local) {
        let idx = snap_byte(&line.text, hit.cursor().min(line.text.len()));
        return snap_byte(&state.doc, line.start + idx);
    }
    if point.x <= bounds.x {
        line.start
    } else {
        line.start + line.text.len()
    }
}

fn layout_paragraph<Renderer: text::Renderer<Font = iced::Font>>(
    _renderer: &Renderer,
    width: f32,
    spans: &[Span<'static, String, Renderer::Font>],
) -> Renderer::Paragraph {
    let size = Pixels(PROSE_SIZE);
    let font = fonts::ui();
    Renderer::Paragraph::with_spans(Text {
        content: spans,
        bounds: Size::new(width, f32::INFINITY),
        size,
        line_height: LINE_HEIGHT,
        font,
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: Wrapping::Word,
    })
}

fn spans_for_line(
    line: &LayoutLine,
    sel: Option<(usize, usize)>,
    ink: Color,
    quote_ink: Color,
    link: Color,
    sel_bg: Color,
) -> Vec<Span<'static, String, iced::Font>> {
    let ink = if line.quote { quote_ink } else { ink };
    let sel = sel.and_then(|(a, b)| {
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == hi { None } else { Some((lo, hi)) }
    });
    let mut out = Vec::new();
    let mut cur = line.start;
    if line.runs.is_empty() {
        return out;
    }
    for run in &line.runs {
        let rs = cur;
        let re = cur + run.text.len();
        cur = re;
        match sel {
            Some((lo, hi)) if hi > rs && lo < re => {
                let a = lo.clamp(rs, re) - rs;
                let b = hi.clamp(rs, re) - rs;
                let a = snap_byte(&run.text, a);
                let b = snap_byte(&run.text, b);
                if a > 0 {
                    out.push(styled_span(&run.text[..a], run, ink, link, None));
                }
                if b > a {
                    out.push(styled_span(&run.text[a..b], run, ink, link, Some(sel_bg)));
                }
                if b < run.text.len() {
                    out.push(styled_span(&run.text[b..], run, ink, link, None));
                }
            }
            _ => out.push(styled_span(&run.text, run, ink, link, None)),
        }
    }
    out
}

fn styled_span(
    text: &str,
    run: &ProseRun,
    ink: Color,
    link: Color,
    highlight: Option<Color>,
) -> Span<'static, String, iced::Font> {
    let mut s = Span::new(text.to_string())
        .font(fonts::ui())
        .size(PROSE_SIZE)
        .line_height(LINE_HEIGHT);
    if run.url.is_some() {
        s = s.link(run.url.clone().unwrap()).underline(true).color(link);
    } else {
        s = s.color(ink);
    }
    if let Some(bg) = highlight {
        s.highlight = Some(Highlight {
            background: Background::Color(bg),
            border: Border::default(),
        });
    }
    s
}

fn draw_quote_rules<Renderer, P>(
    renderer: &mut Renderer,
    state: &State<P>,
    children: &[Layout<'_>],
    viewport: &Rectangle,
    theme: &Theme,
) where
    Renderer: renderer::Renderer,
    P: text::Paragraph,
{
    let mut i = 0;
    while i < state.lines.len() {
        if !state.lines[i].quote {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < state.lines.len() && state.lines[i].quote {
            i += 1;
        }
        let Some(first) = children.get(start) else {
            continue;
        };
        let last = children.get(i - 1).unwrap_or(first);
        let top = first.bounds().y;
        let bottom = last.bounds().y + last.bounds().height;
        let x = first.bounds().x - SPACE_MD - 1.0;
        let bounds = Rectangle::new(Point::new(x, top), Size::new(1.0, (bottom - top).max(1.0)));
        if !bounds.intersects(viewport) {
            continue;
        }
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..Default::default()
            },
            Background::Color(mix_white(
                theme.extended_palette().background.base.color,
                HAIRLINE_A + 0.06,
            )),
        );
    }
}
