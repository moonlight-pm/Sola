# Shell

The Shell (`sola-shell`) is Sola's desktop shell: the menubar, the tab
bar, the launcher, zoning, and the keyboard shortcuts overlay.

## Keyboard

| Chord | Action |
|-------|--------|
| Super+Space | Launcher |
| Super+Tab | Next workspace (Super+Shift+Tab: previous) |
| Super+Backtick | Next tab in the tab bar (Super+Shift+Backtick: previous) |
| Super+H | Hide the focused app |
| Super+Q | Close the focused app |
| Super+K | Keyboard shortcuts overlay (same chord as Omarchy) |
| Super+L | Lock the screen (needs a password: [Security](security.md)) |
| Super+Shift+3 / 4 / 5 | Screenshot: full output / selection (freeze then marquee) / focused window buffer |
| Super+Numpad | Zone snap (NumLock on or off) |

Super+Backtick shows and focuses the next window in the tab bar's order,
whatever its zone, wrapping at the end; a window in Full brings Full on
screen, one in a half brings the halves. Super+Tab shows the next
workspace (Super+1…9 a given one).

Focus follows the mouse only when the mouse moves. When a keyboard
switch, a new window or a close leaves the pointer resting on another
window, focus stays where it went until you move the mouse; then it
follows the pointer again (after a short pause), even within the window
the pointer was already on.

The number pad types digits at session start (NumLock is turned on for
each keyboard). Super+Numpad zoning still works if you turn NumLock off
(the pad then sends Home / End / arrows). Press NumLock to toggle.

### Shortcuts overlay

Super+K opens the overlay; the flower menu also has **Keyboard
Shortcuts**. Type in the overlay to filter; click an entry or press Enter
to run the action. Escape or Super+K again dismisses it, and so does a
click on the pad around the card (the same as a menu dropdown). Clicks on
the desk pass through: the overlay is the card, not a full-output dim.

### Launcher

Super+Space opens the launcher; type to filter and press Enter (or click)
to launch. An app that is already running comes forward instead of
starting again — its window (the last one you used, if it has several) is
shown in its zone, on its workspace, and takes focus. Terminal is the
exception: picking it opens a new tab window right after the focused tab.
The browser asks where to go (below). An app you hid with Super+H
is shown again rather than started twice.

While an app starts, a faint row with a turning ring stands in the tab bar
where its window will land (after the focused tab); the window takes its
place when it opens. A launch that fails says so on the top row.

**Projects** opens a project, not another Projects. Every project lives in
`~/Projects`; picking Projects turns the launcher into a list of those
folders, the field scoped to "Projects". Type to filter.

- A project with a window says **Open on** its workspace; picking it shows
  that workspace and the window.
- Any other folder opens in the workspace you are on, right after the
  focused tab, on its root tab. (Closing a project's window closes it: its
  sessions end and Projects forgets it.)
- **New project…** asks for a name (or takes the one you typed): Enter
  makes `~/Projects/<name>` and opens it the same way.
- **On another computer…** asks for a computer: type its name (or
  `user@name`, or a `~/.ssh/config` alias); the ones you opened before are
  listed, latest first. Enter connects over ssh and lists that computer's
  `~/Projects`, the field scoped to its name; picking one opens it in
  Projects as "<folder> on <computer>", and **New project** makes one
  there. Its tabs run there and work as tabs here do (see
  [Projects on another computer](solactl.md#projects-on-another-computer)).
  The listing never asks for a password: it needs a key or an ssh agent
  that logs you in, and tmux on that computer. A computer you have not
  reached before is trusted the first time; one whose key changed is
  refused. If it cannot be listed, the reason shows above the computers.
- Escape goes back a step: a computer's projects to the computers, the
  computers to this one's projects, those to the apps; again closes the
  launcher.

If Projects is not running, it starts and opens the project.

Super+V pastes into the launcher's field (an address for Browser, say),
and Super+C / X / A copy, cut and select in it, whatever app was focused
when it opened.

**Browser** asks where to go. Picking Browser turns the launcher into a
field scoped to "Browser": type an address or what to search for, as in
the browser's location bar.

- With nothing typed, your latest visits are listed.
- The first row is what Enter does: open the address, or go straight to
  the top result of the search (Kagi). A search also offers Kagi's
  results page.
- Under them, the pages you have visited that match: by site first, then
  by title and address.
- Picking a row opens it in a new page window right after the focused
  tab; the browser starts if it is not running.
- Escape goes back to the apps; again closes the launcher.

## Flower menu

The left-end flower (and the **Shell** application menu when the Shell is
focused) is the session menu:

| Item | What it does |
|------|----------------|
| Launch Application… | Super+Space launcher |
| Settings… | Brings Settings forward, or starts it |
| Keyboard Shortcuts | Super+K overlay |
| Lock Screen | Super+L: locks the screen ([Security](security.md#locking)) |
| Exit Sola | Ends the desktop session (`Topic::Shutdown`); on Oath the console then has the seat, and `oath desk` brings the desk back |
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
flower (in its own circle; amber unless the look recolors it — see
[Theme](theme.md#shell-tokens)); while a message shows, it sits in the
middle; on the right, an island per group:
mail, alerts, mouse and Bluetooth; the spectrum; CPU, GPU and MEM; RX and
TX; the clock. Click an item for its panel; its island lifts while the
panel is open. Windows and the tab bar start a gap below the row.

The clock's panel is the month: six weeks from the Sunday on or before
the 1st, the days of the months either side faint, today on an accent
circle. ‹ and › step the month; Today comes back to this one (dim while
this month shows). Under the month, **Today** lists today's events from
Calendar: its calendar's colour, the time (or "all day") and the title;
one that has ended dims, one under way is brighter. Click one and
Calendar shows it, starting Calendar if it is closed. Past five, a count
opens Calendar. The list is what Calendar last knew, so it is there with
Calendar closed; it appears once Calendar has run.

CPU / GPU / MEM / RX / TX in the menubar are fixed LED graphs of the
last 12 seconds: a column a second, white, turning the theme's warning
past 75% and its danger past 90% (RX in the accent, TX in success). Dots
are whole: a dot lights once its column reaches its middle (the volume
spectrum's too). Click one for its panel, which draws the same LEDs
larger: the value in large type, the last minute as columns tinted the same
way, per-thread load as one column per thread (a gap between threads, two
between cores), top processes with LED bars, memory
and VRAM as a row of LEDs (used lit, cache dim, free dark), and the rest
as figures. Colors follow the theme.

The volume island sits right of the mail / Bluetooth island and left of
CPU. It is a 12-band LED spectrum analyzer of what the default
output is playing; there is no speaker glyph. Click the bars for the
volume popover. The popover lists outputs and inputs; click a row to make
it the default (`wpctl set-default`). Media keys change the volume on that
default sink.

A Lamzu mouse on its 4K receiver (`3554:f510`) shows its battery left of
Bluetooth: a mouse glyph and a percent, in the theme's warning at 20% and below, its danger at 10%
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
the session's name and what it wants. A project on another computer goes
by its folder alone ("Oath", not "Oath on buccina"). The session you are
looking at gets none. Past half the row, a **+N** island lists the rest.

Click an island for its card. A question's card shows the question and
its answers as buttons, with a field for your own answer; a finished
session's card shows its last message and a field to tell it what to do
next (Return sends). Your answer is typed into the session, as you would
type it, and the island goes. **Show** brings up the session's tab
without answering; Esc closes the card and leaves the island.

An island also goes when the session moves on, or when you look at its
tab yourself.

## Tab bar

The tab bar is the column on the left: every window on the workspace in
one list, not grouped by zone. Each window keeps its zone (Full, Left,
Right, Floating) — moving it to another zone leaves its row where it is.
Click a row to show that window in its zone and focus it; drag a row to
reorder the list (the window stays in its zone, and nothing on screen
changes). Super+Backtick steps through the list in this order.
Hovering a row shows a red close dot, the same as a window's own: it
closes that window as the window's own close dot would.

A new window's row goes right after the row of the window that had focus
(a browser page opened with ⌘T or a link, a Terminal tab, a window from
the launcher). Several opened behind it from the same window (⌘-click)
stay in the order you opened them. Closing the window a zone shows brings
back the one it showed before, so closing a page you just opened returns
to the page it came from.

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

Each workspace has a browser profile of its own (rename the workspace
freely; the profile goes by the workspace, not its name): browser pages on
a workspace share its sign-ins, and a page sent to another
workspace reopens there under that workspace's profile. A browser tab's
row shows the page's title alone.

## Window menu

Every focused app gets a **Window** menu (a kit default; an app can replace
it). It has Hide, cycle windows, float, and every zone, so each of those
chords also has a mouse path. The Shell handles those items for XWayland
windows too.

## Hide

Super+H does **not** close the app. Its surfaces drop out of composition
(River `hide`), so they are not drawn until you bring the app back:

- the tab bar — click its row
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
- Super+H is ignored while the launcher is up.
