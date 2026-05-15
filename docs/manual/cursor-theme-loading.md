# Cursor theme loading — known limitation

**Status:** open. Kit pipeline is correct; theme rendering is the gap.
**First seen:** 2026-05-14, while wiring `wp_cursor_shape_v1` into `sola-kit`.

## Symptom

CSS `cursor:` changes inside kit webviews reach river correctly via
`wp_cursor_shape_v1`, but only `text` (I-beam) and `pointer` (hand) actually
render. Every other shape — `crosshair`, `col-resize`, `row-resize`, `move`,
etc. — silently falls back to the default arrow.

## Pipeline diagnosis (already done — don't repeat)

Verified end-to-end via `tracing::info!` instrumentation in
`wayland::cursor::set_pending` and `WaylandClient::apply_pending_cursor`
(both reverted to `debug` after diagnosis):

```
INFO [kit] sola::kit cursor: pending shape=ColResize
INFO [kit] sola::kit cursor: set_shape shape=ColResize serial=8441
```

So:

- CEF emits `on_cursor_change` correctly for every CSS hover transition.
- `wayland::cursor::cef_to_shape` maps CEF cursor types to `wp_cursor_shape_v1`
  shapes (full table, see `crates/sola-kit/src/wayland/cursor.rs`).
- `wp_cursor_shape_device_v1.set_shape(serial, shape)` is called with a valid
  serial (latest pointer-enter via `PointerData::latest_enter_serial()`).
- River's environment has `XCURSOR_THEME=Adwaita` and `XCURSOR_PATH` includes
  `/opt/sola/share/cursors` (verified via `/proc/<river>/environ`).

The wlroots cursor-shape source maps `Shape::ColResize` → `"col-resize"`
(hyphenated, matches Adwaita's filenames). The bundled Adwaita at
`/opt/sola/share/cursors/Adwaita/cursors/` *has* `col-resize`, `row-resize`,
`crosshair`, and 32 other cursor files, all valid XCursor format with real
distinct pixel data (decoded with `xcur2png` for verification — `crosshair`
is a perfect `+`, `col-resize` is the classic horizontal split-arrow).

The wlroots fallback path (`types/wlr_cursor.c`) silently substitutes
`default` when `wlr_xcursor_manager_get_xcursor()` returns NULL. We're hitting
that path for everything except `text` and `pointer`. Why those two work and
visually-fine others don't is opaque from outside river.

## Hypotheses (untested — all need a river restart to evaluate)

1. **Broken `Inherits=AdwaitaLegacy,hicolor` line in
   `/opt/sola/share/cursors/Adwaita/index.theme`.** `AdwaitaLegacy` isn't
   installed; wlroots' bundled libxcursor copy may be choking on the missing
   chain in some way that selectively skips most cursors. Cheapest test —
   strip the `Inherits=` line and see if more cursors render.

2. **Bundled Adwaita is stripped.** All 33 non-animated files are exactly
   78208 bytes (suspiciously uniform), and there are zero symlinks (real
   Adwaita ships ~200 entries with many aliases). May be a pipeline that
   emitted only canonical files and missed the alias web that wlroots
   relies on for some lookups. Replace with `pkgs.adwaita-icon-theme` from
   nixpkgs.

3. **Switch theme entirely.** `Bibata-Modern-Ice` (`pkgs.bibata-cursors`)
   or `Capitaine-cursors` (`pkgs.capitaine-cursors`) are well-tested with
   wlroots/wayland and would also resolve the secondary complaint that
   Adwaita's hand pointer is unloved. Set `XCURSOR_THEME = "Bibata-Modern-Ice"`
   in `/etc/nixos/configuration.nix`'s `environment.sessionVariables`.

## Recommended next attempt

Try (1) first — zero install, one-line edit, easy revert. If that doesn't
unblock the rest of the cursors, jump to (3) since it solves two problems at
once.

## What NOT to do

- **Don't go back into `wayland::cursor` or `WaylandClient::apply_pending_cursor`.**
  The pipeline is provably correct; further code changes there will be
  rabbit-holes.
- **Don't add an XCursor fallback** at our end (load cursor surfaces ourselves
  via `wayland-cursor` and call `wl_pointer.set_cursor` instead of
  `wp_cursor_shape_v1`). Per design we use cursor-shape-v1 only; if a
  compositor doesn't render a shape, that's the compositor's problem, not
  ours.

## Related code

- `crates/sola-kit/src/wayland/cursor.rs` — shape mapping + thread-local channel
- `crates/sola-kit/src/wayland/client.rs` — `cursor_shape_manager` /
  `cursor_shape_device` fields, `apply_pending_cursor`
- `crates/sola-kit/src/cef/handlers.rs` — `KitDisplayHandler::on_cursor_change`
