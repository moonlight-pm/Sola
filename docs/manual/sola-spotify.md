# sola-spotify

Kit-native Spotify client. Browse the library, search, control Spotify Connect devices, and (with Premium) play on this computer through librespot.

**Partial.** **Installed** `spotify` (release, 2026-09-02). Playback logic is adapted from [Fastpotify](https://github.com/crmne/fastpotify) (MIT). Independent of Spotify AB. Launcher row is **Spotify** (`lucide/disc`).

## Requirements

- Spotify account. **Playing on this computer needs Premium.** Free accounts can sign in, browse, search, and control another device.
- `alsa-lib` and `libpulseaudio` at compile time (`pkg-config` `alsa` + `libpulse`). The NixOS module ships both. PipeWire’s Pulse compatibility is enough at runtime.
- After a NixOS rebuild, `cargo make install spotify`. A fresh shell install is needed once for the launcher row.

## Sign in

1. Open **Spotify** from the launcher (Meta+Space).
2. **Sign in with Spotify** opens sola-browser on Spotify’s consent page (Authorization Code + PKCE). Sola never sees the password.
3. Tokens land in `~/.local/state/sola/spotify/` (`0600`). Sign in once per machine.

Playing **here** is a second, one-time approval (**Playback → Set up playback here**, or Settings). Spotify treats streaming separately from the Web API. librespot then caches a reusable credential.

## Use

- **Library** rail: Home, Search, Liked Songs, **Made for you**, Albums,
  Artists, Queue, then your playlists. Made for you is one destination
  (all Spotify mixes). Home shows a short shelf; **See all** opens the
  catalog. Generated tiles are labelled Made for you.
- Click a row or **Play** on a playlist/album to start it. With no other
  speaker on, that **sets up this computer** (a second browser approval,
  once per machine) and plays here. Premium required for local audio.
- A **cyan circled +** means the track is in Liked Songs (same library as
  the official app). Click it to like or unlike. **−** hides a
  row from autoplay: struck title, dim cover, muted meta — does not
  delete from the playlist. Click − again to restore. **−** on the
  playing song skips to the next track (once).
- **list-plus** (row or player bar) adds the song to a playlist you
  own or collaborate on. Last destination is first; type to filter;
  Enter adds to the first match. **New playlist** creates a private
  list named “New playlist” and adds the song. A short **Added to …**
  notice confirms. Escape or a click outside closes the picker.
- Reopen restores the last playlist and the last track (graphite lift =
  selected; cyan play mark + accent title = playing).
- Playlist pages show added dates on the header and on each row.
- **Albums** and **Artists** are wrapping catalogs (same as Made for you).
- Bottom bar: now playing on the left, transport in the center. Idle is
  “Nothing playing” / “Pick a song”; setup copy only if this computer is
  not connected yet. Play/pause, skip, seek, shuffle, repeat, like,
  add to playlist, volume, device picker. **This computer** is always the
  first device. Other Spotify Connect speakers sit under **Other devices**.
- **Space** play/pause; **⌘← / ⌘→** previous/next; **⌘F** search; **⌘H** home; **⌘L** liked.
- **Back / Forward** chevrons sit at the top of the main view (library stays put). **⌥← / ⌘[** back; **⌥→ / ⌘]** forward. Mouse back/forward buttons do the same. Disabled until there is somewhere to go.

Media keys (`solactl media`) go to whichever MPRIS player is Playing. This app registers as `org.mpris.MediaPlayer2.sola-spotify`. Play, pause, next, and previous apply as soon as the key is pressed.

## Files

| Path | What |
|------|------|
| `~/.config/sola/spotify/settings.json` | Connect device name, bitrate, gapless, autoplay, last page, last track, last playlist |
| `~/.local/state/sola/spotify/` | Web API refresh token, librespot credentials, `skipped.json`, `liked.json` |
| `~/.cache/sola/spotify/` | Audio, album art, page JSON (safe to delete) |

## Limits

- No playlist reorder or delete. Add and **New playlist** live on the
  list-plus picker.
- No podcasts UI, lyrics, Winamp skin, MilkDrop, or equalizer (Fastpotify has those).
- No tray / close-to-background; quit from the menu or ⌘Q.
- No personal Spotify developer app (shared public Web API client id, same family as ncspot / spotify-player).
- Local output uses librespot’s PulseAudio backend first, then rodio/ALSA.
