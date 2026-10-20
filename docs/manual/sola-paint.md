# Paint

Paint (`sola-paint`) is Sola's default image viewer and editor.

## Open

- Launcher → **Paint**
- `xdg-open photo.png` or the file manager (MIME via `sola-paint.desktop`)
- `solactl open /path/to/photo.png`
- `sola-paint /path/to/photo.png`

A second open (MIME, `solactl open`, or `sola-paint path`) hands off to
the running window and opens another tab.

Super+Shift+3/4/5 screenshots copy to the clipboard; they do not open
Paint.

## Edit

Open images are tabs on the left. Hover a tab for ×. Tabs (file paths)
come back after quit; missing files are dropped. Unsaved edits are not
kept across a restart.

The toolbar has Open, Crop, rotate, flip, Undo, and Save. Hover an icon for
its name and shortcut.

**Crop** (⌘⇧K): drag on the picture, then **Apply crop** or Enter; Esc
cancels. (⌘K is the Shell's shortcuts overlay.)

Scroll to zoom toward the pointer; drag to pan when zoomed in. ⌘+ and ⌘−
zoom, and ⌘0 fits. The header shows the zoom next to the pixel size.

Open and Save As use the kit file picker (Places and a breadcrumb trail).
The name field is the file name, not the whole path.

Formats: PNG, JPEG, GIF, WebP, BMP, TIFF.

## Limits

Paint has no clipboard image support (Preview has **Copy**), no color
adjustment, no filters, and no layers.
