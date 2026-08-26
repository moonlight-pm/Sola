# sola-arcade

**Status:** dogfood (banner list + windowed gamescope nest).

## What it is

Sola’s **Arcade** is a vertical list of Steam titles from on-disk data
(library manifests, localconfig activity, appinfo names) plus Steam hero
banner art when cached. **Play** starts an installed title under **windowed
gamescope** so the game is one normal host window (float, zone, Meta+Tab).

## Requirements

- Steam installed (`steam` on `PATH`)
- Games installed in a Steam library (default roots under `~/.local/share/Steam`,
  `~/.steam/steam`, Flatpak Steam path, plus `libraryfolders.vdf`)
- **Recommended:** `gamescope` on `PATH`. Without it, launches fall back to
  bare `steam -applaunch`.

## UI

1. Open **Arcade** from the launcher (Meta+Space).
2. **Search** filters by name or AppID. On the same row, icon tools (hover
   for tooltips):
   - **A–Z** (`arrow-down-a-z`) — alphabetical sort  
   - **Recent** (`history`) — most recent player activity first
     (`LastPlayed` / install `LastUpdated`)  
   - **Ready to play only** (`circle-check`, toggle, **on by default**) —
     Steam-style filter: only fully installed titles. Turn **off** to also
     list uninstalled games from activity history (faded banner + **Install**).
   Refresh is Meta+R from the app menu (re-scans Steam in the background).

3. **Library cache:** opens **immediately** from
   `~/.config/sola/arcade-library.json` when present, then always re-scans
   Steam **in the background** and updates the cache. The first launch (no
   cache yet) shows an **initial scan** status until the first scan finishes;
   later opens use the cache and stay quiet while the background refresh runs.
4. Each row uses Steam **`library_hero`** art (when cached on disk) as a faded
   full-width background. Banners are **decoded lazily** for rows in the
   scroll viewport (plus a small overscan), so first paint is not blocked by
   the whole library. Large typeface title on the left, actions on the right:
   - **Play** — nest launch (installed only)  
   - **Nest size** (installed, when gamescope is available) — **Fit to window**
     or a locked resolution. Mutually exclusive; default **1080p**. Fit
     starts from the display at Play, then tracks the gamescope host frame
     when you zone or float. A resolution locks that size. The game may
     still pick a lower internal res. Keep **fullscreen on** for Fit
     (Factorio has no resolution list — fullscreen means “use the nest”).  
   - **Install** — Steam’s install UI (`steam://install/<id>`) for uninstalled  
   - **Store** — Steam store page in the browser  
   - **Uninstall** — Steam’s uninstall UI (`steam://uninstall/<id>`) when installed
5. While a title is **loading** or **playing**:
   - That row’s **Play** becomes **Stop**
   - All other rows’ Play is disabled
   - List **scroll position is preserved** (Play→Stop does not jump to top)
6. **Stop** (on the active row, or menu Meta+Shift+S) ends the session via
   `CloseApp` on `steam-game-<id>`.
7. **Quit from the game’s own menu** ends the game process; Arcade then
   stops the **nested** Steam client so the gamescope host closes (does not
   touch a Steam you started outside Arcade).

## Launch / nest

Play emits `LaunchApp`:

```text
/opt/sola/bin/sola-arcade --run <appid> <width> <height> [fit]
```

`<width> <height>` is the nested virtual monitor (Fit → output pixels at
Play, otherwise the locked resolution; default 1920×1080). A trailing
`fit` token tells Arcade to retarget the nest when the host window size
changes (nested X mode-control; not gamescope `--force-windows-fullscreen`,
which aborted the Wayland backend).

Nest rules:

- **Steam not running** →  
  `gamescope … -- sola-arcade --nested-steam <id>`  
  which rewrites desktop identity and runs `steam -nofriendsui -applaunch <id>`  
  (never host `-f`; **no `-e`**).
- **Steam already open** → bare `steam -applaunch` only (no nest). Arcade
  **will not** force-kill Steam. Quit Steam yourself for a nest on the next Play.

### Steam prepare (shaders / updates) — automatic

First launch of a Proton title often runs Steam’s **shader pre-cache** (and
similar prepare steps) before the game process starts. That is handled **inside
the nest**.

**Why not bare `steam` under gamescope?** gamescope sets
`XDG_CURRENT_DESKTOP=gamescope` for children. Steam then forces **gamepad UI /
Big Picture** (`forcing gamepadui for steamdeck + gamescope`) and `-applaunch`
never finishes. Arcade’s `--nested-steam` helper sets a normal desktop identity
so Steam stays desktop CEF (prepare dialogs can complete) without BPM.

Arcade keeps the session in the “loading / Stop” state for up to a few minutes
while Steam prepares. River holds the host size briefly after map so the nest
stays stable during that phase, then normal zone/float sizing applies.

When the game process exits (in-game quit), the nested-steam helper detects the
gone `AppId=<id>` reaper and kills the nested Steam client so the host window
closes. **Stop** in Arcade does the same path via `CloseApp` + local pkill.

### Host window vs nest size

| Layer | Behavior |
|-------|----------|
| **Host window** | Normal Sola window — zones and floats like any app. Initial size matches the nest setting; shell Frames set size after map. |
| **Nest size** | Per title, next to Play: **Fit to window** or a resolution (720p / 1080p / 1440p / 4K / native, dropping sizes above the display). Default **1080p**. Persisted in `~/.config/sola/arcade-nest.json`. Fit uses the display size **at Play**, then follows the host frame (zone/float): Arcade writes `GAMESCOPE_XWAYLAND_MODE_CONTROL` on the nested X root and moves the focused game to `0,0`. Locked resolutions stay put. Arcade must stay running for live follow. |
| **In-game resolution** | Titles with a picker can still choose a lower internal res than the nest. Titles without one (Factorio) use the nest as their display — keep **fullscreen on** for Fit. Windowed-in-game ignores the nest resize and can leave clicks dead. |
| **Scaler** | gamescope `-S fit` letterbox-scales nested content into the host when the two sizes differ (aspect preserved, black bars). Fit aims for 1:1 so the letterbox goes away after the host settles. |

While a nest is up, Arcade publishes a catalog/menu label so the **menubar and
app switcher show the game title**. River rewrites empty gamescope `app_id` →
`gamescope` from the process cmdline when needed.

Silent/nest launch usually keeps Steam’s own UI out of the way; there is no
in-Arcade “Hide Steam” toggle.

## Limits (honest)

- Some titles fail under the nest (no host window / crash) — game-dependent.
- Banner art only when Steam has cached `library_hero` (or header).
- Uninstalled list (when Ready-to-play is off) is **offline** (localconfig +
  appinfo): never-played owned titles with no local activity may be missing;
  no Steam Web API.
- Multi-store (GOG, Epic, …) not in this app.
- Host resize + letterbox can still stress mouse mapping on some titles.
  **Fit to window** retargets nested size after Play; titles that ignore
  RandR still letterbox. Keep the game fullscreen for Fit.

## Related

- Capability: `arcade` in [`docs/capabilities.md`](../capabilities.md)
