# The background picture

**Date:** 2026-10-24
**Status:** **Accepted** 2026-10-24 (Joshua: "find a nice background image
that fits this desktop resolution and set it … perhaps in the paint app we
can have a menu item to set it as background image … copied into the
users sola settings"; "pick something from james webb"). Built on `next`, applied and smoked
on canto 2026-10-24 (plan [`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)
Phase 16).
**Related:** [Next](2026-10-21-next-tab-bar-design.md) (board 02, the
ground; slice 6, rounded foreign tiles).

## Why

The ground between Next's windows is the compositor's black. A picture
there makes the desk Sola's own. It also changes what "the ground" is: the
tiles, the bar and the foreign tiles' corners were all painted black to
match it, and painted black corners on a picture would show as black
squares.

## Decided (Joshua, 2026-10-24)

| | Decision |
|--|--|
| B1 | A **background picture** fills the output behind every window. |
| B2 | It is set from **Paint**: File → **Set as Background** on the focused image. |
| B3 | Sola **keeps its own copy** under `~/.config/sola/background/`, so the picture stays when the original moves or goes. |
| B4 | The first one: a James Webb image (the Cosmic Cliffs in Carina, NASA, ESA, CSA, STScI; public domain), 1920 px wide for canto's 1920 × 1080. |

## The picture

- **Fill, centre-cropped:** the picture is scaled to cover the output and
  the overflow cut evenly from both sides (the Cosmic Cliffs at 1920 × 1112
  lose 16 px at the top and the bottom). Turned upright by its EXIF
  orientation.
- **No picture:** the plain black ground, as before (`solactl background
  clear`).
- **Where it lives:** `~/.config/sola/background/<file name>` — the one
  picture there; setting another removes the previous copy. Paint copies
  the file as it is on disk; an image edited in Paint and not saved is
  written as `<name>.png` from what is on screen.
- **Also:** `solactl background set <image>` / `solactl background clear`
  (scripts, agents).

## Mechanics

- **Bus.** `Topic::Background { path, set_at }`, **persistent** (survives
  a restart); `path` is the copy, `set_at` makes setting a same-named file
  again a change. Retracted: no picture. Any app may set it (Paint,
  solactl); `sola_core::background::adopt` / `adopt_bytes` make the copy.
- **sola-river draws it** (`client/background.rs`): a River **shell
  surface** (`river_window_manager_v1.get_shell_surface`, the protocol's
  "background image" use) placed at the bottom of the render list, at the
  output's origin, its input region empty (clicks on the ground behave as
  before). XRGB8888 SHM at the output's size times its integer scale
  (`set_buffer_scale`). Decoding and scaling (Lanczos3) run on a thread;
  the result is kept for the corners. A failed decode is logged and not
  retried until the picture or the output changes. A fullscreen window
  covers it, as it covers everything.
- **Corners show the ground, whatever it is:**
  - **Kit windows** clear their frame's corners whether tiled or floating
    (`window_frame`, `float_clip` — the paint-the-ground mode is gone), and
    tiles take the clear fill a float does (`sola_kit::theme_for(chrome)`:
    tiled or floating → `theme::overlay`; Cinema keeps the opaque fill).
  - **The bar** (an island) clears its corners; its window is transparent.
  - **Foreign tiles** (sola-river `corners.rs`): the decorations paint the
    picture's own pixels outside the curve (`background::sample` at the
    window's position), repainted when the window moves or the picture
    changes; black without a picture.
- **Cost.** A tile no longer sets a full Wayland opaque region (its
  corners are transparent), so River blends it and draws the ground under
  it where it is damaged. Nothing redraws at idle, and a tile in a gapped
  grid never scanned out; Cinema keeps its opaque region (scanout).
  `PERFORMANCE.md` has the note.

## Not now

A Settings pane for it (a picker, recent pictures), a picture per
workspace or per output (Sola frames one output), slideshows, dimming or
blurring the picture under the windows, a default picture shipped with
Sola, rounding floating foreign windows (still needs rivulet to clip).
