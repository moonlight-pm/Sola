# sola-shell

Desktop shell: menubar, launcher, switcher, zoning, shortcuts overlay.

**Partial.** Super+H hide **installed** `river` + `shell` (debug, 2026-08-31).
Window menu + Super+K shortcuts overlay **installed** `kit`+`shell`+`paint`+`terminal`
(debug, 2026-08-31). Menubar remapped after a hard-kill left zombie
shell surfaces.

## Keyboard

| Chord | Action |
|-------|--------|
| Super+Space | Launcher |
| Super+Tab | App switcher (release Super to raise) |
| Super+\` | Cycle windows of the focused app |
| Super+H | Hide the focused app |
| Super+Q | Close the focused app |
| Super+K | Keyboard shortcuts overlay (same chord as Omarchy) |
| Super+Shift+3 / 4 / 5 | Screenshot: full output / selection (freeze then marquee) / focused window buffer |
| Super+Numpad | Zone snap (NumLock on or off) |

The number pad types digits at session start (NumLock is turned on for
each keyboard). Super+Numpad zoning still works if you turn NumLock off
(the pad then sends Home / End / arrows). Press NumLock to toggle.

The flower menu also has **Keyboard Shortcuts**. Type in the overlay to
filter; click or Enter runs the action. Escape / Super+K again / click
outside dismisses.

CPU / GPU / MEM / RX / TX in the menubar are fixed dithered pixel
graphs (last ~12 seconds). Exact numbers live in the click dropdown.
The volume chip (right of Bluetooth, left of CPU, with the same gap
on both sides) is a 12-band LED spectrum analyzer of what the default
output is playing. Click the bars for the volume popover; there is no
speaker glyph.

## Window menu

Every focused app gets a **Window** menu (kit default; an app can replace
it). Hide, cycle windows, float, and every zone — the mouse path for
chords that used to be keyboard-only. The shell handles those items even
for XWayland windows.

## Hide

Super+H does **not** close the app. Surfaces drop out of composition
(River `hide`) so they are not drawn until you bring the app back:

- Super+Tab — hidden apps stay in the switcher
- Super+Space — pick a running hidden app to unhide (does not spawn a
  second copy)

The process keeps running. Notifications, mail unread, and an outside
open (`solactl open` / `xdg-open`) also unhide when they raise the app.

Hiding the last visible app leaves the menubar and wallpaper.

## Notifications

The missed-notifications bell (right cluster, with a count) opens a list
grouped by app. A handful of items list as rows (the number on the bell
matches). A flood from one app collapses to one row with a count; click
to expand. Same-tag updates replace the missed row instead of stacking.
Accent while unseen; clicking the bell returns it to normal chrome.
There is no Clear-all; the group × dismisses that app’s missed items.
Click a row to raise the source. Super+Tab shows a count on the app icon
for notifications you have not opened in the pile or visited in that app
(Mail uses inbox unread). Super+Shift+4 with the panel open keeps it in
the freeze.

## Screenshots

Super+Shift+3 / 4 / 5 copy a PNG onto the system clipboard and toast
**Screenshot copied**. The clipboard is offered at the chord — paste in
Slack immediately; the paste waits until encode finishes. They do not
write a file or open Preview.

Super+Shift+4 freezes the live output first (menus, text selections, and
other transient UI stay in the still), then opens a full-brightness
marquee on that still (no dim). The chord does **not** dismiss an open
notifications panel or other menubar popover before the copy — the
freeze is the live pixels. The crop is taken from the freeze — not a
second live capture.

Super+Shift+5 and `solactl compositor screenshot --app` copy the
window’s own buffer. They do not raise the app. The CLI still writes a
PNG path; the chord copies to the clipboard.

## Limits

- Hide is per `app_id` (every window of that app). Steam id variants can
  miss.
- No hide animation.
- Super+H is ignored while the switcher or launcher is up.
- A hard kill of the shell used to leave invisible compositor ghosts;
  sola-river now drops windows whose process is gone.
