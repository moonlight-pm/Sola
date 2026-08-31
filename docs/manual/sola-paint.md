# sola-paint

Default image viewer and editor. First pass — install to use.

## Open

- Launcher → **Paint**
- `xdg-open photo.png` / file manager (MIME via `sola-paint.desktop`)
- `solactl open /path/to/photo.png`
- `sola-paint /path/to/photo.png`
A second `open` (MIME, `solactl open`, `sola-paint path`) hands off to the
running window and opens another tab. Super+Shift+3/4/5 screenshots still
open **Preview**, not Paint.

## Edit

Left tabs are open images. Hover a tab for ×. Tabs (file paths) come back after quit; missing files are dropped. Unsaved edits are not saved across restart.

Toolbar: Open, Crop, rotate, flip, Undo, Save. Hover an icon for the name and shortcut. **Crop** (⌘⇧K) — drag on the picture, **Apply crop** or Enter; Esc cancels. (⌘K is the shell shortcuts overlay.)

Scroll to zoom toward the pointer; drag to pan when zoomed in. ⌘+ / ⌘− / ⌘0
(fit). Header shows the zoom next to the pixel size.

Open / Save as use the kit file picker (Places + breadcrumb trail). The name field is the leaf, not the whole path.

Formats: PNG, JPEG, GIF, WebP, BMP, TIFF.

## Not in this pass

Clipboard image, color adjust, filters, layers.
