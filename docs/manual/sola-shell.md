# Shell

The Shell (`sola-shell`) is Sola's desktop shell: the menubar, the
launcher, the app switcher, zoning, and the keyboard shortcuts overlay.

## Keyboard

| Chord | Action |
|-------|--------|
| Super+Space | Launcher |
| Super+Tab | App switcher (release Super to raise) |
| Super+Backtick | Cycle windows of the focused app |
| Super+H | Hide the focused app |
| Super+Q | Close the focused app |
| Super+K | Keyboard shortcuts overlay (same chord as Omarchy) |
| Super+Shift+3 / 4 / 5 | Screenshot: full output / selection (freeze then marquee) / focused window buffer |
| Super+Numpad | Zone snap (NumLock on or off) |

Super+Tab opens with the previous app highlighted. An icon the pointer is
already resting on stays unselected until the pointer moves. Releasing
Super raises the highlighted app.

The number pad types digits at session start (NumLock is turned on for
each keyboard). Super+Numpad zoning still works if you turn NumLock off
(the pad then sends Home / End / arrows). Press NumLock to toggle.

### Shortcuts overlay

Super+K opens the overlay; the flower menu also has **Keyboard
Shortcuts**. Type in the overlay to filter; click an entry or press Enter
to run the action. Escape or Super+K again dismisses it, and so does a
click on the pad around the card (the same as a menu dropdown). Clicks on
the desk pass through: the overlay is the card, not a full-output dim.

## Flower menu

The left-end flower (and the **Shell** application menu when the Shell is
focused) is the session menu:

| Item | What it does |
|------|----------------|
| Launch Application… | Super+Space launcher |
| Keyboard Shortcuts | Super+K overlay |
| Restart Shell | Exits `sola-shell` only; the process manager respawns it |
| Quit Sola | Ends the desktop session (`Topic::Shutdown`) |
| Restart Computer | Reboots the machine (logind) |
| Shut Down | Powers off the machine (logind) |

Restart Computer and Shut Down have no confirmation and no shortcut.
They are not in the Super+K overlay. A failure shows the toast **Could not
restart the computer** or **Could not shut down**.

Sola has no polkit password prompt. A local seat session is usually
already allowed. Hosts with `services.sola.enable` also let the `wheel`
group reboot and power off without a password (this takes effect after
`nixos-rebuild`).

## Menubar

The menubar is the top row of the desk: a row of islands on the
ground (the background picture, or black), a gap in from the edges,
nothing behind them. On the left, the
flower (amber, in its own circle); while a message shows, it sits in the
middle; on the right, an island per group:
mail, alerts, mouse and Bluetooth; the spectrum; CPU, GPU and MEM; RX and
TX; the clock. Click an item for its panel; its island lifts while the
panel is open. Windows and the tab bar start a gap below the row.

CPU / GPU / MEM / RX / TX in the menubar are fixed dithered pixel graphs
of the last ~12 seconds. Exact numbers are in the dropdown that opens on
click.

The volume island sits right of the mail / Bluetooth island and left of
CPU. It is a 12-band LED spectrum analyzer of what the default
output is playing; there is no speaker glyph. Click the bars for the
volume popover. The popover lists outputs and inputs; click a row to make
it the default (`wpctl set-default`). Media keys change the volume on that
default sink.

A Lamzu mouse on its 4K receiver (`3554:f510`) shows its battery left of
Bluetooth: a mouse glyph and a percent, amber at 20% and below, red at 10%
and below, and a charging glyph while it charges. A sleeping mouse does not
answer, so the chip appears a few seconds after you first move the mouse;
after that the Shell asks once a minute. The number dims after ten minutes
without an answer (mouse asleep, off, or out of range), and the chip
disappears when the receiver is unplugged. The Shell needs read/write
access to the receiver's hidraw node; the NixOS module's udev rule gives
it to the `input` group. Other Lamzu receivers and the wired cable are not
read.

## Agent islands

When a Projects session asks you something (a permission, a question) or
finishes its turn, an island for it appears on the left of the top row,
after the flower: a diamond for a question, a check for a finished turn,
the session's name and what it wants. The session you are looking at gets
none. Past half the row, a **+N** island lists the rest.

Click an island for its card. A question's card shows the question and
its answers as buttons, with a field for your own answer; a finished
session's card shows its last message and a field to tell it what to do
next (Return sends). Your answer is typed into the session, as you would
type it, and the island goes. **Show** brings up the session's tab
without answering; Esc closes the card and leaves the island.

An island also goes when the session moves on, or when you look at its
tab yourself.

## Tab bar

The tab bar is the column on the left: every window, by the zone it is
in (Full, Left, Right, Floating). Click a row to show that window in its
zone and focus it; drag a row to reorder it or move it to another zone.
Hovering a row shows a red close dot, the same as a window's own: it
closes that window as the window's own close dot would.

- The focused window's row is the bright pill.
- The window a zone is showing is lifted a step while it is on screen,
  and in full ink without a lift while its zone is not (Full while the
  halves are up, or the halves while Full is).
- The windows behind are muted.

The head names the workspace you are on. Click it for the workspace
menu: every workspace (the current one checked; Super+1…9 shows one),
then **New Workspace**, **Rename…**, **Move Left** / **Move Right**, and
**Delete** for the current one. A diamond beside the name means a tab on
another workspace waits on a decision; a dot, that a Mail window on
another workspace has unread mail.

## Window menu

Every focused app gets a **Window** menu (a kit default; an app can replace
it). It has Hide, cycle windows, float, and every zone, so each of those
chords also has a mouse path. The Shell handles those items for XWayland
windows too.

## Hide

Super+H does **not** close the app. Its surfaces drop out of composition
(River `hide`), so they are not drawn until you bring the app back:

- Super+Tab — hidden apps stay in the switcher
- Super+Space — pick a running hidden app to unhide (this does not spawn a
  second copy)

The process keeps running. Notifications, mail unread, and an outside open
(`solactl open` / `xdg-open`) also unhide the app when they raise it.

Hiding the last visible app leaves the menubar and wallpaper.

## Notifications

The missed-notifications bell (right cluster, with a count) opens a list
grouped by app. A handful of items list as rows, and the number on the bell
matches. A flood from one app collapses to one row with a count; click it
to expand. A same-tag update replaces the missed row instead of stacking.

The bell shows the accent color while there are unseen items; clicking it
returns it to normal chrome. There is no Clear All; the group × dismisses
that app’s missed items. Click a row to raise the source.

Super+Tab shows a count on an app's icon for notifications you have not
opened in the pile or visited in that app (Mail uses inbox unread).
Super+Shift+4 with the panel open keeps the panel in the freeze.

## Screenshots

Super+Shift+3 / 4 / 5 copy a PNG onto the system clipboard and show the
toast **Screenshot copied**. They do not write a file or open Preview. The
clipboard is offered at the chord, so you can paste right away (in Slack,
for example); the paste waits until encoding finishes.

Super+Shift+4 freezes the live output first (menus, text selections, and
other transient UI stay in the still), then opens a full-brightness marquee
on that still, with no dim. The chord does **not** dismiss an open
notifications panel or other menubar popover before the copy: the freeze
is the live pixels. The crop is taken from the freeze, not from a second
live capture.

Super+Shift+5 and `solactl compositor screenshot --app` copy the window’s
own buffer. They do not raise the app. The CLI writes a PNG path; the
chord copies to the clipboard.

## Limits

- Hide is per `app_id` (every window of that app). Steam id variants can
  miss.
- There is no hide animation.
- Super+H is ignored while the switcher or launcher is up.
