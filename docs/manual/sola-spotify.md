# sola-spotify

Kit-native Spotify client. Browse the library, search, control Spotify Connect devices, and (with Premium) play on this computer through librespot.

**Partial.** First pass **installed** `spotify` (debug, 2026-09-01). Playback logic is adapted from [Fastpotify](https://github.com/crmne/fastpotify) (MIT). Independent of Spotify AB. Launcher row waits on `install shell`; until then run `/opt/sola/bin/sola-spotify`.

## Requirements

- Spotify account. **Playing on this computer needs Premium.** Free accounts can sign in, browse, search, and control another device.
- `alsa-lib` and `libpulseaudio` at compile time (`pkg-config` `alsa` + `libpulse`). The NixOS module ships both. PipeWire’s Pulse compatibility is enough at runtime.
- After a NixOS rebuild, `cargo make install spotify` (and `install shell` once for the launcher row).

## Sign in

1. Open **Spotify** from the launcher (Meta+Space).
2. **Sign in with Spotify** opens sola-browser on Spotify’s consent page (Authorization Code + PKCE). Sola never sees the password.
3. Tokens land in `~/.local/state/sola/spotify/` (`0600`). Sign in once per machine.

Playing **here** is a second, one-time approval (**Playback → Set up playback here**, or Settings). Spotify treats streaming separately from the Web API. librespot then caches a reusable credential.

## Use

- **Library** rail: Home, Search, Liked Songs, Albums, Artists, Queue, playlists.
- Click a row or **Play** on a playlist/album to start it. With no other
  speaker on, that **sets up this computer** (a second browser approval,
  once per machine) and plays here. Premium required for local audio.
- **+** saves to Liked Songs. **−** hides a row from autoplay (dims it;
  does not delete from the playlist). Click − again to restore.
- Reopen remembers the last playlist and paints it from a disk cache,
  then refreshes.
- Bottom bar: play/pause, skip, seek, shuffle, repeat, like, volume, device
  picker. **This computer** is always the first device. Other Spotify Connect
  speakers (phone, Electron, hardware) sit under **Other devices**.
- **Space** play/pause; **⌘← / ⌘→** previous/next; **⌘F** search; **⌘H** home; **⌘L** liked.

Media keys (`solactl media`) go to whichever MPRIS player is Playing. This app registers as `org.mpris.MediaPlayer2.sola-spotify`.

## Files

| Path | What |
|------|------|
| `~/.config/sola/spotify/settings.json` | Connect device name, bitrate, gapless, autoplay, last viewed page |
| `~/.local/state/sola/spotify/` | Web API refresh token, librespot credentials, `skipped.json` |
| `~/.cache/sola/spotify/` | Audio, album art, page JSON (safe to delete) |

## Limits

- No playlist create/reorder/delete in this pass.
- No podcasts UI, lyrics, Winamp skin, MilkDrop, or equalizer (Fastpotify has those).
- No tray / close-to-background; quit from the menu or ⌘Q.
- No personal Spotify developer app (shared public Web API client id, same family as ncspot / spotify-player).
- Local output uses librespot’s PulseAudio backend first, then rodio/ALSA.
