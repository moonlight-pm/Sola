# iced_winit (Sola patch of 0.14.0)

Upstream `iced_winit` 0.14.0 plus one behavior change:

**Wayland `wl_surface.set_opaque_region` follows the iced window fill.**

`State::synchronize` (and first `State::new`) calls
`winit::window::Window::set_transparent` when `style.background_color.a`
crosses 1.0. Tiled kit apps use an opaque `background.base` (via
`sola_kit::theme_for(false, …)`); overlay / float themes use a transparent
base. River GLES cannot scan out ARGB without a full opaque region, which
showed up as idle GPU on 5120×2160 even after iced stopped presenting
storms.

Do not edit this as if it were a Sola crate. Re-diff against iced 0.14
`iced_winit` when bumping iced.

Workspace wiring: `[patch.crates-io] iced_winit = { path = "crates/iced_winit-patched" }`
and `exclude` in the root `Cargo.toml` (same pattern as `wgpu-hal-patched`).
