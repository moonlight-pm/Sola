//! Icon — resolves a name like `"lucide/settings"` or `"lucide/menu"` to
//! an iced Svg widget themed with the current text color.
//!
//! Resolution delegates to [`sola_assets::icons::read_svg`], which reads
//! from `/opt/sola/share/icons/<pack>/<name>.svg` at call time. For
//! widgets rendered repeatedly, wrap the result in a stored
//! `svg::Handle::from_memory(...)` rather than calling this function each
//! frame.
//!
//! First consumers: sola-shell menubar system-menu button, launcher rows,
//! switcher cards.

use iced::widget::svg;
use iced::{Element, Length};

/// Return an iced Svg widget for the named icon, tinted with the active
/// theme's text color and sized to `size × size` logical pixels.
///
/// `name` has the form `"<pack>/<icon>"` (e.g. `"lucide/menu"`), matching
/// the layout of `/opt/sola/share/icons/`.
///
/// If the icon cannot be found on disk, a zero-byte handle is returned
/// (iced renders it as an empty box of the requested size).
pub fn icon<'a, Msg: 'a>(name: &str, size: u16) -> Element<'a, Msg> {
    let bytes = sola_assets::icons::read_svg(name).unwrap_or_default();
    let handle = svg::Handle::from_memory(bytes);
    svg(handle)
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .style(|theme: &iced::Theme, _status| svg::Style {
            color: Some(theme.extended_palette().background.base.text),
        })
        .into()
}
