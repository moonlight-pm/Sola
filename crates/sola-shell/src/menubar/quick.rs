//! The quick launch (`Topic::QuickLaunch`): an island in the middle of the
//! top row, an app's icon each to start it (or bring it forward, as the
//! launcher's row does), and Sleep at the end. None while it holds nothing.
//! Centred on the row, unless that runs into the right-hand islands
//! ([`place`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use iced::widget::{container, image, row, svg};
use iced::{Alignment, Color, ContentFit, Element, Length, Padding};
use sola_kit::components::icon::{icon_handle, icon_svg_colored};

use crate::app::Msg;
use crate::menubar::view::{island, item};
use crate::menubar::{ISLAND_GAP, ISLAND_PAD_H, ITEM_GAP};

/// An app's icon: larger than the row's glyphs, it is something to hit.
const SIZE: u16 = 16;

/// The island's width with `items` on it.
fn width(items: usize) -> f32 {
    items as f32 * (f32::from(SIZE) + ITEM_GAP) + 2.0 * (ISLAND_PAD_H - ITEM_GAP / 2.0)
}

/// The middle island's left edge (this one, or a toast in its place), on
/// an output `output_w` wide: centred, unless that comes within a gap of
/// the right-hand islands (from `right_start`); then just left of them,
/// though never over the left group (to `left_end`).
pub fn place(output_w: f32, width: f32, left_end: f32, right_start: f32) -> f32 {
    let centred = (output_w - width) / 2.0;
    centred
        .min(right_start - ISLAND_GAP - width)
        .max(left_end + ISLAND_GAP)
}

/// The island and its width, or `None` with nothing on it.
pub fn island_of(shell: &crate::app::Shell) -> Option<(Element<'_, Msg>, f32)> {
    let quick = &shell.quick_launch;
    let fg = shell.theme.palette().text;
    let mut items: Vec<Element<'_, Msg>> = quick
        .apps
        .iter()
        .filter_map(|id| shell.applications.get(id))
        .map(|app| {
            let name = if app.icon.trim().is_empty() {
                "lucide/app-window"
            } else {
                app.icon.as_str()
            };
            hit(face(name, fg), Msg::QuickLaunch(app.app_id.clone()))
        })
        .collect();
    if quick.sleep {
        items.push(hit(face("lucide/moon", fg), Msg::QuickSleep));
    }
    if items.is_empty() {
        return None;
    }
    let w = width(items.len());
    // Each item's hit target reaches halfway to the next, so the island is
    // one row of targets with no dead gaps between.
    let island = island(
        row(items).height(Length::Fill).align_y(Alignment::Center),
        false,
        ISLAND_PAD_H - ITEM_GAP / 2.0,
    );
    Some((island, w))
}

fn hit(face: Element<'static, Msg>, on_press: Msg) -> Element<'static, Msg> {
    item(
        container(face).padding(Padding {
            left: ITEM_GAP / 2.0,
            right: ITEM_GAP / 2.0,
            ..Padding::ZERO
        }),
        on_press,
    )
}

/// What an icon ref draws: a pack SVG (tinted) or a picture (as is).
#[derive(Clone)]
enum Face {
    Svg(svg::Handle),
    Raster(PathBuf),
}

/// `name`'s icon, read once: the row redraws often (the spectrum, the
/// meters), and a handle made each frame is rasterized again each frame.
fn face(name: &str, color: Color) -> Element<'static, Msg> {
    static FACES: Mutex<Option<HashMap<String, Face>>> = Mutex::new(None);
    let face = {
        let mut faces = FACES.lock().unwrap_or_else(|p| p.into_inner());
        faces
            .get_or_insert_with(HashMap::new)
            .entry(name.to_string())
            .or_insert_with(|| read(name))
            .clone()
    };
    match face {
        Face::Svg(handle) => icon_svg_colored(handle, SIZE, color),
        Face::Raster(path) => image(image::Handle::from_path(path))
            .width(Length::Fixed(f32::from(SIZE)))
            .height(Length::Fixed(f32::from(SIZE)))
            .content_fit(ContentFit::Contain)
            .into(),
    }
}

/// As the kit's `icon` resolves it: a pack SVG first, then a picture; a
/// missing one an empty box.
fn read(name: &str) -> Face {
    if sola_assets::icons::read_svg(name).is_some() {
        return Face::Svg(icon_handle(name));
    }
    match sola_assets::icons::raster_path(name) {
        Some(path) => Face::Raster(path),
        None => Face::Svg(icon_handle(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_unless_the_right_hand_islands_are_in_the_way() {
        let w = width(4);
        assert_eq!(w, 4.0 * (16.0 + ITEM_GAP) + 2.0 * (ISLAND_PAD_H - ITEM_GAP / 2.0));
        // A wide output: the middle.
        assert_eq!(place(1920.0, 100.0, 40.0, 1200.0), 910.0);
        // A narrow one: just left of the meters.
        assert_eq!(place(1280.0, 100.0, 40.0, 600.0), 600.0 - ISLAND_GAP - 100.0);
        // No room at all: it keeps off the flower.
        assert_eq!(place(400.0, 100.0, 40.0, 100.0), 40.0 + ISLAND_GAP);
    }
}
