//! Icon — resolves a name like `"lucide/settings"` or `"lucide/menu"` to
//! an iced Svg widget themed with the current text color.
//!
//! Resolution delegates to [`sola_assets::icons::read_svg`], which reads
//! from `/opt/sola/share/icons/<pack>/<name>.svg` at call time. The
//! convenience [`icon`] / [`icon_colored`] functions read on every call,
//! which is fine for one-off chrome but pays per-frame disk I/O when a
//! widget re-renders. For repeatedly-rendered icons, build the handle
//! once with [`icon_handle`] and render it with [`icon_svg`] /
//! [`icon_svg_colored`] from your stored handle.
//!
//! First consumers: sola-shell menubar system-menu button, launcher rows,
//! switcher cards.

use iced::widget::svg;
use iced::{Element, Length};

/// Build the `svg::Handle` for a named icon once, so a caller can stash
/// it in its state and avoid re-reading the SVG from disk every frame.
/// `name` has the form `"<pack>/<icon>"` (e.g. `"lucide/menu"`). A
/// missing icon yields a zero-byte handle (renders as an empty box).
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

/// Convenience: resolve `name` and render it tinted with the active
/// theme's foreground text color. Reads from disk on every call — for
/// repeatedly-rendered icons, prefer [`icon_handle`] + [`icon_svg`].
///
/// `name` has the form `"<pack>/<icon>"` (e.g. `"lucide/menu"`), matching
/// the layout of `/opt/sola/share/icons/`.
pub fn icon<'a, Msg: 'a>(name: &str, size: u16) -> Element<'a, Msg> {
    icon_svg(icon_handle(name), size)
}

/// Like [`icon`] but with an explicit `color` override instead of the
/// theme's default foreground. Use this when the target color is known
/// at call-site (e.g. the system-menu button's `text-secondary` tint).
pub fn icon_colored<'a, Msg: 'a>(name: &str, size: u16, color: iced::Color) -> Element<'a, Msg> {
    icon_svg_colored(icon_handle(name), size, color)
}
