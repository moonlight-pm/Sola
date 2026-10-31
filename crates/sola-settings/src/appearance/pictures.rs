//! Background picture: the one set, and every picture Sola keeps
//! (`sola_core::background::kept`) as a gallery. A press sets one again
//! (the shell puts back the look it was worn with); × forgets one.
//! Thumbnails are made off the UI thread one at a time: a picture can be
//! tens of megapixels.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::widget::{Space, button, container, hover, image, svg};
use iced::{Background as Fill, Border, Color, ContentFit, Element, Length, Task, Theme};
use sola_bus::topics::{Background, Topic};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_SM, SPACE_XS, alpha};
use sola_kit::components::text as kit_text;

use super::{AppearanceMsg, AppearanceState, send};
use crate::page;

/// A thumbnail's size on the page (logical px); made at twice that.
const THUMB_W: u32 = 128;
const THUMB_H: u32 = 72;
const RADIUS: f32 = 8.0;

/// The kept pictures and their thumbnails.
#[derive(Default)]
pub struct Gallery {
    /// Newest first.
    kept: Vec<PathBuf>,
    /// Made thumbnails; `None` for a picture that would not decode.
    thumbs: HashMap<PathBuf, Option<image::Handle>>,
    /// The thumbnail being made.
    making: Option<PathBuf>,
}

/// Read the kept pictures again and make the thumbnails missing.
pub fn refresh(state: &mut AppearanceState) -> Task<AppearanceMsg> {
    let g = &mut state.gallery;
    g.kept = sola_core::background::kept();
    let kept = &g.kept;
    g.thumbs.retain(|p, _| kept.contains(p));
    next(g)
}

/// A thumbnail is made: keep it, make the next.
pub fn made(
    state: &mut AppearanceState,
    path: PathBuf,
    thumb: Option<image::Handle>,
) -> Task<AppearanceMsg> {
    let g = &mut state.gallery;
    if g.making.as_ref() == Some(&path) {
        g.making = None;
    }
    g.thumbs.insert(path, thumb);
    next(g)
}

/// Set `path` as the picture.
pub fn wear(path: &Path) {
    let set_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let picture = Background {
        path: path.to_string_lossy().into_owned(),
        set_at,
    };
    send(|b| b.emit(Topic::Background(picture)));
}

/// Remove the kept picture `path` (not the one set) and the look it kept.
pub fn forget(state: &mut AppearanceState, path: &Path) -> Task<AppearanceMsg> {
    if is_set(state, path) {
        return Task::none();
    }
    if let Err(e) = sola_core::background::forget(path) {
        tracing::warn!(path = %path.display(), %e, "appearance: forget a picture");
    }
    let key = path.to_string_lossy();
    if state.look.pictures.remove(key.as_ref()).is_some() {
        let topic = Topic::Appearance(state.look.clone());
        send(|b| b.emit(topic));
    }
    refresh(state)
}

fn next(g: &mut Gallery) -> Task<AppearanceMsg> {
    if g.making.is_some() {
        return Task::none();
    }
    let Some(path) = g.kept.iter().find(|p| !g.thumbs.contains_key(*p)).cloned() else {
        return Task::none();
    };
    g.making = Some(path.clone());
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            let source = path.clone();
            std::thread::spawn(move || {
                let _ = tx.send(thumbnail(&source));
            });
            (path, rx.await.ok().flatten())
        },
        |(path, thumb)| AppearanceMsg::Thumb(path, thumb),
    )
}

/// `path` cut to fill the thumbnail at twice its size. Blocking.
fn thumbnail(path: &Path) -> Option<image::Handle> {
    let img = match ::image::open(path) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!(path = %path.display(), %e, "appearance: read a kept picture");
            return None;
        }
    };
    let (w, h) = (THUMB_W * 2, THUMB_H * 2);
    let scale = (w as f32 / img.width() as f32).max(h as f32 / img.height() as f32);
    let (sw, sh) = (
        ((img.width() as f32 * scale).ceil() as u32).max(w),
        ((img.height() as f32 * scale).ceil() as u32).max(h),
    );
    let small = img
        .thumbnail_exact(sw, sh)
        .crop_imm((sw - w) / 2, (sh - h) / 2, w, h)
        .to_rgba8();
    Some(image::Handle::from_rgba(w, h, small.into_raw()))
}

fn is_set(state: &AppearanceState, path: &Path) -> bool {
    state
        .picture
        .as_ref()
        .is_some_and(|p| Path::new(&p.path) == path)
}

pub fn section(state: &AppearanceState) -> Element<'_, AppearanceMsg> {
    let mut rows = vec![match &state.picture {
        Some(p) => page::setting(
            file_name(&p.path),
            kit_btn::labeled_sm("Clear", kit_btn::secondary).on_press(AppearanceMsg::ClearPicture),
        ),
        None => page::empty("None — the black ground."),
    }];
    let tiles: Vec<Element<'_, AppearanceMsg>> = state
        .gallery
        .kept
        .iter()
        .map(|path| tile(state, path))
        .collect();
    if !tiles.is_empty() {
        rows.push(page::plain(
            iced::widget::Row::from_vec(tiles)
                .spacing(SPACE_SM)
                .wrap()
                .vertical_spacing(SPACE_SM),
        ));
    }
    page::section(
        "Background picture",
        None,
        page::group(rows),
        Some(
            "Every picture you set is kept here with the look you wore with it: \
             pick one to wear both again. Set a new one from Paint (File → Set \
             as Background) or a picture's menu in Browser."
                .into(),
        ),
    )
}

/// A kept picture: ringed in the accent when set; otherwise a press sets
/// it, and × (on hover) forgets it.
fn tile<'a>(state: &'a AppearanceState, path: &'a Path) -> Element<'a, AppearanceMsg> {
    let set = is_set(state, path);
    let ring = if set { 2.0 } else { 1.0 };
    let face: Element<'a, AppearanceMsg> = match state.gallery.thumbs.get(path) {
        Some(Some(handle)) => image(handle.clone())
            .width(THUMB_W as f32)
            .height(THUMB_H as f32)
            .content_fit(ContentFit::Cover)
            .border_radius(RADIUS - ring)
            .into(),
        Some(None) => container(kit_text::caption(file_name(&path.to_string_lossy())).style(kit_text::muted))
            .width(THUMB_W as f32)
            .height(THUMB_H as f32)
            .padding(SPACE_XS)
            .center(Length::Fill)
            .into(),
        None => Space::new().width(THUMB_W as f32).height(THUMB_H as f32).into(),
    };
    let framed = container(face)
        .padding(ring)
        .clip(true)
        .style(move |theme: &Theme| container::Style {
            background: Some(Fill::Color(theme.extended_palette().background.weak.color)),
            border: Border {
                color: if set {
                    theme.extended_palette().primary.base.color
                } else {
                    alpha(Color::WHITE, 0.12)
                },
                width: ring,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        });
    let pick = button(framed)
        .padding(0)
        .style(kit_btn::ghost)
        .on_press_maybe((!set).then(|| AppearanceMsg::Use(path.to_path_buf())));
    if set {
        return pick.into();
    }
    let forget = button(
        svg(page::icon("lucide/x"))
            .width(12)
            .height(12)
            .style(|_: &Theme, _| svg::Style {
                color: Some(Color::WHITE),
            }),
    )
    .padding(3)
    .style(|_: &Theme, status| button::Style {
        background: Some(Fill::Color(alpha(
            Color::BLACK,
            if matches!(status, button::Status::Hovered) {
                0.8
            } else {
                0.55
            },
        ))),
        border: Border {
            radius: 9.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    })
    .on_press(AppearanceMsg::Forget(path.to_path_buf()));
    let corner = container(forget)
        .padding(SPACE_XS)
        .align_right(Length::Fill)
        .align_top(Length::Fill);
    hover(pick, corner)
}

pub(super) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}
