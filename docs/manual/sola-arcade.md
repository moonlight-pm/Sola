# sola-arcade

**Status:** dogfood (banner list + windowed gamescope nest).

## What it is

Sola’s **Arcade** is a vertical list of **installed Steam titles** (on-disk
library manifests + Steam hero banner art when cached). **Play** starts the
title under **windowed gamescope** so the game is one normal host window
(float, zone, Meta+Tab).

## Requirements

- Steam installed (`steam` on `PATH`)
- Games installed in a Steam library (default roots under `~/.local/share/Steam`,
  `~/.steam/steam`, Flatpak Steam path, plus `libraryfolders.vdf`)
- **Recommended:** `gamescope` on `PATH`. Without it, launches fall back to
  bare `steam -applaunch`.

## UI

1. Open **Arcade** from the launcher (Meta+Space).
2. **Search** is the only chrome above the list (filters by name or AppID).
   Refresh is Meta+R from the app menu.
3. Each row uses Steam **`library_hero`** art (1920×620) as a faded full-width
   background, large typeface title on the left, and actions on the right:
   - **Play** — nest launch  
   - **Store** — Steam store page in the browser  
   - **Uninstall** — Steam’s uninstall UI (`steam://uninstall/<id>`)
4. While a title is **loading** or **playing**:
   - That row’s **Play** becomes **Stop**
   - All other rows’ Play is disabled
   - List **scroll position is preserved** (Play→Stop does not jump to top)
5. **Stop** (on the active row, or menu Meta+Shift+S) ends the session via
   `CloseApp` on `steam-game-<id>`.
6. **Quit from the game’s own menu** ends the game process; Arcade then
   stops the **nested** Steam client so the gamescope host closes (does not
   touch a Steam you started outside Arcade).

## Launch / nest

Play emits `LaunchApp`:

```text
/opt/sola/bin/sola-arcade --run <appid> 1920 1080
```

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

### Host window vs game resolution

| Layer | Behavior |
|-------|----------|
| **Host window** | Normal Sola window — zones and floats like any app. Initial size 1920×1080; shell Frames set size after map. |
| **Game resolution** | Whatever the title uses in its settings (not forced by Arcade). |
| **Fit** | gamescope `-S fit` letterbox-scales nested content into the host (aspect preserved, black bars). |

While a nest is up, Arcade publishes a catalog/menu label so the **menubar and
app switcher show the game title**. River rewrites empty gamescope `app_id` →
`gamescope` from the process cmdline when needed.

Silent/nest launch usually keeps Steam’s own UI out of the way; there is no
in-Arcade “Hide Steam” toggle.

## Limits (honest)

- Some titles fail under the nest (no host window / crash) — game-dependent.
- Banner art only when Steam has cached `library_hero` (or header).
- Multi-store (GOG, Epic, …) not in this app.
- Host resize + letterbox can still stress mouse mapping on some titles.

## Related

- Capability: `arcade` in [`docs/capabilities.md`](../capabilities.md)
