# Scope

Scope (`sola-scope`) is a pixel loupe. It magnifies the pixels under the
pointer.

## Use

- Launch **Scope** from the launcher.
- Move the pointer anywhere on the output. The grid follows; the toolbar
  shows the pointer's `x, y`, and the strip under the grid shows the
  swatch, hex, and RGB of the pixel under the pointer (marked on the
  grid).
- **Zoom in / zoom out**: the toolbar, `+` / `-`, the wheel over the grid,
  or **View → Zoom In / Zoom Out** (⌘= / ⌘-). Zoom out goes up to
  **65×65** pixels; zoom in goes down to **3×3**.
- Click the swatch or hex, or use **Edit → Copy Color** (⌘C), to copy
  `#RRGGBB`.

The titlebar has the **Scope**, **View**, and **Edit** menus. The window
floats and is remembered: close and reopen it and it comes back where you
left it.

The magnified grid is the desktop under the pointer, **without** the cursor
sprite. The pointer stays visible on the desktop.

## Requirements

Live follow and the cursor-free grid need River (on `PATH`) built with two
of Sola's patches, both in `nix/patches/`:

- `wlroots-screencopy-omit-sw-cursor.patch`: wlroots copies the screen
  before it draws a software cursor into it;
- `river-live-pointer-position.patch`: River sends the pointer's position
  on every move.

## Call

`solactl compositor sample [--size N]` returns JSON: the pointer `x`/`y`,
the patch `width`/`height`, `hot_x`/`hot_y`, and `pixels` (base64 RGBA8).
`size` is odd, default 15, max 65. It is independent of
`compositor.screenshot`.

## Limits

- First `wl_output` only (coordinates are converted from global layout
  space).
- Samples about 10 times a second while the window is open.
- No freeze.
