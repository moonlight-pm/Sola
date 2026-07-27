//! Icon — resolves a name like `"lucide/settings"`, `"simpleicons/firefox"`,
//! or a filesystem path to a PNG to an iced widget.
//!
//! # Ref forms
//!
//! | Form | Example | Rendering |
//! |---|---|---|
//! | Pack SVG | `lucide/menu` | Theme-tinted stroke SVG via `sola-assets` |
//! | Pack raster | `apps/orca-ide` (`.png` under the pack) | Full-color bitmap, untinted |
//! | Absolute path | `/home/…/orca-ide.png` | Full-color bitmap, untinted |
//! | Home path | `~/.local/share/sola/icons/orca-ide.png` | Full-color bitmap, untinted |
//!
//! Pack SVGs resolve from `/opt/sola/share/icons/<pack>/<name>.svg`.
//! Raster pack names try `.png` / `.jpg` / `.webp` / `.gif` in the same
//! directory. Path refs are only used when the file exists.
//!
//! The convenience [`icon`] / [`icon_colored`] functions read on every call,
//! which is fine for one-off chrome but pays per-frame disk I/O when a
//! widget re-renders. For repeatedly-rendered **SVG** icons, build the
//! handle once with [`icon_handle`] and render it with [`icon_svg`] /
//! [`icon_svg_colored`]. Raster icons always go through [`icon`] (path
//! → `image::Handle::from_path`; iced caches decoded bitmaps by path).
//!
//! First consumers: sola-shell menubar system-menu button, launcher rows,
//! switcher cards.

use std::path::PathBuf;

use iced::widget::{image, svg};
use iced::{ContentFit, Element, Length};

/// Build the `svg::Handle` for a named **pack SVG** once, so a caller can
/// stash it in its state and avoid re-reading from disk every frame.
/// `name` has the form `"<pack>/<icon>"` (e.g. `"lucide/menu"`). A
/// missing icon yields a zero-byte handle (renders as an empty box).
///
/// Path / raster refs are not handled here — use [`icon`].
pub fn icon_handle(name: &str) -> svg::Handle {
    svg::Handle::from_memory(sola_assets::icons::read_svg(name).unwrap_or_default())
}

/// Render a stored [`icon_handle`] tinted with the active theme's
/// foreground text color, sized to `size × size` logical pixels.
pub fn icon_svg<'a, Msg: 'a>(handle: svg::Handle, size: u16) -> Element<'a, Msg> {
    svg(handle)
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .style(|theme: &iced::Theme, _status| svg::Style {
            color: Some(theme.extended_palette().background.base.text),
        })
        .into()
}

/// Render a stored [`icon_handle`] with an explicit `color` override
/// instead of the theme's default foreground.
pub fn icon_svg_colored<'a, Msg: 'a>(
    handle: svg::Handle,
    size: u16,
    color: iced::Color,
) -> Element<'a, Msg> {
    svg(handle)
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .style(move |_theme: &iced::Theme, _status| svg::Style { color: Some(color) })
        .into()
}

/// Full-color raster icon (PNG/JPEG/…). No theme tint — app faces keep
/// their official colors.
fn icon_raster<'a, Msg: 'a>(path: PathBuf, size: u16) -> Element<'a, Msg> {
    image(image::Handle::from_path(path))
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .content_fit(ContentFit::Contain)
        .into()
}

/// Convenience: resolve `name` and render it.
///
/// - Pack SVGs are tinted with the active theme's foreground.
/// - Pack rasters and filesystem paths render full-color (untinted).
///
/// Reads from disk on every call for SVG bytes; raster uses path-based
/// handles. For repeatedly-rendered pack SVGs, prefer [`icon_handle`] +
/// [`icon_svg`].
pub fn icon<'a, Msg: 'a>(name: &str, size: u16) -> Element<'a, Msg> {
    // Prefer SVG packs when both a stroke SVG and a raster exist under
    // the same pack/name (lucide etc.). Path refs and pack-only PNGs
    // fall through to raster.
    if sola_assets::icons::read_svg(name).is_some() {
        return icon_svg(icon_handle(name), size);
    }
    if let Some(path) = sola_assets::icons::raster_path(name) {
        return icon_raster(path, size);
    }
    // Missing: empty SVG box (same as historical missing lucide name).
    icon_svg(icon_handle(name), size)
}

/// Like [`icon`] but with an explicit `color` override for **SVG** pack
/// icons. Raster / path icons ignore `color` and render full-color — app
/// faces are not recolored. Use this when the target color is known at
/// call-site (e.g. the system-menu button's `text-secondary` tint).
pub fn icon_colored<'a, Msg: 'a>(name: &str, size: u16, color: iced::Color) -> Element<'a, Msg> {
    if sola_assets::icons::read_svg(name).is_some() {
        return icon_svg_colored(icon_handle(name), size, color);
    }
    if let Some(path) = sola_assets::icons::raster_path(name) {
        return icon_raster(path, size);
    }
    icon_svg_colored(icon_handle(name), size, color)
}
