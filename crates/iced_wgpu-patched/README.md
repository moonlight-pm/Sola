# iced_wgpu (Sola patch of 0.14.0)

Upstream `iced_wgpu` 0.14.0 plus one change:

**A window's image atlas starts at 512² instead of 2048².**

iced gives every window its own atlas and allocates the first layer when the
window opens, whether or not it ever draws a picture: 16 MiB a window, which
on a desk of 80 windows was 1.3 GiB of VRAM, most of it empty. A 512² layer
is 1 MiB, and more layers come as they are needed.

An image too wide or tall for that shared atlas takes the path a large image
already took (`Cache::upload_raster`): the worker gives it a texture of its
own. Without that it would be split across layers, and a scaled image shows
seams between the pieces. An SVG is rasterized at the size it is drawn, so
one larger than a layer is still split, drawn 1:1 with no seam.

Do not edit this as if it were a Sola crate. Re-diff against iced 0.14
`iced_wgpu` when bumping iced.

Workspace wiring: `[patch.crates-io] iced_wgpu = { path = "crates/iced_wgpu-patched" }`
and `exclude` in the root `Cargo.toml` (same pattern as `iced_winit-patched`).
