//! Use as Desktop Background, in the page's window: right-click a picture,
//! and the browser finds its largest version (`crate::wallpaper`). When
//! its shape differs from the screen's, a lightbox shows it with a frame in
//! the screen's shape: drag the frame to choose what fills the screen,
//! scroll to take less or more of the picture, Return to use it, Esc to
//! leave. The chosen part is saved under `~/Pictures/Backgrounds/` and
//! becomes the background (`Topic::Background`); the shell whispers that
//! it did.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{Space, column, container, image, mouse_area, row, text};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Task, Theme, Vector, mouse};
use sola_bus::topics::{AppToast, Background, Topic};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};

use super::{App, Msg};
use crate::engine::Engine;
use crate::wallpaper::{self as wp, Crop, Picture};

/// What the lightbox is doing.
pub enum Lightbox {
    /// Fetching the candidates and decoding the largest.
    Finding,
    /// Choosing the part that fills the screen.
    Choosing {
        pic: Arc<Picture>,
        preview: image::Handle,
        crop: Crop,
        /// The screen's width over its height, and its size.
        screen: f32,
        size: (u32, u32),
    },
    Saving,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum Event {
    Found(Result<Arc<Picture>, String>),
    /// The frame, moved by a drag.
    Set(Crop),
    /// Take this much of the frame (scroll).
    Zoom(f32),
    Use,
    Cancel,
    Saved(Result<PathBuf, String>),
    /// A click on the veil: it stops there, nothing else.
    Veil,
}

/// The screen when sola-river has not said (`Topic::OutputGeometry`).
const FALLBACK_SCREEN: (u32, u32) = (1920, 1080);
/// Around the picture in the lightbox.
const MARGIN: f32 = 28.0;
/// One wheel notch takes this much less (or more) of the picture.
const ZOOM_STEP: f32 = 0.92;
/// The window's frame, which the lightbox stays inside.
const TITLEBAR: f32 = sola_kit::components::window_frame::HEIGHT;
const WINDOW_RADIUS: f32 = sola_kit::components::window_frame::RADIUS;
const RIM: f32 = 1.0;

impl<E: Engine> App<E> {
    /// The page menu's Use as Desktop Background on the picture `src`
    /// (inside link `link`, if any).
    pub(crate) fn wallpaper_start(&mut self, src: String, link: Option<String>) -> Task<Msg> {
        let referer = self.active_tab_info().map(|t| t.url.clone()).unwrap_or_default();
        let candidates = wp::candidates(&src, link.as_deref());
        tracing::info!(?candidates, "use as desktop background");
        self.wallpaper = Some(Lightbox::Finding);
        wp::set_open(true);
        Task::perform(wp::find_and_decode(candidates, referer), |r| {
            Msg::Wallpaper(Event::Found(r))
        })
    }

    pub(crate) fn on_wallpaper(&mut self, event: Event) -> Task<Msg> {
        match event {
            Event::Found(Ok(pic)) => {
                if self.wallpaper.is_none() {
                    return Task::none();
                }
                let size = self.screen_size.unwrap_or(FALLBACK_SCREEN);
                let screen = wp::aspect(size.0, size.1);
                if wp::same_shape(pic.width, pic.height, screen) {
                    return self.wallpaper_save(pic, None);
                }
                let (pw, ph, rgba) = pic.preview.clone();
                self.wallpaper = Some(Lightbox::Choosing {
                    preview: image::Handle::from_rgba(pw, ph, rgba),
                    crop: wp::fit(pic.width, pic.height, screen),
                    pic,
                    screen,
                    size,
                });
            }
            Event::Found(Err(e)) => self.wallpaper = Some(Lightbox::Failed(e)),
            Event::Set(c) => {
                if let Some(Lightbox::Choosing { crop, .. }) = &mut self.wallpaper {
                    *crop = c;
                }
            }
            Event::Zoom(k) => {
                if let Some(Lightbox::Choosing {
                    pic, crop, screen, ..
                }) = &mut self.wallpaper
                {
                    *crop = wp::zoomed(*crop, k, pic.width, pic.height, *screen);
                }
            }
            Event::Use => {
                if let Some(Lightbox::Choosing { pic, crop, .. }) = &self.wallpaper {
                    let (pic, crop) = (pic.clone(), *crop);
                    return self.wallpaper_save(pic, Some(crop));
                }
            }
            Event::Cancel => self.wallpaper_close(),
            Event::Saved(Ok(path)) => {
                self.wallpaper_close();
                emit_background(&path);
            }
            Event::Saved(Err(e)) => self.wallpaper = Some(Lightbox::Failed(e)),
            Event::Veil => {}
        }
        Task::none()
    }

    fn wallpaper_save(&mut self, pic: Arc<Picture>, crop: Option<Crop>) -> Task<Msg> {
        self.wallpaper = Some(Lightbox::Saving);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || wp::save(&pic, crop))
                    .await
                    .map_err(|e| e.to_string())?
            },
            |r| Msg::Wallpaper(Event::Saved(r)),
        )
    }

    fn wallpaper_close(&mut self) {
        self.wallpaper = None;
        wp::set_open(false);
    }

    /// The lightbox over the page, while one is up.
    pub(crate) fn view_wallpaper(&self) -> Option<Element<'_, Msg>> {
        let lightbox = self.wallpaper.as_ref()?;
        let inner: Element<'_, Msg> = match lightbox {
            Lightbox::Finding => note("Finding the largest version of the picture…", None),
            Lightbox::Saving => note("Saving it as the desktop background…", None),
            Lightbox::Failed(e) => note(e, Some(Msg::Wallpaper(Event::Cancel))),
            Lightbox::Choosing {
                pic,
                preview,
                crop,
                size,
                ..
            } => {
                let chooser = Chooser {
                    width: pic.width,
                    height: pic.height,
                    crop: *crop,
                    label: format!("{} × {} of {} × {}", crop.w.round(), crop.h.round(), pic.width, pic.height),
                };
                // The picture, then the frame over it: in one canvas iced
                // draws fills before images, so the picture would cover
                // the frame. A stack puts the frame in a layer of its own.
                let picture = Canvas::new(Picture2 {
                    preview: preview.clone(),
                    width: pic.width,
                    height: pic.height,
                })
                .width(Length::Fill)
                .height(Length::Fill);
                let chooser: Element<'_, Msg> = iced::widget::stack![
                    picture,
                    Canvas::new(chooser).width(Length::Fill).height(Length::Fill)
                ]
                .into();
                let bar = row![
                    text(format!(
                        "Drag the frame to choose what fills your {} × {} screen. Scroll to take less or more.",
                        size.0, size.1
                    ))
                    .size(13)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.72)),
                    Space::new().width(Length::Fill),
                    kit_button::labeled("Cancel", kit_button::ghost).on_press(Msg::Wallpaper(Event::Cancel)),
                    kit_button::labeled("Use as Desktop Background", kit_button::primary)
                        .on_press(Msg::Wallpaper(Event::Use)),
                ]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center)
                .padding([SPACE_MD, MARGIN]);
                column![chooser, bar].into()
            }
        };
        // The page goes under an opaque near-black veil (a translucent one
        // does not cover the page); clicks stop here. Drawn as an overlay —
        // over the page's shader, like the page menu — below the titlebar
        // and inside the rim, its bottom corners the window's.
        let veil = mouse_area(container(inner).width(Length::Fill).height(Length::Fill).style(|_| {
            container::Style {
                background: Some(iced::Background::Color(Color::from_rgb(0.035, 0.035, 0.035))),
                border: iced::Border {
                    radius: iced::border::bottom(WINDOW_RADIUS - RIM),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            }
        }))
        .on_press(Msg::Wallpaper(Event::Veil));
        Some(over(veil))
    }
}

/// A line in a small card in the middle; with `close`, a Close button.
fn note<'a>(line: &str, close: Option<Msg>) -> Element<'a, Msg> {
    let mut body = column![text(line.to_string()).size(13)].spacing(SPACE_SM);
    if let Some(msg) = close {
        body = body.push(kit_button::labeled("Close", kit_button::secondary).on_press(msg));
    }
    container(card::modal(container(body.width(Length::Fixed(300.0))).padding(SPACE_MD + SPACE_SM)))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// Tell sola-river the new picture and the shell to say so.
fn emit_background(path: &std::path::Path) {
    let set_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let Ok(mut bus) = sola_kit::app::bus().lock() else {
        tracing::warn!("desktop background: bus unavailable");
        return;
    };
    let background = Topic::Background(Background {
        path: path.to_string_lossy().into_owned(),
        set_at,
    });
    if let Err(e) = bus.emit(background) {
        tracing::warn!(error = %e, "emit background failed");
        return;
    }
    let _ = bus.emit(Topic::AppToast(AppToast {
        text: "Set as desktop background".into(),
    }));
}

/// The picture, fitted in the lightbox, with the frame in the screen's
/// shape over it; outside the frame is dimmed.
struct Chooser {
    /// The picture's own size (the crop is in its pixels).
    width: u32,
    height: u32,
    crop: Crop,
    label: String,
}

/// Where a `width × height` picture sits in `bounds`, and its screen
/// pixels per picture pixel.
fn placed(width: u32, height: u32, bounds: Size) -> (Rectangle, f32) {
    let (w, h) = (width as f32, height as f32);
    let room = Size::new((bounds.width - 2.0 * MARGIN).max(1.0), (bounds.height - MARGIN).max(1.0));
    let s = (room.width / w).min(room.height / h);
    let size = Size::new(w * s, h * s);
    let at = Point::new((bounds.width - size.width) / 2.0, MARGIN + (room.height - size.height) / 2.0);
    (Rectangle::new(at, size), s)
}

/// The picture alone, under the [`Chooser`].
struct Picture2 {
    preview: image::Handle,
    width: u32,
    height: u32,
}

impl canvas::Program<Msg> for Picture2 {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let (at, _) = placed(self.width, self.height, bounds.size());
        f.draw_image(at, canvas::Image::new(self.preview.clone()));
        vec![f.into_geometry()]
    }
}

impl Chooser {
    fn placed(&self, bounds: Size) -> (Rectangle, f32) {
        placed(self.width, self.height, bounds)
    }

    fn frame(&self, picture: Rectangle, s: f32) -> Rectangle {
        Rectangle {
            x: picture.x + self.crop.x * s,
            y: picture.y + self.crop.y * s,
            width: self.crop.w * s,
            height: self.crop.h * s,
        }
    }
}

/// A drag in progress: where it started, and the frame then.
#[derive(Default)]
pub struct Drag(Option<(Point, Crop)>);

impl canvas::Program<Msg> for Chooser {
    type State = Drag;

    fn update(
        &self,
        state: &mut Drag,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Msg>> {
        let (picture, s) = self.placed(bounds.size());
        let frame = self.frame(picture, s);
        let at = cursor.position_in(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = at?;
                // Anywhere on the picture grabs the frame.
                if picture.contains(p) || frame.contains(p) {
                    state.0 = Some((p, self.crop));
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (start, from) = state.0?;
                let p = cursor.position_in(bounds).or(cursor.position().map(|q| {
                    Point::new(q.x - bounds.x, q.y - bounds.y)
                }))?;
                let moved = wp::moved(from, (p.x - start.x) / s, (p.y - start.y) / s, self.width, self.height);
                Some(canvas::Action::publish(Msg::Wallpaper(Event::Set(moved))).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.0.take().map(|_| canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                at?;
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if y == 0.0 {
                    return None;
                }
                // Up takes less of the picture (closer in).
                let k = ZOOM_STEP.powf(y);
                Some(canvas::Action::publish(Msg::Wallpaper(Event::Zoom(k))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Drag,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let (picture, s) = self.placed(bounds.size());
        let frame = self.frame(picture, s);
        // Dim what is left out: above, below, left, right of the frame.
        let dim = Color::from_rgba(0.0, 0.0, 0.0, 0.72);
        let r = |x: f32, y: f32, w: f32, h: f32| {
            if w > 0.0 && h > 0.0 {
                Some(Path::rectangle(Point::new(x, y), Size::new(w, h)))
            } else {
                None
            }
        };
        for p in [
            r(picture.x, picture.y, picture.width, frame.y - picture.y),
            r(picture.x, frame.y + frame.height, picture.width, picture.y + picture.height - frame.y - frame.height),
            r(picture.x, frame.y, frame.x - picture.x, frame.height),
            r(frame.x + frame.width, frame.y, picture.x + picture.width - frame.x - frame.width, frame.height),
        ]
        .into_iter()
        .flatten()
        {
            f.fill(&p, dim);
        }
        // Thirds, faint, then the frame.
        let thirds = Color::from_rgba(1.0, 1.0, 1.0, if state.0.is_some() { 0.32 } else { 0.18 });
        for i in 1..3 {
            let t = i as f32 / 3.0;
            let x = frame.x + frame.width * t;
            let y = frame.y + frame.height * t;
            f.stroke(&Path::line(Point::new(x, frame.y), Point::new(x, frame.y + frame.height)), Stroke::default().with_color(thirds).with_width(1.0));
            f.stroke(&Path::line(Point::new(frame.x, y), Point::new(frame.x + frame.width, y)), Stroke::default().with_color(thirds).with_width(1.0));
        }
        f.stroke(
            &Path::rectangle(frame.position(), frame.size()),
            Stroke::default().with_color(Color::WHITE).with_width(1.5),
        );
        f.fill_text(Text {
            content: self.label.clone(),
            position: Point::new(frame.x + 10.0, frame.y + 8.0),
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.85),
            size: 12.0.into(),
            font: sola_kit::fonts::MONO,
            ..Text::default()
        });
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, state: &Drag, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.0.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let (picture, _) = self.placed(bounds.size());
        match cursor.position_in(bounds) {
            Some(p) if picture.contains(p) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }
}

/// `content` over the whole window, above everything the window draws —
/// the page included — as an iced overlay. Takes no room in the layout.
fn over<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    Element::new(Over {
        content: content.into(),
    })
}

struct Over<'a> {
    content: Element<'a, Msg>,
}

impl Widget<Msg, Theme, Renderer> for Over<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, _limits: &layout::Limits) -> layout::Node {
        layout::Node::new(Size::ZERO)
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        _layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'b, Msg, Theme, Renderer>> {
        Some(overlay::Element::new(Box::new(OverLayer {
            content: &mut self.content,
            tree: &mut tree.children[0],
        })))
    }
}

struct OverLayer<'a, 'b> {
    content: &'b mut Element<'a, Msg>,
    tree: &'b mut Tree,
}

impl overlay::Overlay<Msg, Theme, Renderer> for OverLayer<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        // Below the titlebar, inside the rim: the window keeps its frame.
        let inner = Size::new(
            (bounds.width - 2.0 * RIM).max(0.0),
            (bounds.height - TITLEBAR - RIM).max(0.0),
        );
        let node = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, inner));
        layout::Node::with_children(inner, vec![node]).translate(Vector::new(RIM, TITLEBAR))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let child = layout.children().next().expect("over: content");
        self.content
            .as_widget()
            .draw(self.tree, renderer, theme, style, child, cursor, &layout.bounds());
    }

    fn update(
        &mut self,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
    ) {
        let bounds = layout.bounds();
        let child = layout.children().next().expect("over: content");
        self.content
            .as_widget_mut()
            .update(self.tree, event, child, cursor, renderer, clipboard, shell, &bounds);
        // Nothing under the lightbox gets the pointer (the page least).
        if matches!(event, iced::Event::Mouse(_)) {
            shell.capture_event();
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        let child = layout.children().next().expect("over: content");
        self.content
            .as_widget()
            .mouse_interaction(self.tree, child, cursor, &layout.bounds(), renderer)
    }
}
