# Cursor theme loading

**Status:** resolved on 2026-05-20 by switching the vendored theme.

## Background

CSS `cursor:` changes inside kit webviews — and iced's pointer-shape
hints from sola-monitor-iced — reach river correctly via
`wp_cursor_shape_v1`. With the previous vendored theme (Adwaita) only
`text` (I-beam) and `pointer` (hand) actually rendered; every other
shape silently fell back to the default arrow. Resize cursors over the
sola-monitor-iced divider, `crosshair`, `col-resize`, `row-resize`,
`move`, etc. all failed to render.

## Pipeline (verified correct)

`tracing::info!` instrumentation in `wayland::cursor::set_pending` and
`WaylandClient::apply_pending_cursor` confirmed shape requests landed
on the wire:

```
INFO [kit] sola::kit cursor: pending shape=ColResize
INFO [kit] sola::kit cursor: set_shape shape=ColResize serial=8441
```

- CEF emits `on_cursor_change` for every CSS hover transition.
- `wayland::cursor::cef_to_shape` maps CEF cursor types to
  `wp_cursor_shape_v1` shapes.
- `wp_cursor_shape_device_v1.set_shape(serial, shape)` is called with a
  valid serial.
- River's environment has `XCURSOR_PATH` pointing at
  `/opt/sola/share/cursors`.

The wlroots cursor-shape source maps `Shape::ColResize` → `"col-resize"`
(hyphenated). The bundled Adwaita had the file but wlroots' lookup
silently substituted `default` anyway. Suspected cause was the
`Inherits=AdwaitaLegacy,hicolor` line in Adwaita's `index.theme`
pointing at a theme that isn't installed.

## Resolution

Vendored theme switched from `Adwaita` to `McMojave`
(`github:vinceliuice/McMojave-cursors`, GPL-3.0). McMojave ships:

- All 12 resize variants under hyphenated XDG names
  (`col-resize`, `row-resize`, `e-resize`, `ne-resize`, ...).
- A clean `index.theme` with `Name=McMojave Cursors` and **no
  `Inherits=` chain**, which sidesteps the broken-inheritance class of
  failure.
- 111 entries total (47 canonical files + 64 alias names) covering the
  hash-name lookups some compositors use.

Changes:
- `crates/sola-assets/upstream.toml` — `[packs.Adwaita]` replaced with
  `[packs.McMojave]` (`src_dir = "dist/cursors"`).
- `crates/sola-make/src/assets.rs::pull_pack` — cursor-pack `index.theme`
  lookup now tries `src.parent()/index.theme` first (XDG-conventional
  location next to `cursors/`) before falling back to the repo root.
- `crates/sola/src/main.rs::set_cursor_env` — default `XCURSOR_THEME`
  is now `McMojave`.

## What NOT to do

- **Don't go back into `wayland::cursor` or `apply_pending_cursor`.**
  The pipeline is provably correct.
- **Don't add an XCursor fallback** at our end (load cursor surfaces
  ourselves and call `wl_pointer.set_cursor`). Per design we use
  `wp_cursor_shape_v1` only; if a compositor doesn't render a shape,
  that's the compositor's problem, not ours.

## Related code

- `crates/sola-kit/src/wayland/cursor.rs` — shape mapping + thread-local channel
- `crates/sola-kit/src/wayland/client.rs` — `cursor_shape_manager` /
  `cursor_shape_device` fields, `apply_pending_cursor`
- `crates/sola-kit/src/cef/handlers.rs` — `KitDisplayHandler::on_cursor_change`
- `crates/sola-assets/upstream.toml` — pinned McMojave SHA
- `crates/sola-make/src/assets.rs` — cursor-pack pull and `index.theme` lookup
