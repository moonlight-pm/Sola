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
5. **Stop** (on the active row, or menu Meta+Shift+S) ends the session via
   `CloseApp` on `steam-game-<id>`.

## Launch / nest

Play emits `LaunchApp`:

```text
/opt/sola/bin/sola-arcade --run <appid> 1920 1080
```

Nest rules:

- **Steam not running** →  
  `gamescope --backend wayland -b -S fit -W/-H -w/-h -- steam -silent -applaunch <id>`  
  (never host `-f`; scale-to-fit host; nested res fixed; **no `-e`**).
- **Steam already open** → bare `steam -applaunch` only (no nest). Arcade
  **will not** force-kill Steam. Quit Steam yourself for a nest on the next Play.

Default nest size **1920×1080**. Silent/nest launch usually keeps Steam’s own
UI out of the way; there is no in-Arcade “Hide Steam” toggle.

## Limits (honest)

- Some titles fail under the nest (no host window / crash) — game-dependent.
- Banner art only when Steam has cached `library_hero` (or header).
- Multi-store (GOG, Epic, …) not in this app.

## Related

- Capability: `arcade` in [`docs/capabilities.md`](../capabilities.md)
