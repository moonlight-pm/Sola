# Paint

Paint (`sola-paint`) is Sola's default image viewer and editor.

## Open

- Launcher → **Paint**
- `xdg-open photo.png` or the file manager (MIME via `sola-paint.desktop`)
- `solactl open /path/to/photo.png`
- `sola-paint /path/to/photo.png`
- **File → Open…** (⌘O): Sola's file panel — choose one image or several
  (Shift, ⌘); each opens in its own window. **Save As** (⌘⇧S) asks before
  it replaces a file.

Each open image is its own window — a row in the shell's tab bar, named
for the file. A second open (MIME, `solactl open`, or `sola-paint path`)
hands off to the running Paint and opens another window; an image that is
already open comes up instead.

Super+Shift+3/4/5 screenshots copy to the clipboard; they do not open
Paint.

## Edit

**Close Window** (⌘W, or the titlebar's close dot) closes the image;
closing the last window quits Paint. The open images (file paths) come back
after quit, each in its window and place; missing files are dropped.
Unsaved edits are not kept across a restart. With nothing open, Paint
shows one window with **Open image**.

The tools, keys and menus act on the window you are in.

The toolbar has Open, Crop, rotate, flip, Undo, and Save. Hover an icon for
its name and shortcut.

**Crop** (⌘⇧K): drag on the picture, then **Apply crop** or Enter; Esc
cancels. (⌘K is the Shell's shortcuts overlay.)

Scroll to zoom toward the pointer; drag to pan when zoomed in. ⌘+ and ⌘−
zoom, and ⌘0 fits. The header shows the zoom next to the pixel size.

Open and Save As use the kit file picker (Places and a breadcrumb trail).
The name field is the file name, not the whole path.

**File → Set as Background** makes the image in front the picture behind
the windows. Sola keeps its own copy in `~/.config/sola/background/`, so
the picture stays when the file moves or goes: the file as saved, or, with
unsaved edits (a crop), what is on screen as a PNG. The status line says
**Set as background**. It fills the screen, centre-cropped. To go back to
the black ground: `solactl background clear`.

Formats: PNG, JPEG, GIF, WebP, BMP, TIFF.

## Limits

Paint has no clipboard image support (Preview has **Copy**), no color
adjustment, no filters, and no layers.
