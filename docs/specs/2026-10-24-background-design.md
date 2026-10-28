# The background picture

**Date:** 2026-10-24
**Status:** **Accepted** 2026-10-24 (Joshua: "find a nice background image
that fits this desktop resolution and set it … perhaps in the paint app we
can have a menu item to set it as background image … copied into the
users sola settings"; "pick something from james webb"). Built on `next`, applied and smoked
on canto 2026-10-24 (plan [`plans/2026-10-21-next-tab-bar-plan.md`](../plans/2026-10-21-next-tab-bar-plan.md)
Phases 16–21: the picture; the theme following it; variations and the idle
fade; custom colors and rims; modal colors, your looks, the fade's length;
the fade's return and what it keeps).
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
  - **Tint:** one of the picture's colors (see *Modes, not averages*):
    its hue, at a chroma of *strength* × its own chroma (at most 0.08),
    thinning toward black (below L 0.18, in proportion), on the surfaces
    (`bg-*`, `selection`, `border*`, `text-muted`, `shell-menubar-bg`,
    `shell-switcher-bg`); text takes 30% of it.
  - **Accent:** the best-scored of the picture's colors at least 60° from
    the tint (a complement, e.g. the Cosmic Cliffs' blue sky against
    their orange), else the tint's hue; as vivid as the picture is in that
    hue family (its peak chroma, 0.06–0.14), at the seed cyan's lightness
    (0.82), stepped down to 0.68 for a hue that cannot be vivid that light
    (blue) (`accent`, `text-accent`, `accent-dim` at 10%, `shell-flower`
    — except a grey picture's, which keeps the seed amber).
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

- **One scheme per color the picture is made of** (`Scheme::variations`,
  at most 4, the best first; see *Modes, not averages*). Each tints with
  its color and takes as accent the best color at least 60° from it (else
  its own). A grey picture offers one, grey. Each is named by its hue
  (Rose, Ember, Amber, Gold, Olive, Green, Teal, Sky, Blue, Violet,
  Magenta).
- **The shell publishes them** (sticky `Topic::ThemeVariations { path,
  choices: [{ name, base, raised, accent, source }] }`, the colors at the
  current strength, `source` the picture's color itself) after sampling
  and on a strength change; `Appearance.variation` picks one (past the
  end: the last).
- **Settings** shows them under the strength while adapting is on, as
  small windows (the raised titlebar over the base, an accent pill, the
  name under it after a dot of the picture's color), the picked one
  ringed in the accent; a click picks.

### Modes, not averages (2026-10-24)

**Asked (Joshua):** "It looks like you may be taking 'averages' of pixel
tones to determine your color options? I feel like more of a sample from
the modes would offer more interesting options. Averages have a tendency
to be dull."

- **What it was:** the hues were already peaks of a histogram (modes),
  but every scheme's strength was the picture's **mean chroma** — over all
  of it, grey and black included. On the Cosmic Cliffs that is 0.071 (near
  the cap, so it hardly showed); on darker pictures it thins every scheme
  toward grey (Webb's Cartwheel 0.025, another Webb nebula 0.016), and
  every scheme had the same strength whatever its own color. Hue bins 30°
  apart also gave near twins (Ember and Amber; Teal twice on a forest).
- **Now (`sola_core::theme::adapt`):** colored pixels are counted in
  narrow OKLCH **cells** (10° × 0.02 chroma × 0.1 lightness); a cell with
  at least 0.2% of the pixels (and two) and chroma ≥ 0.03 is one of the
  picture's colors, its pixels' mean (a cell is narrow, so that is the
  color they share — a mode). Each is **scored as Material Color
  Utilities' `Score` does** (Material You's wallpaper colors): 70 × the
  share of the picture its hue family (±15°) covers, plus its chroma
  against a target of 0.11 (× 44 below, × 132 above; CAM16's 48 and
  weights scaled to OKLCH), families under 1% dropped. The schemes are
  the best colors **as far apart in hue as they can be** — 90° first,
  closing in (to 30°) only until there are four. Each tint is as colorful
  as its own color (to 0.08).
- **Measured** (eight pictures, 96 px samples, cards at 60%): more
  distinct schemes and stronger tints where the picture is dark (the
  Cartwheel's 0.025 → 0.03–0.08 per scheme); on the Cosmic Cliffs Ember /
  Blue / Rose (Amber, a near twin of Ember, gone). Lightness stays the
  grey theme's — the near-black surfaces are still the main reason a
  scheme reads darker than the picture's color.
- **Research:** Heckbert's popularity and median-cut quantizers (1982)
  and k-means all represent a group by its mean, which drifts to grey
  when the group spans hues; Material Color Utilities quantizes (Wu, then
  weighted k-means) and scores by hue-family proportion and chroma; Lin &
  Hanrahan, *Modeling How People Extract Color Themes from Images* (CHI
  2013): people pick salient, diverse colors, not the most frequent.

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
  surfaces).
- **The focus rim follows the highlight** (Joshua: "tint the focus rim
  too"): a focused window's 1 px rim is the theme's `highlight` a step
  lighter (4% white) in its own hue (`theme::focus_rim`) — `#3d3d3d` on
  the grey theme, where white at 18% gave `#373737`–`#3f3f3f`; tinted on a
  scheme, your highlight's hue when you set one. Kit frames draw it; for
  foreign windows sola-river follows `Topic::Theme` (River's border on
  floats, the rim along a tile's rounded corners). Unfocused rims stay a
  faint white, now **opaque** — the window's fill lightened 7% in its hue
  (the bar's island 12%): the fill's outermost pixel is only partly
  covered (antialiasing), and since tiles' fills went clear for the
  picture, a translucent rim let the picture brighten every window's edge
  (`#4c4c4c`–`#767678` on the Cosmic Cliffs). Foreign windows' unfocused
  rim is opaque too (`theme::rest_rim`: the base 7% lighter; River's
  border and the corners' rim): Steam (XWayland) has a black outermost
  column and row, and the translucent rim showed it as a dark line
  (`#121212`) around the window. Steam's own 1 px grey border inside it
  stays (its content).
- **Custom colors:** Settings → Appearance → *Your colors*: Window,
  Panels, Highlight, Accent (`Appearance.colors`, key → `#rrggbb`), each a
  swatch opening the kit's color picker, **Use** / **Reset**.
- **Your looks** (Joshua: "there needs to be a way to distinguish that
  edit from the original choice and be able to reset it … a 'Custom'
  swatch … or a copy or customize button that creates a new user
  swatch"): changing a color while a scheme is in use makes a **look of
  your own** from it (`Appearance.customs: [{ variation, colors }]`, up to
  6, shown as cards *Custom*, *Custom 2*, … after the schemes) and puts it
  in use (`Appearance.custom`); the scheme is untouched — click it to go
  back, the card to return. While a look of yours is in use, changes go to
  it; **Reset** gives one color back to its scheme, **Remove Custom**
  forgets the look. `variation` and `colors` stay the look in use's, so
  the shell is unchanged; colors saved before looks become one. They apply
  over the greys or the picture's scheme (`Theme::customize`): Window →
  `bg-primary`; Panels → `bg-secondary` and the switcher's fill; Highlight
  → `highlight`, and `selection` takes its hue at its own lightness;
  Accent → `accent`, `text-accent`, `accent-dim` (10%), `shell-flower`.
- **The shell makes the managed colors from scratch each time:** the
  seed's greys, the picture's scheme over them when adapting, your colors
  over that; with neither, the greys once and then hands off.

## The idle fade (2026-10-24)

**Asked (Joshua):** "a 'fade out' effect — sort of in place of a screen
saver. After about 5 minutes (configurable) of no user input, start
'fading' all the windows by adding transparency … about 30 minutes to fully
transparent. On any user input, it snaps back to full opacity."

- **Setting:** Settings → Appearance → Idle: *Fade the windows when idle*
  (on by default), *Start after* 1–60 minutes (5) and *Fade over* 1–120
  minutes (30; Joshua: "add a slider also for the 30 minute part").
  Persistent in `Topic::Appearance { fade_idle, fade_after_min,
  fade_over_min }`; a new length takes from the next step of a fade
  already under way.
- **How (sola-river `client/fade.rs`):** fading every window toward the
  picture is the same image as the picture rising over every window
  (`a·window + (1−a)·picture`), so sola-river puts a shell surface at the
  **top** of the render list showing the picture — a second buffer over
  the background's own memory; black without a picture — and raises its
  opacity with **`wp_alpha_modifier_v1`** (`set_multiplier`, a number per
  step, about once a second; no repaint, no upload). Its input region is
  empty. It covers the top row and the bar too ("all the windows") —
  except as below.
- **The top row's left group stays in sight** (Joshua: "the flower menu
  icon and any islands — things the user may want to respond to, the ones
  aligned to the left — should never go below 20%"): the shell publishes
  the group's extent (sticky `Topic::IdleKeep { x, y, w, h }`, output-local:
  from the left edge to a gap past the last agent island or +N, the row's
  full height; again when the islands, fonts or output change). sola-river
  cuts the overlay around it — the kept rectangle and up to four around
  it, each its own surface over a buffer that is a sub-rectangle of the
  picture's memory (an offset and the full stride; nothing copied) — and
  the kept piece's opacity stops at 80%. An island that appears while
  idle (an agent finishing) cuts it again, the new pieces placed before
  the old go.
- **Idle** comes from **`ext_idle_notifier_v1.get_idle_notification`**
  (rivulet's wlroots idle notifier; any pointer or key input, real or
  virtual, is activity): `idled` starts the fade; `resumed` brings the
  windows back **over a second** (Joshua: "animate the return over a
  second"), the overlay's opacity stepped down every bus tick (20 ms) as
  `from × (1 − t)²` — quick at first, then settling — and then destroyed.
  Turning the fade off still clears it at once. The notification
  honors **idle inhibitors** (an app keeping the screen awake), and no fade
  runs while a window is **fullscreen** (Cinema; it starts over after).
- **The desk's opacity (2026-10-26):** Joshua: "Can we make the default
  opacity for desktop items 90% … put the max transparency in our
  appearance settings ..default to 90". Settings → Appearance → Desk →
  *Opacity*, 50–100% (90), persistent `Appearance.opacity`. At rest the
  same overlay stays up at `100 − opacity` % (its floor), so the windows,
  the bar and the top row are that opaque over the picture; at 100 % it is
  only up while fading. The fade rises from the floor to the picture, a
  return comes back to the floor, and the kept piece (the flower and the
  agent islands) stays at the floor. Nothing is over a fullscreen window
  (it scans out). The overlay is made again when the picture, the output's
  size or the kept rectangle change. The slider sends as it moves.
- **Why not real window opacity:** River's window-management protocol
  has no per-window opacity, and a client can only set its own surfaces'
  alpha; the overlay needs no compositor change and costs one composite a
  second while idle.

## From the browser (2026-10-24)

Joshua: "make a right click menu. If its a picture, we say 'Use as desktop
background'… first see if the picture links to a higher-res version… if
the aspect ratio is not identical to the desktop, show a lightbox… with the
aspect ratio overlaying the image… Save the image. Set as desktop
background."

- **The menu.** Right-click a picture on a page: **Use as Desktop
  Background** heads the browser's page menu (CEF's media type is an image
  and it has a source).
- **The largest version.** The browser fetches, with the page as referrer,
  the link the picture sits in, Wikimedia's original behind a thumbnail
  (`{upload,thumb}.wikimedia.org/…/thumb/a/ab/Name.jpg/640px-…?…` →
  `upload.wikimedia.org/…/a/ab/Name.jpg`), and the picture
  itself; a link to a page (`text/*`) is skipped; the most pixels win
  (`sola-browser` `wallpaper.rs`). Up to 150 MB; decoding may use 1 GB.
- **The lightbox.** When the picture's shape differs from the output's
  (`Topic::OutputGeometry`; more than 1%), the page window shows it on a
  near-black veil with a frame in the output's shape: the largest such part,
  centered. Drag to move it; scroll to take less (down to a fifth) or more;
  outside it is dimmed; Return or **Use as Desktop Background** uses it,
  Esc or Cancel leaves. The same shape skips the lightbox.
- **Saved.** The chosen part as a JPEG (quality 92) — or the file as
  fetched when nothing was cut — in `~/Pictures/Backgrounds/`
  (`$XDG_PICTURES_DIR`), then adopted as the background
  (`sola_core::background::adopt`, `Topic::Background`); the shell
  whispers "Set as desktop background".
- **Drawn** as an overlay over the page window, below its titlebar and
  inside its rim (the bottom corners round with the window's): a
  translucent veil does not cover the page under it, so it is opaque; the
  picture and the frame are two canvases (in one, iced draws the frame's
  fills under the picture).
- **The page menu itself** never showed until now: the engine emptied CEF's
  menu model so CEF would draw nothing, and an empty model means CEF never
  asks `run_context_menu` either.

## Not now

From the browser: a picture's `srcset`, CSS backgrounds, a page's own
full-size viewer (Flickr, Unsplash) — only the link, Wikimedia, and the
picture are tried. A picture picker in Settings (recent pictures), a picture per
workspace or per output (Sola frames one output), slideshows, dimming or
blurring the picture under the windows, a default picture shipped with
Sola, rounding floating foreign windows (still needs rivulet to clip).
For the idle fade: a configurable fade length, fading only the windows
(not the top row), dimming instead of fading, waking on a notification.
For the adapted theme: a light theme from a light
picture, keeping a custom-colored preset under it (off returns to the
seed's greys), the terminal's ANSI colors.
