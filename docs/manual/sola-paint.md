# Paint

Paint (`sola-paint`) is Sola's default image viewer and editor. On Oath it
ships in `pkg:sola`.

## Open

- Launcher → **Paint**
- `solactl open /path/to/photo.png`
- `sola-paint /path/to/photo.png`
- **File → Open…** (⌘O): Sola's file panel — choose one image or several
  (Shift, ⌘); each opens in its own window.

Each open image is its own window — a row in the bar, named for the file.
A second open (`solactl open` or `sola-paint path`) hands off to the
running Paint and opens another window; an image that is already open
comes up instead. Launching Paint again with no path brings its window up.

Paint keeps at most 32 images open; opening a 33rd closes the oldest.

Super+Shift+3/4/5 screenshots copy to the clipboard; they do not open
Paint.

## Windows

The titlebar carries Paint's menus: **Paint** (Quit Paint, ⌘Q), **File**,
**Edit**, and **View**, then the Window menu. The tools, keys and menus act
on the window you are in.

**File → Close Window** (⌘W, the titlebar's close dot, or the close dot on
its row in the bar) closes the image; closing the last window quits Paint.
The open images (file paths) come back after quit, each in its window and
its place in the bar; missing files are dropped. Unsaved edits are not kept
across a restart. With nothing open, Paint shows one window with **Open
image**.

## Edit

The toolbar has Open, Crop, Rotate left, Rotate right, Flip horizontal,
Flip vertical, Undo, and Save. Hover an icon for its name and shortcut.

| Do | How |
|----|-----|
| Crop | ⌘⇧K (**Edit → Crop**): drag on the picture, then **Apply crop** or Enter; Esc cancels. (⌘K is the Shell's shortcuts overlay.) |
| Rotate right / left | ⌘R / ⌘⇧R (**Edit → Rotate Right / Rotate Left**) |
| Flip | Toolbar only |
| Undo | ⌘Z (**Edit → Undo**), up to 8 steps |
| Save | ⌘S (**File → Save**) |
| Save As | ⌘⇧S (**File → Save As…**) |

An image with unsaved edits has a dot after its name. Save writes the
format of the file's extension (PNG when the extension names none); a
JPEG is saved without its alpha. Save As asks before it replaces a file.

Scroll to zoom toward the pointer; drag to pan when zoomed in. ⌘= and ⌘−
zoom, and ⌘0 fits (**View → Zoom In / Zoom Out / Fit**). Zoom runs from
25% to 1600% of the fitted size. The header shows the zoom next to the
pixel size; 100% is the picture fitted to the window, not one image pixel
per screen pixel.

Open and Save As use the kit file panel (Places and a breadcrumb trail).
The name field is the file name, not the whole path.

## Background

**File → Set as Background** makes the image in front the picture behind
the windows. Sola keeps its own copy in `~/.config/sola/background/`, so
the picture stays when the file moves or goes: the file as saved, or, with
unsaved edits (a crop), what is on screen as a PNG. The status line says
**Set as background**. It fills the screen, centre-cropped. Settings →
Appearance lists every picture kept, to set one again
([Kept pictures](theme.md#kept-pictures)). To go back to the black
ground: **Clear** there, or `solactl background clear`.

Formats: PNG, JPEG, GIF, WebP, BMP, TIFF.

## Limits

Paint has no clipboard image support (Preview has **Copy**), no color
adjustment, no filters, and no layers. Flip has no menu item or shortcut.
`pkg:sola` carries no `.desktop` file for Paint, so on Oath Paint is not
registered as an image handler for `xdg-open` or a file manager; use
`solactl open` or `sola-paint <path>`.
