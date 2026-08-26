# sola-scope

Pixel loupe. Magnifies the pixels under the pointer.

**Partial.** Installed `scope` + `river` + `shell` (debug). Live follow
and cursor-free grid smoked 2026-08-26. Needs the patched River
compositor (`/opt/sola/bin/river`).

## Use

- Launch **Scope** from the launcher.
- Move the pointer anywhere on the output. The grid follows.
- **Zoom in / zoom out** (toolbar, `+` / `-`, or wheel over the grid).
  Zoom out goes up to **65×65**; zoom in down to **3×3**.
- Click the swatch or hex, or **Edit → Copy Color** (⌘C), to copy `#RRGGBB`.

## Call

`solactl compositor sample [--size N]` returns JSON: pointer `x`/`y`,
patch `width`/`height`, `hot_x`/`hot_y`, and `pixels` (base64 RGBA8).
Odd `size`, default 15, max 65. Independent of `compositor.screenshot`.

The window is a remembered float: close and reopen and it comes back where
you left it.

The magnified grid is the desktop under the pointer, **without** the
cursor sprite. The pointer stays visible on the desktop. Needs patched
wlroots (`wlroots-screencopy-omit-sw-cursor`: copy on precommit, then
blit the cursor for scanout) and River (`river-live-pointer-position`).

## Limits

- First `wl_output` only (coords are converted from global layout space).
- Samples about 10 times a second while the window is open.
- No freeze yet.
- Live follow and cursor-free capture need the patched River in
  `/opt/sola/bin/river` (live `pointer_position` + wlroots omit-cursor).
