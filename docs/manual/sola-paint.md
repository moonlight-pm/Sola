# sola-paint

Default image viewer and editor. First pass — install to use.

## Open

- Launcher → **Paint**
- `xdg-open photo.png` / file manager (MIME via `sola-paint.desktop`)
- `solactl open /path/to/photo.png`
- `sola-paint /path/to/photo.png`
- Super+Shift+3/4/5 screenshots raise Paint without stealing keyboard

A second `open` may start another process (no single-instance yet).

## Edit

Left tabs are open images. Hover a tab for ×.

Toolbar: Open, Crop, rotate, flip, Undo, Save. **Crop** — drag on the picture, **Apply crop** or Enter; Esc cancels.

Open / Save as use the kit file picker (Places + breadcrumb trail). The name field is the leaf, not the whole path.

Formats: PNG, JPEG, GIF, WebP, BMP, TIFF.

## Not in this pass

Zoom/pan, clipboard image, color adjust, filters, layers.
