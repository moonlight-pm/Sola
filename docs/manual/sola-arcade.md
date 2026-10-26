# Arcade

Arcade is a vertical list of your Steam titles, built from Steam's data on
disk (library manifests, `localconfig` activity, `appinfo` names), with
Steam's hero banner art when it is cached. **Play** starts an installed
title under windowed gamescope, so the game is one normal host window: it
floats, zones, and has a tab bar row like any other.

## Requirements

- Steam installed (`steam` on `PATH`).
- Games installed in a Steam library. Arcade looks in the default roots
  under `~/.local/share/Steam`, `~/.steam/steam`, and the Flatpak Steam
  path, plus the libraries listed in `libraryfolders.vdf`.
- Recommended: `gamescope` on `PATH`. Without it, Play falls back to bare
  `steam -applaunch`.

## Using Arcade

1. Open **Arcade** from the launcher (Super+Space).
2. **Search** filters by name or AppID. The icon tools on the same row sort
   and filter the list (hover for tooltips); the sort mode is kept in
   `~/.config/sola/arcade-prefs.json`. Super+R in the app menu re-scans
   Steam in the background.
   - **A–Z** (`arrow-down-a-z`): alphabetical sort.
   - **Recent** (`history`): most recent player activity first
     (`LastPlayed`, or the install's `LastUpdated`).
   - **Ready to play only** (`circle-check`, a toggle, on by default):
     like Steam's filter, lists only fully installed titles. Turn it off to
     also list uninstalled games from your activity history, with a faded
     banner and **Install**.
3. The list opens at once from `~/.config/sola/arcade-library.json` when
   that cache exists; Arcade then always re-scans Steam in the background
   and updates the cache. The first launch, with no cache yet, shows an
   initial-scan status until the first scan finishes. Later opens use the
   cache and stay quiet while the background refresh runs. Arcade also
   watches each library's `steamapps/` folder (install manifests and
   `libraryfolders.vdf`), so installing or uninstalling in Steam updates
   the list after a short delay, without Super+R.
4. Each row shows Steam's `library_hero` art, when it is cached on disk, as
   a faded full-width background. Banners are decoded only for rows in the
   scroll viewport (plus a few beyond it), so the first paint does not wait
   for the whole library. The title is on the left in a large typeface;
   the actions are on the right:
   - **Play**: launch in the nest (installed titles only).
   - **Nest size** (installed titles, when gamescope is available):
     **Fit to window** or a locked resolution, one or the other; the
     default is **1080p**. Fit starts from the display size at Play, then
     tracks the gamescope host window as you zone or float it. A resolution
     locks that size. The game may still pick a lower internal resolution.
     Keep the game's fullscreen setting on for Fit (Factorio has no
     resolution list; fullscreen there means "use the nest").
   - **Install**: Steam's install UI (`steam://install/<id>`), for
     uninstalled titles.
   - **Store**: the title's Steam store page in the browser.
   - **Uninstall**: Steam's uninstall UI (`steam://uninstall/<id>`), for
     installed titles.
5. While a title is loading or playing:
   - that row's **Play** becomes **Stop**;
   - Play on every other row is disabled;
   - the list keeps its scroll position (Play turning into Stop does not
     jump to the top).
6. **Stop** (on the active row, or Super+Shift+S in the menu) ends the
   session through `CloseApp` on `steam-game-<id>`, then stops only the
   nest processes Arcade owns (`--run`, `--nested-steam`, and that
   gamescope). It does not `pkill` a Steam `AppId=` you started outside
   Arcade.
7. Quitting from the game's own menu ends the game process. Arcade then
   stops the nested Steam client so the gamescope host window closes. A
   Steam you started outside Arcade is not touched.

## Launch and nest

Play emits `LaunchApp` with:

```text
/opt/sola/bin/sola-arcade --run <appid> <width> <height> [fit]
```

`<width> <height>` is the nested virtual monitor: with Fit, the output's
pixels at Play; otherwise the locked resolution (default 1920×1080). A
trailing `fit` tells Arcade to retarget the nest when the host window size
changes (through nested X mode control).

Nest rules:

- **Steam not running**: Arcade runs
  `gamescope … --cursor-scale-height <H> -- sola-arcade --nested-steam <id>`,
  which rewrites the desktop identity and runs
  `steam -nofriendsui -applaunch <id>`. gamescope never gets host `-f`, and
  never `-e`. Nested X cursors are downsampled so the pointer over the game
  stays desktop-sized; otherwise gamescope would hand River the fake
  monitor's cursor at 1:1.
- **Steam already open**: Play is refused, and the status asks you to quit
  Steam first. Arcade does not force-kill Steam and does not `-applaunch`
  into the running client (that often takes exclusive fullscreen on Sola).
  Quit Steam, then Play nests.

Opening Arcade again from the launcher raises the existing window, so there
is never a second Fit driver. The `--run` and `--nested-steam` helper
processes are not covered by that single-instance rule.

### Steam prepare (shaders and updates)

The first launch of a Proton title often runs Steam's shader pre-cache (and
similar prepare steps) before the game process starts. Arcade handles this
inside the nest, with no action from you.

Arcade does not run plain `steam` under gamescope. gamescope sets
`XDG_CURRENT_DESKTOP=gamescope` for its children; Steam then forces the
gamepad UI (Big Picture; its log says
`forcing gamepadui for steamdeck + gamescope`), and `-applaunch` never
finishes. Arcade's `--nested-steam` helper sets a normal desktop identity,
so Steam stays in its desktop (CEF) UI, the prepare dialogs can complete,
and Big Picture stays off.

Arcade keeps the session in the loading (Stop) state for up to a few
minutes while Steam prepares. River holds the host window's size briefly
after it maps, so the nest stays stable during that phase; then normal zone
and float sizing applies.

When the game process exits (an in-game quit), the nested-steam helper sees
that the `AppId=<id>` reaper is gone and kills the nested Steam client, so
the host window closes. Steam's install-script reapers
(`AppId=<id> Install=1`, `iscriptevaluator.exe`) are not the game: Arcade
waits through VC redist and prepare steps, so Proton titles (Palworld, for
example) are not torn down after about 10 seconds. **Stop** in Arcade takes
the same path, through `CloseApp` and a local `pkill`.

### Host window and nest size

| Layer | Behavior |
|-------|----------|
| **Host window** | A normal Sola window: it zones and floats like any app. Its initial size matches the nest setting; shell Frames set the size after map. |
| **Nest size** | Per title, next to Play: **Fit to window** or a resolution (720p, 1080p, 1440p, 4K, or native; sizes above the display are left out). Default **1080p**. Kept in `~/.config/sola/arcade-nest.json`. Fit uses the display size at Play, then follows the host window as you zone or float it: Arcade writes `GAMESCOPE_XWAYLAND_MODE_CONTROL` on the nested X root and moves the focused game to `0,0`. Locked resolutions stay put. Arcade must stay running for Fit to follow. |
| **In-game resolution** | Titles with a resolution picker can still choose a lower internal resolution than the nest. Titles without one (Factorio) use the nest as their display; keep fullscreen on for Fit. Windowed mode in the game ignores the nest resize and can leave clicks dead. |
| **Scaler** | gamescope `-S fit` letterbox-scales the nested content into the host window when the two sizes differ (aspect kept, black bars). Fit aims for 1:1, so the letterbox goes away once the host window settles. |
| **Pointer** | The host cursor is gamescope's nested X cursor, scaled to about 36 px through `--cursor-scale-height` (the same as the initial `-H`). Without that, Factorio-class titles show a giant pointer over the nest. |

While a nest is up, Arcade publishes a catalog and menu label, so the
menubar and the tab bar show the game's title. River rewrites an empty
gamescope `app_id` to `gamescope` from the process command line when
needed.

Nest launches usually keep Steam's own UI out of the way. Arcade has no
"Hide Steam" toggle.

## Limits

- Some titles fail under the nest (no host window, or a crash); this
  depends on the game.
- Banner art shows only when Steam has cached `library_hero` (or the
  header image).
- With **Ready to play only** off, the uninstalled list is offline
  (`localconfig` and `appinfo`): owned titles you have never played, with no
  local activity, may be missing. Arcade does not use the Steam Web API.
- Other stores (GOG, Epic, …) are not in Arcade.
- Host window resizes and letterboxing can still upset mouse mapping in
  some titles. **Fit to window** retargets the nested size after Play;
  titles that ignore RandR still letterbox. Keep the game fullscreen for
  Fit.
