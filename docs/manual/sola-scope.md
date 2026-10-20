# Scope

Scope (`sola-scope`) is a pixel loupe. It magnifies the pixels under the
pointer.

## Use

- Launch **Scope** from the launcher.
- Move the pointer anywhere on the output. The grid follows.
- **Zoom in / zoom out** (toolbar, `+` / `-`, or the wheel over the grid).
  Zoom out goes up to **65×65**; zoom in goes down to **3×3**.
- Click the swatch or hex, or use **Edit → Copy Color** (⌘C), to copy
  `#RRGGBB`.

The window is a remembered float: close and reopen it and it comes back
where you left it.

The magnified grid is the desktop under the pointer, **without** the cursor
sprite. The pointer stays visible on the desktop.

## Requirements

Live follow and the cursor-free grid need Sola's patched River compositor
at `/opt/sola/bin/river`:

- patched wlroots (`wlroots-screencopy-omit-sw-cursor`: copy on precommit,
  then blit the cursor for scanout)
- River's live pointer position (`river-live-pointer-position`)

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
