# Omakade → sola-arcade (parked)

**Status:** idea (parked 2026-09-01). **Slice 2026-09-01** shipped in Arcade:
watch (5), session lifetime without `/proc` poll (8), narrow Stop (9),
single-instance (10), refuse Play while desktop Steam is open (18). Do not
implement the rest from this file. Promote a remaining item into a freeze +
plan + `CURRENT.md` **Now** if work starts.  
**Source:** [tsouth89/omakade](https://github.com/tsouth89/omakade) v1.2.3
(C++20 / Qt 6 / QML, GPL-3.0). Plan: their `PLAN.md`.  
**Sola locks this must not reopen:** Iced + sola-kit; windowed gamescope nest
(never host `-f`); no auto `steam -shutdown`; Arcade UI stays simple (banner
list). See root [`CURRENT.md`](../../CURRENT.md) **Locked models** and
[`manual/sola-arcade.md`](../manual/sola-arcade.md).

Omakade is a **library frontend**: one cover grid over Steam, Lutris, Heroic,
Faugus, RetroArch; Play **delegates** to the owning launcher. Arcade is a
**Sola window for the game**: Steam on-disk library + windowed gamescope nest
(Fit / locked res, host label, Stop). Steal library-engine discipline. Do not
steal Qt, cover-grid-as-identity, or “Play = `steam://` only.”

Operator preference (2026-09-01): keep the simple UI. Next interest is
**performance, stability, nest/resolution** — not details pages, collections,
controller chrome, or multi-store as a library product.

---

## Leave on the floor

| Omakade | Why it does not travel |
|---|---|
| Qt / QML / Omarchy `colors.toml` / glass cards | Kit + bus theme |
| Play = desktop URI only | Throws away nest, Fit, host label, Stop |
| Close-after-launch as default | Live Fit needs Arcade running |
| Auto-shutdown Steam | Crashes River (`river_window_management_v1`) |
| Fuzzy merge across stores | Bad merge is worse than two rows |
| Proton / Wine / ROM manager | Launchers own accounts, DRM, runners, saves |
| Couch mode before nest is solid | Input rewrite is not the nest bug |

---

## Filter: performance, stability, nest / resolution

These are the items worth a freeze. UX catalog is below, parked.

### Already true in Arcade (do not re-solve)

- Library JSON cache + background Steam scan; UI opens from cache.
- Banner path walks deferred until a row is visible; decode off the UI thread;
  downscale to `BANNER_DECODE_H` (336) before GPU upload.
- Launch argv is structured (`sola-arcade --run …`), no shell interpolation.
- Never writes Steam VDF / ACF / userdata.
- Never `steam -shutdown`. Cold Steam → nest; Steam already up → **refuse
  Play** (would exclusive-fullscreen). Quit Steam, then Play.
- Nested helper rewrites desktop identity so gamescope does not force BPM.
- Fit: nested `GAMESCOPE_XWAYLAND_MODE_CONTROL` + focused window at `0,0`;
  refuses host `$DISPLAY`. `--force-windows-fullscreen` **CLI** aborted the
  Wayland backend — property poke only.
- Host pointer: `--cursor-scale-height` = initial `-H`.
- No gamescope `-e` (host never maps).

### Steal from Omakade’s library engine

1. **Treat Steam files as untrusted; cap parse size; tolerate partial writes.**  
   Omakade’s VDF parser reports per-file errors, marks the scan incomplete, and
   keeps going. Arcade’s `vdf_string` is a first-match `"key"` scan with no size
   cap. Steam rewrites ACF/VDF in place; a torn read can drop titles or, worse,
   pick a nested key. Changelog 1.2.1 also capped Steam metadata parsing.  
   **Arcade:** real VDF (or at least AppState-scoped ACF), max file bytes,
   skip + warn instead of aborting the whole library.

2. **Bound artwork decode before `image::open`.**  
   Omakade bounds dimensions and decoded memory; Qt `sourceSize` matches
   device-pixel size so the JPEG is not fully materialized. Arcade
   `decode_banner_handle` does `image::open` (full 1920×620 RGBA) then
   Triangle-resize. A bad/huge file in `librarycache` is a spike and a
   possible OOM.  
   **Arcade:** fail closed on huge files; decode-to-target (or `image`
   limits) instead of full-buffer-then-resize. Same path for any later
   capsule/logo.

3. **Artwork cache budget + eviction.**  
   Omakade: `artworkCacheLimitMb` (default 1024), settings-clearable,
   no unbounded HTTP/art cache. Arcade holds every visited banner `Handle`
   for the process lifetime (orphans dropped only on rescan). Fine at ~40
   titles; unbounded at a big library + scroll.  
   **Arcade:** cap decoded handles (and any on-disk transcode cache if we
   add one). Evict off-viewport first.

4. **Size-aware cached variants; never upscale junk.**  
   Omakade stores the source once and generates size-aware variants; does
   not upscale a 460×215 `header.jpg` to a hero. Arcade already *prefers*
   `library_hero` then `header`, but will decode a small header at row
   height. Prefer skip / letterbox over upsample.

5. **Debounced library watch.** **Shipped 2026-09-01.**  
   Non-recursive watch on each library `steamapps/` (ACF +
   `libraryfolders.vdf`), 1s debounce, silent background rescan. Not
   `librarycache/` (art noise). Scan stays off the UI thread.

6. **Stale-install check before Play.**  
   Omakade verifies the install path still exists and names the source to
   repair. Arcade launches by AppID. A moved/unmounted library drive is a
   mysterious nest failure.  
   **Arcade:** `install_dir` under `library_path/steamapps/common/…` must
   exist (or StateFlags fully-installed still true) before `LaunchApp`.

7. **Incomplete-scan / source health, not a silent short list.**  
   Omakade keeps last scan, discovered roots (native vs Flatpak), per-source
   errors. Arcade’s empty state is “install titles in Steam.” A missing extra
   library drive looks like a missing game.  
   **Arcade:** surface scan warnings (unreadable ACF, vanished library path)
   on the existing status strip — no new settings chrome required.

8. **Do not use the Steam *client* lifetime as the game lifetime.** **Shipped 2026-09-01.**  
   UI Stop/Play follows `LaunchResult` + `UserAppExited` on `steam-game-<id>`
   (`--run` waits on gamescope). No 1s `/proc` poll. Nested-steam still
   watches `AppId=` to tear down the *nested* Steam client. Boot can still
   reattach via a one-shot `session_alive`.

9. **pkill Stop is a last resort.** **Shipped 2026-09-01.**  
   `CloseApp` first. Fallback kills only Arcade-owned cmdlines (`--run`,
   `--nested-steam`, gamescope whose argv is that helper) and their children.
   Never `pkill -f AppId=`.

10. **Single-instance / activate-or-focus.** **Shipped 2026-09-01.**  
    `$XDG_RUNTIME_DIR/sola/arcade.lock.sock`. Second spawn writes `@activate`
    and exits; live Arcade emits `Topic::Focus`. `--run` / `--nested-steam`
    do not claim the lock.

11. **Optional: SQLite (or similar) index instead of rewriting the whole JSON.**  
    Omakade’s `library.sqlite3` is the scan cursor + installations + artwork
    pointers. Arcade rewrites `arcade-library.json` in full after every scan.
    Not urgent at current library size; becomes the right store if we add
    playtime hours / watch-driven incremental updates. Not a UI change.

### Arcade-owned nest / resolution (Omakade never did this)

Omakade sidestepped every item here by not owning the game window. These are
Arcade’s actual hard surface. Listed because the comparison made the gap
obvious — not because Omakade has a patch to copy.

12. **Portal-class nest fail** — some titles never map a host / crash under
    wayland+`-b`+no `-e`. Needs a per-title diagnosis (gamescope log, Steam
    prepare never finishing, exclusive FS inside nest) and a fallback that
    is still windowed if possible.

13. **Residual flicker** on nest map / Fit poke / shader-prepare CEF.

14. **Titles that ignore RandR** stay letterboxed under `-S fit` after host
    resize. Mode-control + `ConfigureWindow` `0,0,w,h` is the Factorio path;
    it is not universal. Need a title that ignores RandR as the next probe
    (SDL vs engine-owned window).

15. **Fit requires the Arcade UI process.** If Arcade quits, follow stops.
    Options (product later): a tiny nest companion in `--run`, or accept
    locked-res when Arcade is gone. Do not close-after-launch.

16. **Host vs nest size / mouse mapping** still stressed when `-S fit`
    letterboxes. Fit aims for 1:1 after settle; locked 1080p in a zoned
    1440p host will keep bars. Worth measuring click offset after Fit
    vs after a locked res.

17. **`--cursor-scale-height` not desk-smoked** (capability gap). Factorio
    was the motivation; confirm on a second title and on Fit after host
    resize (scale height is initial `-H`, not live).

18. **No `-e`.** gamescope Steam integration held first-frame forever on
    River. Overlay / steam-mode remains unsolved. Do not flip `-e` on
    without a River map probe.

19. **Steam already running cannot nest.** **Shipped 2026-09-01 (item 18).**  
    Play is refused while a desktop Steam client is open. Status:
    “Quit Steam first. Arcade will not launch into exclusive fullscreen.”
    `--run` exits 2 if Steam is already running. Not a shutdown.

20. **Fit DISPLAY confusion** is already guarded (skip gamescope argv0,
    skip host `:0`). Keep that test; any new poke path must not touch
    host Xwayland (that aborted gamescope’s input thread).

---

## Parked: library product / UX (not now)

Kept so the session list is not lost. Do not promote while the nest is the
priority.

### Local Steam depth

- Game details (hero + capsule + Play) instead of everything on the row.
- Playtime hours from `localconfig` `Playtime` (Arcade only uses it as a
  recency stub).
- Local achievements from `userdata/.../librarycache/<appid>.json` (no API).
- Custom Steam grid art (`userdata/.../config/grid`), capsules
  (`library_600x900` / `library_capsule`), logo overlay.
- Generated fallback marks when art is missing.
- Persist Ready-to-play (already a capability gap).
- Split game identity vs installation so favorites survive a title that
  disappears and returns.

### Library chrome

- Cover grid as a *second* view (keep banner list).
- Keyboard focus (arrows / Enter / Esc) as a prerequisite if controller
  ever happens.
- Favorites + hidden.
- Sort by playtime.
- Smart filters (All / Favorites / Recent / Hidden; completion later).
- `--demo` fixture library for UI work without Steam.

### Launch honesty (UX-facing)

- Actionable errors (“install dir missing”, “Steam already running — nest
  skipped”) plus ProtonDB / PCGamingWiki on failure.
- Manage in Steam (`steam://` details), distinct from Store.
- Explicit Nest vs Play-in-desktop-Steam choice.

### Input / cinema

- SDL3 controller → existing focus model.
- Fullscreen library / cinema. Couch mode is Omakade M6; they paid for
  shipping controller after the desktop UI.

### Multi-source (only if nest still owns Play)

- Lutris / Heroic (Epic, GOG, Amazon) / RetroArch as **nest targets**, not
  URI handoff. Duplicate linking is explicit, never fuzzy.
- Faugus later. Do not become an emulator manager.

### Optional online

- Steam Web API for owned-but-never-played and achievement enrichment
  (Secret Service; local cache remains primary). Already an Arcade gap.
- IGDB critic / length estimates: skip unless a details page exists.

---

## Reference

- Omakade repo / plan / changelog: linked above.
- Arcade as-built: [`manual/sola-arcade.md`](../manual/sola-arcade.md),
  capability row `arcade` in [`capabilities.md`](../capabilities.md).
- Nest: `crates/sola-arcade/src/{launch,nest,x11_nest}.rs`.
- Scan / art: `crates/sola-arcade/src/steam.rs`.
