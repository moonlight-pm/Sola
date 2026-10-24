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

## Adapting the theme (experiment, 2026-10-24)

**Asked (Joshua, 2026-10-24):** "a setting to 'adapt theme to background
image' … if there is a background image, adapt our theme colors (default
being grey) to a theme that blends well with the background image … sample
the hues from the image, to smartly determine a theme for the islands,
default window title and sidebar colors and such." Settings or the kit.

- **The setting:** Settings → **Appearance** → *Adapt colors to the
  background* and its **Strength** (persistent `Topic::Appearance {
  adapt_theme, strength }`, off by default, strength 60%; the slider sends
  on release). The panel also shows the picture's name and clears it.
  (2026-10-24, Joshua: "everything looks the same" at the first strength —
  0.3 × the picture's mean chroma, at most 0.03 — so the default went up
  and the slider came in.)
- **Who:** the **shell** (it always runs). With the setting on and a
  picture set, it samples the picture off the UI thread (decoded small,
  96 px on its longest side) and rewrites `Topic::Theme`'s color tokens;
  every app follows as it follows any theme change. Off, or the picture
  cleared, writes the seed's greys back on those tokens. Fonts, sizes and
  spacing are never touched. A color edited in the kit's theme editor
  while it is on is overwritten by the picture's.
- **The recipe** (`sola_core::theme::Scheme`, `Theme::adapt` /
  `unadapt`; OKLab / OKLCH):
  - **Lightness is the grey theme's.** Each adapted token keeps its seed
    grey's OKLab lightness (and alpha), so contrast is unchanged.
  - **Tint:** the picture's dominant hue (a chroma-weighted hue
    histogram, 10° bins, smoothed; greys do not vote), at a chroma of
    *strength* × the picture's mean chroma (at most 0.08), thinning toward
    black (below L 0.18, in proportion), on the surfaces (`bg-*`, `selection`,
    `border*`, `text-muted`, `shell-menubar-bg`, `shell-switcher-bg`);
    text takes 30% of it.
  - **Accent:** the most vivid bright hue at least 60° from the tint when
    it has a fifth of the best weight (a complement, e.g. the Cosmic
    Cliffs' blue sky against their orange), else the tint's hue; at the
    seed cyan's lightness (0.82), stepped down to 0.68 for a hue that
    cannot be vivid that light (blue); chroma up to 0.14 (`accent`,
    `text-accent`, `accent-dim` at 10%).
  - Status colors (danger, success, warning) stay.
  - A grey picture changes nothing.
- **The sidebar's surface follows the theme:** the kit's chrome surface
  (sidebars, top strips) was a constant `#141414`; it is now
  `style::chrome_surface()`, nine tenths from the base to the raised fill,
  installed with the theme (`theme::install_atoms`).
- **On the Cosmic Cliffs:** tint hue 37° (the cliffs' orange), mean
  chroma 0.071; accent hue 262° (the sky), `accent #73a3fc`. At 60%:
  `bg-primary #170502`, `bg-secondary #250d07`, `border #3c221b`,
  `text-primary #f4e9e6`. Full strength goes maroon near black.

### Variations (2026-10-24)

**Asked (Joshua):** "Make 3-4 variations that can be chosen. Your
algorithm for this picture chose the brown tone — but we should make
variations by switching through the other major colors. Let the user pick
one."

- **One scheme per major hue** (`Scheme::variations`, at most 4): the
  heaviest bins of the tint histogram, each at least 30° from those
  before it and with at least 5% of the heaviest's weight, the dominant
  first. Each tints with its hue (at the picture's one strength) and takes
  as accent the most vivid hue at least 60° from it (else its own). A grey
  picture offers one, grey. Each is named by its hue (Rose, Ember, Amber,
  Gold, Olive, Green, Teal, Sky, Blue, Violet, Magenta).
- **The shell publishes them** (sticky `Topic::ThemeVariations { path,
  choices: [{ name, base, raised, accent }] }`, the colors at the current
  strength) after sampling and on a strength change; `Appearance.variation`
  picks one (past the end: the last).
- **Settings** shows them under the strength while adapting is on, as
  small windows (the raised titlebar over the base, an accent pill, the
  name under it), the picked one ringed in the accent; a click picks.

### Custom colors and highlights (2026-10-24)

**Asked (Joshua):** "allow in that same place some options for the user to
manually customize certain colors — base backgrounds, highlights, etc.";
"the tab highlights and focus look grey on this color scheme, did those get
any influence from the color selection?" — they did only faintly: the
sidebar's rows were white mixes of the column (a tint washed out to grey),
and the window's focus rim is white by the Next decision.

- **Highlights keep the hue:** a sidebar row's lift is the white mix's
  lightness with the column's own hue and chroma (`style::lift_in_hue`,
  `theme::keep_hue`; identical on a grey). The column's **active row** is
  a new theme color, **`highlight`** (`#353535`, the old 14% lift; a kit
  atom delivered process-wide like `selection`; adapted like the other
  surfaces). The focus rim stays white.
- **Custom colors:** Settings → Appearance → *Custom colors*: Window,
  Panels, Highlight, Accent (`Appearance.colors`, key → `#rrggbb`), each a
  swatch opening the kit's color picker, **Use** / **Reset**. They apply
  over the greys or the picture's scheme (`Theme::customize`): Window →
  `bg-primary`; Panels → `bg-secondary` and the switcher's fill; Highlight
  → `highlight`, and `selection` takes its hue at its own lightness;
  Accent → `accent`, `text-accent`, `accent-dim` (10%).
- **The shell makes the managed colors from scratch each time:** the
  seed's greys, the picture's scheme over them when adapting, your colors
  over that; with neither, the greys once and then hands off.

## The idle fade (2026-10-24)

**Asked (Joshua):** "a 'fade out' effect — sort of in place of a screen
saver. After about 5 minutes (configurable) of no user input, start
'fading' all the windows by adding transparency … about 30 minutes to fully
transparent. On any user input, it snaps back to full opacity."

- **Setting:** Settings → Appearance → Idle: *Fade the windows when idle*
  (on by default) and *Start after* 1–60 minutes (5); the fade itself takes
  30 minutes (`Appearance::FADE_MINUTES`). Persistent in
  `Topic::Appearance { fade_idle, fade_after_min }`.
- **How (sola-river `client/fade.rs`):** fading every window toward the
  picture is the same image as the picture rising over every window
  (`a·window + (1−a)·picture`), so sola-river puts a shell surface at the
  **top** of the render list showing the picture — a second buffer over
  the background's own memory; black without a picture — and raises its
  opacity with **`wp_alpha_modifier_v1`** (`set_multiplier`, a number per
  step, about once a second; no repaint, no upload). Its input region is
  empty. It covers the top row and the bar too ("all the windows").
- **Idle** comes from **`ext_idle_notifier_v1.get_idle_notification`**
  (rivulet's wlroots idle notifier; any pointer or key input, real or
  virtual, is activity): `idled` starts the fade, `resumed` takes the
  overlay away at once (opacity 0, then destroyed). The notification
  honors **idle inhibitors** (an app keeping the screen awake), and no fade
  runs while a window is **fullscreen** (Cinema; it starts over after).
- **Why not real window opacity:** River's window-management protocol
  has no per-window opacity, and a client can only set its own surfaces'
  alpha; the overlay needs no compositor change and costs one composite a
  second while idle.

## Not now

A picture picker in Settings (recent pictures), a picture per
workspace or per output (Sola frames one output), slideshows, dimming or
blurring the picture under the windows, a default picture shipped with
Sola, rounding floating foreign windows (still needs rivulet to clip).
For the idle fade: a configurable fade length, fading only the windows
(not the top row), dimming instead of fading, waking on a notification.
For the adapted theme: a light theme from a light
picture, keeping a custom-colored preset under it (off returns to the
seed's greys), the terminal's ANSI colors.
